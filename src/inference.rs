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

/// Decode LLaMA byte-fallback encoding from vLLM's output.
///
/// LLaMA tokenizer encodes raw bytes 0x00–0xFF as Unicode characters in the
/// U+0100–U+01FF range (byte + 0x0100).  vLLM 0.22.1 handles most of this
/// internally, but a small number of tokens may still leak through.  This
/// function reverses those remaining fallback characters.
///
/// **IMPORTANT**: Latin-1 characters (U+0080–U+00FF) are NOT decoded back to
/// raw bytes.  They are valid Unicode (e.g. `é` in "José") and converting
/// them would corrupt adjacent characters by forming spurious multi-byte
/// UTF-8 sequences from unrelated Latin-1 bytes.  vLLM's built-in tokenizer
/// already handles the LLaMA tokenizer's Latin-1 output correctly.
fn decode_llama_byte_fallback(text: &str) -> String {
    let mut bytes: Vec<u8> = Vec::with_capacity(text.len());
    for ch in text.chars() {
        let cp = ch as u32;
        match cp {
            // Only decode the explicit byte-fallback range.
            // Latin-1 (0x0080..=0x00FF) passes through as valid UTF-8.
            0x0100..=0x01FF => bytes.push((cp - 0x0100) as u8),
            _ => {
                let mut buf = [0u8; 4];
                let len = ch.encode_utf8(&mut buf).len();
                bytes.extend_from_slice(&buf[..len]);
            }
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
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

    // ── Latin-1 preservation tests (the key fix) ────────────────────────

    #[test]
    fn test_latin1_preserved_not_decoded() {
        // Latin-1 characters are valid Unicode — they must pass through,
        // NOT be converted back to raw bytes.  Converting them causes
        // adjacent Latin-1 chars to combine into spurious multi-byte
        // UTF-8 sequences (e.g. U+00D0 + U+00B4 → 0xD0 0xB4 → 'д').
        let input = "Jos\u{00E9}"; // José
        let decoded = decode_llama_byte_fallback(input);
        assert_eq!(decoded, "Jos\u{00E9}");
        assert!(!decoded.contains('\u{FFFD}'));
    }

    #[test]
    fn test_latin1_no_spurious_cyrillic() {
        // U+00D0 (Ð) + U+00B4 (´) → if decoded as raw bytes:
        //   0xD0 + 0xB4 = UTF-8 for 'д' (Cyrillic)
        // After fix: both pass through as valid Latin-1
        let input = "\u{00D0}\u{00B4}";
        let decoded = decode_llama_byte_fallback(input);
        assert_eq!(decoded, "\u{00D0}\u{00B4}");
        assert!(!decoded.contains('\u{0434}')); // NOT Cyrillic 'д'
        assert!(!decoded.contains('\u{FFFD}'));
    }

    #[test]
    fn test_latin1_no_spurious_replacement_char() {
        // U+00E2 (â) alone → if decoded as 0xE2 (UTF-8 continuation byte):
        //   from_utf8_lossy would produce U+FFFD (orphan byte)
        // After fix: passes through unchanged
        let input = "\u{00E2}";
        let decoded = decode_llama_byte_fallback(input);
        assert_eq!(decoded, "\u{00E2}");
        assert!(!decoded.contains('\u{FFFD}'));
    }

    #[test]
    fn test_latin1_replacement_char_still_present_for_real_corruption() {
        // U+FFFD that was already in the input should still be there
        let input = "bad \u{FFFD} char";
        let decoded = decode_llama_byte_fallback(input);
        assert!(decoded.contains('\u{FFFD}'));
    }

    #[test]
    fn test_latin1_full_spanish_word() {
        // Real-world: Spanish/Portuguese names in theorem descriptions
        let input = "Jos\u{00E9} Carlos G\u{00F3}mez";
        let decoded = decode_llama_byte_fallback(input);
        assert_eq!(decoded, "Jos\u{00E9} Carlos G\u{00F3}mez");
    }

    // ── Mixed Latin-1 + byte-fallback ──────────────────────────────────

    #[test]
    fn test_mixed_latin1_and_byte_fallback() {
        // Latin-1 é (U+00E9) + byte-fallback space (U+0120) + text
        let input = "resum\u{00E9}\u{0120}proof";
        let decoded = decode_llama_byte_fallback(input);
        assert_eq!(decoded, "resum\u{00E9} proof");
    }

    // ── Real corruption scenarios from goedel-prover-dpo ────────────────

    #[test]
    fn test_dpo_style_corruption_prevented() {
        // Simulates: "h" + U+00D0 + U+00B4 + "$"
        // Old code: 0xD0+0xB4 → 'д', producing "hд$"
        // New code: preserves Latin-1, producing "hÐ´$"
        let input = "h\u{00D0}\u{00B4}$";
        let decoded = decode_llama_byte_fallback(input);
        assert_eq!(decoded, "h\u{00D0}\u{00B4}$");
        assert!(!decoded.contains('\u{0434}')); // no Cyrillic
    }

    #[test]
    fn test_multiple_latin1_sequence_preserved() {
        // A string of consecutive Latin-1 bytes — all must pass through
        let input = "\u{00E2}\u{0080}\u{0099}\u{00E2}\u{0080}\u{0099}";
        let decoded = decode_llama_byte_fallback(input);
        // Should be identical (no re-interpretation)
        assert_eq!(decoded, input);
        assert!(!decoded.contains('\u{FFFD}'));
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
