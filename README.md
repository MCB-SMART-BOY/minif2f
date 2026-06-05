# minif2f — LLM Theorem Proof Generator

Generate theorem proofs for [miniF2F](https://github.com/openai/miniF2F) (488 theorems) using 6 Lean 4 theorem-proving LLMs. Output is a nested JSON file per model with 128 proof attempts per theorem.

**Stack**: Pure Rust + llama.cpp (inference). Zero Python at runtime.

## Quick Start

```bash
# New machine: one-command setup
./scripts/setup.sh

# Everything (setup → quality → build → generate all 6 models)
./run  → 8) Do It All

# Or step by step:
./run              # Interactive menu
```

## Menu

```
1) Setup Environment
2) List Models
3) Quality Gates (fmt + clippy + test)
4) Build Project
5) Check Status
6) Generate Proofs (single model)
7) Generate All Models (tmux, 6 models, 128 attempts)
8) Do It All (setup → quality → build → generate all)
```

## Commands

```bash
cargo run -- list-models                        # Show 6 model configs
cargo run -- generate -m <model> -p <gguf>       # Generate proofs → JSON
cargo run -- generate -m <model> -p <gguf> -n 64 # Custom attempts
cargo run -- status --run-id <id>               # Checkpoint progress
```

### Generate Options

```
-m, --model <NAME>       Model name (required)
-p, --model-path <PATH>  Path to GGUF file (required)
--port <PORT>           llama-server port [default: 8080]
--run-id <ID>           Checkpoint ID [default: default]
-n, --attempts <N>      Attempts per theorem [default: 128]
```

## Output

```
output/<model>.json
```

```json
{
  "kimina-prover-rl-1.7b": {
    "amc12a_2019_p21": {
      "attempt_1": "import Mathlib\n...",
      "attempt_2": "...",
      ...
      "attempt_128": "..."
    }
  }
}
```

## Project

```
├── run                    # Entry point
├── scripts/
│   ├── setup.sh           # One-time deployment
│   └── generate-all.sh    # Batch generation (tmux, 6 models)
└── src/
    ├── main.rs            # CLI (clap)
    ├── lib.rs             # Modules (8)
    ├── config.rs          # ModelConfig, PipelineConfig
    ├── models.rs          # 6-model registry
    ├── data.rs            # Dataset + Theorem (3 tests)
    ├── prompts.rs         # Chat templates + proof extraction
    ├── inference.rs       # llama-server manager
    ├── checkpoint.rs      # Crash recovery
    └── pipeline.rs        # Orchestrator → JSON
```

## Supported Models

| Model | Architecture | Chat Template | Prompt | ctx | Status |
|-------|-------------|---------------|--------|-----|--------|
| kimina-prover-rl-1.7b | Qwen3 | ChatML + nothink | kimina | 8192 | ✅ GGUF ready |
| goedel-prover-v2-8b | Qwen3 | ChatML + nothink | goedel_v2 | 8192 | ❌ |
| deepseek-prover-v2-7b | DeepSeek V2 | Unicode ｜ | goedel_v2 | 8192 | ❌ |
| kimina-prover-distill-8b | Qwen3 | ChatML + nothink | kimina | 8192 | ❌ |
| goedel-prover-dpo | DeepSeek Coder | ### | simple | 4096 | ❌ |
| stp-model-lean | DeepSeek Coder | ### | simple | 2048 | ❌ |

## Design

- **Disable thinking**: Qwen ChatML injects empty `<think>\n\n</think>\n\n` block (`enable_thinking=false`)
- **Context**: 8192 tokens for most models (HF official specs), 4096/2048 for DeepSeek Coder models
- **Output**: Nested JSON `{model: {theorem: {attempt_N: proof}}}` — flat in `output/`
- **128 attempts**: Default. Configurable via `-n`. Used for Pass@k evaluation
- **Crash recovery**: `results/checkpoints/<model>__<run_id>.json` — resume with `--run-id`
- **Multi-GPU**: Use `--port` for parallel model inference

## Hardware

- **GPU**: RTX 4060 8GB (Vulkan) or RTX 5090 24GB (CUDA)
- **1.7B FP16**: ~3.2 GB VRAM. **7-8B**: Q4_K_M GGUF ~4-5 GB
- **Per theorem × 1**: ~25s. **488 × 1**: ~3.4 hours. **488 × 128**: ~18 days

## Quality

```bash
cargo fmt --check          # clean
cargo clippy -- -D warnings  # 0 warnings
cargo test                 # 21/21 passed
```
