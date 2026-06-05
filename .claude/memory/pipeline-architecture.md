---
name: pipeline-architecture
description: Rust async pipeline — generate → validate → collect → JSON with checkpoint resume
metadata:
  type: project
---

## Pipeline (src/pipeline.rs, tokio async)

```
EvaluationPipeline::run(model_cfg, model_path)
  │
  ├─ 1. load_all() → 488 Theorem from data/raw/minif2f.jsonl
  │     Propagates errors if both splits fail (no silent empty returns)
  │
  ├─ 2. load_existing_results() → populate BTreeMap from prior output JSON
  │     Enables checkpoint resume without data loss
  │
  ├─ 3. InferenceEngine::start() → spawn llama-server, wait /health
  │
  ├─ 4. For each theorem (not in checkpoint):
  │      PromptBuilder::build() → arch-specific chat template + format-specific user prompt
  │      Qwen3: model generates <think> naturally (no prepopulated empty block)
  │      Goedel-V2/Simple: includes `sorry` placeholder
  │      For each attempt (default 128):
  │        generate_batch_retry(prompt, 128, 0) → GPU inference
  │        extract_proof() → multi-strategy with validation:
  │          1. Find ```lean4 after </think>, validate has_proof_body
  │          2. Fallback: any fenced code, validate has_proof_body
  │          3. Fallback: extract_lean_from_text (indented tactics)
  │          4. Last resort: strip noise + markdown
  │      Collect into BTreeMap<theorem, BTreeMap<attempt, proof>>
  │
  ├─ 5. engine.stop() → kill llama-server, free GPU
  │
  └─ 6. Write nested JSON → output/<model>.json
       Includes both prior results + newly generated theorems
```

## Chat Templates (post-audit)

| Architecture | Format | Models |
|-------------|--------|--------|
| qwen3 | ChatML (model generates `<think>` naturally) | kimina, goedel-v2, distill |
| deepseek_v2 | Unicode ｜ (U+FF5C) | deepseek-prover-v2 |
| deepseek_coder | ### Instruction: / ### Response: | goedel-dpo, stp |

## Proof Extraction (post-audit)

Multi-strategy with validation:
1. Fenced code after `</think>` → `has_proof_body()` (≥2 chars after `:= by`)
2. Any fenced code → `has_proof_body()`
3. Raw text Lean extraction (indented tactics after `:= by`)
4. Fallback: strip noise, chat tokens, markdown

Markdown commentary lines (`# `, `## `, `**`) are stripped from extracted proofs.

## Script System

```
run                          → Interactive menu (8 options)
scripts/setup.sh             → One-time deployment
scripts/generate-all.sh      → Batch generation (tmux, 2 parallel slots, ports 8080/8081)
```

## 9 Source Files (~500 LOC)

`main` `lib` `config` `models` `data` `prompts` `inference` `checkpoint` `pipeline`
