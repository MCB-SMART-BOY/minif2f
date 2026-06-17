# Generate Proofs

Triggers: "跑pipeline" "生成证明" "重跑" "generate" "开始跑" "续跑"

Full 8-step lifecycle. See [[07-blueprint]] for the complete architecture.

## Phase 0: Preflight
- GPU free: `nvidia-smi --query-gpu=memory.used --format=csv,noheader`
- Port 8080 free: `! fuser 8080/tcp 2>/dev/null`
- Release current: `cargo build --release`
- No orphans: `! pgrep -f "vllm.entrypoints"`
- Model dirs exist: check each in generate-all.sh MODELS
- Run: `bash .claude/hooks/pre-generate.sh`

## Phase 1: Plan
1. Determine scope — all models / specific
2. Per model: read checkpoint → SKIP(488) / RESUME(N) / NEW
3. STP excluded — runs separately via `python scripts/stp_runner.py`
4. Set RUN_ID_PREFIX — reuse for resume, new `v128-YYYYMMDD-fix2` otherwise
5. Display plan: ordered list + status + estimated time + total ETA
6. Confirm with user

## Phase 2: Execute
1. `tmux new-session -d -s minif2f-gen -c $(pwd)`
2. Send: `export ATTEMPTS=128 RUN_ID_PREFIX='...'; bash /tmp/minif2f-worker.sh ...`
3. Wait vLLM ready: poll `/health` every 2s, timeout 300s
4. Wait first theorem done
5. Sample 3 raw_output — no U+FFFD/Cyrillic
6. Confirm GPU >90%
7. Report status and detach

## Phase 3: Verify (per model, after completion)
1. Structure: JSON valid, 488 theorems, 128 attempts each
2. Encoding: per-architecture thresholds (Qwen3: U+FFFD=0, Llama: <1%)
3. Quality: non-empty rate, extraction rate, file sizes
4. Sampling: 5 theorems × 5 attempts — basic Lean validity
5. Result: PASS → next | WARN → note + continue | ERROR → report + ask
Run: `bash .claude/hooks/verify-output.sh <model>`

## Recovery Paths
| Failure | Action |
|---------|--------|
| Shutdown mid-run | Restart same RUN_ID_PREFIX — checkpoint auto-resumes |
| vLLM OOM | Reduce parallel in MODELS array — restart same run_id |
| Model load fail | Check HF_TOKEN, disk space, model files |
| Encoding corruption | Complete pipeline — post-processing script |
| 0% extraction | Sample raw → diagnose → fix code → re-run |
| Verification ERROR | Report to user — decide: continue / fix / skip |
