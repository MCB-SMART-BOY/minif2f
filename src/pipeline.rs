use crate::checkpoint::CheckpointManager;
use crate::config::{ModelConfig, PipelineConfig};
use crate::data::{load_all, Theorem};
use crate::inference::InferenceEngine;
use crate::prompts::PromptBuilder;
use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use serde_json::Map;
use std::collections::BTreeMap;

/// (raw_output, lean_code) tuple per attempt, keyed by attempt name.
type AttemptMap = BTreeMap<String, (String, String)>;
/// Theorem results: theorem name → attempt map.
type ResultsMap = BTreeMap<String, AttemptMap>;

/// Generates proofs for all loaded theorems and saves two nested JSON files:
///   output/raw_output/<model>.json  — unfiltered model completions
///   output/lean_code/<model>.json   — extracted + assembled Lean proofs
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
    /// Uses a continuous request pool: all theorem × attempt requests are submitted
    /// through a semaphore-controlled channel. Results flow as they arrive
    /// with NO per-theorem barrier — the GPU queue stays saturated.
    ///
    /// # Errors
    ///
    /// Returns an error if the dataset cannot be loaded, the inference engine
    /// fails to start, or output cannot be written.
    pub async fn run(&self, model_cfg: &ModelConfig, model_path: &str) -> Result<()> {
        let theorems = load_all(&self.config.data_path())?;
        println!("   Theorems: {}", theorems.len());

        // ── Start inference engine ──────────────────────────────────────
        println!("\n🔧 Starting vLLM server...");
        let engine = InferenceEngine::start(
            model_cfg.clone(),
            model_path,
            self.config.port,
            &self.config.uv_project_dir,
            self.config.parallel,
        )
        .await?;
        println!("   ✅ vLLM server ready");

        // ── Prepare output directories ──────────────────────────────────
        println!("\n🧠 Generating proofs...");
        let output_dir = self.config.output_dir();
        let raw_dir = output_dir.join("raw_output");
        let lean_dir = output_dir.join("lean_code");
        std::fs::create_dir_all(&raw_dir)?;
        std::fs::create_dir_all(&lean_dir)?;

        let checkpoint =
            CheckpointManager::new(&self.config.checkpoint_dir(), &model_cfg.name, &self.run_id)?;

        let n_attempts = self.config.completion_attempts;
        let pb = PromptBuilder::new(model_cfg.clone());
        let pb_extract = PromptBuilder::new(model_cfg.clone()); // for main loop

        // ── Load existing results from prior runs (checkpoint resume) ──
        let raw_json_path =
            raw_dir.join(format!("{}.json", model_cfg.name.replace(['/', ' '], "_")));
        let lean_json_path =
            lean_dir.join(format!("{}.json", model_cfg.name.replace(['/', ' '], "_")));
        let mut results: ResultsMap =
            load_existing_results(&raw_json_path, &lean_json_path, &model_cfg.name)?;

        // ── Build the full job list (skip checkpointed theorems) ────────
        let mut pending_theorems: Vec<Theorem> = Vec::new();
        for theorem in &theorems {
            if !checkpoint.is_done(&theorem.name) {
                pending_theorems.push(theorem.clone());
            }
        }

        if pending_theorems.is_empty() {
            println!("   All theorems already done via checkpoint.");
            return Ok(());
        }

        let total_jobs = pending_theorems.len() * n_attempts;
        let bar = ProgressBar::new(total_jobs as u64);
        bar.set_style(ProgressStyle::default_bar().template("{msg} [{bar:40}] {pos}/{len} {eta}")?);
        bar.set_message("Generating");

        // ── Continuous request pool via buffer_unordered ─────────────────
        //
        // `buffer_unordered(N)` keeps N HTTP requests in flight.  When one
        // completes, the next job starts immediately.  Jobs are ordered by
        // theorem so per-theorem checkpointing works correctly.
        //
        // For models with long outputs (800–5000 tokens), GPU time (2–17s
        // per request) dominates — utilisation near 100%.  For STP with
        // very short outputs (~50 tokens, ~170ms/req), proof extraction
        // (~40ms each) is the limiting factor, producing brief GPU dips
        // between batches.  These are inherent to the short-output regime
        // and do not justify added pipeline complexity.

        use futures::stream::{self, StreamExt as _};

        let concurrency = self.config.parallel as usize;
        let url = format!("{}/v1/completions", engine.base_url());
        let client = engine.http_client().clone();
        let max_tokens = model_cfg.max_tokens;
        let temperature = model_cfg.temperature;
        let top_p = model_cfg.top_p;
        let base_seed = model_cfg.seed;
        let stop_sequences = model_cfg.stop_sequences.clone();

        // Build job list ordered by theorem.
        let mut all_jobs: Vec<(Theorem, usize, serde_json::Value)> = Vec::with_capacity(total_jobs);
        for theorem in &pending_theorems {
            let prompt = pb.build(theorem);
            for i in 0..n_attempts {
                all_jobs.push((
                    theorem.clone(),
                    i,
                    serde_json::json!({
                        "prompt": &prompt,
                        "n_predict": max_tokens,
                        "temperature": temperature,
                        "top_p": top_p,
                        "seed": base_seed.wrapping_add(i as u64) as u32,
                        "stop": &stop_sequences,
                        "n_probs": 0,
                    }),
                ));
            }
        }

        let mut result_stream = stream::iter(all_jobs)
            .map(|(theorem, attempt, body)| {
                let url = url.clone();
                let client = client.clone();
                async move {
                    let text =
                        InferenceEngine::generate_one_with_retry(&client, &url, body, 3).await;
                    (theorem, attempt, text)
                }
            })
            .buffer_unordered(concurrency);

        // ── Per-theorem batch extraction with rayon ─────────────────────
        //
        // buffer_unordered may interleave results from adjacent theorems,
        // so we accumulate each theorem's results in its own batch.
        // When a batch reaches n_attempts, parallel extraction via rayon frees
        // the main loop to continue feeding the GPU.

        // Write JSON incrementally to prevent data loss on crash.
        // Checkpoint files only record theorem names, not proof data —
        // without incremental writes, a crash loses all generated proofs.
        const INCREMENTAL_WRITE_EVERY: u32 = 20;

        let mut batches: BTreeMap<String, (Theorem, Vec<(usize, String)>)> = BTreeMap::new();
        let mut thms_since_write: u32 = 0;

        while let Some((theorem, attempt, text)) = result_stream.next().await {
            let entry = batches
                .entry(theorem.name.clone())
                .or_insert_with(|| (theorem.clone(), Vec::with_capacity(n_attempts)));
            entry.1.push((attempt, text));

            if entry.1.len() >= n_attempts {
                let (thm, batch) = batches.remove(&theorem.name).unwrap();
                flush_batch(
                    &mut results,
                    &thm,
                    &batch,
                    &pb_extract,
                    &self.config.checkpoint_dir(),
                    &model_cfg.name,
                    &self.run_id,
                    &bar,
                );

                thms_since_write += 1;
                if thms_since_write >= INCREMENTAL_WRITE_EVERY {
                    write_flat_json(&raw_json_path, model_cfg, &results, |(raw, _lean)| raw)?;
                    write_flat_json(&lean_json_path, model_cfg, &results, |(_raw, lean)| lean)?;
                    thms_since_write = 0;
                }
            }
        }

        bar.finish_with_message("Generation done");

        println!(
            "   ✅ {} theorems generated ({} skipped)",
            pending_theorems.len(),
            checkpoint.initial_skipped
        );

        // Shut down inference engine (frees GPU)
        engine.stop();

        // ── Final JSON write (catch remaining after last incremental) ────
        println!("\n📝 Writing final JSON...");

        write_flat_json(&raw_json_path, model_cfg, &results, |(raw, _lean)| raw)?;
        write_flat_json(&lean_json_path, model_cfg, &results, |(_raw, lean)| lean)?;

        let raw_size = std::fs::metadata(&raw_json_path)
            .map(|m| m.len())
            .unwrap_or(0);
        let lean_size = std::fs::metadata(&lean_json_path)
            .map(|m| m.len())
            .unwrap_or(0);

        println!("\n╔══════════════════════════════════╗");
        println!("║  Generation complete!            ║");
        println!("╠══════════════════════════════════╣");
        println!(
            "║  Theorems:  {:>4}                 ║",
            pending_theorems.len()
        );
        println!(
            "║  Raw:       {} ({:.1} MB) ║",
            raw_json_path.display(),
            raw_size as f64 / 1_000_000.0
        );
        println!(
            "║  Lean:      {} ({:.1} MB) ║",
            lean_json_path.display(),
            lean_size as f64 / 1_000_000.0
        );
        println!("╚══════════════════════════════════╝");

        Ok(())
    }
}

/// Parallel batch extraction via rayon, then sequential BTreeMap insert.
#[allow(clippy::too_many_arguments)]
fn flush_batch(
    results: &mut ResultsMap,
    theorem: &Theorem,
    batch: &[(usize, String)],
    pb: &PromptBuilder,
    checkpoint_dir: &std::path::Path,
    model_name: &str,
    run_id: &str,
    bar: &ProgressBar,
) {
    use rayon::prelude::*;

    // Parallel extraction — rayon splits across CPU cores
    let extracted: Vec<(usize, String, String)> = batch
        .par_iter()
        .map(|(attempt, text)| {
            let proof = pb.extract_proof(text);
            let lean = if proof.is_empty() {
                String::new()
            } else if proof.contains("import ") {
                proof
            } else {
                theorem.make_proof_file(&proof)
            };
            // Validate: reject incomplete Lean (missing tactics, has sorry, etc.)
            let lean = if pb.validate_lean_code(&lean) {
                lean
            } else {
                String::new()
            };
            (*attempt, text.clone(), lean)
        })
        .collect();

    // Sequential insertion (BTreeMap not Sync)
    for (attempt, text, lean) in extracted {
        results
            .entry(theorem.name.clone())
            .or_default()
            .insert(format!("attempt_{}", attempt + 1), (text, lean));
        bar.inc(1);
    }

    // Checkpoint — each flush is a complete theorem
    let ck_thm = theorem.name.clone();
    let ck_dir = checkpoint_dir.to_path_buf();
    let ck_model = model_name.to_string();
    let ck_run = run_id.to_string();
    tokio::task::spawn_blocking(move || {
        if let Ok(mut ck) = CheckpointManager::new(&ck_dir, &ck_model, &ck_run) {
            let _ = ck.mark_done(&ck_thm);
        }
    });
}

/// Write a flat JSON file: `{ "<model>": { "<theorem>": { "attempt_N": "<text>" } } }`.
fn write_flat_json(
    path: &std::path::Path,
    model_cfg: &ModelConfig,
    results: &ResultsMap,
    pick: fn(&(String, String)) -> &str,
) -> Result<()> {
    let mut model_obj = Map::new();
    let mut theorem_map = Map::new();
    for (thm_name, attempts) in results {
        let mut attempt_map = Map::new();
        for (attempt_key, tuple) in attempts {
            attempt_map.insert(
                attempt_key.clone(),
                serde_json::Value::String(pick(tuple).to_string()),
            );
        }
        theorem_map.insert(thm_name.clone(), serde_json::Value::Object(attempt_map));
    }
    model_obj.insert(
        model_cfg.name.clone(),
        serde_json::Value::Object(theorem_map),
    );

    let json_str = serde_json::to_string_pretty(&model_obj)?;
    std::fs::write(path, &json_str)?;
    Ok(())
}

/// Load theorem results from existing raw_output and lean_code JSON files
/// (for checkpoint resume). Returns a merged map keyed by theorem name.
fn load_existing_results(
    raw_path: &std::path::Path,
    lean_path: &std::path::Path,
    model_name: &str,
) -> Result<ResultsMap> {
    let mut results: ResultsMap = BTreeMap::new();

    let read_json = |path: &std::path::Path| -> Result<Option<serde_json::Value>> {
        if !path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("reading: {}", path.display()))?;
        if content.trim().is_empty() {
            return Ok(None);
        }
        Ok(Some(
            serde_json::from_str(&content)
                .with_context(|| format!("parsing: {}", path.display()))?,
        ))
    };

    let raw_data = read_json(raw_path)?;
    let lean_data = read_json(lean_path)?;

    let raw_map = extract_model_data(raw_data.as_ref(), model_name);
    let lean_map = extract_model_data(lean_data.as_ref(), model_name);

    // Merge: collect all theorem names from both sources
    let all_theorems: std::collections::BTreeSet<String> =
        raw_map.keys().chain(lean_map.keys()).cloned().collect();

    for thm_name in all_theorems {
        let mut attempts: AttemptMap = BTreeMap::new();

        let raw_attempts = raw_map.get(&thm_name);
        let lean_attempts = lean_map.get(&thm_name);

        let all_attempts: std::collections::BTreeSet<String> = raw_attempts
            .iter()
            .flat_map(|m| m.keys())
            .chain(lean_attempts.iter().flat_map(|m| m.keys()))
            .cloned()
            .collect();

        for att_key in all_attempts {
            let raw = raw_attempts
                .and_then(|m| m.get(&att_key))
                .cloned()
                .unwrap_or_default();
            let lean = lean_attempts
                .and_then(|m| m.get(&att_key))
                .cloned()
                .unwrap_or_default();
            attempts.insert(att_key, (raw, lean));
        }

        results.insert(thm_name, attempts);
    }

    Ok(results)
}

/// Extract per-theorem attempts map from a flat JSON model object.
fn extract_model_data(
    data: Option<&serde_json::Value>,
    model_name: &str,
) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    if let Some(data) = data {
        if let Some(model_obj) = data.get(model_name) {
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
                    out.insert(thm_name.clone(), attempts_map);
                }
            }
        }
    }
    out
}
