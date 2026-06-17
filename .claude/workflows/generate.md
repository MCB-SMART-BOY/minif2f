# Generate Proofs

Triggers: "跑pipeline" "生成证明" "重跑" "generate" "开始跑" "续跑"

## Phase 0: Preflight

Check each — report failure, ask whether to force:

- GPU free: `nvidia-smi --query-gpu=memory.used --format=csv,noheader`
- Port 8080 free: `! fuser 8080/tcp 2>/dev/null`
- Release current: `cargo build --release` must succeed
- No orphans: `! pgrep -f "vllm.entrypoints"`

## Phase 1: Plan

1. Determine scope from user input (all models / specific model)
2. For each model in scope: read checkpoint, determine SKIP (488) / RESUME (N) / NEW
3. Exclude STP — runs separately via `python scripts/stp_runner.py`
4. Set RUN_ID_PREFIX: reuse existing for resume, new `v128-$(date +%Y%m%d)-fix2` otherwise
5. Display ordered model list with status, estimated time, total ETA
6. Confirm with user before proceeding

## Phase 2: Execute

1. Start tmux session: `tmux new-session -d -s minif2f-gen`
2. Send command with explicit env: `export ATTEMPTS=128 RUN_ID_PREFIX='...'; bash worker.sh ...`
3. Wait vLLM ready: poll `/health` every 2s, timeout 300s
4. Wait first theorem done (checkpoint or output JSON grows)
5. Sample 3 raw_output entries, verify no encoding corruption
6. Confirm GPU >90% utilization
7. Report status and detach

## Phase 3: Verify (per model)

- JSON valid, exactly 488 theorems, exactly 128 attempts each
- U+FFFD: 0 for Qwen3, minimal for LLaMA
- Cyrillic: 0
- Non-empty rate >95%, extraction rate reported
- File sizes recorded

## Recovery

| Failure | Action |
|---------|--------|
| Shutdown | Restart with same RUN_ID_PREFIX — checkpoint auto-resumes |
| vLLM OOM | Reduce parallel in MODELS array — restart same run_id |
| Model load fail | Check HF_TOKEN, disk, model files exist |
| Encoding corrupt | Complete run — post-processing script |
| 0% extraction | Sample raw output — diagnose — fix — re-run |
