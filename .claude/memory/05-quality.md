---
name: quality-standards
description: Quality gates — mandatory checks before commit, output validation criteria
layer: on-demand
metadata:
  type: project
---

# 05 — Quality Standards

## Code Quality Gates (MUST pass before commit)

```bash
cargo fmt --check          # Formatting — auto-fix with cargo fmt if fails
cargo clippy -- -D warnings  # Lint — 0 warnings required
cargo test                 # All 69 tests must pass
```

**Rationale**: These are automated and objective. No human judgment needed. If any fail, fix them before committing.

## Git Commit Style

- **No** `Co-Authored-By: Claude` or any Claude attribution in commit messages
- Use conventional commits: `fix:`, `feat:`, `docs:`, `refactor:`
- One logical change per commit
- Push after commit

## Output Validation Criteria

After each model completes, validate:

| Check | Method | Threshold |
|-------|--------|:---------:|
| JSON validity | `python3 -m json.tool` | Must parse |
| Theorem count | `len(data[model].keys())` | Must be 488 |
| Attempts per theorem | Check all have 128 entries | Must be 128 |
| Non-empty rate | Count non-empty strings / total | >95% normal, <50% alert |
| U+FFFD count | Scan for `�` | 0 for Qwen3, <0.1% for LLaMA |
| Cyrillic count | Scan U+0400-U+04FF | Must be 0 |
| Latin-1 leakage | Scan U+0080-U+00FF (excl. U+00B7) | Record count |
| Extraction rate | Lean non-empty / Raw non-empty | Compare with benchmarks |

## Test Count Reference

```
69 tests total:
  prompts.rs:    27 (prompt formats + proof extraction + validation)
  inference.rs:  22 (byte-fallback decoder + architecture-conditional)
  models.rs:      9 (registry + official specs)
  pipeline.rs:    5 (JSON round-trip + data extraction)
  data.rs:        3 (Theorem + make_proof_file)
  checkpoint.rs:  4 (CRUD + resume)
```

## Pre-commit Hook

Run `bash .claude/hooks/quality.sh` before every commit. It executes fmt→clippy→test in sequence and blocks on failure.
