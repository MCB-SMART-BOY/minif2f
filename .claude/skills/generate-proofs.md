# generate-proofs

Run proof generation for one or all 6 models. Output: `output/<model>.json`

## Key architectural notes

- **Qwen3 models**: Model generates `<think>reasoning</think>` naturally. Do NOT prepopulate think blocks.
- **Goedel-V2/Simple**: Theorem statements include `sorry` placeholder (official format).
- **Checkpoint resume**: Existing output JSON is loaded on restart. No data loss.
- **Proof extraction**: Multi-strategy with validation — rejects header-only code blocks, strips markdown.

## Usage

```
./run → 6) Generate Proofs (single model)
./run → 7) Generate All Models (sequential via tmux)
```

## Manual

```bash
cargo run --release -- generate -m <model> -p <gguf> [-n 128] [--parallel 8] [--port 8080]
```

## JSON format

```json
{"<model>": {"<theorem>": {"attempt_1": "...", "attempt_128": "..."}}}
```

## Resume

Same `--run-id` preserves prior results and skips completed theorems:
```bash
cargo run -- status --run-id <id>
cargo run -- generate -m <model> -p <gguf> --run-id <id>  # resumes
```

## Quality checks before generation

```bash
cargo fmt --check && cargo clippy -- -D warnings && cargo test  # 23/23
```
