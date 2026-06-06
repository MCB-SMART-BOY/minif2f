---
name: model-registry
description: 6 target models — HF repos, architecture, chat templates, GGUF status, prompt formats
metadata:
  type: reference
---

Defined in `src/models.rs`. Models inherit from `defaults()`: `temperature=0.6, top_p=0.95, seed=42`.

| CLI Name | HF Repo | Arch | Chat | Prompt | ctx | max_tok |
|----------|---------|------|------|--------|-----|---------|
| `kimina-prover-rl-1.7b` | AI-MO/Kimina-Prover-RL-1.7B | qwen3 | ChatML | kimina | 131072 | 8096 |
| `goedel-prover-dpo` | Goedel-LM/Goedel-Prover-DPO | deepseek_coder | `###` | simple | 4096 | 2048 |
| `goedel-prover-v2-8b` | Goedel-LM/Goedel-Prover-V2-8B | qwen3 | ChatML | goedel_v2 | 131072 | 32768 |
| `deepseek-prover-v2-7b` | deepseek-ai/DeepSeek-Prover-V2-7B | deepseek_v2 | Unicode ｜ | goedel_v2 | 32768 | 8192 |
| `kimina-prover-distill-8b` | AI-MO/Kimina-Prover-Distill-8B | qwen3 | ChatML | kimina | 131072 | 8096 |
| `stp-model-lean` | kfdong/STP_model_Lean | **raw** | **none** | **deepseek_prover** | 1024 | 1024 |

All values match official HuggingFace `config.json`, `tokenizer_config.json`, and training data specs.

## Key design decisions

- **Qwen3 models (kimina, goedel-v2, distill)**: ChatML template WITHOUT prepopulated `<think>` block. Model generates `<think>reasoning</think>` naturally — this is REQUIRED by RL format reward during training.
- **Goedel-V2/Simple formats**: Include `sorry` placeholder in theorem statement, matching official HF prompt format.
- **DeepSeek Prover / STP format**: NO `sorry` placeholder — model generates directly from `:= by`. Raw architecture (no chat template). Matches STP paper §3.1.
- **DeepSeek V2**: Uses Unicode fullwidth `｜` (U+FF5C) tokens — exact format from official docs.
- **STP model**: `max_model_len=1024` matches official STP eval script. Raw architecture because STP was trained on raw Lean text, not chat-formatted data. Informal prefix is excluded to save context space.
- **Goedel-DPO**: DeepSeek Coder chat format with prepopulated `### Response:\n\`\`\`lean4\n{code}` — opens a code block so the model generates tactics inside it. **CRITICAL**: trailing ``` stripped from prepopulated content (`.strip_suffix("\`\`\`")`). If included, model sees closed code block and outputs EOS (72% empty in testing before fix).

**How to apply:** Find by name with `find_model()`. Add new models by extending `builtin_models()`.
