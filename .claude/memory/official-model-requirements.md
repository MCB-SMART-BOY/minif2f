# Official Model Requirements — Complete Reference

> **Last verified**: 2026-06-11
> **Sources**: HuggingFace model cards & config files, official GitHub repos & eval scripts, published papers.

---

## 1. Goedel-Prover-DPO

| 项目 | 官方值 | 来源 |
|------|--------|------|
| **HF Repo** | `Goedel-LM/Goedel-Prover-DPO` | HuggingFace |
| **论文** | [arXiv 2502.07640](https://arxiv.org/abs/2502.07640) | Goedel-Prover |
| **Eval 脚本** | `github.com/Goedel-LM/Goedel-Prover` → `eval/step1_inference.py` | GitHub |

### Prompt 格式 (raw completion)

```
Complete the following Lean 4 code with explanatory comments preceding each line of code:

```lean4
{header}{informal_prefix}{formal_statement}
```

- **raw completion** — 不使用 chat template
- `{header}`: `import Mathlib\nimport Aesop\n\nset_option maxHeartbeats 0\n\nopen BigOperators Real Nat Topology Rat\n\n`
- `{informal_prefix}`: `/-- ... -/` (可为空)
- `{formal_statement}`: `theorem name ... := by\n`
- **代码块保持开放** — 模型在 `\`\`\`lean4` 内部生成代码
- **无 `sorry`** — minif2f 数据中 `formal_statement` 结尾是 `:= by`

### 推理参数

| 参数 | 官方值 | 来源 |
|------|--------|------|
| `max_model_len` | **4096** | eval script + config.json |
| `max_tokens` | **2048** | eval script |
| `temperature` | **1.0** | eval script |
| `top_p` | **0.95** | eval script |
| `seed` | **1** | eval script |
| `stop_sequences` | `<｜end▁of▁sentence｜>`, `<\|EOT\|>`, `### Instruction:`, `</s>` | tokenizer_config.json |
| `dtype` | `bfloat16` | config.json |

### EOS / 特殊 Token

| Token | ID | 用途 |
|-------|-----|------|
| `<｜begin▁of▁sentence｜>` | 100000 | BOS |
| `<｜end▁of▁sentence｜>` | 100001 | EOS (也用作 PAD) |
| `<\|EOT\|>` | — | DeepSeek Coder chat 的结束标记 |

### 架构信息

| 项目 | 值 |
|------|-----|
| 基座模型 | LLaMA-7B (DeepSeek-Prover-V1.5-Base) |
| `model_type` | `llama` |
| `architectures` | `LlamaForCausalLM` |
| GQA | 无 (`num_kv_heads=32 == num_attention_heads=32`) |
| `max_position_embeddings` | 4096 |
| Tokenizer | `LlamaTokenizer` |
| Chat template | DeepSeek Coder 格式 (`### Instruction:` / `### Response:`) |

### 代码提取 (官方)

```python
# eval/step1_inference.py
pattern = r'```lean4\n(.*?)\n```'
matches = re.findall(pattern, model_input + output.text, re.DOTALL)
```

提取时拼接 `model_input + output.text`，然后用正则拿 `\`\`\`lean4` 块的内容。

---

## 2. Kimina-Prover-RL-1.7B

| 项目 | 官方值 | 来源 |
|------|--------|------|
| **HF Repo** | `AI-MO/Kimina-Prover-RL-1.7B` | HuggingFace |
| **博客** | [huggingface.co/blog/AI-MO/kimina-prover-rl](https://huggingface.co/blog/AI-MO/kimina-prover-rl) | Project Numina |
| **训练框架** | Verl (GRPO / DrGRPO) | GitHub |

### System Prompt

```
You are an expert in mathematics and proving theorems in Lean 4.
```

### User Prompt 格式 (Qwen3 ChatML)

```
Think about and solve the following problem step by step in Lean 4.
# Problem:{natural_language_description}
# Formal statement:
```lean4
{header}
{informal_prefix}
{formal_statement}
```
```

- **Chat 格式**: Qwen3 ChatML，system + user
- **无 `sorry`** — 定理以 `:= by` 结尾
- **不在 prompt 中预填 `<think>`** — 模型自己生成

### 期望输出格式

```
<think>
[自然语言推理过程，可能包含 `tactics` 代码块]
</think>

```lean4
[完整的 Lean 4 证明代码]
```
```

### Format Reward 规则 (RL 训练用)

1. 必须有且仅有一个 `<think>...</think>` 块和一个 `\`\`\`lean4` 块
2. 拒绝重复推理行（幻觉/退化标志）
3. think 块内 tactics 块数量要足够，非注释行要足够
4. think 和 code 都要有合理的注释密度
5. 推理中描述的 tactics 与最终代码做语义对齐 (IoU / subcode coverage)
6. 惩罚过长的输出

### 推理参数

| 参数 | 官方值 | 来源 |
|------|--------|------|
| `max_model_len` | **40960** | config.json (`max_position_embeddings`) |
| `max_tokens` | **8096** | 博客 vLLM 示例 + HF quickstart |
| `temperature` | **0.6** | generation_config.json + 博客 |
| `top_p` | **0.95** | generation_config.json + 博客 |
| `top_k` | **20** | generation_config.json (HF 默认，vLLM 可能不用) |
| `seed` | — | 博客未指定，建议 42 |
| `dtype` | `bfloat16` | config.json |

> ⚠️ 博客 vLLM 示例写 `max_model_len=131072`，但 config.json 是 `max_position_embeddings=40960`。Qwen3-1.7B-Base 原生就是 40960，博客可能是从 8B 版本复制的错误。

### EOS / 特殊 Token

| Token | ID | 用途 |
|-------|-----|------|
| `<\|im_end\|>` | 151645 | EOS |
| `<\|endoftext\|>` | 151643 | 替代 EOS / PAD |
| `<think>` | added token | 模型自己生成的思考标签 |
| `</think>` | added token | 模型自己生成的思考标签 |

### 架构信息

| 项目 | 值 |
|------|-----|
| 基座模型 | Qwen3-1.7B-Base |
| `model_type` | `qwen3` |
| `architectures` | `Qwen3ForCausalLM` |
| GQA | 有 (`num_kv_heads=8, num_attention_heads=16`) |
| Tokenizer | `Qwen2Tokenizer` |
| Chat template | Qwen3 ChatML (`<\|im_start\|>` / `<\|im_end\|>`) |

---

## 3. Goedel-Prover-V2-8B

> **最后验证**: 2026-06-12 — 从 HuggingFace Model Card (WebSearch) + 论文 arXiv 2508.03613 确认。
> **HF Quick Start 代码**: `model.generate(inputs, max_new_tokens=32768)` — 仅传 `max_new_tokens`，其余用 Transformers 默认。
> **性能**: MiniF2F Pass@32 = 84.6%，self-correction = 86.7%（论文 Table 1）。

| 项目 | 官方值 | 来源 |
|------|--------|------|
| **HF Repo** | `Goedel-LM/Goedel-Prover-V2-8B` | HuggingFace |
| **论文** | [arXiv 2508.03613](https://arxiv.org/abs/2508.03613) | ICLR 2026 |
| **GitHub** | `github.com/Goedel-LM/Goedel-Prover-V2` | GitHub |

### System Prompt

**无** — 只用 user message。HF Model Card 确认 `apply_chat_template([{"role": "user", "content": prompt}])`。

### Prompt 格式 (Qwen3 ChatML, CoT — 唯一官方模式)

HF Model Card 和 GitHub README 只展示 CoT 模式：

```
Complete the following Lean 4 code:

```lean4
{formal_statement}
  sorry
```
Before producing the Lean 4 code to formally prove the given theorem, provide a detailed proof plan outlining the main proof steps and strategies.
The plan should highlight key ideas, intermediate lemmas, and proof structures that will guide the construction of the final formal proof.
```

- **Chat 格式**: Qwen3 ChatML，仅 user message，`add_generation_prompt=True`
- **`{formal_statement}`** 官方示例已含 `sorry`（如 `theorem mathd_algebra_10 : ... := by\n  sorry`）。minif2f 数据不含 `sorry`，代码需添加。
- **代码块闭合** (`\`\`\``) — 模型先出 proof plan，再出新的 `\`\`\`lean4` 代码块
- **期望输出**: 先自然语言 proof plan（`### Detailed Proof and Analysis`），再 `\`\`\`lean4` 块含 subgoal sketch（`have ... := by sorry`），最后完整证明代码块

### 推理参数（HF Quick Start 确认）

| 参数 | 官方值 | 来源 |
|------|--------|------|
| `max_model_len` | **40960** | config.json |
| `max_new_tokens` | **32768** | HF Quick Start 代码（仅有的 generate 参数） |
| `temperature` | **0.6** | generation_config.json |
| `top_p` | **0.95** | generation_config.json |
| `top_k` | **20** | generation_config.json |
| `seed` | **30** | HF README (`torch.manual_seed(30)`) |
| `dtype` | `bfloat16` | config.json |

> ⚠️ **HF 官方代码只传 `max_new_tokens=32768`**，不显式设置 temperature/top_p。但 `generation_config.json` 写 `temperature=0.6, top_p=0.95`。两者可能等效（`generate()` 会加载 generation_config.json 的默认值）。
>
> ⚠️ **CoT 输出可能极长**。论文 Table 3 显示 DeepSeek 7B CoT 平均 4488.5 tokens。Goedel-V2 8B 类似或更长。实测 minif2f 部分定理输出可达 85K 字符（~21K tokens）。这是官方 32K 窗口的正常行为 — CoT 包含 proof plan + subgoal sketch + 完整证明。

### EOS / 特殊 Token

| Token | ID | 用途 |
|-------|-----|------|
| `<\|im_end\|>` | 151645 | EOS |
| `<\|endoftext\|>` | 151643 | 替代 EOS / PAD |

### 架构信息

| 项目 | 值 |
|------|-----|
| 基座模型 | Qwen3-8B-Base |
| `model_type` | `qwen3` |
| GQA | 有 (`num_kv_heads=8, num_attention_heads=32`) |
| Tokenizer | `Qwen2Tokenizer` |
| `max_position_embeddings` | 40960 |

---

## 4. DeepSeek-Prover-V2-7B

> **最后验证**: 2026-06-12 — HF Model Card (WebSearch) + 论文 arXiv 2504.21801 + Appendix A。
> **HF Quick Start**: `model.generate(inputs, max_new_tokens=8192)` — 仅此参数，其余 Transformers 默认。
> **论文 Table 1**: non-CoT 75.0%, CoT 80.7% @Pass@8192（miniF2F-test）。
> **论文 Table 3**: 7B non-CoT avg 442.6 tokens, CoT avg 4488.5 tokens。

| 项目 | 官方值 | 来源 |
|------|--------|------|
| **HF Repo** | `deepseek-ai/DeepSeek-Prover-V2-7B` | HuggingFace |
| **论文** | [arXiv 2504.21801](https://arxiv.org/abs/2504.21801) | DeepSeek-AI |
| **GitHub** | `github.com/deepseek-ai/DeepSeek-Prover-V2` | GitHub |

### System Prompt

**无** — 只用 user message。HF: `chat = [{"role": "user", "content": prompt}]`。

### 两种推理模式（论文 Section 2.3 + Appendix A, B）

**两个模式都是推理模式**，非仅训练用。Table 1 两种模式均有评测结果。

#### CoT Prompt（Appendix A.2 / HF Model Card）

```
Complete the following Lean 4 code:

```lean4
{formal_statement}
  sorry
```
Before producing the Lean 4 code to formally prove the given theorem, provide a detailed proof plan outlining the main proof steps and strategies.
The plan should highlight key ideas, intermediate lemmas, and proof structures that will guide the construction of the final formal proof.
```

#### Non-CoT Prompt（Appendix A.1）

```
Complete the following Lean 4 code:

```lean4
{formal_statement}
  sorry
```
```

- **CoT**: 模型输出 proof plan + subgoal sketch (`have ... := by sorry`) + 完整 Lean 证明块
- **Non-CoT**: 模型直接输出 `\`\`\`lean4` 块 `theorem ... := by ...`。7B 模型 `often inserts brief natural language comments`（论文 Section 3.1）
- **同一模型，通过 prompt 切换模式**
- **代码块均闭合** — 模型在新的 `\`\`\`lean4` 块中输出证明
- **Chat 格式**: `apply_chat_template(chat, add_generation_prompt=True)`
- ⚠️ **论文 + Unsloth community**: 不用此 prompt 格式会输出乱码
- HF 官方示例中 `formal_statement` 已含 `sorry`。minif2f 不含，代码需添加 `\n  sorry`

### 推理参数（HF Quick Start 确认）

| 参数 | 官方值 | 来源 |
|------|--------|------|
| `max_model_len` | **65536** | config.json |
| `max_new_tokens` | **8192** | HF README Quick Start 代码 |
| `seed` | **30** | HF README (`torch.manual_seed(30)`) |
| `temperature` | Transformers 默认 1.0 | HF 代码未指定 |
| `top_p` | 默认 | HF 代码未指定 |
| `dtype` | `bfloat16` | config.json |

> ⚠️ **HF 官方代码只传 `max_new_tokens=8192`**，temperature/top_p 用 Transformers 默认。无 `generation_config.json`。

### EOS / Byte-Fallback / 特殊 Token

| Token | ID | 用途 |
|-------|-----|------|
| `<｜begin▁of▁sentence｜>` | 100000 | BOS（vLLM 根据 `add_bos_token=True` 自动添加） |
| `<｜end▁of▁sentence｜>` | 100001 | EOS |
| `<｜User｜>` | 100006 | Chat 用户分隔 |
| `<｜Assistant｜>` | 100007 | Chat 助手分隔 |
| `vocab_size` | 102400 |

> **Byte-fallback**: LLaMA tokenizer 用 Ġ (U+0120) 编码前缀空格。vLLM API Server 已用 tokenizer 正确解码为普通空格。`decode_llama_byte_fallback()` 对 API Server 输出为 no-op，保留用于兼容。 |

### 架构信息

| 项目 | 值 |
|------|-----|
| 基座模型 | DeepSeek-Prover-V1.5-Base (LLaMA-7B) |
| `model_type` | `llama` |
| GQA | 无 (`num_kv_heads=32 == num_attention_heads=32`) |
| `max_position_embeddings` | 65536 |
| Tokenizer | `LlamaTokenizerFast`（DeepSeek 定制，vocab 102400） |
| `add_bos_token` | `True` — vLLM 自动添加 BOS |

---

## 5. Kimina-Prover-Distill-8B

| 项目 | 官方值 | 来源 |
|------|--------|------|
| **HF Repo** | `AI-MO/Kimina-Prover-Distill-8B` | HuggingFace |
| **基座** | Kimina-Prover-72B → 蒸馏到 Qwen3-8B | 博客 |

### System Prompt

```
You are an expert in mathematics and Lean 4.
```

> ⚠️ 和 RL 版本不同：RL 是 "expert in mathematics **and proving theorems in** Lean 4"

### User Prompt 格式

与 Kimina-Prover-RL 完全相同：

```
Think about and solve the following problem step by step in Lean 4.
# Problem:{natural_language_description}
# Formal statement:
```lean4
{header}
{informal_prefix}
{formal_statement}
```
```

### 期望输出格式

与 RL 相同：`<think>...</think>` + `\`\`\`lean4` 块

### 推理参数

| 参数 | 官方值 | 来源 |
|------|--------|------|
| `max_model_len` | **40960** | config.json |
| `max_tokens` | **8096** | HF quickstart |
| `temperature` | **0.6** | generation_config.json |
| `top_p` | **0.95** | generation_config.json |
| `top_k` | **20** | generation_config.json |
| `seed` | — | 未指定 |
| `dtype` | `bfloat16` | config.json |

### EOS / 架构

与 Kimina-Prover-RL-1.7B 相同（Qwen3 ChatML，EOS=151645）。

---

## 6. STP_model_Lean

| 项目 | 官方值 | 来源 |
|------|--------|------|
| **HF Repo** | `kfdong/STP_model_Lean` | HuggingFace |
| **论文** | [arXiv 2502.00212](https://arxiv.org/abs/2502.00212) | Stanford |
| **GitHub** | `github.com/kfdong/STP` | GitHub |

### System Prompt

**无** — raw completion，不使用 chat template。

### Prompt 格式 (raw completion)

```
Complete the following Lean 4 code:

```lean4
import Mathlib
import Aesop
set_option maxHeartbeats 0
open BigOperators Real Nat Topology Rat
{formal_statement}
```

- **raw completion** — 不使用 chat template
- `{formal_statement}`: 定理声明，结尾是 `:= by`
- **不包括** `informal_prefix` — STP 只有 1024 context
- 去掉最后的 `sorry`（官方: `rsplit("sorry", 1)[0].strip()`）
- 代码块保持开放 — 模型从 `:= by` 后生成 tactics

### 推理参数

| 参数 | 官方值 | 来源 |
|------|--------|------|
| `max_model_len` | **1024** | `run_generation_and_test.sh` + `model_utils.py` |
| `max_tokens` | **1024** | `model_utils.py` (默认) |
| `temperature` | **1.0** | `run_generation_and_test.sh` |
| `top_p` | **1.0** | `model_utils.py` (默认) |
| `seed` | **1** | `run_generation_and_test.sh` |
| `dtype` | — | config.json 未指定 |

### EOS / 特殊 Token

| Token | ID | 用途 |
|-------|-----|------|
| `<｜begin▁of▁sentence｜>` | 100000 | BOS |
| `<｜end▁of▁sentence｜>` | 100001 | EOS |
| `[PAD]` | 100002 | PAD (独立 pad token，与 EOS 不同) |
| `<unk>` | — | UNK (独立 unk token) |

### 架构信息

| 项目 | 值 |
|------|-----|
| 基座模型 | DeepSeek-Prover-V1.5-SFT (LLaMA-7B) |
| `model_type` | `llama` |
| `architectures` | `LlamaForCausalLM` |
| GQA | 无 (`num_kv_heads=32 == num_attention_heads=32`) |
| `max_position_embeddings` | 4096 (但 eval 仅用 1024) |
| `vocab_size` | 100004 |
| Tokenizer | `LlamaTokenizer` |
| `tokenizer.truncation_side` | `left` |

### 论文结果

| Benchmark | Pass@3200 |
|-----------|-----------|
| miniF2F-test | **65.0%** |
| ProofNet-test | 23.9% |
| PutnamBench | 8/644 |

---

## 汇总: Prompt 模板对比

### 官方 vs 当前代码

| # | 模型 | 格式名 | 官方 | 当前代码 |
|---|------|--------|------|----------|
| 1 | goedel-prover-dpo | `simple` | raw, open code block ✅ | 匹配 ✅ |
| 2 | kimina-prover-rl-1.7b | `kimina` | Qwen3 ChatML, system+user ✅ | 匹配 ✅ |
| 3 | goedel-prover-v2-8b | `goedel_v2` | Qwen3 ChatML, user only, CoT ✅ | 匹配 ✅ |
| 4 | deepseek-prover-v2-7b | `goedel_v2_nocot` | DeepSeek V2, user only, **non-CoT** | 匹配 ✅ (刚改的) |
| 5 | kimina-prover-distill-8b | `kimina` | Qwen3 ChatML, system+user ✅ | 匹配 ✅ |
| 6 | stp-model-lean | `deepseek_prover` | raw, open code block, no informal_prefix ✅ | 匹配 ✅ |

### 官方 vs 当前参数

| # | 模型 | 参数 | 官方 | 当前 | 状态 |
|---|------|------|------|------|------|
| 1 | goedel-prover-dpo | 全部 | — | — | ✅ |
| 2 | kimina-prover-rl-1.7b | `max_model_len` | 40960 | 40960 | ✅ |
| 3 | goedel-prover-v2-8b | 全部 | — | — | ✅ |
| 4 | deepseek-prover-v2-7b | `max_model_len` | 65536 | 65536 | ✅ |
| 4 | deepseek-prover-v2-7b | `prompt_format` | non-CoT | `goedel_v2_nocot` | ✅ |
| 5 | kimina-prover-distill-8b | `max_model_len` | 40960 | 40960 | ✅ |
| 5 | kimina-prover-distill-8b | `system_prompt` | "expert...Lean 4" | 同 | ✅ |
| 6 | stp-model-lean | `max_model_len` | 1024 | 1024 | ✅ |

---

## 汇总: 官方来源索引

| # | 模型 | HF | 论文 | 官方代码 | 博客 |
|---|------|-----|------|----------|------|
| 1 | goedel-prover-dpo | [Goedel-LM/Goedel-Prover-DPO](https://huggingface.co/Goedel-LM/Goedel-Prover-DPO) | [2502.07640](https://arxiv.org/abs/2502.07640) | [Goedel-Prover](https://github.com/Goedel-LM/Goedel-Prover) | — |
| 2 | kimina-prover-rl-1.7b | [AI-MO/Kimina-Prover-RL-1.7B](https://huggingface.co/AI-MO/Kimina-Prover-RL-1.7B) | — | [kimina-prover-rl](https://github.com/project-numina/kimina-prover-rl) | [Blog](https://huggingface.co/blog/AI-MO/kimina-prover-rl) |
| 3 | goedel-prover-v2-8b | [Goedel-LM/Goedel-Prover-V2-8B](https://huggingface.co/Goedel-LM/Goedel-Prover-V2-8B) | [2508.03613](https://arxiv.org/abs/2508.03613) | [Goedel-Prover-V2](https://github.com/Goedel-LM/Goedel-Prover-V2) | — |
| 4 | deepseek-prover-v2-7b | [deepseek-ai/DeepSeek-Prover-V2-7B](https://huggingface.co/deepseek-ai/DeepSeek-Prover-V2-7B) | [2504.21801](https://arxiv.org/abs/2504.21801) | [DeepSeek-Prover-V2](https://github.com/deepseek-ai/DeepSeek-Prover-V2) | — |
| 5 | kimina-prover-distill-8b | [AI-MO/Kimina-Prover-Distill-8B](https://huggingface.co/AI-MO/Kimina-Prover-Distill-8B) | — | 同上 | 同上 |
| 6 | stp-model-lean | [kfdong/STP_model_Lean](https://huggingface.co/kfdong/STP_model_Lean) | [2502.00212](https://arxiv.org/abs/2502.00212) | [STP](https://github.com/kfdong/STP) | — |
