---
name: kimina-prover-think-mode
description: RL format reward requires <think> reasoning — empty think breaks model output
metadata:
  type: project
---

## Official RL Training Format (GRPO + DrGRPO)

Kimina-Prover models (AI-MO/Kimina-Prover-RL-1.7B, AI-MO/Kimina-Prover-Distill-8B) were trained via RL with a **format reward** that enforces:

1. Exactly one `<think>...</think>` block with reasoning + optional Lean snippets
2. Exactly one ` ```lean4 ` code block with the completed proof

**Reward goes to ZERO** (regardless of proof correctness) if format rules are violated. Checks include:
- Exact count of think/code blocks
- No repetitive/hallucinated reasoning
- Sufficient tactic content in think block
- Semantic alignment between described tactics and actual code

## The Bug (fixed 2026-06-05)

An empty `<think>\n\n</think>` was prepopulated in the Qwen3 prompt, telling the model to SKIP reasoning. This clashed with the RL-trained format reward, causing:
- 57.6% of theorems: all 128 attempts byte-identical
- 44.5% of proofs: markdown commentary mixed in
- 42.1% of proofs: truncated (header-only, no actual proof body)

## The Fix

Removed the prepopulated think block. The Qwen3 template now ends at `<|im_start|>assistant\n` and the model generates `<think>` naturally — matching the RL training distribution.

**Why:** The format reward during RL training penalizes missing/malformed think blocks. When we prepopulate an empty one, the model's training fights against our prompt — it tries to fill in reasoning but the empty block constrains it to output nothing there, causing degenerate behavior.

**How to apply:** In `prompts.rs`, Qwen3 branch: use plain `<|im_start|>assistant\n` with no `<think>` tag. The model generates its own.
