---
name: code-audit-report
description: Full code audit findings across all 11 project files — correctness, safety, performance, dead code, coverage
metadata:
  type: project
---

# Code Audit Report — 2026-06-11

## Files Audited (11 files, ~2870 LOC)

| File | LOC | Tests | Status |
|------|-----|-------|--------|
| `config.rs` | 60 | 0 | ✅ Clean |
| `models.rs` | 265 | 6 | ✅ Clean |
| `data.rs` | 164 | 3 | 🟡 1 finding |
| `prompts.rs` | 1078 | 27 | ✅ Clean (bugs fixed in this session) |
| `inference.rs` | 290 | 0 | 🟠 1 finding, 🟡 2 findings |
| `checkpoint.rs` | 139 | 4 | ✅ Clean |
| `pipeline.rs` | 422 | 0 | 🟠 1 finding, 🟡 1 finding |
| `main.rs` | 127 | 0 | ✅ Clean |
| `lib.rs` | 7 | 0 | ✅ Clean |
| `server.py` | 154 | 0 | ✅ Clean (quantization added in this session) |
| `generate-all.sh` | 164 | 0 | ✅ Clean |

## Findings Summary

| Severity | Count | Description |
|----------|-------|-------------|
| 🔴 Critical | 0 | — |
| 🟠 High | 2 | Un-awaited checkpoint JoinHandle, untested inference/pipeline |
| 🟡 Medium | 4 | Silent error in checkpoint resume, O(n²) checkpoint, dead code, silent dataset error |
| 🟢 Low | 1 | Stale comment |

## Detailed Findings

### 🟠 FINDING-1: Un-awaited checkpoint JoinHandle (pipeline.rs:297-301)

```rust
tokio::task::spawn_blocking(move || {
    if let Ok(mut ck) = CheckpointManager::new(&ck_dir, &ck_model, &ck_run) {
        let _ = ck.mark_done(&ck_thm);
    }
});
```

**Problem**: The `JoinHandle` returned by `spawn_blocking` is dropped. If the blocking task panics, the panic is silently swallowed. Checkpoint failures could go undetected — a theorem might be generated successfully but not recorded. On resume, it would be re-generated unnecessarily.

**Fix**: Spawn with `tokio::spawn` and `.await` the result, or at minimum log the error:
```rust
tokio::spawn(async move {
    if let Err(e) = tokio::task::spawn_blocking(move || { ... }).await {
        eprintln!("Checkpoint task panicked for {ck_thm}: {e}");
    }
});
```

### 🟠 FINDING-2: No tests for inference.rs or pipeline.rs

**Problem**: The vLLM integration (process spawn, health check, HTTP generation) and the pipeline orchestration (buffer_unordered, batch flush, JSON write, checkpoint resume) have zero tests. These are the most complex parts of the system.

**Mitigation**: Integration testing requires a running vLLM server. Unit-level testing of the `decode_llama_byte_fallback` function and `write_flat_json` / `load_existing_results` functions is practical and should be added.

### 🟡 FINDING-3: O(n²) checkpoint writes (checkpoint.rs + pipeline.rs)

**Problem**: `mark_done()` serializes the entire `HashSet<String>` each time, and `CheckpointManager::new()` deserializes it. With 488 theorems, each checkpoint write is ~4KB initial → ~16KB final. With 488 writes, this is ~5MB total — negligible. But the pattern is O(n²).

**Fix**: Append-only JSONL checkpoint: each line is one theorem name. `new()` reads all lines into HashSet. `mark_done()` appends one line. This is O(1) per write and immune to corruption (each line is independent).

### 🟡 FINDING-4: Dead code in inference.rs

**Problem**: `generate_stream()` and `generate_batch_retry()` are unused. The pipeline uses `generate_one_with_retry()` directly via `buffer_unordered`. These functions have 30+ lines of dead code.

**Fix**: Either remove or mark with `#[allow(dead_code)]` and a comment explaining why they're kept.

### 🟡 FINDING-5: Silent dataset error swallowing (data.rs:94-111)

**Problem**: If one split loads successfully but the other fails, the error is silently ignored. Only when BOTH fail does the error surface. A corrupted `valid.jsonl` would cause the pipeline to run with only `test` theorems, with no warning.

**Fix**: Log a warning when one split fails:
```rust
Err(e) => eprintln!("Warning: failed to load '{split}' split: {e}"),
```

### 🟡 FINDING-6: `strip_fence_lang_specifier` edge case (prompts.rs)

**Problem**: If a code block's first line is a single Lean tactic word (e.g., `calc`, `rw`, `simp`), it could be incorrectly stripped as a "language specifier". However, language-specific fences (` ```lean4`, ` ```tactics`) are tried first and only bare ``` blocks go through this function. In practice, model outputs use language-specific fences.

**Mitigation**: Already mitigated by the fence_start priority order. Only bare ``` blocks are affected, and those are rare in practice.

### 🟢 FINDING-7: Stale "llama-server" comment (prompts.rs:41)

Line 41: `// llama-server via tokenizer config (add_bos_token)` should say `vLLM`. Purely cosmetic.

## Test Coverage Matrix

| Function | Tested? | Notes |
|----------|---------|-------|
| `extract_proof` | ✅ 8 tests | Think block, fallback, `###` rejection, English rejection |
| `validate_lean_code` | ✅ 3 tests | sorry rejection, commentary rejection |
| `strip_theorem_header` | ✅ 2 tests | have-block preservation |
| `strip_fence_lang_specifier` | ✅ 1 test | New in this session |
| `has_proof_body` | ✅ 1 test | Markdown rejection |
| `is_proof_body` | ✅ 2 tests | Commentary rejection, `#` rejection |
| `build_*` prompts | ✅ 7 tests | All 5 formats covered |
| `make_proof_file` | ✅ 2 tests | With/without informal_prefix |
| `checkpoint` CRUD | ✅ 4 tests | Create, mark, resume, multiple |
| `decode_llama_byte_fallback` | ❌ 0 tests | Should test with known garbled output |
| `generate_one_with_retry` | ❌ 0 tests | Requires mock HTTP |
| `write_flat_json` | ❌ 0 tests | Simple, could add round-trip test |
| `load_existing_results` | ❌ 0 tests | Could add round-trip with write_flat_json |
| `InferenceEngine::start` | ❌ 0 tests | Requires vLLM |

## Automated Checks

```
cargo fmt --check    ✅ PASS
cargo clippy -D warn ✅ PASS (0 warnings)
cargo test           ✅ PASS (40/40)
```
