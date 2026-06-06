# generate-proofs

Run proof generation for one or all models. Output: `output/raw_output/<model>.json` + `output/lean_code/<model>.json`. 488 theorems × 128 attempts × 6 models.

## Architecture

**Continuous request pool** — NO per-theorem barrier. All 488×128 jobs flow through `buffer_unordered(parallel)`, keeping N HTTP requests in flight. Results arrive in completion order, batched per theorem. When a batch reaches 128, `rayon::par_iter()` runs parallel proof extraction + validation across CPU cores. Incremental JSON writes every 20 theorems.

## Key architectural notes

- **Qwen3 models (kimina, goedel-v2)**: Model generates `<think>reasoning</think>` naturally via RL-trained format reward. Do NOT prepopulate think blocks — breaks reasoning chain.
- **Goedel-Prover-DPO (deepseek_coder + simple)**: Prepopulated `### Response:\n\`\`\`lean4\n{code}` WITHOUT closing ```. **CRITICAL**: strip trailing ``` from prepopulated content — if model sees a closed code block, it outputs EOS (72% empty). Fixed in `src/prompts.rs`.
- **Goedel-V2 format**: Theorem statement includes `sorry` placeholder (official format). User message only, no system prompt.
- **Kimina / Simple / STP**: No `sorry` — model generates directly from `:= by`.
- **DeepSeek-V2 format**: Unicode fullwidth `｜` (U+FF5C). No system prompt. BOS added automatically by tokenizer.
- **STP (raw architecture)**: No chat template. `max_model_len=1024`, `max_tokens=1024`. No informal prefix (context is tight).
- **buffer_unordered(N)**: Keeps N HTTP reqs in flight. GPU stays saturated (~73% SM util for Q4_K_M — memory-bandwidth bound, not compute).
- **rayon::par_iter**: Parallel proof extraction across CPU cores. Sequential BTreeMap insert after.
- **validate_lean_code**: 8-layer check — has `:= by`, no `sorry`, ≥2 chars tactics, no markdown/chat artefacts, `is_proof_body`, `strip_block_comments`.
- **Incremental writes**: JSON written every 20 theorems — crash resilience independent of checkpoint system.
- **Checkpoint resume**: Existing raw_output + lean_code JSON loaded on restart. No data loss.
- **Proof extraction**: `find` (not `rfind`) preserves nested `have ... := by`.

## Usage

```
./run → 6) Generate Proofs (single model)
./run → 7) Generate All Models (sequential, tmux)
```

### Manual

```bash
cargo run --release -- generate -m <model> -p <gguf> [-n 128] [--parallel <n>] [--port 8080] [--run-id <id>]
```

## Models and --parallel values (RTX 5090 32GB)

Q4_K_M models are memory-bandwidth bound (~1.7 TB/s). 7B Q4_K_M ~4.5 GB → single-stream max ~378 t/s. 16-way parallel drops to ~65-78 t/s per slot. Optimal parallel maximizes total throughput (p × per_slot_tps), not VRAM utilization.

| Model | Arch | Size | --parallel | ctx-size | Per-slot | Reality |
|-------|------|------|-----------|----------|----------|---------|
| goedel-prover-dpo | LLaMA-7B | 7B | **16** | 65536 | 4096 | ~70 t/s per slot |
| deepseek-prover-v2-7b | LLaMA-7B | 7B | **7** | 86016 | 12288 | ~70 t/s per slot |
| kimina-prover-rl-1.7b | Qwen3 | 1.7B FP16 | **24** | 292608 | 12192 | ~200+ t/s per slot |
| goedel-prover-v2-8b | Qwen3 | 8B | **8** | 294912 | 36864 | ~60 t/s per slot |
| kimina-prover-distill-8b | Qwen3 | 8B | **24** | 292608 | 12192 | ~40 t/s per slot |

**LLaMA-7B** (no GQA, kv=256KB/tok): p=16 sweet spot. p=22 caused 4.5× per-slot slowdown (memory bandwidth contention).
**Qwen3** (GQA, kv=64KB/tok): can push higher parallel, less memory pressure.

## ctx-size formula

llama-server divides ctx by parallel for per-slot context:
```rust
let per_slot = (config.max_tokens + 4096).min(config.max_model_len);
let ctx = per_slot * parallel;
```

## JSON format

Two directories, same flat format:
```json
{"<model>": {"<theorem>": {"attempt_1": "...", ..., "attempt_128": "..."}}}
```

- `output/raw_output/<model>.json` — unfiltered model completions
- `output/lean_code/<model>.json` — extracted + validated Lean proofs ("" if invalid)

## Resume

Same `--run-id` preserves prior results and skips completed theorems:
```bash
cargo run -- status --run-id <id>
cargo run -- generate -m <model> -p <gguf> --run-id <id>  # resumes
```

## Quality gates

```bash
cargo fmt --check && cargo clippy -- -D warnings && cargo test  # 34/34
```

## Full architecture

See `ARCHITECTURE.md` for complete function-level documentation.
