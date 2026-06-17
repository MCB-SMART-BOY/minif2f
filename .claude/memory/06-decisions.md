---
name: architecture-decisions
description: ADR — Architecture Decision Records for key design choices
layer: reference
metadata:
  type: reference
---

# 06 — Architecture Decision Records

## ADR-001: vLLM as Primary Inference Engine

**Date**: 2026-06-04
**Status**: Accepted (with exception for STP)

**Decision**: Use vLLM API Server with FP8 quantization for 5 of 6 models. STP_model_Lean uses HuggingFace `model.generate()` as a separate Python script.

**Rationale**:
- vLLM provides continuous batching (`buffer_unordered`), keeping GPU at 90%+ utilization
- FP8 quantization halves VRAM usage (~7GB per 7B model), leaving ~24GB for KV cache
- Single unified pipeline for 5 models reduces code complexity
- **Exception**: STP model requires `begin_suppress_tokens` which vLLM does not support. Using HF `model.generate()` with BF16 native precision for this model only.

**Alternatives considered**:
- All HF generate: too slow, no continuous batching
- All vLLM: STP produces 0% output (EOS immediately)

## ADR-002: Continuous Request Pool (buffer_unordered)

**Date**: 2026-06-04
**Status**: Accepted

**Decision**: Use `buffer_unordered(N)` with NO per-theorem barrier. All theorem×attempt requests flow through a single stream with N concurrent HTTP requests.

**Rationale**:
- Per-theorem barrier caused GPU utilization drops to 4-8% between theorems
- buffer_unordered keeps N requests in flight at all times — GPU saturated
- Results arrive in completion order, batched per-theorem in a BTreeMap

**Trade-off**: Slightly more complex accumulation logic (BTreeMap batches), but the GPU utilization gain is worth it.

## ADR-003: Architecture-Conditional Byte-Fallback Decoder

**Date**: 2026-06-13
**Status**: Accepted

**Decision**: Apply `decode_llama_byte_fallback()` only to LLaMA-based architectures (`raw`, `deepseek_v2`, `deepseek_coder`). Qwen3 architecture passes through unchanged.

**Rationale**:
- LLaMA tokenizer encodes raw bytes 0x00-0xFF as U+0100-U+01FF (byte-fallback)
- Qwen2Tokenizer outputs standard UTF-8 — decoding would corrupt legitimate Latin Extended characters
- Applying decoder universally caused 30.1% U+FFFD in Distill outputs, 85.7% in DPO

**Implementation**: `generate_one_with_retry()` accepts `architecture` parameter.

## ADR-004: FP8 Quantization for All vLLM Models

**Date**: 2026-06-04
**Status**: Accepted

**Decision**: Run all 5 vLLM models with `--quantization fp8 --dtype half`.

**Rationale**:
- BF16 weights → FP16 → FP8 at load time via vLLM
- Halves VRAM vs BF16, enabling higher concurrency
- Kimina official blog uses vLLM + FP8 — precedent for correctness

**Known issue**: BF16→FP16→FP8 pipeline may slightly alter output distributions vs official BF16 evaluations. Not quantified.

## ADR-005: STP Separate Python Script

**Date**: 2026-06-13
**Status**: Accepted

**Decision**: Run STP_model_Lean via standalone `scripts/stp_runner.py` using HF `model.generate()` with BF16 native precision.

**Rationale**:
- vLLM produces 0% output for STP (model generates EOS immediately without `begin_suppress_tokens`)
- HF `model.generate()` respects `begin_suppress_tokens: [100000, 100001, 100002]` from config.json
- STP outputs are very short (~50-200 tokens), so serial generation is fast enough
- Output format matches vLLM pipeline exactly for downstream compatibility

## ADR-006: Incremental JSON Writes Every 20 Theorems

**Date**: 2026-06-04
**Status**: Accepted

**Decision**: Write output JSONs to disk every 20 theorems, independent of checkpoint system.

**Rationale**:
- Checkpoint files only record theorem names, not proof data
- A crash after generating 100 theorems but before final JSON write would lose all 100
- 20-theorem granularity means worst-case data loss is 19 theorems × 128 attempts = 2,432 proofs

## ADR-007: `find` Not `rfind` for Theorem Header Stripping

**Date**: 2026-06-05
**Status**: Accepted

**Decision**: Use `find` (first occurrence) for `:= by` matching in `strip_theorem_header`, not `rfind` (last occurrence).

**Rationale**:
- `have h : x = y := by ...; rw [h]` — `rfind` matches the inner `:= by`, stripping everything before it and losing the `have` statement
- `find` matches the theorem's `:= by`, preserving nested `have ... := by` blocks
