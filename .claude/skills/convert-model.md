# convert-model

**Note**: With vLLM backend, models are loaded directly from HF safetensors — no GGUF conversion needed. This skill is kept for historical reference.

## Current: vLLM direct loading

vLLM loads BF16 safetensors directly from `data/models/<name>/` and applies FP8 quantization at load time:

```bash
# vLLM is spawned automatically by the Rust pipeline:
# InferenceEngine::start() runs: uv run --directory tools/vllm python server.py <model_path> --port <port> --quantization fp8 ...
```

Models are downloaded once via `huggingface-cli download <repo> --local-dir data/models/<name>`.

## Legacy: llama.cpp GGUF conversion

```bash
source tools/venv/bin/activate
export HF_TOKEN="hf_..."

# 1.7B: f16 (~3.2 GB)
python tools/llama.cpp/convert_hf_to_gguf.py data/models/<name> \
  --outfile models/<name>.gguf --outtype f16

# 7-8B: q4_k_m (~4-5 GB)
python tools/llama.cpp/convert_hf_to_gguf.py data/models/<name> \
  --outfile models/<name>.gguf --outtype q4_k_m
```
