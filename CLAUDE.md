# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Generate 128 proof attempts per theorem for [miniF2F](https://github.com/openai/miniF2F) (488 theorems) using 6 Lean 4 theorem-proving LLMs. Output is two flat JSON files per model: raw output + extracted Lean code.

**Stack**: Rust orchestrator + vLLM (Python, managed via `uv` venv) for GPU inference. FP8 quantization for models.

## Commands

```bash
# Quality gates (ALL must pass)
cargo fmt --check          # formatting
cargo clippy -- -D warnings  # lint (0 warnings)
cargo test                 # unit tests (36/36)

# Build
cargo build                # debug
cargo build --release      # optimized

# CLI
cargo run -- list-models
cargo run -- generate -m <model> -p data/models/<name>               # defaults: -n 128 --parallel 8
cargo run -- generate -m <model> -p data/models/<name> -n 64 --parallel 12 # custom
cargo run -- status --run-id v1

# Scripts
./run                       # Interactive menu (8 options)
./scripts/setup.sh          # One-time deployment
./scripts/generate-all.sh   # Sequential generation (tmux, 6 models, one at a time)
```

## Architecture

```
CLI (clap derive) → EvaluationPipeline::run() (tokio async)
  │
  ├─ 0. load_existing_results() → populate ResultsMap from prior JSON
  │      Merges raw_output + lean_code tuples from previous runs
  │
  ├─ 1. InferenceEngine::start() → spawns vLLM via `uv run`
  │      loads HF safetensors → GPU with FP8 quantization, waits /health
  │      args: --quantization fp8 --max-model-len <n> --max-num-seqs <n>
  │      --gpu-memory-utilization 0.92 --enforce-eager
  │
  ├─ 2. Continuous request pool (NO per-theorem barrier):
  │      All theorem×attempt jobs → stream::iter → buffer_unordered(N)
  │      N = --parallel (vLLM --max-num-seqs, continuous batching).
  │      When one request completes, the next starts immediately.
  │      Results arrive in completion order → batched per theorem.
  │
  ├─ 3. Per-theorem flush (when configured attempts collected):
  │      rayon::par_iter() → parallel extract_proof() + validate_lean_code()
  │      Sequential BTreeMap insert → checkpoint → incremental JSON write
  │
  ├─ 4. engine.stop() → kills vLLM server, frees GPU
  │
  └─ 5. Write two flat JSON files (every 20 theorems + final):
       output/raw_output/<model>.json  — unfiltered completions
       output/lean_code/<model>.json   — extracted + validated proofs
```

## Source Map (9 files, ~650 LOC)

| File | Purpose | Tests |
|------|---------|-------|
| `main.rs` | CLI: `generate`, `list-models`, `report`, `status` | 0 |
| `lib.rs` | Module declarations | 0 |
| `config.rs` | `ModelConfig` (serde), `PipelineConfig` | 0 |
| `models.rs` | 6-model registry with per-model official specs | 6 |
| `data.rs` | `Theorem` struct, JSONL loader, `make_proof_file()` | 3 |
| `prompts.rs` | Chat templates + 5 prompt formats + proof extraction + validation | 21 |
| `inference.rs` | `InferenceEngine`: vLLM server lifecycle, HTTP `/v1/completions` | 0 |
| `checkpoint.rs` | Atomic JSON-set crash recovery | 4 |
| `pipeline.rs` | Continuous request pool → two-layer JSON output | 0 |

## Data Flow

```
data/raw/minif2f.jsonl (488 theorems)
  → Theorem { name, split, header, informal_prefix, formal_statement }
  → prompts.rs: arch-specific chat template + 4 format-specific user prompts
  → stream::iter(all_jobs).buffer_unordered(N) → HTTP POST /v1/completions
  → vLLM GPU inference (FP8 quantization, continuous batching)
  → Per-theorem batch (configured attempts) → rayon::par_iter():
       extract_proof() → make_proof_file() → validate_lean_code()
  → ResultsMap: { theorem → { attempt_N → (raw_output, lean_code) } }
  → Write output/raw_output/<model>.json every 20 theorems
  → Write output/lean_code/<model>.json  (empty string if invalid)
```

## Output Structure

```
output/
├── raw_output/
│   ├── kimina-prover-rl-1.7b.json    # unfiltered model completions
│   └── ...
└── lean_code/
    ├── kimina-prover-rl-1.7b.json    # extracted + assembled Lean proofs
    └── ...

results/
└── checkpoints/
    └── <model>__<run_id>.json
```

Both use the same flat JSON format:
```json
{
  "<model>": {
    "<theorem>": {
      "attempt_1": "<text>",
      "attempt_128": "<text>"
    }
  }
}
```

- **raw_output**: unfiltered model completions
- **lean_code**: `extract_proof()` → assembled Lean code (empty string if extraction failed)

## Pipeline: Continuous Request Pool

The key architectural decision: **NO per-theorem barrier**. The old architecture
processed theorems sequentially — submit N requests, wait for all N, do CPU
work, then start the next theorem. GPU utilization dropped to 4-8% between theorems.

The current architecture uses **buffer_unordered + rayon parallel extraction**:

```
stream::iter(all_jobs)               Per-theorem accumulation
  .buffer_unordered(N)               BTreeMap<name, Vec<(idx, text)>>
  │                                  │
  ├─ HTTP POST → vLLM /v1/completions  ├─ Batch reaches attempts → flush
  ├─ N requests in flight            │
  ├─ GPU saturated (~90%+)           ├─ rayon::par_iter():
  └─ Results in completion order     │    extract_proof()
                                     │    make_proof_file()
                                     │    validate_lean_code()
                                     │
                                     ├─ Sequential BTreeMap insert
                                     ├─ Checkpoint mark_done()
                                     └─ Incremental JSON write (every 20)
```

Jobs ordered by theorem → per-theorem batch completion preserves ordering.
Rayon parallel extraction keeps CPU work off the async runtime.
GPU utilization stays at ~90%+ with no idle gaps.

## Chat Templates (per architecture)

| Architecture | Format | Used by |
|-------------|--------|---------|
| `qwen3` | `<\|im_start\|>` ChatML | kimina, goedel-v2, distill |
| `deepseek_v2` | Unicode fullwidth `｜` (U+FF5C) | deepseek-prover-v2 |
| `deepseek_coder` | `### Instruction:` / `### Response:` | legacy support |
| `raw` | None (bare message) | goedel-prover-dpo, stp-model-lean |

**Qwen3**: Do NOT prepopulate an empty `<think>` block. Kimina models are expected
to generate their own `<think>...</think>` reasoning before the Lean code block.
Goedel-V2 is Qwen3 too, but its official requirement is a proof plan followed by
Lean code, not the Kimina RL format reward. When `system_prompt` is empty
(Goedel-V2), the system message block is omitted from ChatML — matching the
official `apply_chat_template` behaviour.

**Goedel-DPO**: The `simple` format is a raw completion prompt matching the official
Goedel-Prover eval script: "Complete the following Lean 4 code with explanatory comments..."
plus an open ```lean4 block. No chat wrapper and no `sorry`.

## Proof Extraction & Validation

Multi-strategy, returns proof body only (tactics after `:= by`):

1. Find ` ```lean4 ` block after `</think>` → strip theorem header → validate `has_proof_body`
2. Fallback: any ` ```lean4 ` block → strip header → validate
3. Fallback: extract Lean tactics from raw text (indented lines after `:= by`)
4. Last resort: strip think/chat/markdown → validate `has_proof_body`; return `""` if fails

**Validation** (`validate_lean_code`, 8 checks): has `:= by` → no `sorry` → tactics ≥2 chars
after `:= by` → no markdown/chat artefacts → `is_proof_body()` passes → `strip_block_comments()`
leaves ≥2 chars of real tactics.

Key functions:
- `strip_theorem_header`: **`find` (first)**, not `rfind` — preserves nested `have ... := by`
- `is_proof_body`: detects tactic content vs new theorem/definition/English-prose statements
- `has_proof_body`: ≥2 chars, rejects backtick-only/markdown artefacts
- `strip_block_comments`: removes Lean `/- ... -/` blocks (handles nesting); rejects commentary-only proofs
- `validate_lean_code`: 8-layer validation gate — rejects incomplete/wrong/commentary-only proofs
- `extract_lean_from_text`: checks `line.starts_with(' ')` (before trim) for indented tactics

## Prompt Formats (5 total)

| Format | Used by | Input | `sorry` | Content |
|--------|---------|-------|---------|---------|
| `kimina` | kimina-rl-1.7b, kimina-distill-8b | Chat (system+user) | No | "Think about and solve..." with `# Problem:` and `# Formal statement:` |
| `goedel_v2` | goedel-v2-8b | Chat (user only) | Yes | "Complete the following Lean 4 code:" + proof plan request (CoT) |
| `goedel_v2_nocot` | deepseek-prover-v2-7b | Chat (user only) | Yes | "Complete the following Lean 4 code:" only — no proof plan (non-CoT) |
| `simple` | goedel-prover-dpo | Completion (raw) | No | "Complete... with explanatory comments..." + open code block |
| `deepseek_prover` | stp-model-lean | **Completion** (raw) | **No** | "Complete the following Lean 4 code:" + open code block (from `:= by`, no `informal_prefix`) |

## 6 Models (Official Specs)

| CLI Name | Arch | Base | ctx | max_tok | temp | top_p | seed | Prompt | SysPrompt | HF Repo |
|----------|------|------|-----|---------|------|-------|------|--------|-----------|---------|
| `kimina-prover-rl-1.7b` | qwen3 | Qwen3-1.7B | **40960** | **8096** | 0.6 | 0.95 | 42 | kimina | expert math+Lean4 | [AI-MO/Kimina-Prover-RL-1.7B](https://huggingface.co/AI-MO/Kimina-Prover-RL-1.7B) |
| `goedel-prover-dpo` | **raw** | LLaMA-7B | 4096 | **2048** | 1.0 | 0.95 | 1 | simple | _(none)_ | [Goedel-LM/Goedel-Prover-DPO](https://huggingface.co/Goedel-LM/Goedel-Prover-DPO) |
| `goedel-prover-v2-8b` | qwen3 | Qwen3-8B | **40960** | **32768** | 0.6 | 0.95 | 30 | goedel_v2 | _(none)_ | [Goedel-LM/Goedel-Prover-V2-8B](https://huggingface.co/Goedel-LM/Goedel-Prover-V2-8B) |
| `deepseek-prover-v2-7b` | deepseek_v2 | LLaMA-7B | **65536** | 8192 | 0.6 | 0.95 | 30 | **goedel_v2_nocot** | _(none)_ | [deepseek-ai/DeepSeek-Prover-V2-7B](https://huggingface.co/deepseek-ai/DeepSeek-Prover-V2-7B) |
| `kimina-prover-distill-8b` | qwen3 | Qwen3-8B | **40960** | **8096** | 0.6 | 0.95 | 42 | kimina | expert math+Lean4 | [AI-MO/Kimina-Prover-Distill-8B](https://huggingface.co/AI-MO/Kimina-Prover-Distill-8B) |
| `stp-model-lean` | **raw** | DS-Prover-V1.5 | **1024** | **1024** | 1.0 | 1.0 | 1 | **deepseek_prover** | _(none)_ | [kfdong/STP_model_Lean](https://huggingface.co/kfdong/STP_model_Lean) |

Bold values are sourced from explicit HuggingFace model cards, HuggingFace `config.json` / `tokenizer_config.json`, or official eval scripts. When those sources differ, `ctx` follows the model card if it explicitly sets `max_model_len`; otherwise it follows model `config.json` (`max_position_embeddings`).

### Official sources (HuggingFace model cards):
1. [Goedel-Prover-DPO](https://huggingface.co/Goedel-LM/Goedel-Prover-DPO) — raw completion prompt. Prompt: "Complete the following Lean 4 code with explanatory comments..." + open ```lean4 block. `full_code = extract_code(model_input + model_output)` in the official eval script. EOS=100001, max_tokens=2048, temperature=1.0, top_p=0.95, seed=1.
2. [Kimina-Prover-RL-1.7B](https://huggingface.co/AI-MO/Kimina-Prover-RL-1.7B) — Qwen3 ChatML. System: "expert in mathematics and proving theorems in Lean 4". Prompt: "Think about and solve..." with `# Problem:` and `# Formal statement:`. NO `sorry` — theorem ends with `:= by`. EOS=151645, max_tokens=8096.
3. [Goedel-Prover-V2-8B](https://huggingface.co/Goedel-LM/Goedel-Prover-V2-8B) — Qwen3 ChatML, user message only (NO system prompt). Prompt: "Complete the following Lean 4 code:" + proof plan request. `formal_statement` includes `sorry`. EOS=151645, `max_position_embeddings=40960`, max_new_tokens=32768, seed=30.
4. [DeepSeek-Prover-V2-7B](https://huggingface.co/deepseek-ai/DeepSeek-Prover-V2-7B) — DeepSeek V2 ChatML, user message only (NO system prompt). Uses **non-CoT** prompt (no proof plan request). EOS=100001, 65536 context (config.json), max_new_tokens=8192, seed=30.
5. [Kimina-Prover-Distill-8B](https://huggingface.co/AI-MO/Kimina-Prover-Distill-8B) — Qwen3 ChatML. System: "expert in mathematics and Lean 4". Same prompt as Kimina-RL. NO `sorry`. EOS=151645, max_tokens=8096.
6. [STP_model_Lean](https://huggingface.co/kfdong/STP_model_Lean) — Completion (NOT chat). `max_model_len=1024` (official eval), `max_tokens=1024`, `temperature=1.0`, `top_p=1.0`, seed=1. Prompt: "Complete the following Lean 4 code:" + open ```lean4 block with header+statement (no informal_prefix). `statement = formal_statement.rsplit('sorry', 1)[0].strip()`. EOS=100001.

## Checkpointing

`results/checkpoints/<model>__<run_id>.json`. Atomic write (tmp → rename). Resume with `--run-id`.

Per-theorem checkpoint triggered when all configured attempts complete (via `flush_batch`).
Checkpoint resume loads existing raw_output + lean_code JSON, merges tuples.

**Incremental JSON writes**: Every 20 theorems, both output JSONs are written to disk
independently of checkpoint files. Checkpoints only record theorem names — without
incremental writes, a crash loses all proofs generated since the last complete theorem.

## Hardware

- **GPU**: RTX 5090 32GB (CUDA) primary. RTX 4060 8GB (Vulkan) for testing.
- **Models**: BF16 safetensors → FP8 quantization at load time (~7-8 GB VRAM per 7-8B model)
- vLLM: `--quantization fp8 --max-num-seqs <n> --gpu-memory-utilization 0.92 --enforce-eager`
- vLLM uses **PagedAttention** — KV cache is dynamically managed, more efficient than static slots
- **FP8 quantization** cuts weight VRAM in half vs BF16, leaving ~24 GB for KV cache
- **Per-model --max-num-seqs** (see `scripts/generate-all.sh` for current values): 9–38 depending on model VRAM needs
- vLLM's **continuous batching** eliminates idle slot waste — requests are batched dynamically

## Model Conversion (one-time per model)

```bash
source tools/venv/bin/activate.fish
export HF_TOKEN="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"

python tools/llama.cpp/convert_hf_to_gguf.py data/models/<name> \
  --outfile models/<name>.gguf --outtype f16     # 1.7B
  --outfile models/<name>.gguf --outtype q4_k_m  # 7-8B
```
