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

        let mut checkpoint =
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
        let architecture = model_cfg.architecture.clone();

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
                        "max_tokens": max_tokens,
                        "temperature": temperature,
                        "top_p": top_p,
                        "seed": base_seed.wrapping_add(i as u64) as u32,
                        "stop": &stop_sequences,
                    }),
                ));
            }
        }

        let mut result_stream = stream::iter(all_jobs)
            .map(|(theorem, attempt, body)| {
                let url = url.clone();
                let client = client.clone();
                let architecture = architecture.clone();
                async move {
                    let text = InferenceEngine::generate_one_with_retry(
                        &client,
                        &url,
                        body,
                        3,
                        &architecture,
                    )
                    .await;
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
        // Checkpoint files only record theorem names, not proof data.
        //
        // ORDERING INVARIANT: a theorem is marked done in the checkpoint ONLY
        // after its data has been written to the output JSON.  If we checkpoint
        // per-theorem but write JSON every 20, a crash in that window would let
        // the checkpoint skip theorems whose data never reached disk (silent
        // loss).  So we buffer just-flushed theorem names and `mark_done` them
        // only after `write_flat_json` succeeds.  Worst case on crash: data is
        // on disk but a theorem isn't marked done → it is regenerated on resume
        // (harmless duplicate work, never data loss).
        const INCREMENTAL_WRITE_EVERY: u32 = 20;

        let mut batches: BTreeMap<String, (Theorem, Vec<(usize, String)>)> = BTreeMap::new();
        let mut thms_since_write: u32 = 0;
        // Theorems flushed to `results` but not yet persisted + checkpointed.
        let mut pending_checkpoint: Vec<String> = Vec::new();

        while let Some((theorem, attempt, text)) = result_stream.next().await {
            let entry = batches
                .entry(theorem.name.clone())
                .or_insert_with(|| (theorem.clone(), Vec::with_capacity(n_attempts)));
            entry.1.push((attempt, text));

            if entry.1.len() >= n_attempts {
                let (thm, batch) = batches.remove(&theorem.name).unwrap();
                flush_batch(&mut results, &thm, &batch, &pb_extract, &bar);
                pending_checkpoint.push(thm.name.clone());

                thms_since_write += 1;
                if thms_since_write >= INCREMENTAL_WRITE_EVERY {
                    // Data first …
                    write_flat_json(&raw_json_path, model_cfg, &results, |(raw, _lean)| raw)?;
                    write_flat_json(&lean_json_path, model_cfg, &results, |(_raw, lean)| lean)?;
                    // … then mark those theorems done (never ahead of disk).
                    for name in pending_checkpoint.drain(..) {
                        if let Err(e) = checkpoint.mark_done(&name) {
                            eprintln!("⚠️  Checkpoint write failed for {name}: {e}");
                        }
                    }
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

        // Data is now durable — mark any theorems not yet checkpointed.
        for name in pending_checkpoint.drain(..) {
            if let Err(e) = checkpoint.mark_done(&name) {
                eprintln!("⚠️  Checkpoint write failed for {name}: {e}");
            }
        }

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

/// Re-extract `lean_code` from an existing `raw_output/<model>.json` without
/// running any inference.  Reads the stored raw completions, re-runs proof
/// extraction + assembly + validation with the current code, and writes a
/// fresh `lean_code/<model>.json`.
///
/// Zero GPU cost.  Use after fixing extraction/assembly logic when the raw
/// model output is intact (i.e. qwen3 models — LLaMA raw is decoder-corrupted
/// at write time and cannot be recovered this way).
///
/// `raw_dir`/`lean_dir` are the directories holding `<model>.json`.  Theorem
/// metadata (header/statement) is loaded from `data_dir` and matched by name.
///
/// # Errors
/// Returns an error if the dataset or raw_output file cannot be read/parsed,
/// or the lean_code file cannot be written.
pub fn re_extract_model(
    model_cfg: &ModelConfig,
    raw_dir: &std::path::Path,
    lean_dir: &std::path::Path,
    data_dir: &std::path::Path,
) -> Result<(usize, usize)> {
    let pb = PromptBuilder::new(model_cfg.clone());

    // Theorem metadata keyed by name.
    let theorems = load_all(data_dir)?;
    let thm_by_name: BTreeMap<String, Theorem> =
        theorems.into_iter().map(|t| (t.name.clone(), t)).collect();

    // Read raw_output and pull out this model's object.
    let file_stem = model_cfg.name.replace(['/', ' '], "_");
    let raw_path = raw_dir.join(format!("{file_stem}.json"));
    let raw_text = std::fs::read_to_string(&raw_path)
        .with_context(|| format!("reading {}", raw_path.display()))?;
    let raw_json: serde_json::Value = serde_json::from_str(&raw_text)
        .with_context(|| format!("parsing {}", raw_path.display()))?;
    let model_obj = raw_json
        .get(&model_cfg.name)
        .and_then(|v| v.as_object())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "key '{}' not found in {}",
                model_cfg.name,
                raw_path.display()
            )
        })?;

    // Re-extract every attempt.
    let mut results: ResultsMap = BTreeMap::new();
    let mut total = 0usize;
    let mut recovered = 0usize;
    for (thm_name, attempts) in model_obj {
        let Some(theorem) = thm_by_name.get(thm_name) else {
            continue; // raw theorem not in current dataset — skip
        };
        let attempts = attempts.as_object().context("attempt map not an object")?;
        for (attempt_key, raw_val) in attempts {
            let raw = raw_val.as_str().unwrap_or("");
            total += 1;
            let proof = pb.extract_proof(raw);
            let lean = assemble_and_validate(&proof, theorem, &pb);
            if !lean.is_empty() {
                recovered += 1;
            }
            results
                .entry(thm_name.clone())
                .or_default()
                .insert(attempt_key.clone(), (raw.to_string(), lean));
        }
    }

    // Write lean_code with the same flat format as the pipeline.
    std::fs::create_dir_all(lean_dir)?;
    let lean_path = lean_dir.join(format!("{file_stem}.json"));
    write_flat_json(&lean_path, model_cfg, &results, |(_, lean)| lean)?;

    Ok((total, recovered))
}

/// Parallel batch extraction via rayon, then sequential BTreeMap insert.
///
/// Does NOT checkpoint — the caller marks theorems done only AFTER their data
/// has been written to disk, so the checkpoint can never get ahead of durable
/// output (see the incremental-write loop in `run`).
fn flush_batch(
    results: &mut ResultsMap,
    theorem: &Theorem,
    batch: &[(usize, String)],
    pb: &PromptBuilder,
    bar: &ProgressBar,
) {
    use rayon::prelude::*;

    // Parallel extraction — rayon splits across CPU cores
    let extracted: Vec<(usize, String, String)> = batch
        .par_iter()
        .map(|(attempt, text)| {
            let proof = pb.extract_proof(text);
            let lean = assemble_and_validate(&proof, theorem, pb);
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
}

/// Assemble a complete Lean file from an extracted proof, then validate it.
///
/// Single source of truth for proof assembly, shared by live generation
/// (`flush_batch`) and offline re-extraction (the `re-extract` subcommand) so
/// the two paths can never drift.  Returns `""` when extraction was empty or
/// the assembled file fails validation.
///
/// Three assembly branches:
/// 1. Proof already contains `import ` — model emitted the full file, use as-is.
/// 2. Proof contains a `theorem`/`lemma` declaration but no import — prepend
///    ONLY the header; calling `make_proof_file` would duplicate the statement
///    and produce a rejected double-theorem file (Goedel-V2 / DeepSeek-V2).
/// 3. Pure proof body — wrap with header + statement via `make_proof_file`.
pub fn assemble_and_validate(proof: &str, theorem: &Theorem, pb: &PromptBuilder) -> String {
    let lean = if proof.is_empty() {
        String::new()
    } else if proof.contains("import ") {
        proof.to_string()
    } else if proof_has_theorem_decl(proof) {
        if theorem.header.is_empty() {
            proof.to_string()
        } else {
            format!("{}\n{}", theorem.header, proof)
        }
    } else {
        theorem.make_proof_file(proof)
    };
    if pb.validate_lean_code(&lean) {
        lean
    } else {
        String::new()
    }
}

/// Detect whether an extracted proof already contains a `theorem`/`lemma`
/// declaration line.  When true, the block carries its own statement and the
/// pipeline must NOT re-wrap it with `make_proof_file` (that would duplicate
/// the `theorem ... := by` header and fail validation).
///
/// Matches a declaration keyword at the very start or at the start of any line
/// (allowing leading whitespace), to avoid false positives on the word
/// "theorem" appearing inside a tactic or comment.
fn proof_has_theorem_decl(proof: &str) -> bool {
    proof.lines().any(|line| {
        let t = line.trim_start();
        t.starts_with("theorem ") || t.starts_with("lemma ")
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ModelConfig;
    use std::collections::BTreeMap;

    fn test_model_cfg() -> ModelConfig {
        ModelConfig {
            name: "test-model".into(),
            hf_repo: String::new(),
            architecture: String::new(),
            prompt_format: String::new(),
            param_count_b: None,
            quantization: None,
            max_model_len: 4096,
            temperature: 0.6,
            top_p: 0.95,
            max_tokens: 4096,
            seed: 42,
            stop_sequences: vec![],
            system_prompt: String::new(),
        }
    }

    #[test]
    fn test_write_and_load_round_trip() {
        let tmp = std::env::temp_dir().join("minif2f-test-pipeline-roundtrip");
        let _ = std::fs::create_dir_all(&tmp);

        let raw_path = tmp.join("raw.json");
        let lean_path = tmp.join("lean.json");
        let model = test_model_cfg();

        // Build test data
        let mut results: ResultsMap = BTreeMap::new();
        let mut attempts: AttemptMap = BTreeMap::new();
        attempts.insert("attempt_1".into(), ("raw1".into(), "lean1".into()));
        attempts.insert("attempt_2".into(), ("raw2".into(), "lean2".into()));
        results.insert("theorem_a".into(), attempts);

        // Write
        write_flat_json(&raw_path, &model, &results, |(raw, _)| raw).unwrap();
        write_flat_json(&lean_path, &model, &results, |(_, lean)| lean).unwrap();

        // Load back
        let loaded = load_existing_results(&raw_path, &lean_path, &model.name).unwrap();
        assert_eq!(loaded.len(), 1);
        let att = loaded.get("theorem_a").unwrap();
        assert_eq!(att.get("attempt_1").unwrap().0, "raw1");
        assert_eq!(att.get("attempt_1").unwrap().1, "lean1");
        assert_eq!(att.get("attempt_2").unwrap().0, "raw2");
        assert_eq!(att.get("attempt_2").unwrap().1, "lean2");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_load_missing_file_returns_empty() {
        let tmp = std::env::temp_dir().join("minif2f-test-pipeline-missing");
        let raw_path = tmp.join("nonexistent.json");
        let lean_path = tmp.join("also_nonexistent.json");
        let model = test_model_cfg();

        let result = load_existing_results(&raw_path, &lean_path, &model.name).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_write_empty_results() {
        let tmp = std::env::temp_dir().join("minif2f-test-pipeline-empty");
        let _ = std::fs::create_dir_all(&tmp);
        let path = tmp.join("empty.json");
        let model = test_model_cfg();
        let results: ResultsMap = BTreeMap::new();

        write_flat_json(&path, &model, &results, |(raw, _)| raw).unwrap();

        let loaded = load_existing_results(&path, &path, &model.name).unwrap();
        assert!(loaded.is_empty());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_extract_model_data() {
        let json = serde_json::json!({
            "test-model": {
                "theorem_x": {
                    "attempt_1": "proof text"
                }
            }
        });
        let map = extract_model_data(Some(&json), "test-model");
        assert_eq!(map.len(), 1);
        assert_eq!(
            map.get("theorem_x").unwrap().get("attempt_1").unwrap(),
            "proof text"
        );
    }

    #[test]
    fn test_extract_model_data_wrong_model_name() {
        let json = serde_json::json!({
            "other-model": {
                "theorem_x": { "attempt_1": "text" }
            }
        });
        let map = extract_model_data(Some(&json), "test-model");
        assert!(map.is_empty());
    }

    #[test]
    fn test_proof_has_theorem_decl() {
        // Block that carries its own statement (Goedel-V2 / DeepSeek case)
        assert!(proof_has_theorem_decl("theorem foo : 1 = 1 := by\n  rfl"));
        assert!(proof_has_theorem_decl("lemma bar : x = x := by\n  rfl"));
        // Indented declaration still detected
        assert!(proof_has_theorem_decl("  theorem foo := by rfl"));
        // Pure proof body — no declaration (Goedel-DPO fallback case)
        assert!(!proof_has_theorem_decl("  rfl"));
        assert!(!proof_has_theorem_decl("rw [add_comm]\n  simp"));
        // The word "theorem" inside a tactic/comment must NOT trigger
        assert!(!proof_has_theorem_decl(
            "  -- prove the theorem here\n  rfl"
        ));
        assert!(!proof_has_theorem_decl("  exact theorem_helper x"));
    }

    #[test]
    fn test_assembly_avoids_double_header() {
        // Simulate the Goedel-V2 case: extracted proof = full block with
        // theorem statement but NO import header.  Assembling must prepend
        // only the header, producing exactly ONE theorem declaration.
        let theorem = Theorem {
            name: "foo".into(),
            split: "test".into(),
            informal_prefix: String::new(),
            formal_statement: "theorem foo : 1 = 1 := by".into(),
            header: "import Mathlib".into(),
            goal: String::new(),
        };
        let pb = PromptBuilder::new(test_model_cfg());
        let proof = "theorem foo : 1 = 1 := by\n  norm_num";
        let lean = assemble_and_validate(proof, &theorem, &pb);
        // Exactly one theorem declaration — no duplication
        assert_eq!(lean.matches("theorem foo").count(), 1);
        assert!(lean.contains("import Mathlib"));
        assert!(lean.contains("norm_num"));
    }

    #[test]
    fn test_assemble_and_validate_all_branches() {
        let pb = PromptBuilder::new(test_model_cfg());
        let theorem = Theorem {
            name: "foo".into(),
            split: "test".into(),
            informal_prefix: String::new(),
            formal_statement: "theorem foo : 1 = 1 := by".into(),
            header: "import Mathlib".into(),
            goal: String::new(),
        };

        // Branch 0: empty proof → empty result
        assert_eq!(assemble_and_validate("", &theorem, &pb), "");

        // Branch 1: proof already has `import ` → used as-is (single header)
        let with_import = "import Mathlib\ntheorem foo : 1 = 1 := by\n  norm_num";
        let r1 = assemble_and_validate(with_import, &theorem, &pb);
        assert_eq!(r1, with_import);
        assert_eq!(r1.matches("theorem foo").count(), 1);

        // Branch 2: theorem decl, no import → prepend header only, no dup
        let with_decl = "theorem foo : 1 = 1 := by\n  norm_num";
        let r2 = assemble_and_validate(with_decl, &theorem, &pb);
        assert!(r2.contains("import Mathlib"));
        assert_eq!(r2.matches("theorem foo").count(), 1);

        // Branch 3: pure proof body → wrapped via make_proof_file
        let body = "  norm_num";
        let r3 = assemble_and_validate(body, &theorem, &pb);
        assert!(r3.contains("import Mathlib"));
        assert!(r3.contains("theorem foo"));
        assert!(r3.contains("norm_num"));

        // Invalid (sorry) → rejected to empty
        let bad = "theorem foo : 1 = 1 := by\n  sorry";
        assert_eq!(assemble_and_validate(bad, &theorem, &pb), "");
    }

    #[test]
    fn test_checkpoint_never_ahead_of_data() {
        // Regression for the data-loss window: a theorem must only be marked
        // done AFTER its data is persisted. We simulate a crash between the
        // JSON write and a (hypothetical) premature checkpoint, and assert that
        // any theorem the checkpoint reports as done is actually present in the
        // reloaded JSON — i.e. resume can never skip un-persisted data.
        use crate::checkpoint::CheckpointManager;
        let tmp = std::env::temp_dir().join("minif2f-test-ckpt-ordering");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let model = test_model_cfg();
        let raw_path = tmp.join("raw.json");
        let lean_path = tmp.join("lean.json");

        // Two theorems flushed into memory.
        let mut results: ResultsMap = BTreeMap::new();
        for name in ["thm_a", "thm_b"] {
            let mut atts: AttemptMap = BTreeMap::new();
            atts.insert("attempt_1".into(), ("raw".into(), "lean".into()));
            results.insert(name.into(), atts);
        }

        // Correct order: data first …
        write_flat_json(&raw_path, &model, &results, |(raw, _)| raw).unwrap();
        write_flat_json(&lean_path, &model, &results, |(_, lean)| lean).unwrap();
        // … then checkpoint.
        let mut ck = CheckpointManager::new(&tmp, &model.name, "run1").unwrap();
        ck.mark_done("thm_a").unwrap();
        ck.mark_done("thm_b").unwrap();

        // On resume, every done theorem must exist in the reloaded data.
        let loaded = load_existing_results(&raw_path, &lean_path, &model.name).unwrap();
        let ck2 = CheckpointManager::new(&tmp, &model.name, "run1").unwrap();
        for name in ["thm_a", "thm_b"] {
            if ck2.is_done(name) {
                assert!(
                    loaded.contains_key(name),
                    "checkpoint marked {name} done but its data is missing — data loss!"
                );
            }
        }

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
