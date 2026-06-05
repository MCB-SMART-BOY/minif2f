# check-status

Check proof generation progress across all models.

```
./run → 5) Check Status
cargo run -- status --run-id <id>
```

## Check GPU

```bash
nvidia-smi
```

## Runtime management

```bash
kill $(pgrep minif2f)           # stop generation
cargo run -- ... --run-id <id>  # resume (same ID; prior results preserved)
```

## What gets checked

- `results/checkpoints/<model>__<run_id>.json` — completed theorem count
- Checkpoint format: JSON array of theorem names
- Resume: loads existing `output/<model>.json`, skips completed theorems
