# code-audit

Comprehensive code audit covering all 9 Rust source files + 2 scripts. Checks correctness, safety, performance, dead code, test coverage, and documentation consistency.

## Audit Scope (11 files, ~2870 LOC)

| File | LOC | Audit Focus |
|------|-----|-------------|
| `main.rs` | 127 | CLI args, error propagation |
| `lib.rs` | 7 | Module declarations |
| `config.rs` | 60 | Serde correctness, defaults |
| `models.rs` | 265 | Config accuracy, test coverage |
| `data.rs` | 164 | JSONL parsing, file I/O |
| `prompts.rs` | 1078 | Prompt building, proof extraction, validation |
| `inference.rs` | 290 | vLLM process lifecycle, HTTP, decoding |
| `checkpoint.rs` | 139 | Atomic writes, resume |
| `pipeline.rs` | 422 | buffer_unordered, rayon, JSON writes |
| `server.py` | 154 | vLLM wrapper, HTTP server, SamplingParams |
| `generate-all.sh` | 164 | Worker script, cleanup, retry |

## Audit Rubric

### 1. Correctness (highest priority)
- [ ] No logic errors or off-by-one bugs
- [ ] Edge cases handled (empty input, timeout, error paths)
- [ ] All error paths propagate correctly (no swallowed errors)
- [ ] Data races (Send/Sync correctness in async)
- [ ] Integer overflow/truncation

### 2. Safety
- [ ] No `unwrap()` / `expect()` that could panic in production
- [ ] No `unsafe` blocks
- [ ] Resource cleanup (child processes, file handles, HTTP connections)
- [ ] No silent truncation or data loss

### 3. Performance
- [ ] No unnecessary allocations or clones in hot paths
- [ ] No blocking calls in async context
- [ ] Efficient data structures (HashMap vs BTreeMap choice)
- [ ] Buffer sizes and parallelism tuned

### 4. Dead Code / Unused
- [ ] No unused imports, functions, or structs
- [ ] Dead code paths that can never be reached
- [ ] Legacy/deprecated functions that should be removed

### 5. Test Coverage
- [ ] Critical paths have tests
- [ ] Edge cases have tests
- [ ] Error paths tested where practical
- [ ] Newly added code has tests

### 6. Documentation Consistency
- [ ] Comments match actual behavior
- [ ] CLAUDE.md / ARCHITECTURE.md match code
- [ ] No stale or misleading comments

## Execution

```bash
# Quick checks (automated)
cargo clippy -- -D warnings
cargo clippy -- -W clippy::all -W clippy::pedantic
cargo test
cargo fmt --check

# Manual review (per file)
# Read each source file, trace through logic, check edge cases
```

## Severity Levels

| Level | Meaning | Action |
|-------|---------|--------|
| 🔴 **Critical** | Data loss, crash, silent incorrect results | Must fix immediately |
| 🟠 **High** | Panics in edge cases, memory leak, wrong output | Fix before next run |
| 🟡 **Medium** | Unnecessary allocation, dead code, misleading comment | Fix when convenient |
| 🟢 **Low** | Style inconsistency, minor optimization, missing test | Nice to have |
