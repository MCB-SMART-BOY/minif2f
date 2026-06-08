use crate::config::ModelConfig;
use anyhow::{Context, Result};
use futures::stream::{FuturesUnordered, StreamExt};
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
                "server.py",
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
                "--enforce-eager",
                "--trust-remote-code",
            ])
            .env("CUDA_HOME", &cu13)
            .env("VLLM_USE_FLASHINFER_SAMPLER", "0")
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
                        // Decode LLaMA byte-fallback encoding (vLLM tokenizer bug).
                        // LLaMA tokenizer encodes bytes 0x00-0xFF as U+0100-U+01FF.
                        // e.g. 0x20→U+0120(Ġ), 0x0A→U+010A(Ċ), 0xE2→U+01E2(â)
                        // Multi-byte UTF-8 chars like ℕ(0xE2,0x84,0x95) become
                        // â(U+01E2) + some_fallback(0x84) + some_fallback(0x95).
                        // Fix: collect all U+0100-U+01FF as bytes, keep rest as UTF-8.
                        return decode_llama_byte_fallback(&raw);
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

    /// Generate n completions — returns a stream of (attempt_index, text) as
    /// each request completes. No barrier: results are yielded immediately,
    /// keeping vLLM's continuous batching saturated.
    #[allow(clippy::cast_possible_truncation)]
    pub fn generate_stream(
        &self,
        prompt: &str,
        n: usize,
        attempt_offset: usize,
    ) -> FuturesUnordered<impl futures::Future<Output = (usize, String)>> {
        let url = format!("{}/v1/completions", self.base_url);
        let prompt = prompt.to_string();
        let client = self.client.clone();
        let max_tokens = self.config.max_tokens;
        let temperature = self.config.temperature;
        let top_p = self.config.top_p;
        let base_seed = self.config.seed;
        let stop_sequences = self.config.stop_sequences.clone();

        let futures: FuturesUnordered<_> = FuturesUnordered::new();
        for i in 0..n {
            let body = serde_json::json!({
                "model": self.config.name,
                "prompt": prompt,
                "max_tokens": max_tokens,
                "temperature": temperature,
                "top_p": top_p,
                "seed": base_seed.wrapping_add(attempt_offset as u64 + i as u64) as u32,
                "stop": stop_sequences,
                "stream": false,
            });
            let client = client.clone();
            let url = url.clone();
            futures.push(async move {
                let text = Self::generate_one_with_retry(&client, &url, body, 3).await;
                (i, text)
            });
        }
        futures
    }

    /// Generate n completions with per-request retries (graceful degradation).
    /// Legacy batch API — returns all results at once (barrier).
    /// Prefer `generate_stream` for better GPU utilization.
    pub async fn generate_batch_retry(
        &self,
        prompt: &str,
        n: usize,
        attempt_offset: usize,
    ) -> Vec<String> {
        let mut stream = self.generate_stream(prompt, n, attempt_offset);
        let mut results: Vec<Option<String>> = vec![None; n];
        while let Some((i, text)) = stream.next().await {
            results[i] = Some(text);
        }
        results.into_iter().map(|r| r.unwrap_or_default()).collect()
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
/// LLaMA tokenizer encodes raw bytes 0x00–0xFF as Unicode characters
/// U+0100–U+01FF.  Characters outside that range are already valid UTF-8.
/// This function reverses the fallback: collect byte-fallback chars into
/// bytes, pass through everything else as UTF-8, then decode the result.
/// Decode LLaMA byte-fallback + Latin-1 encoding from vLLM's output.
///
/// vLLM's tokenizer for LLaMA models emits a mix of:
///   - Latin-1: bytes 0x80-0xFF → U+0080-U+00FF (e.g. 0xE2 → â)
///   - Byte-fallback: bytes 0x00-0xFF → U+0100-U+01FF (e.g. 0x20 → Ġ)
///
/// Characters outside these ranges pass through as valid UTF-8.
fn decode_llama_byte_fallback(text: &str) -> String {
    let mut bytes: Vec<u8> = Vec::with_capacity(text.len());
    for ch in text.chars() {
        let cp = ch as u32;
        match cp {
            0x0080..=0x00FF => bytes.push(cp as u8), // Latin-1 → raw byte
            0x0100..=0x01FF => bytes.push((cp - 0x0100) as u8), // byte-fallback → raw byte
            _ => {
                let mut buf = [0u8; 4];
                let len = ch.encode_utf8(&mut buf).len();
                bytes.extend_from_slice(&buf[..len]);
            }
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}
