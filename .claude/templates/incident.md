# Incident Report

## Discovery
- **Date**: YYYY-MM-DD
- **Discovered by**: [user / Claude / automated check]
- **Symptom**: [what was observed — e.g. "all STP outputs empty", "30% U+FFFD in distill"]

## Impact
- **Models affected**: [which models]
- **Theorems affected**: [how many / all?]
- **Data corrupted**: [Y/N — how many entries, what kind of corruption]
- **Pipeline state**: [was pipeline running? completed? mid-run?]

## Root Cause
- **What went wrong**: [technical root cause]
- **Why it wasn't caught earlier**: [process gap]
- **Related code**: [files + line numbers]

## Fix
- **What was changed**: [summary]
- **Files modified**: [list with brief description]
- **Tests added/updated**: [which tests]

## Recovery
- [ ] Code fix committed and pushed
- [ ] Corrupted outputs backed up to `output/old_corrupted/`
- [ ] Checkpoints cleaned for affected models
- [ ] Pipeline restarted with fix
- [ ] Verified new outputs: U+FFFD=0, extraction rate normal

## Prevention
- **Process change**: [what process/tool/check would prevent this in future]
- **Automated check**: [can this be added to hooks/ or CI?]

## Archive
Place completed report in `.claude/archive/incidents/YYYY-MM-DD-<slug>.md`.

---

## Example (completed incident)

### Discovery
- **Date**: 2026-06-13
- **Discovered by**: Claude (encoding scan)
- **Symptom**: 30.1% U+FFFD in kimina-prover-distill-8b output; 85.7% in goedel-prover-dpo

### Impact
- **Models affected**: distill (Qwen3), DPO (LLaMA), DeepSeek (LLaMA)
- **Theorems affected**: All 488 in each affected model
- **Data corrupted**: Yes — 18805/62464 distill, 53560/62464 DPO, 8916/17920 deepseek
- **Pipeline state**: DeepSeek running, distill and DPO completed

### Root Cause
- **What went wrong**: `decode_llama_byte_fallback` was applied to ALL architectures including Qwen3. Qwen2Tokenizer outputs valid Latin Extended characters (U+0100-U+01FF) which the decoder incorrectly converted to raw bytes.
- **Why not caught earlier**: No per-architecture encoding validation in pipeline; decoder was designed for LLaMA only but not gated on architecture.
- **Related code**: `src/inference.rs:144`, `src/pipeline.rs:150`

### Fix
- **What was changed**: `generate_one_with_retry` now accepts `architecture` parameter. Only `raw|deepseek_v2|deepseek_coder` get byte-fallback decoding; Qwen3 passes through.
- **Files modified**: `src/inference.rs` (+5 lines, +5 tests), `src/pipeline.rs` (+3 lines)
- **Tests added**: `test_decode_skipped_for_qwen3`, `test_decode_applied_for_llama_architectures`, `test_qwen3_lean_proof_with_special_chars_preserved`, `test_llama_architecture_decodes_entire_range`, `test_unknown_architecture_treated_as_qwen3`

### Recovery
- [x] Code fix committed and pushed (92d23ca)
- [x] Corrupted outputs backed up to `output/old_corrupted/`
- [x] Checkpoints cleaned
- [x] Pipeline restarted with fix
- [x] Verified: DPO U+FFFD=0, Kimina-RL U+FFFD=0
