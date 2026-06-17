# STP Generate

Triggers: "跑STP" "stp"

**STP does NOT use vLLM.** Uses HF `model.generate()` with BF16 native precision.
Reason: vLLM does not respect `begin_suppress_tokens` → 100% empty output.

## Phase 0: Preflight
- GPU free (STP needs ~14GB BF16)
- `scripts/stp_runner.py` exists and syntax OK
- Check existing checkpoint: `results/checkpoints/stp-model-lean__stp-hf.json`

## Phase 1: Execute
```bash
python scripts/stp_runner.py --attempts 128 --batch 4
```
- Automatically loads model (BF16), builds prompts, generates, extracts proofs
- Supports `--skip N` for resume from checkpoint
- Output format identical to vLLM models

## Phase 2: Verify
Same validation as vLLM models:
- JSON valid, 488 theorems, 128 attempts each
- U+FFFD: 0 (HF tokenizer output is clean)
- Extraction rate reported
- File sizes: raw_output/ + lean_code/
