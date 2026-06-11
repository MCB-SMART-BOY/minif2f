# generate-proofs

Run proof generation for one or all models via vLLM. Output: `output/raw_output/<model>.json` + `output/lean_code/<model>.json`. 488 theorems × 128 attempts × 6 models.

## Quick start

```bash
# Single model
cargo run --release -- generate -m <model> -p data/models/<name> -n 128 --parallel <N>

# All 6 models sequentially (tmux background)
ATTEMPTS=128 bash scripts/generate-all.sh

# Resume from checkpoint
cargo run --release -- generate -m <model> -p data/models/<name> --run-id v128-20260607-vllm-<model>
```

## Architecture

**Continuous request pool** — NO per-theorem barrier. All theorem×attempt jobs flow through `buffer_unordered(parallel)`, keeping N HTTP requests in flight to vLLM's `/v1/completions`. Results arrive in completion order, batched per theorem. When a batch reaches 128, `rayon::par_iter()` runs parallel proof extraction across CPU cores. Incremental JSON writes every 20 theorems.

## Key architectural notes

- **Kimina models**: Model generates `<think>...</think>` naturally via official RL output format. Do NOT prepopulate think blocks.
- **Goedel-Prover-DPO**: Raw completion with open `\`\`\`lean4` block. temp=1.0, top_p=0.95, max_tok=2048, seed=1. See [[goedel-dpo-prompt-fix]] for historical context.
- **Goedel-V2**: Qwen3 ChatML, user-only, CoT proof plan. `sorry` placeholder. temp=0.6, max_tok=32768, seed=30.
- **DeepSeek-Prover-V2**: DeepSeek V2 ChatML, user-only, **non-CoT** (no proof plan request). max_tok=8192, seed=30. See [[official-model-requirements]].
- **STP**: Raw completion, open code block, no informal_prefix. max_model_len=1024, max_tok=1024, temp=1.0, top_p=1.0, seed=1.
- **vLLM continuous batching**: `--max-num-seqs` = `--parallel`. Requests batched dynamically — no idle slot waste.
- **FP8 quantization**: Applied at load time (`--quantization fp8`). Cuts weight VRAM in half vs BF16.
- **validate_lean_code**: 8-layer check — `:= by`, no `sorry`, ≥2 chars tactics, no markdown/chat artefacts, `is_proof_body`, `strip_block_comments`.
- **Incremental writes**: JSON written every 20 theorems — crash resilience independent of checkpoint system.

## Prompt Format Quick Reference

| Model | Format | Arch | `<think>`? | `sorry`? |
|-------|--------|------|-----------|---------|
| kimina-rl-1.7b | kimina | qwen3 | Model generates | No |
| kimina-distill-8b | kimina | qwen3 | Model generates | No |
| goedel-prover-dpo | simple | raw | No | No |
| goedel-prover-v2-8b | goedel_v2 | qwen3 | No | Yes |
| deepseek-prover-v2-7b | goedel_v2_nocot | deepseek_v2 | No | Yes |
| stp-model-lean | deepseek_prover | raw | No | No |

See [[official-model-requirements]] for exact prompt templates for every model.
