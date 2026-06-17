---
name: project-identity
description: Project definition — what this is, goals, non-goals, constraints
layer: always-load
metadata:
  type: project
---

# 00 — Project Identity

## What This Is

Generate 128 proof attempts per theorem for [miniF2F](https://github.com/openai/miniF2F) (488 theorems, test+valid splits) using 6 Lean 4 theorem-proving LLMs. Output: two flat JSON files per model — `output/raw_output/<model>.json` + `output/lean_code/<model>.json`.

**Stack**: Rust orchestrator + vLLM for 5 models + HF `model.generate()` for STP.

## Goals

1. Produce Pass@128 evaluation data for 6 theorem-proving models
2. Each model × 488 theorems × 128 attempts = 62,464 completions
3. Output must be: correctly encoded (no U+FFFD/Cyrillic), structurally valid JSON, extractable Lean proofs
4. Pipeline must survive crashes/shutdowns via checkpoint resume

## Non-Goals

- We are NOT training models
- We are NOT building a general inference platform
- We are NOT optimizing for maximum throughput beyond what RTX 5090 32GB can sustain
- We are NOT evaluating proof correctness (that's downstream)

## Constraints

- **Hardware**: Single RTX 5090 32GB CUDA. One model loaded at a time.
- **Precision**: vLLM FP8 quantization for 5 models, BF16 native for STP.
- **Sequential only**: Models run one after another via `scripts/generate-all.sh`.
- **Max context**: Per-model `max_model_len` from official configs (1024–65536).
- **No Python at Rust runtime**: vLLM runs as a separate process; STP runs as independent Python script.

## Models

| # | CLI Name | HF Repo | Architecture | Backend |
|---|----------|---------|-------------|---------|
| 1 | `goedel-prover-dpo` | [Goedel-LM/Goedel-Prover-DPO](https://huggingface.co/Goedel-LM/Goedel-Prover-DPO) | raw / LLaMA | vLLM |
| 2 | `kimina-prover-rl-1.7b` | [AI-MO/Kimina-Prover-RL-1.7B](https://huggingface.co/AI-MO/Kimina-Prover-RL-1.7B) | qwen3 | vLLM |
| 3 | `goedel-prover-v2-8b` | [Goedel-LM/Goedel-Prover-V2-8B](https://huggingface.co/Goedel-LM/Goedel-Prover-V2-8B) | qwen3 | vLLM |
| 4 | `deepseek-prover-v2-7b` | [deepseek-ai/DeepSeek-Prover-V2-7B](https://huggingface.co/deepseek-ai/DeepSeek-Prover-V2-7B) | deepseek_v2 / LLaMA | vLLM |
| 5 | `kimina-prover-distill-8b` | [AI-MO/Kimina-Prover-Distill-8B](https://huggingface.co/AI-MO/Kimina-Prover-Distill-8B) | qwen3 | vLLM |
| 6 | `stp-model-lean` | [kfdong/STP_model_Lean](https://huggingface.co/kfdong/STP_model_Lean) | raw / LLaMA | HF generate |

See [[02-models]] for complete official configs including sampling params, prompt formats, EOS tokens.

## Output Structure

```
output/
├── raw_output/<model>.json    # {"<model>": {"<theorem>": {"attempt_N": "<text>"}}}
└── lean_code/<model>.json     # Same format, "" if extraction/validation failed

results/checkpoints/<model>__<run_id>.json  # JSON array of done theorem names
```

## Key Commands

```bash
./run                           # Interactive menu
cargo run -- generate -m <model> -p data/models/<name> -n 128 --parallel <N>
ATTEMPTS=128 bash scripts/generate-all.sh       # All vLLM models
python scripts/stp_runner.py --attempts 128      # STP only
```
