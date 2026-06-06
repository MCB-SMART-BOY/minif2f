# pipeline-monitor

Real-time pipeline monitoring — GPU, throughput, output quality, and ETA. Use during long-running generation to catch issues early.

## Health dashboard (one-liner)

```bash
echo "=== $(date) ==="
echo "GPU: $(nvidia-smi --query-gpu=utilization.gpu,memory.used,memory.total --format=csv,noheader)"
echo "Completions: $(grep -c 'stop processing:' /tmp/llama-server-8080.log 2>/dev/null)"
echo "Throughput: $(grep 'n_decoded' /tmp/llama-server-8080.log 2>/dev/null | tail -5 | grep -oP 'tg =\s*\d+\.\d+' | awk -F'= ' '{sum+=$2; c++} END {printf "%.0f t/s avg (n=%d)", sum/c, c}')"
echo "Avg n_tokens: $(grep 'n_tokens' /tmp/llama-server-8080.log 2>/dev/null | tail -100 | grep -oP 'n_tokens = \d+' | awk -F'= ' '{sum+=$2; c++} END {printf "%.0f (n=%d)", sum/c, c}')"
echo "Truncated: $(grep -c 'truncated = 1' /tmp/llama-server-8080.log 2>/dev/null)"
echo "Progress: $(tmux capture-pane -t minif2f-gen -p 2>/dev/null | grep Generating | tail -1)"
```

## Output quality snapshot

```bash
python3 << 'PYEOF'
import json, os

# Check all existing output files
for ftype in ['raw_output', 'lean_code']:
    dirpath = f'output/{ftype}'
    if not os.path.exists(dirpath):
        continue
    for fname in sorted(os.listdir(dirpath)):
        if not fname.endswith('.json'):
            continue
        try:
            d = json.load(open(os.path.join(dirpath, fname)))
            model = list(d.keys())[0]
            theorems = d[model]
            total = sum(len(attempts) for attempts in theorems.values())
            # Count non-empty: len > 10 chars (ignore whitespace-only)
            nonempty = sum(
                1 for attempts in theorems.values()
                for v in attempts.values()
                if isinstance(v, str) and len(v.strip()) > 10
            )
            avg_len = sum(
                len(v.strip()) for attempts in theorems.values()
                for v in attempts.values()
                if isinstance(v, str)
            ) / max(total, 1)
            print(f'{ftype:12s} {fname:40s} {nonempty:6d}/{total} non-empty ({100*nonempty/max(1,total):5.1f}%) avg_len={avg_len:.0f}')
        except Exception as e:
            print(f'{ftype:12s} {fname:40s} ERROR: {e}')
PYEOF
```

## Red flags to watch

| Symptom | Action |
|---------|--------|
| `truncated = 1` count rising | Increase ctx-size or reduce max_tokens |
| avg n_tokens < 300 | Model hitting EOS immediately — check prompt format |
| avg n_tokens near max_tokens | Output being truncated — increase max_tokens |
| GPU utilization < 30% | Server may be idle (check `fuser 8080/tcp`) |
| Non-empty % < 20% | Prompt format likely wrong — use debug-prompt skill |
| VRAM > 95% | Reduce parallel or ctx-size (OOM risk) |
| Per-slot t/s dropping | Memory bandwidth contention — reduce parallel |

## ETA estimation

```bash
# Compute from actual rate
total_completions=$(grep -c 'stop processing:' /tmp/llama-server-8080.log 2>/dev/null)
server_uptime_sec=$(ps -o etimes= -p $(pgrep -f "llama-server.*8080") 2>/dev/null | tr -d ' ')
if [ -n "$server_uptime_sec" ] && [ "$total_completions" -gt 100 ]; then
    rate=$(echo "scale=1; $total_completions / $server_uptime_sec" | bc 2>/dev/null || echo "?")
    remaining=$((62464 - $(tmux capture-pane -t minif2f-gen -p 2>/dev/null | grep -oP '\d+(?=/\d+)' | head -1 || echo 0)))
    eta_sec=$(echo "$remaining / $rate" | bc 2>/dev/null || echo "?")
    echo "Rate: ${rate}/s, Remaining: ${remaining}, ETA: ${eta_sec}s ($(echo "scale=1; $eta_sec/3600" | bc 2>/dev/null)h)"
fi
```

## Server restart (if hung)

```bash
# Kill and restart cleanly
fuser -k 8080/tcp 2>/dev/null
sleep 2
# Resume from checkpoint:
cd /root/autodl-tmp/minif2f
RUN_ID_PREFIX="v128-$(date +%Y%m%d)-resume" bash scripts/generate-all.sh
```
