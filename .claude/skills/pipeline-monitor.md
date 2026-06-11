# pipeline-monitor

Real-time pipeline monitoring — GPU, throughput, output quality, and ETA.

## Health dashboard

```bash
echo "=== $(date) ==="
echo "GPU: $(nvidia-smi --query-gpu=utilization.gpu,memory.used,memory.total --format=csv,noheader)"
echo "vLLM process: $(ps aux | grep 'server.py' | grep -v grep | awk '{print $2, $11, $12}')"
echo "Generator: $(ps aux | grep 'minif2f generate' | grep -v grep | grep -oP '\-m \S+')"
echo "Progress: $(tmux capture-pane -t minif2f-gen -p 2>/dev/null | grep Generating | tail -1)"
```

## Output quality snapshot

```bash
# Current model's proof validity rate
python3 << 'PYEOF'
import json, os, glob
# Find the most recently modified lean_code output
files = glob.glob("output/lean_code/*.json")
if files:
    latest = max(files, key=os.path.getmtime)
    model = os.path.basename(latest).replace('.json', '')
    with open(latest) as f:
        data = json.load(f)
    lean = data[model]
    total = sum(len(v) for v in lean.values())
    non_empty = sum(1 for t in lean for a,c in lean[t].items() if c.strip())
    print(f"Model: {model}")
    print(f"Non-empty proofs: {non_empty}/{total} ({non_empty/total*100:.1f}%)")
PYEOF
```

## Checkpoint progress (all models)

```bash
python3 << 'PYEOF'
import json, os, glob
for f in sorted(glob.glob("results/checkpoints/*.json")):
    with open(f) as fh:
        ckpt = json.load(fh)
    name = os.path.basename(f).split("__")[0]
    count = len(ckpt) if isinstance(ckpt, list) else len(ckpt.get(name, ckpt))
    print(f"{name}: {count}/488")
PYEOF
```
