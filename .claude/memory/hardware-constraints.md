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
16-way parallel: ~1120 t/s total → ~70 t/s per slot
22-way parallel: ~1250 t/s total → ~57 t/s per slot (4.5× per-slot drop!)
```

The sweet spot maximizes **total throughput** (p × per_slot_tps), not VRAM utilization. GPU SM utilization at 73% is NORMAL — cores wait on memory.

## llama-server
- Launch: `llama-server -m <gguf> --port <port> -ngl 99 --ctx-size <n> --parallel <n> --no-warmup --cache-type-k q8_0 --cache-type-v q8_0 --cache-reuse 256 --flash-attn on`
- Rust manages lifecycle: spawn on start, kill on stop + Drop
- HTTP `/completion` endpoint with `/health` polling
- Stderr → `/tmp/llama-server-{port}.log`

## Per-model --parallel (RTX 5090 32GB, bandwidth-optimized)

| Model | Arch | Size | --parallel | ctx-size | Per-slot |
|-------|------|------|-----------|----------|----------|
| goedel-prover-dpo | LLaMA-7B | 7B | **16** | 65536 | 4096 |
| deepseek-prover-v2-7b | LLaMA-7B | 7B | **7** | 86016 | 12288 |
| kimina-prover-rl-1.7b | Qwen3 | 1.7B | **24** | 292608 | 12192 |
| goedel-prover-v2-8b | Qwen3 | 8B | **8** | 294912 | 36864 |
| kimina-prover-distill-8b | Qwen3 | 8B | **24** | 292608 | 12192 |
| stp-model-lean | LLaMA-7B | 7B | **16** | 16384 | 1024 |

**LLaMA-7B** (no GQA, kv=256KB/tok): larger KV cache per token → fewer slots fit. p=16 is sweet spot.
**Qwen3** (GQA, kv=64KB/tok): 4× smaller KV per token → can push higher parallel. 1.7B FP16 smaller model → even faster.

## ctx-size formula
```rust
let per_slot = (config.max_tokens + 4096).min(config.max_model_len);
let ctx = per_slot * parallel;
```
llama-server divides ctx by parallel for per-slot context. Formula ensures each slot has prompt + generation headroom.

## Multi-model
- Single port (8080), sequential execution
- `./scripts/generate-all.sh` creates tmux session, runs models one at a time
- ETA per model: 7B LLaMA ~4h, 8B Qwen3 ~6h, 1.7B Qwen3 ~0.5h. Total depends on enabled GGUFs in `scripts/generate-all.sh`.

**Why:** Q4_K_M + memory bandwidth bottleneck means more parallel ≠ faster. The optimal parallel balances per-slot speed with total throughput.

**How to apply:** Per-model parallel values are in `scripts/generate-all.sh` (MODELS array). For manual runs, use `--parallel` flag. Check `nvidia-smi` for VRAM, but don't chase GPU utilization — 73% is normal for Q4_K_M.
