---
name: model-registry
description: 6 target models — HF repos, architecture, chat templates, GGUF status, prompt formats
metadata:
  type: reference
---

Defined in `src/models.rs`. Models inherit from `defaults()`: `temperature=0.6, top_p=0.95, seed=42`; official eval scripts override these per model where explicit.

| CLI Name | HF Repo | Arch | Chat | Prompt | ctx | max_tok | temp | top_p | seed |
|----------|---------|------|------|--------|-----|---------|------|-------|------|
| `kimina-prover-rl-1.7b` | AI-MO/Kimina-Prover-RL-1.7B | qwen3 | ChatML | kimina | 131072 | 8096 | 0.6 | 0.95 | 42 |
| `goedel-prover-dpo` | Goedel-LM/Goedel-Prover-DPO | raw | none | simple | 4096 | 2048 | 1.0 | 0.95 | 1 |
| `goedel-prover-v2-8b` | Goedel-LM/Goedel-Prover-V2-8B | qwen3 | ChatML | goedel_v2 | 40960 | 32768 | 0.6 | 0.95 | 30 |
| `deepseek-prover-v2-7b` | deepseek-ai/DeepSeek-Prover-V2-7B | deepseek_v2 | Unicode ｜ | goedel_v2 | 32768 | 8192 | 0.6 | 0.95 | 30 |
| `kimina-prover-distill-8b` | AI-MO/Kimina-Prover-Distill-8B | qwen3 | ChatML | kimina | 131072 | 8096 | 0.6 | 0.95 | 42 |
| `stp-model-lean` | kfdong/STP_model_Lean | **raw** | **none** | **deepseek_prover** | 1024 | 1024 | 1.0 | 1.0 | 1 |

Values are sourced from explicit Hugging Face model cards, Hugging Face `config.json` / `tokenizer_config.json`, and official eval scripts. When sources differ, `ctx` follows an explicit model-card `max_model_len` first; otherwise it follows model `config.json` (`max_position_embeddings`).

## Key design decisions

- **Qwen3 template**: ChatML template WITHOUT a prepopulated empty `<think>` block. Kimina models generate `<think>...</think>` naturally because the official Kimina RL format requires one thinking block followed by one Lean 4 block. Goedel-V2 uses the official proof-plan prompt instead.
- **Goedel-V2 / DeepSeek-V2 format**: Includes a `sorry` placeholder in the theorem statement, matching the official HF prompt format. Goedel-DPO `simple` does not add `sorry`.
- **DeepSeek Prover / STP format**: NO `sorry` placeholder — model generates directly from `:= by`. Raw architecture (no chat template), open ```lean4 block. Matches STP paper/eval scripts.
- **DeepSeek V2**: Uses Unicode fullwidth `｜` (U+FF5C) tokens — exact format from official docs.
- **STP model**: `max_model_len=1024`, `temperature=1.0`, `top_p=1.0`, seed=1 match official STP miniF2F generation scripts. Informal prefix is excluded to save context space.
- **Goedel-DPO**: Raw completion prompt with an open ```lean4 block, matching the official Goedel-Prover eval script. Sampling: `temperature=1.0`, `top_p=0.95`, `max_tokens=2048`, seed=1.

**How to apply:** Find by name with `find_model()`. Add new models by extending `builtin_models()`.
