---
name: goedel-dpo-prompt-fix
description: Goedel-DPO prompt format fix — trailing ``` was causing model to output nothing
metadata:
  type: project
---

## Bug

Goedel-DPO's `deepseek_coder` chat template prepopulates `### Response:` with a ```lean4
code block containing the theorem statement. The prepopulated content included a trailing
``` (closing fence), which told the model "the code block is already complete".

Result: model hit EOS immediately for 72.3% of completions, producing empty strings.

## Fix

In `src/prompts.rs`, the `deepseek_coder` → `simple` template now strips the trailing ```
from the prepopulated response content:

```rust
let prepop = prepop.strip_suffix("```").unwrap_or(prepop).trim_end();
```

The prompt now ends with `:= by` (open code block), and the model generates Lean tactics
inside the block.

## Impact

| Metric | Before | After |
|--------|--------|-------|
| avg n_tokens | ~207 | ~1288 |
| Empty outputs | 72.3% | 0% |
| Substantial (>1000 tok) | ~5% | 84% |

**How to apply**: Only affects Goedel-DPO (deepseek_coder + simple format). Other models
use different architectures/chat templates that don't prepopulate response code blocks.
