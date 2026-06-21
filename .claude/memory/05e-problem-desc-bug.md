---
name: extract-problem-desc-newline-bug
description: extract_problem_desc checks ends_with("-/") but all 488 informal_prefix end with "-/\n", so the /-- -/ markers leak into kimina models' "# Problem:" prompt line.
metadata:
  type: project
---

## extract_problem_desc Trailing-Newline Bug (found 2026-06-21)

**Severity: LOW-MEDIUM — degrades kimina prompt quality, both kimina models.**

### Bug (prompts.rs:314-320)
```rust
fn extract_problem_desc(prefix: &str) -> String {
    if prefix.starts_with("/--") && prefix.ends_with("-/") {   // never true
        prefix[3..prefix.len()-2].trim().to_string()
    } else {
        prefix.trim().to_string()    // ← all 488 take this; markers NOT stripped
    }
}
```

### Verified
All 488 `informal_prefix` start with `/--` and end with `"-/\n"` (trailing
newline) — so `ends_with("-/")` is **false for every single one**. The markers
`/--` and `-/` leak into the kimina `# Problem:` line:
- Actual:  `# Problem:/-- Let $z=...$ ... }36.-/`
- Intended: `# Problem:Let $z=...$ ...`

Affects kimina-prover-rl-1.7b + kimina-prover-distill-8b (the only `kimina`
format models). The test fixture (prompts.rs:707) uses `"...even -/"` with NO
trailing newline, so the test passes while real data fails.

### Fix
`prefix.trim_end().ends_with("-/")` and slice on the trimmed string, or strip
`/--`..`-/` more robustly. After fix, kimina models would need re-generation to
benefit (prompt changed) — but impact is mild (models still parse the content;
`/-- -/` is valid Lean doc-comment syntax they saw in training). Low priority
vs the decoder/checkpoint issues. Related: [[02-models]], [[03-pipeline]].
