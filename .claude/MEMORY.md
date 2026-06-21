# Project Knowledge Index

## Quick Reference — Common Intents

| User says | → Trigger workflow | → Key commands |
|-----------|-------------------|----------------|
| "跑pipeline" "生成证明" | [[workflows/generate]] | `bash scripts/generate-all.sh` |
| "看进度" "速度" | [[workflows/status]] | `tmux capture-pane -t minif2f-gen -p` |
| "乱码" "空输出" "检查质量" | [[workflows/debug]] | `bash .claude/hooks/verify-output.sh <model>` |
| "修改" "修复代码" | [[workflows/code-change]] | `bash .claude/hooks/quality.sh` |
| "跑STP" | [[workflows/stp]] | `python scripts/stp_runner.py` |

## Always-Load (🔴 session-critical)
These define the project and must be loaded at the start of every session.

- [[00-identity]]     — Project: 6 models × 488 theorems × 128 attempts → Pass@128 eval
- [[01-architecture]] — Code structure: file map, data flow, module dependencies
- [[02-models]]       — 6-model registry: official specs, prompt formats, sampling params, EOS tokens, HF URLs, paper citations

## ⚠️ Active Findings (fixes applied 2026-06-21; reruns pending pipeline completion)

- [[decoder-gpt2-bytefallback-bug]] — 🔴 ROOT CAUSE. FIXED in inference.rs (GPT-2 bytes_to_unicode inverse table replaces cp-0x100). Corrupted LLaMA raw is IRREVERSIBLE → goedel-dpo + deepseek-v2 MUST rerun with fixed binary after goedel-v2 finishes.
- [[extraction-double-header-bug]] — FIXED in pipeline.rs (assemble_and_validate shared fn). qwen3 recovered offline via `re-extract` subcommand: kimina-rl + kimina-distill re-extracted 2026-06-21, double-headers 1832→0 / 449→0, ZERO genuine regressions. goedel-v2 re-extract pending its run completion.
- [[stp-runner-walrus-bug]]        — FIXED in stp_runner.py (`pos = find(); if pos != -1`) + per-batch torch.manual_seed. STP still not run — run after GPU frees.
- [[checkpoint-data-loss-window]]  — ✅ FIXED 2026-06-21. mark_done moved out of flush_batch → after write_flat_json (data durable before checkpoint). Test test_checkpoint_never_ahead_of_data. Takes effect on NEXT run (rerun dpo/deepseek), not the live goedel-v2 (old binary).
- [[extract-problem-desc-newline-bug]] — 🟡 OPEN. ends_with("-/") never matches (data ends "-/\n") → /-- -/ leaks into kimina # Problem: line. Mild; fix = trim_end first.

## ✅ Tooling added 2026-06-21
- `re-extract` CLI subcommand (main.rs) + `pipeline::re_extract_model` + `assemble_and_validate` (shared by generate & re-extract, no drift). `scripts/re-extract.sh` repaired (was calling a nonexistent subcommand). Zero-GPU lean_code recovery from clean raw_output (qwen3 only).
- ✅ `run` + `setup.sh` migrated to vLLM/safetensors stack (2026-06-21): `run` uses `data/models/<name>/` + per-model `--parallel` + STP→stp_runner.py routing + re-extract menu option; `setup.sh` provisions `tools/vllm` via `uv sync` + downloads safetensors (no GGUF). Dead weight still on disk (~31 GB: `models/*.gguf`, `tools/llama.cpp`, `tools/venv`, `tools/download_model.sh`) — safe to delete, awaiting user OK.

## On-Demand (🟡 task-specific)
Load when the current task touches these areas.

- [[03-pipeline]]     — Pipeline internals: buffer_unordered + rayon extraction + checkpoint + incremental writes
- [[04-hardware]]      — RTX 5090 32GB CUDA, vLLM FP8, per-model --max-num-seqs values
- [[05-quality]]       — Quality gates (cargo fmt/clippy/test 69/69), output validation criteria, git style

## Reference-Only (🟢 cited explicitly)
Only load when specifically referenced.

- [[06-decisions]]     — ADR: vLLM vs HF generate, continuous request pool, byte-fallback decoder, STP decision
- [[07-blueprint]]     — Future architecture: provenance, config YAML, errors, logging, backend trait, CI/CD

## External Documentation

- [[CLAUDE.md]]        — Primary project instructions + model config summary
- [[ARCHITECTURE.md]]  — Full function-level architecture (all source files, end-to-end data flow)

## Archive
Historical incidents and old audit reports — not loaded into context:

- [incidents/2026-06-05-dpo-empty.md](archive/incidents/)
- [incidents/2026-06-05-kimina-think.md](archive/incidents/)
- [incidents/2026-06-10-checkpoint-loss.md](archive/incidents/)
- [code-audit-2026-06-11.md](archive/code-audit-2026-06-11.md)
