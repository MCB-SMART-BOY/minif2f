---
name: industrialization-blueprint
description: Complete future architecture design — provenance, config, errors, logging, backend trait, CI/CD
layer: reference
metadata:
  type: reference
---

# 07 — Industrialization Blueprint

This file is the canonical reference for the future architecture. Implementation is phased; details below.

## Phase 2: Provenance (next)

Embed `_metadata` in every output JSON with: schema_version, run (id, status, timestamps), model (name, hf_repo, hf_commit), inference (backend, version, quantization, params), code (git version, dirty), hardware (gpu, vram), dataset (source, theorems, file_sha256), output (sizes, rates, encoding stats).

**Schema**: See `ARCHITECTURE.md` > Output Schema (v2). Backward compatible — adds `_metadata` field, old tools ignore it.

**Collection points**: git rev-parse + diff-stat, nvidia-smi, model.safetensors.index.json for hf_commit, config SHA256.

## Phase 3: Config YAML

Model configs move from `src/models.rs` to `configs/models/<name>.yaml`. Three-layer merge: `defaults.yaml` ← `hardware.yaml` overrides ← model spec.

Each model YAML contains: name, hf_repo, backend (type, quantization), architecture (type, tokenizer), prompt (format, template, include_sorry, code_block), inference (all sampling params), validation (thresholds), sources (URLs).

**Validation rules**: temperature ∈ [0,2], top_p ∈ [0,1], max_model_len ≥ max_tokens, architecture + tokenizer combo must be valid, model files must exist on disk.

## Phase 4: Errors + Logging

**Error types**: `PipelineError` enum with `Environment` (no retry), `Transient` (retry 3x exponential backoff), `DataError` (skip attempt), `ModelError` (skip model). Each variant carries context for debugging.

**Logging**: JSON-line format. Typed events: `PipelineStart/Complete`, `ModelStart/Complete/Skip`, `BackendStart/Ready/Stop`, `TheoremDone/Skip`, `RequestSent/Done/Retry/Failed`, `EncodingCorrupt`, `ExtractionFailed`, `ValidationRejected`, `CheckpointWrite`, `GpuMetrics`, `HealthCheck`. Events written to `results/logs/<run_id>.jsonl`.

## Phase 5: CI/CD

GitHub Actions: `quality.yml` (fmt + clippy + test on push/PR), `smoke.yml` (single-theorem × 3 attempts on self-hosted GPU runner, verifies output, only on PR touching src/ or configs/).

## Phase 6: Backend Trait

`InferenceBackend` trait with `start()`, `generate()`, `stop()`, `health_check()`, `recommended_concurrency()`, `architecture()`. Implementations: `VllmBackend` (HTTP to vLLM), `HfGenerateBackend` (stdin/stdout to Python worker). Pipeline uses `Box<dyn InferenceBackend>` — unaware of backend type.

## Target Directory Structure

```
configs/models/<name>.yaml      # Per-model config
configs/prompts/<format>.txt    # Prompt templates
src/backend/{mod,vllm,hf_generate}.rs
src/config/{mod,model,pipeline,validation}.rs
src/prompts/{mod,extraction}.rs
src/pipeline/{mod,checkpoint,flush}.rs
src/provenance/{mod,collect,write}.rs
src/logging/{mod,events}.rs
src/errors.rs
output/provenance/<run_id>/manifest.json
results/logs/<run_id>.jsonl
results/reports/<run_id>/{summary,encoding,failures}.json
.github/workflows/{quality,smoke}.yml
tests/integration/{single_generate,pipeline_smoke}.rs
```

## Development Workflow (target)

```
git push → CI (fmt+clippy+test) → PR review → merge
  → pre-generate hooks → run pipeline → auto-verify each model
  → provenance written → report generated → output ready
```

## State Machine

```
IDLE → PLANNING → PREFLIGHT → RUNNING → VERIFYING → COMPLETE
                    ↓            ↓  ↕        ↓
                  ABORTED      FAILED    PAUSED
                                  ↓
                              PLANNING (fix + re-plan)
```

## Run Lifecycle (8 steps)

1. INIT: run_id, manifest.partial, structured log
2. PREFLIGHT: GPU free, port free, model exists, config valid
3. BACKEND START: vLLM or HF generate, health check
4. GENERATE: buffer_unordered loop, checkpoint, incremental write
5. BACKEND STOP: free GPU
6. VERIFY: 5-level check → PASS/WARN/ERROR/FATAL
7. PROVENANCE: finalize manifest, embed _metadata
8. NEXT MODEL (if any) → step 2, else COMPLETE

Crash: manifest.partial + checkpoint preserved. Resume: load + continue, parent_run_id tracks chain.
