# minif2f — LLM Theorem Proof Generator

Generate 128 proof attempts for each of [miniF2F](https://github.com/openai/miniF2F)'s 488 theorems using 6 Lean 4 theorem-proving LLMs. Output is two flat JSON files per model: raw output + extracted Lean code.

**Stack**: Rust orchestrator + vLLM (Python, managed via `uv` venv) for GPU inference. FP8 quantization for models.

**Models**: [Goedel-Prover-DPO](https://huggingface.co/Goedel-LM/Goedel-Prover-DPO) · [Kimina-Prover-RL-1.7B](https://huggingface.co/AI-MO/Kimina-Prover-RL-1.7B) · [Goedel-Prover-V2-8B](https://huggingface.co/Goedel-LM/Goedel-Prover-V2-8B) · [DeepSeek-Prover-V2-7B](https://huggingface.co/deepseek-ai/DeepSeek-Prover-V2-7B) · [Kimina-Prover-Distill-8B](https://huggingface.co/AI-MO/Kimina-Prover-Distill-8B) · [STP_model_Lean](https://huggingface.co/kfdong/STP_model_Lean)

## Quick Start

```bash
# New machine: one-command setup
./scripts/setup.sh

# Everything: setup → quality → build → generate all 6 models
./run  → 8) Do It All

# Or step by step:
./run              # Interactive menu
```

## Menu

```
1) Setup Environment
2) List Models
3) Quality Gates (fmt + clippy + test)
4) Build Project
5) Check Status
6) Generate Proofs (single model)
7) Generate All Models (tmux background, sequential, 128 attempts)
8) Do It All (setup → quality → build → generate all)
```

## Commands

```bash
cargo run -- list-models
cargo run -- generate -m <model> -p data/models/<name>               # defaults: -n 128 --parallel 8
cargo run -- generate -m <model> -p data/models/<name> -n 64 --parallel 12 # custom
cargo run -- status --run-id <id>
```

### Generate Options

```
-m, --model <NAME>       Model name (required)
-p, --model-path <PATH>  Path to model directory (required)
--port <PORT>           vLLM server port [default: 8080]
--run-id <ID>           Checkpoint ID [default: default]
-n, --attempts <N>      Attempts per theorem [default: 128]
--parallel <N>          vLLM --max-num-seqs (continuous batching) [default: 8]
```

## Output

Two directories with flat JSON:

```
output/
├── raw_output/
│   └── <model>.json    # unfiltered model completions
└── lean_code/
    └── <model>.json    # extracted + assembled Lean proofs
```

```json
{
  "kimina-prover-rl-1.7b": {
    "amc12a_2019_p21": {
      "attempt_1": "import Mathlib\n...",
      "attempt_128": "..."
    }
  }
}
```

- **raw_output**: unfiltered — exactly what the model generated
- **lean_code**: `extract_proof()` → `make_proof_file()` → `validate_lean_code()` assembled code
  - Header + statement from `data/raw/minif2f.jsonl`; proof body from model output
  - Empty string if extraction failed OR validation rejected the proof

## Project Structure

```
├── run                    # Entry point (interactive menu)
├── scripts/
│   ├── setup.sh           # One-time deployment
│   └── generate-all.sh    # Sequential generation (tmux, single slot, 6 models)
└── src/
    ├── main.rs            # CLI (clap)
    ├── lib.rs             # Modules
    ├── config.rs          # ModelConfig, PipelineConfig
    ├── models.rs          # 6-model registry
    ├── data.rs            # Dataset + Theorem
    ├── prompts.rs         # Chat templates + proof extraction
    ├── inference.rs       # vLLM server manager
    ├── checkpoint.rs      # Crash recovery
    └── pipeline.rs        # Orchestrator → two-layer JSON
```

## Supported Models

| Model | HF Repo | Arch | ctx | max_tok | temp | top_p | seed |
|-------|---------|------|-----|---------|------|-------|------|
| kimina-prover-rl-1.7b | [AI-MO/Kimina-Prover-RL-1.7B](https://huggingface.co/AI-MO/Kimina-Prover-RL-1.7B) | Qwen3/ChatML | 40960 | 8096 | 0.6 | 0.95 | 42 |
| kimina-prover-distill-8b | [AI-MO/Kimina-Prover-Distill-8B](https://huggingface.co/AI-MO/Kimina-Prover-Distill-8B) | Qwen3/ChatML | 40960 | 8096 | 0.6 | 0.95 | 42 |
| goedel-prover-dpo | [Goedel-LM/Goedel-Prover-DPO](https://huggingface.co/Goedel-LM/Goedel-Prover-DPO) | Raw | 4096 | 2048 | 1.0 | 0.95 | 1 |
| goedel-prover-v2-8b | [Goedel-LM/Goedel-Prover-V2-8B](https://huggingface.co/Goedel-LM/Goedel-Prover-V2-8B) | Qwen3/ChatML | 40960 | 32768 | 0.6 | 0.95 | 30 |
| deepseek-prover-v2-7b | [deepseek-ai/DeepSeek-Prover-V2-7B](https://huggingface.co/deepseek-ai/DeepSeek-Prover-V2-7B) | DeepSeek V2 | 65536 | 8192 | 0.6 | 0.95 | 30 |
| stp-model-lean | [kfdong/STP_model_Lean](https://huggingface.co/kfdong/STP_model_Lean) | Raw | 1024 | 1024 | 1.0 | 1.0 | 1 |

## Design

- **Deep thinking (Kimina models)**: Kimina official RL notes require the model output to contain its own `<think>...</think>` reasoning block before the Lean code block. Do not prepopulate an empty think block. Goedel-V2 is also Qwen3, but its official prompt requirement is a proof plan plus Lean code, not the Kimina format reward.
- **`sorry` placeholder**: Goedel-V2 format includes `sorry` in theorem statement, matching official HF prompt format. Kimina, Simple (Goedel-DPO), and STP formats do NOT include `sorry` — model generates from `:= by`.
- **Goedel-DPO**: Raw completion prompt with an open ```lean4 block, matching the official Goedel-Prover eval script. Sampling is `temperature=1.0`, `top_p=0.95`, `max_tokens=2048`, seed 1.
- **Proof extraction**: Multi-strategy with 8-layer validation — `find` (not `rfind`) preserves nested `have ... := by` blocks. `strip_block_comments()` rejects commentary-only proofs. `validate_lean_code()` ensures complete compilable Lean files.
- **Checkpoint resume**: Loads existing raw_output + lean_code JSON on startup, merges tuples. Previously-completed theorems are not re-generated.
- **Incremental writes**: JSON written every 20 theorems — crash resilience independent of checkpoint system.
- **Two-layer output**: `output/raw_output/` (unfiltered) + `output/lean_code/` (extracted + validated). Same flat JSON format in both.
- **STP model**: Raw architecture (no chat template), DeepSeek Prover format with an open ```lean4 block, no `sorry`, no informal_prefix. `max_model_len=1024`, `top_p=1.0`, seed 1. Matches the official STP eval scripts.
- **128 attempts**: Default. Configurable via `-n`. Used for Pass@k evaluation.
- **Sequential generation**: `generate-all.sh` runs all configured models one at a time on port 8080 with per-model `--parallel` values. Single tmux session.
- **GPU**: RTX 5090 32GB CUDA. KV cache q8_0 shared paged pool — `--parallel` does NOT linearly multiply VRAM.
- **Crash recovery**: `results/checkpoints/<model>__<run_id>.json` — resume with `--run-id`

## Hardware

- **GPU**: RTX 5090 32GB (CUDA) primary. RTX 4060 8GB (Vulkan) for testing.
- **BF16 safetensors → FP8 quantized at load time**: ~7-8 GB VRAM per 7-8B model
- **KV cache**: q8_0 quantization, shared paged pool — `--parallel` does NOT linearly multiply VRAM

## Quality

```bash
cargo fmt --check          ✅
cargo clippy -- -D warnings  ✅
cargo test                 ✅ 36/36
```
