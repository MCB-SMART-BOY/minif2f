---
name: code-quality-standards
description: Rust code quality — 3 checks that MUST pass before committing, 37 tests
metadata:
  type: project
---

All code changes MUST pass:

```bash
cargo fmt --check
cargo clippy -- -D warnings
cargo test                 # 37/37 (prompts 21 + models 6 + data 3 + checkpoint 4 + pipeline 0)
```

- 6 modules tested: prompts.rs (21 tests), models.rs (6), data.rs (3), checkpoint.rs (4)
- Also: main.rs (0), lib.rs (0), doc-tests (0)
