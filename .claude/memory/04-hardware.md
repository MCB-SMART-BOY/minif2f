---
name: hardware-constraints
description: RTX 5090 32GB (CUDA), vLLM with FP8 quantization, per-model --max-num-seqs values
metadata:
  type: project
---

## GPU
- **RTX 5090**: 32 GB, CUDA backend, vLLM for GPU inference
- **FP8 quantization**: BF16 safetensors → FP8 at load time by vLLM (`--quantization fp8`)
- One model at a time (sequential generation via `scripts/generate-all.sh`)

## VRAM Usage
- **7-8B models (FP8)**: ~7-8 GB VRAM for weights
- **1.7B models (FP8)**: ~1.7 GB VRAM for weights
- **Remaining VRAM (~24 GB)**: KV cache (vLLM PagedAttention)

## Per-Model --max-num-seqs (per `scripts/generate-all.sh`, empirically tested on RTX 5090 32GB)

| Model | parallel | Rationale |
|-------|----------|-----------|
| kimina-prover-distill-8b | 48 | Qwen3-8B, 16GB + GQA |
| stp-model-lean | 64 | DS-Prover-V1.5, 13GB + short ctx (1024) |
| goedel-prover-dpo | 40 | LLaMA-7B, 13GB, no GQA |
| deepseek-prover-v2-7b | 32 | LLaMA-7B + long ctx (8192), no GQA |
| kimina-prover-rl-1.7b | 64 | Qwen3-1.7B, 3.4GB + GQA, light model |
| goedel-prover-v2-8b | 16 | Qwen3-8B, 16GB + long ctx (32768), CoT |

## vLLM Flags
```
--quantization fp8
--max-model-len <per_seq>
--max-num-seqs <parallel>
--gpu-memory-utilization 0.92
--enforce-eager
--dtype half
--trust-remote-code
```

## vLLM PagedAttention
KV cache is dynamically managed via PagedAttention — more efficient than static slot allocation. `--max-num-seqs` uses continuous batching to eliminate idle slot waste.

## Disk
- Model files: ~75 GB (6 models, BF16 safetensors)
- Output JSON: ~190 MB per completed model pair (raw_output + lean_code)
- Root partition: 250 GB (129 GB used = 52%)
