# convert-model

Download HF model → convert to GGUF for llama-server.

```
./scripts/setup.sh → option 2 (Download from HuggingFace)
```

## Manual

```bash
source tools/venv/bin/activate
export HF_TOKEN="hf_..."

# Convert
python tools/llama.cpp/convert_hf_to_gguf.py data/models/<name> \
  --outfile models/<name>.gguf --outtype f16     # 1.7B (~3.2 GB)
  --outfile models/<name>.gguf --outtype q4_k_m  # 7-8B (~4-5 GB)
```
