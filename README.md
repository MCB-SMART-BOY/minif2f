# minif2f — LLM Theorem Proof Generator

**v2.0.0** — 6 models × 488 theorems × 128 attempts. Provenance-ready output, crash-safe checkpoints, GPT-2 ByteLevel decoder.

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
8) Re-extract lean_code from raw_output (no GPU)
9) Do It All (setup → quality → build → generate all)
```

## Commands

```bash
cargo run -- list-models
cargo run -- generate -m <model> -p data/models/<name>               # defaults: -n 128 --parallel 8
cargo run -- generate -m <model> -p data/models/<name> -n 64 --parallel 12 # custom
cargo run -- re-extract -m <model>          # re-derive lean_code from raw_output, no GPU
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
│   ├── setup.sh           # One-time deployment (provisions tools/vllm via uv sync)
│   ├── generate-all.sh    # Sequential vLLM generation (tmux, one model at a time)
│   ├── re-extract.sh      # Offline lean_code recovery from raw_output (no GPU)
│   └── stp_runner.py      # STP standalone HF generate runner
├── src/                   # Rust orchestrator (9 files, ~750 LOC)
│   ├── main.rs            # CLI (clap): generate, re-extract, status, list-models
│   ├── config.rs          # ModelConfig, PipelineConfig
│   ├── models.rs          # 6-model registry
│   ├── data.rs            # Dataset + Theorem
│   ├── prompts.rs         # Chat templates + proof extraction
│   ├── inference.rs       # vLLM server manager
│   ├── checkpoint.rs      # Crash recovery
│   └── pipeline.rs        # Orchestrator → two-layer JSON
├── .claude/
│   ├── MEMORY.md          # Three-layer knowledge index
│   ├── memory/            # 7 numbered project knowledge files
│   ├── workflows/         # 5 structured workflows
│   ├── hooks/             # Automation scripts (quality, pre-generate, verify)
│   ├── templates/         # New model + incident templates
│   └── archive/           # Historical incidents (not loaded into context)
├── output/
│   ├── raw_output/        # Unfiltered model completions
│   └── lean_code/         # Extracted + validated Lean proofs
└── results/checkpoints/   # Crash recovery state
```
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
- **Proof assembly**: `assemble_and_validate` (shared by live generation and `re-extract`) prepends the header only when the extracted block already carries the theorem statement — avoiding the double-theorem file that previously rejected Goedel-V2 / DeepSeek-V2 proofs.
- **Decoder**: LLaMA-based tokenizers (raw, deepseek_v2) are GPT-2 ByteLevel BPE; `decode_llama_byte_fallback` reverses them with the GPT-2 `bytes_to_unicode` inverse table so multi-byte math symbols (ℤ/ℕ/ℝ) survive. Qwen3 passes through unchanged.
- **Offline re-extraction**: `re-extract -m <model>` re-derives `lean_code` from existing `raw_output` with zero GPU. Valid for qwen3 models (clean raw); LLaMA raw is decoder-corrupted at write time and must be regenerated.
- **Checkpoint resume**: Loads existing raw_output + lean_code JSON on startup, merges tuples. Previously-completed theorems are not re-generated.
- **Incremental writes**: JSON written every 20 theorems. Checkpoint marks a theorem done only AFTER its data is written to disk — the checkpoint never gets ahead of durable output, so a crash causes harmless regeneration, never silent data loss.
- **Two-layer output**: `output/raw_output/` (unfiltered) + `output/lean_code/` (extracted + validated). Same flat JSON format in both.
- **STP model**: Uses standalone `scripts/stp_runner.py` with HF `model.generate()` (BF16 native). vLLM does not support `begin_suppress_tokens` required by this model, causing 100% empty output when attempted. See `workflows/stp.md`.
- **Byte-fallback decoder**: Architecture-conditional — only applied to LLaMA tokenizer models (DPO, DeepSeek, STP). Qwen3 models pass through unchanged. See [[06-decisions]].
- **Encoding validation**: Output checked for U+FFFD, Cyrillic, and Latin-1 leakage. LLaMA models may have residual Latin-1 leakage from vLLM tokenizer incomplete byte-fallback handling.

## Industrialization Roadmap

| Phase | Description | Status |
|:-----:|-------------|:------:|
| 1 | `.claude/` workflow structure + documentation | ✅ Done |
| 2 | Provenance: `_metadata` block in output JSON | 📋 Planned |
| 3 | Config-as-Code: YAML model configs (from `models.rs`) | 📋 Planned |
| 4 | Structured errors + JSON-line logging | 📋 Planned |
| 5 | CI/CD: GitHub Actions quality gates + smoke tests | 📋 Planned |
| 6 | Backend trait: `InferenceBackend` abstraction | 📋 Planned |

See `CLAUDE.md` > Industrialization Roadmap and `ARCHITECTURE.md` > Future Architecture for full design.
- **128 attempts**: Default. Configurable via `-n`. Used for Pass@k evaluation.
- **Sequential generation**: `generate-all.sh` runs all configured models one at a time on port 8080 with per-model `--parallel` values. Single tmux session.
- **GPU**: RTX 5090 32GB CUDA. vLLM PagedAttention KV cache, shared paged pool — `--parallel` does NOT linearly multiply VRAM.
- **Crash recovery**: `results/checkpoints/<model>__<run_id>.json` — resume with `--run-id`

## Hardware

- **GPU**: RTX 5090 32GB (CUDA) primary. RTX 4060 8GB (Vulkan) for testing.
- **BF16 safetensors → FP8 quantized at load time**: ~7-8 GB VRAM per 7-8B model
- **KV cache**: vLLM PagedAttention, shared paged pool — `--parallel` does NOT linearly multiply VRAM

## Quality

```bash
cargo fmt --check          ✅
cargo clippy -- -D warnings  ✅
cargo test                 ✅ 73/73
```
