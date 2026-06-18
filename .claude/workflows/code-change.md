# Code Change

Triggers: "修改" "修复" "加功能"

## Phase 0: Context
- Read affected source files and their tests
- `grep -rn <symbol> src/` — find all references
- If modifying model config: cross-check [[02-models]] for official requirements
- If modifying prompt format: check all 6 models use the right format
- If modifying decoder: check both LLaMA and Qwen3 paths

## Phase 1: Implement
- Edit source code
- Add/update tests for each changed function (minimum 1 test per new function)
- If removing code: verify no callers remain
- If changing a function signature: update all call sites

## Phase 2: Verify (MANDATORY)
```
bash .claude/hooks/quality.sh
```
This runs sequentially:
1. `cargo fmt --check` → auto-fix if fails → re-add
2. `cargo clippy -- -D warnings` → must pass with 0 warnings
3. `cargo test` → all 69+ tests must pass
4. New functions must have test coverage

If model config changed: show the diff, explain the impact, ask user to confirm.

## Phase 3: Commit
1. Generate conventional commit: `fix:` / `feat:` / `docs:` / `refactor:`
2. Show `git diff --stat` for user review
3. `git add` + commit + push

## Phase 4: Impact Assessment
After commit, check if pipeline re-run is needed:
- **Model config changed** → that model must be re-generated
- **prompts.rs changed** → all models using that format must be re-generated
- **inference.rs decoder changed** → all LLaMA models must be re-generated
- **pipeline.rs changed** → assess whether checkpoint compatibility is affected
- **Documentation only** → no re-run needed

Remind user if re-run is needed. See [[workflows/generate]] for execution.
