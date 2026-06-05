---
name: project-purpose
description: Generate 128 proofs per theorem for miniF2F using 6 LLMs → nested JSON
metadata:
  type: project
---

Generate 128 proof attempts for each of 488 miniF2F theorems using 6 Lean 4 theorem-proving LLMs. Output is nested JSON at `output/<model>.json`.

**Output format**: `{model: {theorem: {attempt_1...attempt_128: proof_string}}}`

**Scale**: 6 models × 488 theorems × 128 attempts = 374,784 proof generations.

Built as pure Rust CLI. Uses `llama-server` (C binary, managed as child process) for GPU inference. Zero Python at runtime.

**Why:** Pass@128 evaluation for 6 theorem-proving models. Architecture templates and prompt formats match each model's official HF configuration.

**How to apply:** `./run → 8) Do It All` for full pipeline, or `./run` for step-by-step.
