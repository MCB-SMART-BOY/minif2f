---
name: model-registry
description: 6 models — HF repos, architecture, chat templates, official config.json specs, prompt formats
metadata:
  type: reference
---

Defined in `src/models.rs`. Each model inherits from `defaults()` then overrides per-model parameters from official sources (HF config.json, generation_config.json, eval scripts, papers).

## Complete Model Table

| CLI Name | HF Repo | Arch | Base | ctx | max_tok | temp | top_p | seed | Prompt | SysPrompt |
|----------|---------|------|------|-----|---------|------|-------|------|--------|-----------|
| `goedel-prover-dpo` | Goedel-LM/Goedel-Prover-DPO | raw | LLaMA-7B | 4096 | 2048 | 1.0 | 0.95 | 1 | simple | _(none)_ |
| `kimina-prover-rl-1.7b` | AI-MO/Kimina-Prover-RL-1.7B | qwen3 | Qwen3-1.7B | 40960 | 8096 | 0.6 | 0.95 | 42 | kimina | expert math+Lean4 |
| `goedel-prover-v2-8b` | Goedel-LM/Goedel-Prover-V2-8B | qwen3 | Qwen3-8B | 40960 | 32768 | 0.6 | 0.95 | 30 | goedel_v2 | _(none)_ |
| `deepseek-prover-v2-7b` | deepseek-ai/DeepSeek-Prover-V2-7B | deepseek_v2 | LLaMA-7B | 65536 | 8192 | 0.6 | 0.95 | 30 | goedel_v2_nocot | _(none)_ |
| `kimina-prover-distill-8b` | AI-MO/Kimina-Prover-Distill-8B | qwen3 | Qwen3-8B | 40960 | 8096 | 0.6 | 0.95 | 42 | kimina | expert math+Lean4 |
| `stp-model-lean` | kfdong/STP_model_Lean | raw | DS-Prover-V1.5 | 1024 | 1024 | 1.0 | 1.0 | 1 | deepseek_prover | _(none)_ |

## Architecture Details

| Model | model_type | GQA | kv_heads | attn_heads | vocab | EOS token | EOS ID |
|-------|-----------|-----|----------|------------|-------|-----------|--------|
| goedel-prover-dpo | llama | No | 32 | 32 | 102400 | `<｜end▁of▁sentence｜>` | 100001 |
| kimina-prover-rl-1.7b | qwen3 | Yes | 8 | 16 | 151936 | `<\|im_end\|>` | 151645 |
| goedel-prover-v2-8b | qwen3 | Yes | 8 | 32 | 151936 | `<\|im_end\|>` | 151645 |
| deepseek-prover-v2-7b | llama | No | 32 | 32 | 102400 | `<｜end▁of▁sentence｜>` | 100001 |
| kimina-prover-distill-8b | qwen3 | Yes | 8 | 32 | 151936 | `<\|im_end\|>` | 151645 |
| stp-model-lean | llama | No | 32 | 32 | 100004 | `<｜end▁of▁sentence｜>` | 100001 |

## Prompt Formats

| Format | Architecture | Chat Template | Used by |
|--------|-------------|---------------|---------|
| `simple` | raw | None (bare prompt) | goedel-prover-dpo |
| `kimina` | qwen3 | `<\|im_start\|>` ChatML | kimina-rl-1.7b, kimina-distill-8b |
| `goedel_v2` | qwen3 | `<\|im_start\|>` ChatML (user only) | goedel-v2-8b |
| `goedel_v2_nocot` | deepseek_v2 | Unicode `｜` (U+FF5C) | deepseek-prover-v2-7b |
| `deepseek_prover` | raw | None (bare prompt) | stp-model-lean |

## Official Sources

See [[official-model-requirements]] for the complete reference with exact prompt templates, inference parameters, EOS tokens, and paper citations for every model.

## vLLM Inference

All models served via vLLM with FP8 quantization on RTX 5090 32GB. vLLM's `--max-model-len` is set to `(max_tokens + 4096).min(max_model_len)`. The `stop` parameter in API calls provides additional stop strings beyond vLLM's built-in EOS token handling.

## Context Size Formula

```
per_seq = (max_tokens + 4096).min(max_model_len)
vLLM --max-model-len = per_seq
--max-num-seqs = parallel (continuous batching)
```
