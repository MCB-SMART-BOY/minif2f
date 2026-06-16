---
name: project-purpose
description: Generate 128 proofs per theorem for miniF2F using 6 LLMs → nested JSON
metadata:
  type: project
---

Generate 128 proof attempts for each of 488 miniF2F theorems using 6 Lean 4 theorem-proving LLMs. Output is two-layer flat JSON at `output/raw_output/<model>.json` + `output/lean_code/<model>.json`.

**Output format**: `{model: {theorem: {attempt_1...attempt_128: proof_string}}}`
- `raw_output/`: unfiltered model completions
- `lean_code/`: extracted + assembled Lean proofs

**Scale**: 6 models × 488 theorems × 128 attempts = 374,784 proof generations.

Built as pure Rust CLI. Uses vLLM (Python, managed as child process via `uv run`) for GPU inference.

**Why:** Pass@128 evaluation for 6 theorem-proving models. Architecture templates and prompt formats match each model's official HF configuration.

**How to apply:** `./run → 8) Do It All` for full pipeline, or `./run` for step-by-step. Full architecture documented in `ARCHITECTURE.md`. See [[official-model-requirements]] for exact HF repos and prompt formats.
