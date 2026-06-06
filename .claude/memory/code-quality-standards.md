---
name: code-quality-standards
description: Rust code quality — 3 checks that MUST pass before committing, 34 tests
metadata:
  type: project
---

All code changes MUST pass:

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test                 # 34/34 (prompts:21 + models:6 + data:3 + checkpoint:4)
```

**Why:** Pure Rust project, 9 source files, ~910 LOC. Zero warnings, zero failures.

**How to apply:** Run all three checks before every commit. See `ARCHITECTURE.md` for full function-level documentation.
