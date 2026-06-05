# minif2f — LLM Theorem Proof Generator

Generate 128 proof attempts for each of [miniF2F](https://github.com/openai/miniF2F)'s 488 theorems using 6 Lean 4 theorem-proving LLMs. Output is a nested JSON file per model.

**Stack**: Pure Rust + llama.cpp (inference). Zero Python at runtime.

## Quick Start

```bash
# New machine: one-command setup
./scripts/setup.sh

# Everything: setup → quality → build → generate all 6 models
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
7) Generate All Models (tmux background, sequential, 128 attempts)
8) Do It All (setup → quality → build → generate all)
```

## Commands

```bash
cargo run -- list-models
cargo run -- generate -m <model> -p <gguf>                         # defaults: -n 128 --parallel 8
cargo run -- generate -m <model> -p <gguf> -n 64 --parallel 12    # custom
cargo run -- status --run-id <id>
```

### Generate Options

```
-m, --model <NAME>       Model name (required)
-p, --model-path <PATH>  Path to GGUF file (required)
--port <PORT>           llama-server port [default: 8080]
--run-id <ID>           Checkpoint ID [default: default]
-n, --attempts <N>      Attempts per theorem [default: 128]
--parallel <N>          llama-server parallel slots [default: 8]
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
      "attempt_128": "..."
    }
  }
}
```

## Project Structure

```
├── run                    # Entry point (interactive menu)
├── scripts/
│   ├── setup.sh           # One-time deployment
│   └── generate-all.sh    # Sequential batch generation (tmux)
└── src/
    ├── main.rs            # CLI (clap)
    ├── lib.rs             # Modules
    ├── config.rs          # ModelConfig, PipelineConfig
    ├── models.rs          # 6-model registry
    ├── data.rs            # Dataset + Theorem
    ├── prompts.rs         # Chat templates + proof extraction
    ├── inference.rs       # llama-server manager
    ├── checkpoint.rs      # Crash recovery
    └── pipeline.rs        # Orchestrator → JSON
```

## Supported Models

| Model | Architecture | Chat Template | Prompt | ctx | Status |
|-------|-------------|---------------|--------|-----|--------|
| kimina-prover-rl-1.7b | Qwen3 | ChatML | kimina | 8192 | ✅ |
| goedel-prover-v2-8b | Qwen3 | ChatML | goedel_v2 | 8192 | ✅ |
| deepseek-prover-v2-7b | DeepSeek V2 | Unicode ｜ | goedel_v2 | 8192 | ✅ |
| kimina-prover-distill-8b | Qwen3 | ChatML | kimina | 8192 | ✅ |
| goedel-prover-dpo | DeepSeek Coder | ### | simple | 4096 | ✅ |
| stp-model-lean | DeepSeek Coder | ### | simple | 2048 | ✅ |

## Design

- **Deep thinking (Qwen3 models)**: Model generates `<think>reasoning</think>` naturally (RL-trained format reward). Empty think block breaks the model — removed after audit discovered 57.6% duplicate outputs.
- **`sorry` placeholder**: Goedel-V2 and Simple formats include `sorry` in theorem statement, matching official HF prompt format.
- **Proof extraction**: Multi-strategy with validation — rejects header-only code blocks, strips markdown commentary.
- **Checkpoint resume**: Loads existing output JSON on startup, preserves prior results. No data loss on restart.
- **Context**: 8192 for most models (HF official specs), 4096/2048 for DeepSeek Coder models
- **Output**: Nested JSON `{model: {theorem: {attempt_N: proof}}}` — flat in `output/`
- **128 attempts**: Default. Configurable via `-n`. Used for Pass@k evaluation
- **Sequential generation**: One model at a time — loads, generates, unloads, next starts
- **Slots**: `--parallel 8` (default), fits 5090 32GB. 8 concurrent requests per batch
- **Crash recovery**: `results/checkpoints/<model>__<run_id>.json` — resume with `--run-id`

## Hardware

- **GPU**: RTX 4060 8GB (Vulkan, `--parallel 2`) / RTX 5090 32GB (CUDA, `--parallel 8`)
- **1.7B FP16**: ~3.2 GB VRAM. **7-8B**: Q4_K_M GGUF ~4-5 GB
- **Per theorem × 1**: ~25s. **488 × 128 × --parallel 8**: ~2.3 days/model

## Quality

```bash
cargo fmt --check          ✅
cargo clippy -- -D warnings  ✅
cargo test                 ✅ 23/23
```
