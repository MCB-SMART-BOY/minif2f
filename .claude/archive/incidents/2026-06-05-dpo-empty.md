---
name: goedel-dpo-prompt-fix
description: Historical Goedel-DPO chat-prepopulation fix; current code uses official raw prompt
metadata:
  type: project
---

## Historical Bug

An older Goedel-DPO implementation used the `deepseek_coder` chat template and
prepopulated `### Response:` with a ```lean4 code block containing the theorem
statement. The prepopulated content included a trailing ``` closing fence, which
told the model "the code block is already complete".

Result: model hit EOS immediately for 72.3% of completions, producing empty strings.

## Superseded Fix

The historical fix stripped the trailing ``` from the prepopulated response content:

```rust
let prepop = prepop.strip_suffix("```").unwrap_or(prepop).trim_end();
```

Current code no longer routes Goedel-DPO through `deepseek_coder`. It uses the
official Goedel-Prover raw completion prompt directly:

~~~text
Complete the following Lean 4 code with explanatory comments preceding each line of code:

```lean4
{header}{informal_prefix}{formal_statement}
~~~

The Lean code block is intentionally left open; the model generates tactics from `:= by`.

## Impact

| Metric | Before | After |
|--------|--------|-------|
| avg n_tokens | ~207 | ~1288 |
| Empty outputs | 72.3% | 0% |
| Substantial (>1000 tok) | ~5% | 84% |

**How to apply**: Do not reintroduce DPO chat prepopulation unless explicitly testing an
alternative. Official project config is `architecture=raw`, `prompt_format=simple`,
`temperature=1.0`, `top_p=0.95`, `max_tokens=2048`, seed=1.
