# Check Status

Triggers: "进度" "状态" "怎么样" "速度"

## Phase 0: Collect (parallel sources)
1. Checkpoints: count each `results/checkpoints/*.json`
2. GPU: `nvidia-smi --query-gpu=utilization.gpu,memory.used,temperature.gpu --format=csv,noheader`
3. tmux: `tmux capture-pane -t minif2f-gen -p | tail -10`
4. Output files: `ls -lh output/raw_output/ output/lean_code/`
5. (Future) Log: `tail -50 results/logs/<run_id>.jsonl`

## Phase 1: Analyze
From tmux: current model name, progress N/M, ETA, errors
From checkpoints: actual theorems done, speed (delta vs last check)
From GPU: utilization <80% → stuck/between, temp >85°C → throttling
From output files: sizes, last-modified timestamps
From log (future): error rates by type, retry count, encoding alerts

## Phase 2: Report
```
GPU: <util>% | <used>/<total> GB | <temp>°C

| Model | Progress | ETA | Status |
|-------|:--------:|-----|:------:|
| model1 | 488 | — | ✅ |
| model2 | N/488 | Xh | 🔄 |
| model3 | 0/488 | — | ⬜ |

⚠️ Issues: [auto-detect: low GPU, HTTP errors, zero extraction]
```
