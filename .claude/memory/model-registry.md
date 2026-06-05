---
name: model-registry
description: 6 target models — HF repos, architecture, chat templates, GGUF status, prompt formats
metadata:
  type: reference
---

Defined in `src/models.rs`. Models inherit from `defaults()`: `temperature=0.6, top_p=0.95, seed=42`.

| CLI Name | HF Repo | Arch | Chat | Prompt | ctx/tok | GGUF |
|----------|---------|------|------|--------|---------|------|
| `kimina-prover-rl-1.7b` | AI-MO/Kimina-Prover-RL-1.7B | qwen3 | ChatML | kimina | 8192/8192 | ✅ |
| `goedel-prover-dpo` | Goedel-LM/Goedel-Prover-DPO | deepseek_coder | ### | simple | 4096/4096 | ❌ |
| `goedel-prover-v2-8b` | Goedel-LM/Goedel-Prover-V2-8B | qwen3 | ChatML | goedel_v2 | 8192/8192 | ❌ |
| `deepseek-prover-v2-7b` | deepseek-ai/DeepSeek-Prover-V2-7B | deepseek_v2 | Unicode ｜ | goedel_v2 | 8192/8192 | ❌ |
| `kimina-prover-distill-8b` | AI-MO/Kimina-Prover-Distill-8B | qwen3 | ChatML | kimina | 8192/8192 | ❌ |
| `stp-model-lean` | kfdong/STP_model_Lean | deepseek_coder | ### | simple | 2048/2048 | ❌ |

## Key design decisions

- **Qwen3 models (kimina, goedel-v2, distill)**: ChatML template WITHOUT prepopulated `<think>` block. Model generates `<think>reasoning</think>` naturally — this is REQUIRED by RL format reward during training.
- **Goedel-V2/Simple formats**: Include `sorry` placeholder in theorem statement, matching official HF prompt format.
- **DeepSeek V2**: Uses Unicode fullwidth `｜` (U+FF5C) tokens — exact format from official docs.

**How to apply:** Find by name with `find_model()`. Add new models by extending `builtin_models()`.
