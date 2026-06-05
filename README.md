# minif2f — LLM Theorem Proof Generator

Generate theorem proofs for [miniF2F](https://github.com/openai/miniF2F) (488 theorems) using 6 Lean 4 theorem-proving LLMs. Output is a nested JSON file per model.

**Stack**: Pure Rust + llama.cpp (inference). Zero Python at runtime.

## Quick Start

```bash
# New machine: one-command setup
./scripts/setup.sh

# Generate proofs (interactive menu)
./run

# Or directly:
cargo run --release -- generate -m kimina-prover-rl-1.7b -p models/kimina-1.7b.gguf --run-id v1

# Check progress
cargo run -- status --run-id v1
```

## Workflow

```
Setup (one-time)  →  Generate (per model)  →  JSON Output
./scripts/setup.sh           ./run → Generate         output/<model>.json
```

## Commands

```bash
cargo run -- list-models                        # Show 6 model configs
cargo run -- generate -m <model> -p <gguf>       # Generate proofs → JSON
cargo run -- status --run-id <id>               # Checkpoint progress
```

### Generate Options

```
-m, --model <NAME>       Model name (required)
-p, --model-path <PATH>  Path to GGUF file (required)
--run-id <ID>           Checkpoint ID [default: default]
```

## Output

```
output/<model>.json
```

```json
{
  "kimina-prover-rl-1.7b": {
    "amc12a_2019_p21": {
      "attempt_1": "import Mathlib\nimport Aesop\n..."
    }
  }
}
```

## Project

```
src/
├── main.rs          # CLI (clap)
├── lib.rs           # Modules (8)
├── config.rs        # ModelConfig, PipelineConfig
├── models.rs        # 6-model registry
├── data.rs          # Dataset + Theorem (3 tests)
├── prompts.rs       # Chat templates + proof extraction
├── inference.rs     # llama-server manager
├── checkpoint.rs    # Crash recovery
└── pipeline.rs      # Orchestrator → JSON
```

## Supported Models

| Model | Architecture | Chat Template | Prompt | ctx | Status |
|-------|-------------|---------------|--------|-----|--------|
| kimina-prover-rl-1.7b | Qwen3 | ChatML + nothink | kimina | 8192 | ✅ GGUF ready |
| goedel-prover-v2-8b | Qwen3 | ChatML + nothink | goedel_v2 | 8192 | ❌ |
| deepseek-prover-v2-7b | DeepSeek V2 | Unicode ｜ | goedel_v2 | 8192 | ❌ |
| kimina-prover-distill-8b | Qwen3 | ChatML + nothink | kimina | 8192 | ❌ |
| goedel-prover-dpo | DeepSeek Coder | ### | simple | 4096 | ❌ |
| stp-model-lean | DeepSeek Coder | ### | simple | 4096 | ❌ |

## Design

- **Disable thinking**: Qwen ChatML injects empty `<think>\n\n</think>\n\n` block (`enable_thinking=false`), so models output Lean code directly
- **Context**: 8192 tokens for most models (HF official specs), 4096 for DeepSeek Coder models
- **Output**: Nested JSON `{model: {theorem: {attempt_N: proof}}}` — flat in `output/`
- **Crash recovery**: `results/checkpoints/<model>__<run_id>.json` — resume with `--run-id`

## Hardware

- **GPU**: RTX 4060 Laptop 8 GB, Vulkan backend, ~59 tok/s
- **1.7B FP16**: ~3.2 GB VRAM. **7-8B**: Q4_K_M GGUF ~4-5 GB
- **Per theorem**: ~25s. **All 488**: ~3.4 hours

## Quality

```bash
cargo fmt --check          # clean
cargo clippy -- -D warnings  # 0 warnings
cargo test                 # 3/3 passed
```
