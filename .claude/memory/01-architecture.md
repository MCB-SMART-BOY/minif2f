---
name: architecture
description: Code structure — file map, data flow, module dependencies
layer: reference
metadata:
  type: project
---

# 01 — Code Architecture

## File Map (9 files, ~650 LOC)

| File | Purpose | Key Functions |
|------|---------|---------------|
| `main.rs` | CLI (clap derive) | `list-models`, `generate`, `status` |
| `lib.rs` | Module declarations | — |
| `config.rs` | `ModelConfig` + `PipelineConfig` | serde structs + path helpers |
| `models.rs` | 6-model registry | `builtin_models()`, `find_model()` |
| `data.rs` | Dataset + `Theorem` struct | `load_all()`, `make_proof_file()` |
| `prompts.rs` | Prompt building + proof extraction | `PromptBuilder::build()`, `extract_proof()`, `validate_lean_code()` |
| `inference.rs` | vLLM server lifecycle + HTTP client | `InferenceEngine::start()`, `generate_one_with_retry()`, `decode_llama_byte_fallback()` |
| `checkpoint.rs` | Crash recovery | `CheckpointManager::mark_done()` (atomic write) |
| `pipeline.rs` | Orchestrator | `EvaluationPipeline::run()` → buffer_unordered + rayon extraction |

## Data Flow

```
data/raw/minif2f.jsonl (488 theorems)
  → Theorem { name, header, informal_prefix, formal_statement }
  → PromptBuilder::build(theorem) → model-specific prompt
  → stream::iter(all_jobs).buffer_unordered(N) → POST /v1/completions
  → vLLM GPU inference (FP8, continuous batching)
  → Per-theorem batch → rayon::par_iter():
       extract_proof() → make_proof_file() → validate_lean_code()
  → ResultsMap: { theorem → { attempt_N → (raw_output, lean_code) } }
  → Write output/raw_output/<model>.json + output/lean_code/<model>.json
```

## Module Dependencies

```
main.rs → config.rs + models.rs + pipeline.rs
pipeline.rs → data.rs + inference.rs + prompts.rs + checkpoint.rs
prompts.rs → config.rs + data.rs
inference.rs → config.rs
```

## Key Architectural Decisions

See [[06-decisions]] for ADR entries. Summary:

1. **Continuous request pool** (buffer_unordered) over per-theorem barrier → GPU 90%+
2. **Rayon parallel extraction** → CPU work off async runtime
3. **Incremental JSON writes** every 20 theorems → crash resilience
4. **`find` not `rfind`** for theorem header stripping → preserves nested `have ... := by`
5. **Architecture-conditional byte-fallback decoder** → LLaMA only, Qwen3 pass-through

## Chat Templates

| Architecture | Format | Used by |
|-------------|--------|---------|
| `qwen3` | `<\|im_start\|>` ChatML | kimina, goedel-v2, distill |
| `deepseek_v2` | Unicode `｜` (U+FF5C) | deepseek-prover-v2 |
| `raw` | None (bare message) | goedel-prover-dpo, stp-model-lean |
