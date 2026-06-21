---
name: stp-runner-walrus-bug
description: scripts/stp_runner.py uses `if pos := str.find(...)` which is wrong — find returns -1 (truthy) when absent, 0 (falsy) when at index 0. STP not yet run, fix before running.
metadata:
  type: project
---

## STP Runner Walrus Bug (discovered 2026-06-21)

`scripts/stp_runner.py` uses `if pos := code.find(pattern):` in three places:
- `strip_theorem_header` lines 89, 93, 99
- `extract_proof` strategy 3, line 193

**Bug**: Python `str.find()` returns `-1` when the pattern is ABSENT (which is
truthy → wrongly enters branch, slices garbage) and `0` when the pattern is at
index 0 (falsy → wrongly skips). Verified empirically.

Correct form:
```python
pos = code.find(pattern)
if pos != -1:
    ...
```

**Impact**: STP has NOT been run yet (vLLM can't suppress begin_suppress_tokens →
separate HF script). STP prompt starts with "```lean4\n" so `:= by` is rarely at
index 0; main symptom is "garbage slice when := by absent". Fix BEFORE running STP
so it doesn't repeat the lean_code extraction disaster.

**Also** (line 340-347): seed is NOT applied per-attempt in `model.generate()`.
Rust side uses `base_seed.wrapping_add(i)` per attempt. Python should match via
`torch.Generator` or per-batch manual_seed for reproducibility + sample diversity.

Related: [[extraction-double-header-bug]]
