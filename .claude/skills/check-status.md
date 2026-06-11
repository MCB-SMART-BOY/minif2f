# check-status

Check proof generation progress across all models — GPU, throughput, output quality.

## Quick status

```bash
# Pipeline progress (tmux)
tmux capture-pane -t minif2f-gen -p | tail -5

# GPU state
nvidia-smi --query-gpu=utilization.gpu,utilization.memory,memory.used,memory.total,temperature.gpu --format=csv,noheader

# Check vLLM process
ps aux | grep "server.py" | grep -v grep

# Current model running
ps aux | grep "minif2f generate" | grep -v grep | grep -oP '\-m \S+'

# Check if vLLM server is healthy
curl -s http://localhost:8080/health 2>/dev/null || echo "Server not responding"
```

## Output quality check

```bash
# Sample non-empty proofs from latest model
python3 << 'PYEOF'
import json, random
model = "deepseek-prover-v2-7b"  # adjust
with open(f"output/lean_code/{model}.json") as f:
    data = json.load(f)
lean = data[model]
non_empty = [(t,a,c) for t in lean for a,c in lean[t].items() if c.strip()]
print(f"Non-empty proofs: {len(non_empty)} / {sum(len(v) for v in lean.values())}")
if non_empty:
    sample = random.sample(non_empty, min(3, len(non_empty)))
    for t,a,c in sample:
        print(f"\n--- {t}/{a} ---")
        print(c[:500])
PYEOF

# Count completed theorems in checkpoint
python3 -c "
import json
with open('results/checkpoints/kimina-prover-rl-1.7b__v128-20260607-vllm-kimina-prover-rl-1.7b.json') as f:
    ckpt = json.load(f)
print(f'{len(ckpt)}/488 theorems done')
"
```

## Throughput estimate

```bash
# Count completed attempts vs total
python3 << 'PYEOF'
import json, os, time
model = "kimina-prover-rl-1.7b"
path = f"output/raw_output/{model}.json"
if os.path.exists(path):
    with open(path) as f:
        data = json.load(f)
    thms = len(data[model])
    attempts = sum(len(v) for v in data[model].values())
    mtime = os.path.getmtime(path)
    age_h = (time.time() - mtime) / 3600
    print(f"Theorems: {thms}/488 ({thms/488*100:.1f}%)")
    print(f"Attempts: {attempts}")
    print(f"File age: {age_h:.1f}h")
PYEOF
```
