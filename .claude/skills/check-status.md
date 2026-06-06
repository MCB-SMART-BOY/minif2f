# check-status

Check proof generation progress across all models — GPU, throughput, output quality.

## Quick status

```bash
# Pipeline progress (tmux)
tmux capture-pane -t minif2f-gen -p | tail -5

# GPU state
nvidia-smi --query-gpu=utilization.gpu,utilization.memory,memory.used,memory.total,temperature.gpu --format=csv,noheader

# Server log (throughput, tokens)
grep "n_decoded" /tmp/llama-server-8080.log | tail -5
grep -c "stop processing:" /tmp/llama-server-8080.log  # total completions
```

## Output quality check

```bash
python3 -c "
import json
for model in ['goedel-prover-dpo','deepseek-prover-v2-7b','kimina-prover-rl-1.7b','goedel-prover-v2-8b','kimina-prover-distill-8b']:
    for ftype in ['raw_output', 'lean_code']:
        try:
            d = json.load(open(f'output/{ftype}/{model}.json'))
            data = d.get(model, {})
            total = sum(len(v) for v in data.values())
            nonempty = sum(1 for v in data.values() for p in v.values() if p.strip())
            print(f'{model:30s} {ftype:12s}: {nonempty:6d}/{total} non-empty ({100*nonempty/max(1,total):5.1f}%)')
        except: pass
"
```

## Throughput analysis

```bash
# Average tokens per completion
grep "n_tokens" /tmp/llama-server-8080.log | grep -oP 'n_tokens = \d+' | awk -F'= ' '{sum+=$2; c++} END {printf "avg n_tokens: %.0f (n=%d)\n", sum/c, c}'

# Generation speed per slot
grep "n_decoded" /tmp/llama-server-8080.log | tail -10 | grep -oP 'tg =\s*\d+\.\d+' | awk -F'= ' '{sum+=$2; c++} END {printf "avg tg: %.1f t/s (n=%d)\n", sum/c, c}'

# Completions per minute (approximate)
echo "Completions: $(grep -c 'stop processing:' /tmp/llama-server-8080.log)"
```

## Checkpoint status

```bash
# List completed theorems per model
for f in results/checkpoints/*.json; do
    echo "$f: $(python3 -c "import json; d=json.load(open('$f')); print(len(d))") theorems done"
done
```

## Runtime management

```bash
tmux attach -t minif2f-gen     # view running generation
Ctrl-B d                        # detach
fuser -k 8080/tcp               # kill llama-server
tmux kill-session -t minif2f-gen # kill pipeline
cargo run -- generate ... --run-id <id>  # resume (same ID)
```

## What gets checked

- `results/checkpoints/<model>__<run_id>.json` — completed theorem names (JSON array)
- Per-theorem checkpoint: triggered when all 128 attempts for a theorem complete
- Resume: loads existing `output/raw_output/<model>.json` and `output/lean_code/<model>.json`, merges (raw, lean) tuples, skips completed theorems
- Incremental writes: JSONs written every 20 theorems independently of checkpoint system

See `ARCHITECTURE.md` for full pipeline documentation.
