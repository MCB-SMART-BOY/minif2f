# Check Status

Triggers: "进度" "状态" "怎么样" "速度"

## Phase 0: Collect (parallel)

Four data sources simultaneously:

1. Checkpoints: `ls results/checkpoints/` then count each
2. GPU: `nvidia-smi --query-gpu=utilization.gpu,memory.used,temperature.gpu --format=csv,noheader`
3. tmux: `tmux capture-pane -t minif2f-gen -p | tail -10`
4. Output files: `ls -lh output/raw_output/ output/lean_code/`

## Phase 1: Analyze

From tmux: extract current model name, progress N/M, ETA, any errors.

From checkpoints: compute actual theorems done, speed vs last check.

From GPU: utilization <80% → stuck/between models; temp >85°C → throttling.

## Phase 2: Report

```
GPU: <util>% | <used>/<total> GB | <temp>°C

| Model | Progress | ETA | Status |
|-------|:--------:|-----|:------:|
| model1 | 488 | — | ✅ |
| model2 | N/488 | Xh | 🔄 |

⚠️ Issues: [auto-detect anomalies: low GPU, errors, 0% extraction]
```
