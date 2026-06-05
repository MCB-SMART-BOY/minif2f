# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Generate 128 proof attempts per theorem for [miniF2F](https://github.com/openai/miniF2F) (488 theorems) using 6 Lean 4 theorem-proving LLMs. Output is a nested JSON file per model.

**Stack**: Pure Rust. `llama-server` (C binary, child process) for GPU inference. Zero Python at runtime.

## Commands

```bash
# Quality gates (ALL must pass)
cargo fmt --check          # formatting
cargo clippy -- -D warnings  # lint (0 warnings)
cargo test                 # unit tests (23/23)

# Build
cargo build                # debug
cargo build --release      # optimized

# CLI
cargo run -- list-models
cargo run -- generate -m <model> -p <gguf>                      # defaults: -n 128 --parallel 8
cargo run -- generate -m <model> -p <gguf> -n 64 --parallel 12 # custom
cargo run -- status --run-id v1

# Scripts
./run                       # Interactive menu (8 options)
./scripts/setup.sh          # One-time deployment
./scripts/generate-all.sh   # Sequential generation (tmux, 6 models)
```

## Architecture

```
CLI (clap derive) → EvaluationPipeline::run() (tokio async)
  │
  ├─ 0. load_existing_results() → populate BTreeMap from prior output JSON
  │      Enables checkpoint resume without data loss
  │
  ├─ 1. InferenceEngine::start() → spawns llama-server
  │      loads GGUF → GPU, waits /health
  │      args: -ngl 99 --ctx-size <n> --parallel <n> --no-warmup
  │
  ├─ 2. For each theorem (488, skip if checkpoint done):
  │      PromptBuilder::build() → arch-specific chat template + user prompt
  │      generate_stream(prompt, 128, 0) → FuturesUnordered stream
  │        Results arrive as each completion finishes — no barrier
  │        llama-server processes --parallel slots in background
  │      extract_proof() → multi-strategy + has_proof_body validation
  │      Collect into BTreeMap
  │
  ├─ 3. engine.stop() → kills llama-server, frees GPU
  │
  └─ 4. Write nested JSON → output/<model>.json
       Includes prior results + newly generated theorems
```

## Script System

```
run                          → Interactive menu (8 options)
scripts/setup.sh             → One-time deployment
scripts/generate-all.sh      → Sequential generation (tmux, 6 models, one at a time)
```

`generate-all.sh` runs models sequentially: loads one → generates 488×128 → unloads → next starts. All in a single tmux window that survives detach. Same port (8080) for every model since only one runs at a time.

## Source Map (9 files, ~500 LOC)

| File | Purpose | Tests |
|------|---------|-------|
| `main.rs` | CLI: `generate`, `list-models`, `report`, `status` | 0 |
| `lib.rs` | Module declarations | 0 |
| `config.rs` | `ModelConfig` (serde), `PipelineConfig` | 0 |
| `models.rs` | 6-model registry with per-model settings | 6 |
| `data.rs` | `Theorem` struct, JSONL loader, `make_proof_file()` | 3 |
| `prompts.rs` | Chat templates + user prompts + proof extraction | 8 |
| `inference.rs` | `InferenceEngine`: llama-server lifecycle, HTTP `/completion` | 0 |
| `checkpoint.rs` | Atomic JSON-set crash recovery | 4 |
| `pipeline.rs` | Async orchestrator → nested JSON output | 0 |

## Data Flow

```
data/raw/minif2f.jsonl (488 theorems)
  → Theorem { name, split, header, informal_prefix, formal_statement }
  → prompts.rs: arch-specific chat template + format-specific user prompt
  → llama-server GPU inference (~25s/theorem)
  → extract_proof(): find ```lean4 block after </think>
  → Collect into BTreeMap<model, BTreeMap<theorem, BTreeMap<attempt, proof>>>
  → serde_json::to_string_pretty → output/<model>.json
```

## Output Structure

```
output/
├── kimina-prover-rl-1.7b.json
├── goedel-prover-dpo.json
└── ...

results/
└── checkpoints/
    └── <model>__<run_id>.json
```

JSON format:
```json
{
  "kimina-prover-rl-1.7b": {
    "amc12a_2019_p21": {
      "attempt_1": "import Mathlib\n...",
      "...": "...",
      "attempt_128": "..."
    }
  }
}
```

## Chat Templates (per architecture)

**Qwen3** (kimina, goedel-v2, distill): `<|im_start|>` ChatML.
Model generates `<think>` block naturally (RL-trained format reward).
Do NOT prepopulate an empty think block — that breaks the reasoning chain.
```
<|im_start|>system
{system_prompt}<|im_end|>
<|im_start|>user
{user_prompt}<|im_end|>
<|im_start|>assistant
```

**DeepSeek V2** (deepseek-prover): Unicode fullwidth `｜` (U+FF5C)
```
<｜begin▁of▁sentence｜>{system}<｜User｜>{user}<｜Assistant｜>
```

**DeepSeek Coder** (goedel-dpo, stp): `### Instruction:` / `### Response:`
```
{system}### Instruction:
{user}
### Response:
```

## Proof Extraction

Multi-strategy with validation:

1. Find ```lean4``` block after `</think>` → validate has_proof_body
2. Fallback: any ```lean4``` block in raw text → validate has_proof_body
3. Fallback: extract Lean tactics from raw text (indented lines after `:= by`)
4. Last resort: strip think blocks + chat tokens + markdown commentary

`has_proof_body()`: strips theorem header, checks ≥2 chars of proof content remain.
Markdown commentary lines (`# `, `## `, `**`) are stripped from extracted proofs.

## Prompt Formats (per model)

| Format | Used by | Content |
|--------|---------|---------|
| `kimina` | kimina-prover-rl-1.7b, kimina-prover-distill-8b | "Think about and solve the following problem step by step..." |
| `goedel_v2` | goedel-prover-v2-8b, deepseek-prover-v2-7b | "Complete the following Lean 4 code..." (includes `sorry` placeholder) |
| `simple` | goedel-prover-dpo, stp-model-lean | "This is a theorem written in Lean 4..." (includes `sorry` placeholder) |

## 6 Models

| CLI Name | Arch | Chat | Prompt | ctx | max_tok | seed |
|----------|------|------|--------|-----|---------|------|
| `kimina-prover-rl-1.7b` | qwen3 | ChatML | kimina | 8192 | 8192 | 42 |
| `goedel-prover-dpo` | deepseek_coder | ### | simple | 4096 | 4096 | 42 |
| `goedel-prover-v2-8b` | qwen3 | ChatML | goedel_v2 | 8192 | 8192 | 30 |
| `deepseek-prover-v2-7b` | deepseek_v2 | Unicode ｜ | goedel_v2 | 8192 | 8192 | 30 |
| `kimina-prover-distill-8b` | qwen3 | ChatML | kimina | 8192 | 8192 | 42 |
| `stp-model-lean` | deepseek_coder | ### | simple | 2048 | 2048 | 42 |

All 6 models converted to GGUF. ✅ Ready on 5090 server.

## Checkpointing

`results/checkpoints/<model>__<run_id>.json`. Atomic write (tmp → rename). Resume with same `--run-id`.

Checkpoint resume preserves prior results: on startup, existing output JSON is loaded
and merged with new results. Previously-completed theorems are not re-generated and
their proofs are not lost.

## Hardware

- **GPU**: RTX 4060 8GB (Vulkan, `--parallel 2`) or RTX 5090 32GB (CUDA, `--parallel 8`)
- **1.7B FP16**: ~3.2 GB. **7-8B**: Q4_K_M GGUF ~4-5 GB
- llama-server: `-ngl 99 --parallel <n> --no-warmup`
- Each slot: ~2-3GB KV cache (ctx-size dependent)

## Model Conversion (one-time per model)

```bash
source tools/venv/bin/activate.fish
export HF_TOKEN="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"

python tools/llama.cpp/convert_hf_to_gguf.py data/models/<name> \
  --outfile models/<name>.gguf --outtype f16     # 1.7B
  --outfile models/<name>.gguf --outtype q4_k_m  # 7-8B
```
