---
name: hardware-constraints
description: RTX 5090 32GB (CUDA), llama-server GPU inference, --parallel 8
metadata:
  type: project
---

## GPU
- **RTX 5090**: 32 GB, CUDA backend, 1 model at a time (sequential generation)
- **RTX 4060 Laptop**: 8 GB, Vulkan backend, `--parallel 2`

## VRAM Usage
- **1.7B FP16**: ~3.2 GB
- **7-8B Q4_K_M GGUF**: ~4-5 GB

## llama-server
- Launch: `llama-server -m <gguf> --port <port> -ngl 99 --ctx-size <n> --parallel 8 --no-warmup`
- Rust manages lifecycle: spawn on start, kill on stop + Drop
- HTTP `/completion` endpoint with `/health` polling

## Multi-model
- `--port` flag for different ports (8080-8085)
- `./scripts/generate-all.sh` creates tmux session with 2 slots (ports 8080, 8081)
- `--parallel 8` default — safe for 32GB 5090. DO NOT use 128.

**Why:** These constraints drove the llama.cpp choice, Q4_K_M quantization for 7-8B models, and the parallel port-based architecture.

**How to apply:** Check `nvidia-smi` before running. Use `--port` for parallel models. Keep `--parallel <= 16`.
