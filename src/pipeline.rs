use crate::checkpoint::CheckpointManager;
use crate::config::{ModelConfig, PipelineConfig};
use crate::data::load_all;
use crate::inference::InferenceEngine;
use crate::prompts::PromptBuilder;
use anyhow::Result;
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
        )
        .await?;
        println!("   ✅ llama-server ready");

        // ── Generate ────────────────────────────────────────────────────
        println!("\n🧠 Generating proofs...");
        let output_dir = self.config.output_dir();
        std::fs::create_dir_all(&output_dir)?;

        let mut checkpoint =
            CheckpointManager::new(&self.config.checkpoint_dir(), &model_cfg.name, &self.run_id)?;

        let n_attempts = self.config.completion_attempts;

        let pb = PromptBuilder::new(model_cfg.clone());

        let bar = ProgressBar::new(theorems.len() as u64);
        bar.set_style(ProgressStyle::default_bar().template("{msg} [{bar:40}] {pos}/{len} {eta}")?);
        bar.set_message("Generating");

        // Build nested result: BTreeMap<theorem_name, BTreeMap<attempt_key, proof>>
        let mut results: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();

        for theorem in &theorems {
            if checkpoint.is_done(&theorem.name) {
                continue;
            }

            // Build prompt (reused across all attempts)
            let prompt = pb.build(theorem);
            let mut attempts: BTreeMap<String, String> = BTreeMap::new();

            // Generate n proofs (batched for efficiency)
            for batch_start in (0..n_attempts).step_by(self.config.completion_attempts.min(8)) {
                let batch_size = (n_attempts - batch_start).min(8);
                let texts = engine
                    .generate_batch_retry(&prompt, batch_size, batch_start)
                    .await;
                for (j, text) in texts.iter().enumerate() {
                    let attempt_num = batch_start + j + 1; // 1-indexed
                    let raw = text.as_str();
                    let proof = pb.extract_proof(raw);
                    let lean_source = if proof.contains("import ") {
                        proof
                    } else {
                        theorem.make_proof_file(&proof)
                    };
                    attempts.insert(format!("attempt_{attempt_num}"), lean_source);
                }
            }

            results.insert(theorem.name.clone(), attempts);

            checkpoint.mark_done(&theorem.name)?;
            bar.inc(1);
        }
        bar.finish_with_message("Generation done");

        let done = checkpoint.total_done() - checkpoint.initial_skipped;
        let skipped = checkpoint.initial_skipped;
        println!("   ✅ {done} theorems generated ({skipped} skipped)");

        // Shut down inference engine (frees GPU)
        engine.stop();

        // ── Write nested JSON ──────────────────────────────────────────
        println!("\n📝 Writing JSON...");
        let json_path =
            output_dir.join(format!("{}.json", model_cfg.name.replace(['/', ' '], "_")));

        // Build: { "<model>": { "<theorem>": { "attempt_1": "..." } } }
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
        println!("║  Theorems: {done:>4}                 ║");
        let size_mb = file_size as f64 / 1_000_000.0;
        println!("║  Output:   {} ({:.1} MB) ║", json_path.display(), size_mb);
        println!("╚══════════════════════════════════╝");

        Ok(())
    }
}
