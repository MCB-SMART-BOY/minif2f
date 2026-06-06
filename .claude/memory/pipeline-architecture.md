---
name: pipeline-architecture
description: buffer_unordered GPU feeder + rayon parallel CPU extraction → two-layer JSON with checkpoint resume + incremental writes
metadata:
  type: project
---

## Pipeline (src/pipeline.rs, tokio async)

### Architecture: buffer_unordered + rayon parallel extraction

```
┌─────────────────────────────────────────────────────────┐
│ Stage 1: GPU inference (buffer_unordered)               │
│                                                         │
│  stream::iter(all_jobs)                                 │
│    .map(|job| async { HTTP POST → llama-server })       │
│    .buffer_unordered(N)   ← N = --parallel              │
│                                                         │
│  Keeps N requests in flight. When one completes,        │
│  the next job starts immediately. GPU saturated.        │
└──────────────────────┬──────────────────────────────────┘
                       │ (theorem, attempt, raw_text)
                       ▼
┌─────────────────────────────────────────────────────────┐
│ Accumulation: BTreeMap<theorem, Vec<(idx, text)>>       │
│                                                         │
│  Results arrive in completion order. Batched per        │
│  theorem. When batch reaches 128 → flush.               │
└──────────────────────┬──────────────────────────────────┘
                       │ batch of 128
                       ▼
┌─────────────────────────────────────────────────────────┐
│ Stage 2: CPU extraction (rayon par_iter)                │
│                                                         │
│  batch.par_iter().map(|(idx, text)| {                   │
│      proof = pb.extract_proof(text)                     │
│      lean  = theorem.make_proof_file(&proof)            │
│      lean  = pb.validate_lean_code(&lean) ? lean : ""   │
│      → (idx, raw, lean)                                 │
│  })                                                     │
│                                                         │
│  Parallel across all CPU cores. Sequential BTreeMap     │
│  insert after (BTreeMap is not Sync).                   │
└─────────────────────────────────────────────────────────┘
```

### Key design decisions

- **buffer_unordered, not FuturesUnordered**: Equivalent semantics with backpressure. Jobs ordered by theorem → per-theorem batch ordering preserved.
- **rayon::par_iter**: CPU-bound extraction (string manipulation, regex-like operations) runs on rayon's global thread pool. Keeps async runtime free.
- **Sequential BTreeMap insert**: BTreeMap not Sync — insertion done sequentially after parallel extraction completes.
- **Incremental writes**: Every 20 theorems, both JSON files written (independently of checkpoint). Checkpoint only records theorem names.
- **validate_lean_code**: 8-layer check — has `:= by`, no `sorry`, has tactics ≥2 chars, no markdown/chat artefacts, is_proof_body, strip_block_comments leaves ≥2 chars.

## Architecture Routing (prompts.rs, 4 architectures)

| Architecture | Chat Template | Models |
|-------------|---------------|--------|
| `qwen3` | `<|im_start|>` ChatML | kimina, goedel-v2, distill |
| `deepseek_v2` | Unicode `｜` (U+FF5C) | deepseek-prover-v2 |
| `deepseek_coder` | `### Instruction:` / `### Response:` | goedel-dpo |
| `raw` | None (bare message) | stp-model-lean |

**DeepSeek Coder (Goedel-DPO)**: Prepopulated `### Response:\n\`\`\`lean4\n{code}` WITHOUT closing ```. Trailing ``` stripped in `src/prompts.rs` (`.strip_suffix("\`\`\`")`), otherwise model outputs EOS immediately (72% empty observed before fix).

## Proof Extraction (prompts.rs, 4 strategies + validation)

1. Fenced code after `</think>` → `strip_theorem_header(find)` → `has_proof_body`
2. Any fenced code → same pipeline
3. `extract_lean_from_text`: `line.starts_with(' ')` indentation detection
4. Last resort: strip noise → `has_proof_body` → `""` if fails
5. **validate_lean_code**: 8 checks reject incomplete/invalid proofs

## Two-Layer Output

- `output/raw_output/<model>.json` — unfiltered completions
- `output/lean_code/<model>.json` — extracted + validated proofs ("" if invalid)
- Both flat JSON: `{"<model>": {"<theorem>": {"attempt_N": "<text>"}}}`
- Incremental write every 20 theorems → crash resilience

## Script System

- `./run` — Interactive menu (8 options)
- `scripts/generate-all.sh` — **Sequential** (5 models, one at a time), per-model `--parallel`, tmux session
- `scripts/setup.sh` — One-time deployment

## Quality Gates

```bash
cargo fmt --check          ✅
cargo clippy -- -D warnings ✅
cargo test                 ✅ 34/34
```
