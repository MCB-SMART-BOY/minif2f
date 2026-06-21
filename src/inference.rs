use crate::config::ModelConfig;
use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::Value;
use std::process::{Child, Command};
use std::time::Duration;

/// Manages a vLLM server process (via `uv run`) and provides an HTTP inference client.
pub struct InferenceEngine {
    pub config: ModelConfig,
    client: Client,
    server: Child,
    base_url: String,
}

impl InferenceEngine {
    /// Start vLLM server via `uv run` and wait for it to be ready.
    ///
    /// # Errors
    ///
    /// Returns an error if vLLM cannot be spawned, the health check
    /// times out, or the HTTP client cannot be created.
    pub async fn start(
        config: ModelConfig,
        model_path: &str,
        port: u16,
        uv_project_dir: &str,
        parallel: u32,
    ) -> Result<Self> {
        // vLLM uses --max-num-seqs for max concurrent sequences
        // --max-model-len caps the per-sequence context window
        let max_model_len = (config.max_tokens + 4096).min(config.max_model_len);

        // Resolve model path to absolute — vLLM requires absolute paths for local models
        let model_path = std::path::absolute(model_path)
            .context("Failed to resolve model path")?
            .to_string_lossy()
            .to_string();

        // CUDA toolkit path for FlashInfer JIT compilation on Blackwell (SM 12.x)
        let cu13 = format!("{uv_project_dir}/.venv/lib/python3.12/site-packages/nvidia/cu13");

        let child = Command::new("uv")
            .args([
                "run",
                "--directory",
                uv_project_dir,
                "python",
                "-m",
                "vllm.entrypoints.openai.api_server",
                "--model",
                &model_path,
                "--port",
                &port.to_string(),
                "--max-model-len",
                &max_model_len.to_string(),
                "--max-num-seqs",
                &parallel.to_string(),
                "--gpu-memory-utilization",
                "0.92",
                "--dtype",
                "half",
                "--trust-remote-code",
                "--quantization",
                "fp8",
                "--tokenizer-mode",
                "slow",
                "--disable-custom-all-reduce",
                "--disable-log-stats",
            ])
            .env("CUDA_HOME", &cu13)
            .env("VLLM_USE_FLASHINFER_SAMPLER", "0")
            .env("VLLM_ATTENTION_BACKEND", "FLASH_ATTN")
            .env("OMP_NUM_THREADS", "")
            .stdout(std::process::Stdio::null())
            .stderr(std::fs::File::create(format!(
                "/tmp/vllm-server-{port}.log"
            ))?)
            .spawn()
            .context("Failed to start vLLM wrapper. Is `uv sync` done in tools/vllm/?")?;

        let base_url = format!("http://localhost:{port}");

        let engine = Self {
            config,
            client: Client::builder()
                .timeout(Duration::from_mins(10))
                .no_proxy() // CRITICAL: bypass HTTP_PROXY for localhost
                .build()?,
            server: child,
            base_url: base_url.clone(),
        };

        // Wait for vLLM to load the model and be ready
        // vLLM model loading can take 30–120s depending on model size
        let health_url = format!("{base_url}/health");
        let start = std::time::Instant::now();
        let timeout = Duration::from_mins(5);

        loop {
            if start.elapsed() > timeout {
                engine.kill();
                anyhow::bail!(
                    "vLLM server did not become ready within {}s",
                    timeout.as_secs()
                );
            }
            match engine.client.get(&health_url).send().await {
                Ok(resp) if resp.status().is_success() => break,
                Ok(_) | Err(_) => {
                    // Model is loading — keep waiting
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }

        Ok(engine)
    }

    /// Generate a single completion with retries for transient errors.
    /// Public for use by the pipeline's streaming request pool.
    pub async fn generate_one_with_retry(
        client: &Client,
        url: &str,
        body: serde_json::Value,
        max_retries: usize,
        architecture: &str,
    ) -> String {
        for attempt in 0..=max_retries {
            match client.post(url).json(&body).send().await {
                Ok(resp) => match resp.json::<Value>().await {
                    Ok(json) => {
                        // OpenAI-compatible: choices[0].text
                        let raw = json["choices"]
                            .get(0)
                            .and_then(|c| c["text"].as_str())
                            .unwrap_or("")
                            .to_string();
                        // LLaMA-based architectures (raw, deepseek_v2, deepseek_coder)
                        // use LlamaTokenizer which encodes bytes 0x00–0xFF as
                        // U+0100–U+01FF.  e.g. 0x20→U+0120(Ġ), 0x0A→U+010A(Ċ).
                        // Qwen3 architecture uses Qwen2Tokenizer which outputs
                        // standard UTF-8 — byte-fallback decoding would corrupt
                        // legitimate Latin Extended characters.
                        return match architecture {
                            "raw" | "deepseek_v2" | "deepseek_coder" => {
                                decode_llama_byte_fallback(&raw)
                            }
                            _ => raw, // qwen3 — decoder would corrupt valid Unicode
                        };
                    }
                    Err(e) if attempt < max_retries => {
                        eprintln!(
                            "⚠️  JSON parse error (retry {}/{}): {e}",
                            attempt + 1,
                            max_retries
                        );
                        tokio::time::sleep(Duration::from_secs(1 << attempt)).await;
                    }
                    Err(e) => {
                        eprintln!("⚠️  JSON parse error (final): {e}");
                    }
                },
                Err(e) if attempt < max_retries => {
                    eprintln!(
                        "⚠️  HTTP error (retry {}/{}): {e}",
                        attempt + 1,
                        max_retries
                    );
                    tokio::time::sleep(Duration::from_secs(1 << attempt)).await;
                }
                Err(e) => {
                    eprintln!("⚠️  HTTP error (final): {e}");
                }
            }
        }
        String::new()
    }

    /// Access the HTTP client (for streaming requests from pipeline).
    #[must_use]
    pub fn http_client(&self) -> &Client {
        &self.client
    }

    /// Access the vLLM server base URL.
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn kill(&self) {
        // Try graceful shutdown first (SIGTERM), then force kill
        let pid = self.server.id();
        let _ = Command::new("kill")
            .args(["-15", &pid.to_string()])
            .output();
        std::thread::sleep(Duration::from_secs(2));
        let _ = Command::new("kill").args(["-9", &pid.to_string()]).output();
    }

    /// Shut down server immediately (frees GPU). Drop handles zombie reaping.
    pub fn stop(self) {
        self.kill();
    }
}

impl Drop for InferenceEngine {
    fn drop(&mut self) {
        let _ = self.server.kill();
        let _ = self.server.wait();
    }
}

/// Reverse GPT-2 ByteLevel encoding from vLLM's output.
///
/// DeepSeek/Goedel "LLaMA-based" tokenizers are actually GPT-2 **ByteLevel BPE**
/// (`tokenizer.json`: `model.type = BPE`, `decoder.type = ByteLevel`,
/// `byte_fallback = false`, vocab uses `Ġ`/`â`).  Their vocabulary maps each raw
/// byte 0x00–0xFF to a printable Unicode char via the GPT-2 `bytes_to_unicode`
/// table — which is NOT a flat `byte + 0x0100` offset.  When vLLM runs with
/// `--tokenizer-mode slow`, the ByteLevel decoder is bypassed and these encoded
/// chars (e.g. `âĦ¤` for `ℤ`) leak into the completion text.  This function
/// reverses them with the correct inverse table.
///
/// The previous implementation used `byte = codepoint - 0x100`, which only
/// agrees with the GPT-2 table on ASCII and mis-decodes 35 continuation bytes
/// (e.g. `Ħ`/U+0126 → 0x26 `&` instead of 0x84), shredding multi-byte math
/// symbols like ℤ/ℕ/ℝ.  See `.claude/memory/05c-decoder-bug.md`.
///
/// Only chars present in the GPT-2 byte table are mapped back to bytes; any
/// other char (already-correct UTF-8, e.g. a literal space, newline, or a math
/// symbol vLLM detokenized correctly) passes through unchanged.
fn decode_llama_byte_fallback(text: &str) -> String {
    let table = gpt2_unicode_to_byte();
    let mut bytes: Vec<u8> = Vec::with_capacity(text.len());
    for ch in text.chars() {
        if let Some(&b) = table.get(&ch) {
            bytes.push(b);
        } else {
            let mut buf = [0u8; 4];
            let len = ch.encode_utf8(&mut buf).len();
            bytes.extend_from_slice(&buf[..len]);
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Build the GPT-2 `bytes_to_unicode` inverse map (printable char → raw byte).
///
/// Mirrors the canonical GPT-2/RoBERTa implementation: the 188 "printable"
/// bytes map to themselves, and the remaining 68 bytes map to U+0100, U+0101, …
/// in order.  Inverting gives char → byte.
fn gpt2_unicode_to_byte() -> std::collections::HashMap<char, u8> {
    // The printable byte ranges that map to their own codepoint.
    let mut bs: Vec<u32> = Vec::new();
    bs.extend(0x21..=0x7E); // '!'..='~'
    bs.extend(0xA1..=0xAC); // '¡'..='¬'
    bs.extend(0xAE..=0xFF); // '®'..='ÿ'

    let mut map = std::collections::HashMap::with_capacity(256);
    let mut n: u32 = 0;
    for b in 0u32..256 {
        if bs.contains(&b) {
            // byte maps to the char with the same codepoint
            if let Some(c) = char::from_u32(b) {
                map.insert(c, b as u8);
            }
        } else {
            // byte maps to U+0100 + n
            if let Some(c) = char::from_u32(0x100 + n) {
                map.insert(c, b as u8);
            }
            n += 1;
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_pure_ascii_passes_through() {
        let input = "  rfl";
        assert_eq!(decode_llama_byte_fallback(input), "  rfl");
    }

    #[test]
    fn test_decode_byte_fallback_range() {
        // U+0120 (Ġ) → 0x20 (space)
        let decoded = decode_llama_byte_fallback("\u{0120}");
        assert_eq!(decoded, " ");
    }

    #[test]
    fn test_decode_byte_fallback_newline() {
        // U+010A (Ċ) → 0x0A (newline)
        let decoded = decode_llama_byte_fallback("\u{010A}");
        assert_eq!(decoded, "\n");
    }

    #[test]
    fn test_decode_byte_fallback_cr() {
        // U+010D (č) → 0x0D (carriage return)
        let decoded = decode_llama_byte_fallback("\u{010D}");
        assert_eq!(decoded, "\r");
    }

    #[test]
    fn test_decode_mixed_byte_fallback_and_ascii() {
        // Byte-fallback space + normal text
        let input = "rw\u{0120}[h\u{0120}]"; // Ġ → space
        let decoded = decode_llama_byte_fallback(input);
        assert_eq!(decoded, "rw [h ]");
    }

    #[test]
    fn test_decode_valid_utf8_preserved() {
        // Characters like ℕ (U+2115) are valid Unicode and should pass through
        let input = "theorem foo : \u{2115}";
        let decoded = decode_llama_byte_fallback(input);
        assert!(decoded.contains('\u{2115}'));
    }

    // ── ByteLevel round-trip tests (the key fix) ───────────────────────
    //
    // DeepSeek/Goedel tokenizers are GPT-2 ByteLevel BPE.  Multi-byte UTF-8
    // is encoded byte-by-byte into printable chars; the decoder must reverse
    // the GPT-2 `bytes_to_unicode` table, NOT a flat `cp - 0x100`.

    #[test]
    fn test_recovers_blackboard_z() {
        // ℤ (U+2124, utf-8 E2 84 A4) → GPT-2 byte-encoded "âĦ¤"
        //   â=U+00E2→0xE2, Ħ=U+0126→0x84, ¤=U+00A4→0xA4
        let input = "\u{00E2}\u{0126}\u{00A4}"; // âĦ¤
        let decoded = decode_llama_byte_fallback(input);
        assert_eq!(decoded, "\u{2124}"); // ℤ
        assert!(!decoded.contains('\u{FFFD}'));
    }

    #[test]
    fn test_recovers_math_symbols_in_context() {
        // "(n : ℤ)" byte-encoded. '(' 'n' ' '(Ġ) ':' ' '(Ġ) âĦ¤ ')'
        let input = "(n\u{0120}:\u{0120}\u{00E2}\u{0126}\u{00A4})";
        let decoded = decode_llama_byte_fallback(input);
        assert_eq!(decoded, "(n : \u{2124})");
    }

    #[test]
    fn test_recovers_right_single_quote() {
        // U+2019 (’) utf-8 = E2 80 99 → byte-encoded "âĢĻ"
        //   â=0xE2, Ģ=U+0122→0x80, Ļ=U+013B→0x99
        let input = "\u{00E2}\u{0122}\u{013B}";
        let decoded = decode_llama_byte_fallback(input);
        assert_eq!(decoded, "\u{2019}");
    }

    #[test]
    fn test_byte_encoded_accented_letter_round_trips() {
        // A real é (U+00E9, utf-8 C3 A9) is byte-encoded as "Ã©"
        //   Ã=U+00C3→0xC3, ©=U+00A9→0xA9
        let input = "Jos\u{00C3}\u{00A9}"; // "JosÃ©"
        let decoded = decode_llama_byte_fallback(input);
        assert_eq!(decoded, "Jos\u{00E9}"); // José
    }

    #[test]
    fn test_decode_valid_utf8_passes_through() {
        // Chars NOT in the GPT-2 byte table (e.g. ℕ U+2115, already-correct
        // UTF-8 that vLLM detokenized properly) pass through unchanged.
        let input = "theorem foo : \u{2115}";
        let decoded = decode_llama_byte_fallback(input);
        assert!(decoded.contains('\u{2115}'));
    }

    #[test]
    fn test_real_corruption_marker_preserved() {
        // U+FFFD already in input is not in the table → passes through.
        let input = "bad \u{FFFD} char";
        let decoded = decode_llama_byte_fallback(input);
        assert!(decoded.contains('\u{FFFD}'));
    }

    #[test]
    fn test_space_and_newline_pass_through() {
        // A literal space/newline (vLLM already detokenized) are NOT in the
        // GPT-2 table (which has Ġ/Ċ for those bytes) → must pass through.
        let input = "rw [h]\n  rfl";
        let decoded = decode_llama_byte_fallback(input);
        assert_eq!(decoded, input);
    }

    #[test]
    fn test_gpt2_table_is_bijective_over_256_bytes() {
        // Every one of the 256 byte values must have exactly one char.
        let table = gpt2_unicode_to_byte();
        assert_eq!(table.len(), 256);
        let mut seen = [false; 256];
        for (_, &b) in &table {
            seen[b as usize] = true;
        }
        assert!(seen.iter().all(|&s| s), "table must cover all 256 bytes");
    }

    #[test]
    fn test_decode_empty_string() {
        assert_eq!(decode_llama_byte_fallback(""), "");
    }

    #[test]
    fn test_decode_ascii_only_long_text() {
        let input = "import Mathlib\n\ntheorem foo : 1 = 1 := by\n  rfl";
        assert_eq!(decode_llama_byte_fallback(input), input);
    }

    // ── Architecture-conditional decoding tests ───────────────────────

    #[test]
    fn test_decode_applied_for_llama_architectures() {
        // LLaMA architectures — byte-fallback range should be decoded
        let input = "rw\u{0120}[h]"; // U+0120 (Ġ) → space
        for arch in &["raw", "deepseek_v2", "deepseek_coder"] {
            let result = decode_if_llama(input, arch);
            assert_eq!(result, "rw [h]", "arch={arch} should decode byte-fallback");
        }
    }

    #[test]
    fn test_decode_skipped_for_qwen3() {
        // Qwen3 — U+0100-U+01FF characters are legitimate Unicode, not byte-fallback
        let input = "theorem_foo\u{0120}bar"; // Ġ is valid Latin Extended-A
        let result = decode_if_llama(input, "qwen3");
        // Must pass through unchanged — NOT converted to space
        assert_eq!(result, "theorem_foo\u{0120}bar");
        assert!(result.contains('\u{0120}'));
    }

    #[test]
    fn test_qwen3_lean_proof_with_special_chars_preserved() {
        // Simulates real Qwen3 output: Lean tactics with various Unicode chars
        let input = "  constructor\n  \u{00B7} intro h\n  rw [h]";
        let result = decode_if_llama(input, "qwen3");
        assert_eq!(result, input, "Qwen3 output must pass through unchanged");
        assert!(!result.contains('\u{FFFD}'));
    }

    #[test]
    fn test_llama_architecture_decodes_entire_range() {
        // LLaMA architecture: U+0100-U+01FF should all be decoded to raw bytes
        let input = "\u{0120}\u{010A}\u{010D}"; // space, newline, CR
        let result = decode_if_llama(input, "raw");
        assert_eq!(result, " \n\r");
    }

    #[test]
    fn test_unknown_architecture_treated_as_qwen3() {
        // Safety: unknown architectures default to no decoding
        let input = "\u{0120}test";
        let result = decode_if_llama(input, "unknown_arch");
        assert_eq!(result, input, "unknown arch should skip decoding");
    }

    /// Apply `decode_llama_byte_fallback` only for LLaMA-based architectures.
    /// Qwen3 (qwen3) uses Qwen2Tokenizer which outputs standard UTF-8 — byte-fallback
    /// decoding would corrupt legitimate Latin Extended characters.
    fn decode_if_llama(text: &str, architecture: &str) -> String {
        match architecture {
            "raw" | "deepseek_v2" | "deepseek_coder" => decode_llama_byte_fallback(text),
            _ => text.to_string(),
        }
    }
}
