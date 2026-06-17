# Project Knowledge Index

## Always-Load (🔴 session-critical)
These define the project and must be loaded at the start of every session.

- [[00-identity]]     — Project: 6 models × 488 theorems × 128 attempts → Pass@128 eval
- [[01-architecture]] — Code structure: file map, data flow, module dependencies
- [[02-models]]       — 6-model registry: official specs, prompt formats, sampling params, EOS tokens, HF URLs, paper citations

## On-Demand (🟡 task-specific)
Load when the current task touches these areas.

- [[03-pipeline]]     — Pipeline internals: buffer_unordered + rayon extraction + checkpoint + incremental writes
- [[04-hardware]]      — RTX 5090 32GB CUDA, vLLM FP8, per-model --max-num-seqs values
- [[05-quality]]       — Quality gates (cargo fmt/clippy/test 69/69), output validation criteria, git style

## Reference-Only (🟢 cited explicitly)
Only load when specifically referenced.

- [[06-decisions]]     — ADR: vLLM vs HF generate, continuous request pool, byte-fallback decoder, STP decision

## External Documentation

- [[CLAUDE.md]]        — Primary project instructions + model config summary
- [[ARCHITECTURE.md]]  — Full function-level architecture (all source files, end-to-end data flow)

## Archive
Historical incidents and old audit reports — not loaded into context:

- [incidents/2026-06-05-dpo-empty.md](archive/incidents/)
- [incidents/2026-06-05-kimina-think.md](archive/incidents/)
- [incidents/2026-06-10-checkpoint-loss.md](archive/incidents/)
- [code-audit-2026-06-11.md](archive/code-audit-2026-06-11.md)
