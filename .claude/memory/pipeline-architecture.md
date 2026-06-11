---
name: pipeline-architecture
description: buffer_unordered GPU feeder + rayon parallel CPU extraction → two-layer JSON with checkpoint resume + incremental writes
metadata:
  type: project
---

## Pipeline (src/pipeline.rs, tokio async)

### Architecture: buffer_unordered + rayon parallel extraction

```
┌─────────────────────────────────────────────────────────┐
│ Stage 1: GPU inference (buffer_unordered)               │
│                                                         │
│ stream::iter(all_jobs)                                  │
│   .buffer_unordered(N)   ← N = --parallel               │
│   │                                                     │
│   ├─ HTTP POST → vLLM /v1/completions                  │
│   ├─ N requests in flight (continuous batching)        │
│   ├─ GPU saturated (~90%+)                             │
│   └─ Results in completion order                       │
│                                                         │
├─────────────────────────────────────────────────────────┤
│ Stage 2: Per-theorem batch accumulation                │
│                                                         │
│ BTreeMap<name, Vec<(idx, text)>>                       │
│   └─ Batch reaches 128 → flush                         │
│                                                         │
├─────────────────────────────────────────────────────────┤
│ Stage 3: Parallel extraction (rayon)                    │
│                                                         │
│ rayon::par_iter():                                      │
│   extract_proof() → make_proof_file() → validate_lean_code() │
│                                                         │
├─────────────────────────────────────────────────────────┤
│ Stage 4: Sequential insert + checkpoint + JSON write    │
│                                                         │
│ BTreeMap insert → Checkpoint mark_done()                │
│   → Incremental JSON write (every 20 theorems)          │
└─────────────────────────────────────────────────────────┘
```

### Key Design Decisions

1. **Continuous request pool** — NO per-theorem barrier. buffer_unordered keeps N HTTP requests in flight. GPU stays at ~90%+ utilization.
2. **Rayon parallel extraction** — CPU-bound proof extraction splits across all cores, async loop stays responsive.
3. **Multi-strategy proof extraction** — handles 5 different model output formats.
4. **Incremental JSON writes** — every 20 theorems, crash resilience independent of checkpoint system.
5. **`find` not `rfind`** — preserves nested `have ... := by` blocks.

### Output Structure

```
output/
├── raw_output/<model>.json    # unfiltered completions
└── lean_code/<model>.json     # extracted + validated proofs ("" if invalid)

results/
└── checkpoints/<model>__<run_id>.json  # HashSet of theorem names
```

### vLLM Server Management (src/inference.rs)

```
InferenceEngine::start() → spawns vLLM via `uv run`
  │
  ├─ Loads HF safetensors → GPU with FP8 quantization
  ├─ Waits /health (up to 2 min timeout)
  └─ Returns engine handle
```

API: `POST /v1/completions` with JSON body:
```json
{"prompt": "...", "n_predict": max_tokens, "temperature": T, "top_p": P, "seed": S, "stop": [...], "n_probs": 0}
```

### Per-Slot Context (vLLM --max-model-len)

`max_model_len = (config.max_tokens + 4096).min(config.max_model_len)`

| Model | max_tok | max_model_len | per_seq |
|-------|---------|---------------|---------|
| kimina-prover-rl-1.7b | 8096 | 40960 | 12,192 |
| goedel-prover-v2-8b | 32768 | 40960 | 36,864 |
| deepseek-prover-v2-7b | 8192 | 65536 | 12,288 |
| kimina-prover-distill-8b | 8096 | 40960 | 12,192 |
| goedel-prover-dpo | 2048 | 4096 | 6,144 |
| stp-model-lean | 1024 | 1024 | 5,120 |
