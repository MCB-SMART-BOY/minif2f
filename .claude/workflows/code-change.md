# Code Change

Triggers: "修改" "修复" "加功能"

## Phase 0: Context
- Read affected source files and tests
- Grep for all references to changed function/symbol
- If change affects model config: cross-check [[02-models]] for official requirements

## Phase 1: Implement
- Edit source code
- Add/update tests for each new/changed function
- If removing code: check all callers first

## Phase 2: Verify (MANDATORY)
1. `cargo fmt --check` → auto-fix if fails
2. `cargo clippy -- -D warnings` → 0 warnings
3. `cargo test` → all pass
4. New functions must have tests
5. Model config changes: show diff, confirm with user

## Phase 3: Commit
1. Generate conventional commit message (fix:/feat:/docs:/refactor:)
2. Show git diff --stat for user review
3. `git add` + commit + push
4. If pipeline-relevant: remind user about re-run implications
