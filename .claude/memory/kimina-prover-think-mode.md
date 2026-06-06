---
name: kimina-prover-think-mode
description: RL format reward requires <think> reasoning — empty think breaks model output
metadata:
  type: project
---

## Official RL Training Format (GRPO + DrGRPO)

Kimina-Prover models follow the Kimina reasoning-then-Lean output convention. The official Kimina-Prover-RL training notes state that the RL format reward enforces:

1. Exactly one `<think>...</think>` block with reasoning + optional Lean snippets
2. Exactly one ` ```lean4 ` code block with the completed proof

For Kimina-Prover-RL rollouts, **reward goes to ZERO** (regardless of proof correctness) if format rules are violated. Checks include:
- Exact count of think/code blocks
- No repetitive/hallucinated reasoning
- Sufficient tactic content in think block
- Semantic alignment between described tactics and actual code

## The Bug (fixed 2026-06-05)

An empty `<think>\n\n</think>` was prepopulated in the Qwen3 prompt, telling Kimina models to SKIP reasoning. This clashed with the RL-trained format reward, causing:
- 57.6% of theorems: all 128 attempts byte-identical
- 44.5% of proofs: markdown commentary mixed in
- 42.1% of proofs: truncated (header-only, no actual proof body)

## The Fix

Removed the prepopulated think block. The Qwen3 template now ends at `<|im_start|>assistant\n`; Kimina models generate `<think>` naturally, matching their official output convention.

**Why:** The format reward during RL training penalizes missing/malformed think blocks. When we prepopulate an empty one, the model's training fights against our prompt — it tries to fill in reasoning but the empty block constrains it to output nothing there, causing degenerate behavior.

**How to apply:** In `prompts.rs`, Qwen3 branch: use plain `<|im_start|>assistant\n` with no `<think>` tag. The model generates its own.
