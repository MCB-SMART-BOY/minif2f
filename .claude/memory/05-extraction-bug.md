---
name: extraction-double-header-bug
description: lean_code assembly bug — extracted full theorem block re-wrapped by make_proof_file produces double theorem header, validate rejects. raw_output is clean.
metadata:
  type: project
---

## Extraction/Assembly Bug (discovered 2026-06-21)

**Symptom**: goedel-prover-v2-8b lean_code extraction rate 1.4% despite clean raw_output.

**Root cause** (`pipeline.rs:273-285`):
- `extract_proof()` (prompts.rs) returns the FULL ```lean4 block including the
  `theorem ... := by` header line — NOT just the proof body.
- Assembly logic: `lean = if proof.contains("import ") { proof } else { make_proof_file(proof) }`.
- The `contains("import ")` check is a fragile proxy. It only holds when the model
  re-emits `import Mathlib` inside the block (Kimina models do; Goedel-V2/DeepSeek do not).
- When the block has NO import, `make_proof_file()` prepends header + formal_statement
  AGAIN in front of a block that already contains the theorem statement →
  **DOUBLE `theorem ... := by` header** → first `:= by` is followed by a second
  `theorem` line → `is_proof_body` rejects → `validate_lean_code` returns "" .

**Why models differ** (sampled ~4000 attempts each):
| Model | block has `import ` | extract path | affected? |
|-------|--------------------|--------------|-----------|
| kimina-distill/rl | ~90% yes | proof used as-is | mostly OK |
| goedel-v2 | 0% | make_proof_file → double header | DEVASTATED (1.4%) |
| deepseek-v2 | 0% | make_proof_file (single-line `:=by` masks it) | partial |
| goedel-dpo | n/a (no ```lean4 block, raw completion) | extract_lean_from_text → body only → make_proof_file correct | OK |

**Goedel-V2 model behavior is CORRECT**: 97.6% of outputs contain a complete
proof block (no sorry). The model emits TWO ```lean4 blocks — first a sorry
skeleton (`### Lean 4 have Statements`), then the complete proof
(`### Complete Lean 4 Proof`). `extract_fenced_code` picks the longest block →
correctly selects the complete proof. The bug is purely in re-assembly.

**KEY INSIGHT — raw_output is the irreplaceable asset; lean_code is derived.**
- This applies to QWEN3 models only (goedel-v2, kimina×2). Their raw is clean.
- ⚠️ CORRECTION: LLaMA-arch raw (goedel-dpo, deepseek-v2) is NOT clean — corrupted
  at write time by the decoder bug. See [[decoder-gpt2-bytefallback-bug]].
- For qwen3: lean_code re-extractable offline at ZERO GPU cost. DO NOT rerun.
- goedel-v2: let it finish, fix extractor after, re-extract offline.

**Fix sketch** (validate on REAL Rust, not Python port — fallback paths differ):
- When extracted proof already contains a theorem/lemma declaration, prepend ONLY
  the header (imports/opens/set_option), NOT formal_statement (block already has it).
- When it's a pure body (DPO fallback path), keep make_proof_file as-is.
- Naive `has_decl → prepend header only` BROKE goedel-dpo (-73.8%) in Python test —
  the rule must distinguish extraction paths. Needs careful per-path handling +
  full regression on all 5 completed models before applying.
- Projected after correct fix: goedel-v2 1.4%→~88%, others unchanged.

Related: [[stp-runner-walrus-bug]], [[02-models]]
