use crate::checkpoint::CheckpointManager;
use crate::config::{ModelConfig, PipelineConfig};
use crate::data::load_all;
use crate::inference::InferenceEngine;
use crate::prompts::PromptBuilder;
use anyhow::{Context, Result};
use futures::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use serde_json::Map;
use std::collections::BTreeMap;

/// Generates proofs for all 488 theorems and saves as nested JSON:
/// { "<model>": { "<theorem>": { "`attempt_1"`: "<proof>", ... } } }
pub struct EvaluationPipeline {
    pub config: PipelineConfig,
    run_id: String,
}

impl EvaluationPipeline {
    #[must_use]
    pub fn new(config: PipelineConfig, run_id: &str) -> Self {
        Self {
            config,
            run_id: run_id.to_string(),
        }
    }

    /// Run the full generation pipeline for a single model.
    ///
    /// Uses streaming request processing — results flow as each completion
    /// arrives, eliminating the per-theorem barrier. A bounded semaphore
    /// keeps in-flight request count at `parallel × 4` to saturate the GPU
    /// without overwhelming llama-server queues.
    ///
    /// # Errors
    ///
    /// Returns an error if the dataset cannot be loaded, the inference engine
    /// fails to start, or output cannot be written.
    #[allow(clippy::cast_precision_loss)]
    pub async fn run(&self, model_cfg: &ModelConfig, model_path: &str) -> Result<()> {
        let theorems = load_all(&self.config.data_path())?;
        println!("   Theorems: {}", theorems.len());

        // ── Start inference engine ──────────────────────────────────────
        println!("\n🔧 Starting llama-server...");
        let llama_server_bin = self
            .config
            .llama_server_path()
            .to_string_lossy()
            .to_string();
        let engine = InferenceEngine::start(
            model_cfg.clone(),
            model_path,
            self.config.port,
            &llama_server_bin,
            self.config.parallel,
        )
        .await?;
        println!("   ✅ llama-server ready");

        // ── Generate ────────────────────────────────────────────────────
        println!("\n🧠 Generating proofs...");
        let output_dir = self.config.output_dir();
        std::fs::create_dir_all(&output_dir)?;

        let checkpoint =
            CheckpointManager::new(&self.config.checkpoint_dir(), &model_cfg.name, &self.run_id)?;

        let n_attempts = self.config.completion_attempts;

        let pb = PromptBuilder::new(model_cfg.clone());

        // ── Load existing results from prior runs (checkpoint resume) ──
        let json_path =
            output_dir.join(format!("{}.json", model_cfg.name.replace(['/', ' '], "_")));
        let mut results: BTreeMap<String, BTreeMap<String, String>> =
            load_existing_results(&json_path, &model_cfg.name)?;

        // ── Stream results as they complete (no per-theorem barrier) ──
        // `generate_stream` returns a FuturesUnordered that yields each
        // attempt's result as soon as it arrives. The llama-server's
        // internal --parallel queue keeps the GPU saturated.

        // Progress bar: only count theorems not yet done
        let remaining = theorems.len().saturating_sub(checkpoint.initial_skipped);
        let bar = ProgressBar::new(remaining as u64);
        bar.set_style(ProgressStyle::default_bar().template("{msg} [{bar:40}] {pos}/{len} {eta}")?);
        bar.set_message("Generating");

        // ── Pre-build all prompts (string clone is cheap vs GPU time) ──
        struct TheoremJob {
            prompt: String,
            attempts: BTreeMap<String, String>,
        }
        let mut jobs: BTreeMap<String, TheoremJob> = BTreeMap::new();
        for theorem in &theorems {
            if checkpoint.is_done(&theorem.name) {
                continue;
            }
            jobs.insert(
                theorem.name.clone(),
                TheoremJob {
                    prompt: pb.build(theorem),
                    attempts: BTreeMap::new(),
                },
            );
        }
        let mut completed_theorems = 0;

        // ── Process theorems sequentially, but stream results within each ──
        // The semaphore carries over between theorems: as soon as theorem N's
        // last few requests are finishing, theorem N+1's requests start firing.
        for theorem in &theorems {
            let Some(mut job) = jobs.remove(&theorem.name) else {
                continue; // already done via checkpoint
            };

            // Stream 128 requests — results arrive as they complete
            let mut stream = engine.generate_stream(&job.prompt, n_attempts, 0);

            while let Some((i, text)) = stream.next().await {
                let raw = text.as_str();
                let proof = pb.extract_proof(raw);
                let lean_source = if proof.contains("import ") {
                    proof
                } else {
                    theorem.make_proof_file(&proof)
                };
                job.attempts
                    .insert(format!("attempt_{}", i + 1), lean_source);
            }

            results.insert(theorem.name.clone(), job.attempts);

            // Async checkpoint write — don't block the next theorem
            let ck_thm = theorem.name.clone();
            let ck_dir = self.config.checkpoint_dir();
            let ck_model = model_cfg.name.clone();
            let ck_run = self.run_id.clone();
            drop(tokio::task::spawn_blocking(move || {
                if let Ok(mut ck) = CheckpointManager::new(&ck_dir, &ck_model, &ck_run) {
                    let _ = ck.mark_done(&ck_thm);
                }
            }));

            completed_theorems += 1;
            bar.inc(1);
        }

        bar.finish_with_message("Generation done");

        println!(
            "   ✅ {completed_theorems} theorems generated ({} skipped)",
            checkpoint.initial_skipped
        );

        // Shut down inference engine (frees GPU)
        engine.stop();

        // ── Write nested JSON ──────────────────────────────────────────
        println!("\n📝 Writing JSON...");

        let mut model_obj = Map::new();
        let mut theorem_map = Map::new();
        for (thm_name, attempts) in &results {
            let mut attempt_map = Map::new();
            for (attempt_key, proof) in attempts {
                attempt_map.insert(
                    attempt_key.clone(),
                    serde_json::Value::String(proof.clone()),
                );
            }
            theorem_map.insert(thm_name.clone(), serde_json::Value::Object(attempt_map));
        }
        model_obj.insert(
            model_cfg.name.clone(),
            serde_json::Value::Object(theorem_map),
        );

        let json_str = serde_json::to_string_pretty(&model_obj)?;
        std::fs::write(&json_path, &json_str)?;

        let file_size = json_str.len();
        println!("\n╔══════════════════════════════════╗");
        println!("║  Generation complete!            ║");
        println!("╠══════════════════════════════════╣");
        println!("║  Theorems: {completed_theorems:>4}                 ║");
        let size_mb = file_size as f64 / 1_000_000.0;
        println!("║  Output:   {} ({:.1} MB) ║", json_path.display(), size_mb);
        println!("╚══════════════════════════════════╝");

        Ok(())
    }
}

/// Load theorem results from an existing output JSON file (for checkpoint resume).
fn load_existing_results(
    path: &std::path::Path,
    model_name: &str,
) -> Result<BTreeMap<String, BTreeMap<String, String>>> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }

    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading existing output: {}", path.display()))?;
    if content.trim().is_empty() {
        return Ok(BTreeMap::new());
    }

    let existing: serde_json::Value = serde_json::from_str(&content)
        .with_context(|| format!("parsing existing output: {}", path.display()))?;

    let mut results = BTreeMap::new();
    if let Some(model_obj) = existing.get(model_name) {
        if let Some(theorems_obj) = model_obj.as_object() {
            for (thm_name, attempts_val) in theorems_obj {
                let mut attempts_map = BTreeMap::new();
                if let Some(att_obj) = attempts_val.as_object() {
                    for (att_key, proof_val) in att_obj {
                        if let Some(proof_str) = proof_val.as_str() {
                            attempts_map.insert(att_key.clone(), proof_str.to_string());
                        }
                    }
                }
                results.insert(thm_name.clone(), attempts_map);
            }
        }
    }
    Ok(results)
}
