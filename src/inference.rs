use crate::config::ModelConfig;
use anyhow::{Context, Result};
use futures::stream::{FuturesUnordered, StreamExt};
use reqwest::Client;
use serde_json::Value;
use std::process::{Child, Command};
use std::time::Duration;

/// Manages a llama-server process and provides an HTTP inference client.
pub struct InferenceEngine {
    pub config: ModelConfig,
    client: Client,
    server: Child,
    base_url: String,
}

impl InferenceEngine {
    /// Start llama-server and wait for it to be ready.
    ///
    /// # Errors
    ///
    /// Returns an error if llama-server cannot be spawned, the health check
    /// times out, or the HTTP client cannot be created.
    pub async fn start(
        config: ModelConfig,
        model_path: &str,
        port: u16,
        llama_server_binary: &str,
        parallel: u32,
    ) -> Result<Self> {
        let child = Command::new(llama_server_binary)
            .args([
                "-m",
                model_path,
                "--port",
                &port.to_string(),
                "-ngl",
                "99",
                "--ctx-size",
                &{
                    // Each slot needs prompt + max_tokens (capped at model training limit).
                    // llama-server divides ctx-size by --parallel: per_slot = ctx / parallel.
                    // So: ctx = per_slot_needed * parallel
                    let per_slot = (config.max_tokens + 4096).min(config.max_model_len);
                    (per_slot * parallel).to_string()
                },
                "--batch-size",
                "2048",
                "--parallel",
                &parallel.to_string(),
                "--cache-type-k",
                "q8_0",
                "--cache-type-v",
                "q8_0",
                "--cache-reuse",
                "256",
                "--flash-attn",
                "on",
                "--no-warmup",
                "--api-key",
                "minif2f",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::fs::File::create(format!(
                "/tmp/llama-server-{port}.log"
            ))?)
            .spawn()
            .context("Failed to start llama-server. Is llama.cpp installed?")?;

        let base_url = format!("http://localhost:{port}");

        let engine = Self {
            config,
            client: Client::builder()
                .timeout(Duration::from_mins(5))
                .no_proxy()
                .build()?,
            server: child,
            base_url: base_url.clone(),
        };

        // Wait for server to be ready (model loads async)
        let health_url = format!("{base_url}/health");
        let start = std::time::Instant::now();
        let timeout = Duration::from_mins(2);

        loop {
            if start.elapsed() > timeout {
                engine.kill();
                anyhow::bail!(
                    "llama-server did not become ready within {}s",
                    timeout.as_secs()
                );
            }
            match engine
                .client
                .get(&health_url)
                .header("Authorization", "Bearer minif2f")
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => break,
                Ok(resp) if resp.status().as_u16() == 503 => {
                    // Model is loading — keep waiting
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                Ok(_) | Err(_) => {
                    tokio::time::sleep(Duration::from_millis(500)).await;
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
            match client
                .post(url)
                .header("Authorization", "Bearer minif2f")
                .json(&body)
                .send()
                .await
            {
                Ok(resp) => match resp.json::<Value>().await {
                    Ok(json) => {
                        return json["content"].as_str().unwrap_or("").to_string();
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
    /// keeping llama-server slots fed and GPU saturated.
    #[allow(clippy::cast_possible_truncation)]
    pub fn generate_stream(
        &self,
        prompt: &str,
        n: usize,
        attempt_offset: usize,
    ) -> FuturesUnordered<impl futures::Future<Output = (usize, String)>> {
        let url = format!("{}/completion", self.base_url);
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
                "prompt": prompt,
                "n_predict": max_tokens,
                "temperature": temperature,
                "top_p": top_p,
                "seed": base_seed.wrapping_add(attempt_offset as u64 + i as u64) as u32,
                "stop": stop_sequences,
                "n_probs": 0,
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

    /// Access the llama-server base URL.
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn kill(&self) {
        let _ = Command::new("kill")
            .args(["-9", &self.server.id().to_string()])
            .output();
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
