---
name: hardware-constraints
description: RTX 5090 32GB (CUDA), llama-server GPU inference, --parallel 7–24, Q4_K_M bandwidth bottleneck
metadata:
  type: project
---

## GPU
- **RTX 5090**: 32 GB, CUDA backend, 1 model at a time (sequential generation)
- **RTX 4060 Laptop**: 8 GB, Vulkan backend, `--parallel 2`

## VRAM Usage
- **1.7B FP16**: ~3.2 GB
- **7-8B Q4_K_M GGUF**: ~4-5 GB
- **KV cache**: q8_0 quantization, shared paged pool — `--parallel` does NOT linearly multiply VRAM

## Memory Bandwidth Bottleneck (CRITICAL)

Q4_K_M quantized models are **memory-bandwidth bound**, not compute bound. RTX 5090 has ~1.7 TB/s bandwidth. A 7B Q4_K_M model (~4.5 GB) has a theoretical single-stream max of ~378 t/s. With parallel streams, bandwidth is shared:

```
LLaMA-7B (no GQA, kv=256KB/tok): bandwidth ceiling ~1,400-1,500 t/s total
  16-way: ~1,470 t/s total → ~92 t/s per slot  ← sweet spot, more p won't help
Qwen3 (GQA, kv=64KB/tok): 4× lighter KV → bandwidth ceiling higher
  24-way: ~2,040 t/s total → ~85 t/s per slot  ← still room to push
```

Key insight: **VRAM-constrained models (p=7-8) are well below bandwidth ceiling**.
Increasing their parallel has meaningful headroom. LLaMA-7B at p=16 is already
at ceiling — more parallel would drop per-slot t/s enough to cancel the gain.
GPU SM utilization at 73% is NORMAL — cores wait on memory.

## llama-server
- Launch: `llama-server -m <gguf> --port <port> -ngl 99 --ctx-size <n> --parallel <n> --no-warmup --cache-type-k q8_0 --cache-type-v q8_0 --cache-reuse 256 --flash-attn on`
- Rust manages lifecycle: spawn on start, kill on stop + Drop
- HTTP `/completion` endpoint with `/health` polling
- Stderr → `/tmp/llama-server-{port}.log`

## Per-model --parallel (RTX 5090 32GB, VRAM-maximized for total throughput)

| Model | Arch | Size | --parallel | ctx-size | Per-slot | VRAM est |
|-------|------|------|-----------|----------|----------|----------|
| goedel-prover-dpo | LLaMA-7B | 7B | **16** | 65536 | 4096 | ~21 GB |
| deepseek-prover-v2-7b | LLaMA-7B | 7B | **9** | 110592 | 12288 | ~31 GB |
| kimina-prover-rl-1.7b | Qwen3 | 1.7B | **38** | 463296 | 12192 | ~31 GB |
| goedel-prover-v2-8b | Qwen3 | 8B | **12** | 442368 | 36864 | ~32 GB |
| kimina-prover-distill-8b | Qwen3 | 8B | **36** | 438912 | 12192 | ~31 GB |
| stp-model-lean | LLaMA-7B | 7B | **16** | 16384 | 1024 | ~8 GB |

**LLaMA-7B** (no GQA, kv=256KB/tok): bandwidth ceiling ~1,400-1,500 t/s total.
Goedel-DPO p=16 already at ~1,470 t/s — no gain from more parallel.
DeepSeek-V2 was at p=7 (VRAM-constrained, not bandwidth-constrained) → p=9.
**Qwen3** (GQA, kv=64KB/tok): 4× lighter KV cache per token → can push 36-38
parallel while staying under 32 GB. GQA models are VRAM-bound, not bandwidth-bound.

## ctx-size formula
```rust
let per_slot = (config.max_tokens + 4096).min(config.max_model_len);
let ctx = per_slot * parallel;
```
llama-server divides ctx by parallel for per-slot context. Formula ensures each slot has prompt + generation headroom.

## Multi-model
- Single port (8080), sequential execution
- `./scripts/generate-all.sh` creates tmux session, runs models one at a time
- ETA per model: 7B LLaMA ~4-11h, 8B Qwen3 ~17-67h, 1.7B Qwen3 ~5h. Total ~7 days.

**Why:** Q4_K_M + memory bandwidth bottleneck means more parallel ≠ always faster. LLaMA-7B hits bandwidth ceiling at p=16. DeepSeek-V2 and Qwen3 models are VRAM-constrained below ceiling — pushing parallel higher has real gains.
**How to apply:** Per-model parallel values are in `scripts/generate-all.sh` (MODELS array). For manual runs, use `--parallel` flag. Check `nvidia-smi` for VRAM, but don't chase GPU utilization — 73% is normal for Q4_K_M.
