# Generate Proofs

Triggers: "跑pipeline" "生成证明" "重跑" "generate" "开始跑" "续跑"

Full 8-step lifecycle. See [[07-blueprint]] for complete architecture.
Encounter problems? → [[workflows/debug]]

## Preconditions (all must pass before execution)

- [ ] GPU free: `<500 MiB used`
- [ ] Port 8080 free: `! fuser 8080/tcp`
- [ ] Release binary current: `cargo build --release`
- [ ] No orphan vLLM: `! pgrep -f vllm.entrypoints`
- [ ] Model directories exist for all planned models
- [ ] `bash .claude/hooks/pre-generate.sh` passes

**Any failure → report + ask user: force / fix / cancel**

## Phase 1: Plan

1. Determine scope: user says "all" → read generate-all.sh MODELS; says "model name" → single
2. Per model: read `results/checkpoints/<model>__<run_id>.json`
   - File missing → NEW run
   - Has 488 theorems → SKIP (already done)
   - Has N < 488 → RESUME from N
3. Exclude STP — runs via separate `python scripts/stp_runner.py` (see [[workflows/stp]])
4. Set RUN_ID_PREFIX:
   - Resume → reuse existing prefix (critical for checkpoint match)
   - New → `v128-$(date +%Y%m%d)-fix2`
5. Display plan table: model name | status | theorems to do | estimated time
6. Wait for user confirmation → "继续？[Y/n]"

## Phase 2: Execute

1. `tmux new-session -d -s minif2f-gen -c $(pwd)`
2. Send: `export ATTEMPTS=128 RUN_ID_PREFIX='...'; bash /tmp/minif2f-worker.sh ...`
3. Verify RUN_ID_PREFIX was inherited (check tmux output: `--run-id v128-...-fix2-<model>`)
4. Wait vLLM ready: `curl --noproxy '*' localhost:8080/health` every 2s, timeout 300s
5. Wait first theorem checkpointed (file appears or grows)
6. Sample 3 raw_output entries → verify no U+FFFD/Cyrillic
7. Confirm GPU >90% utilization via nvidia-smi
8. Report: "✅ Pipeline running — `tmux attach -t minif2f-gen` to watch"

## Phase 3: Verify (per model, after completion)

Run: `bash .claude/hooks/verify-output.sh <model>`

Check levels:
1. **Structure**: JSON valid, exactly 488 theorems, exactly 128 attempts each
2. **Encoding**: Qwen3 → U+FFFD=0, Cyrillic=0 | LLaMA → U+FFFD<1%, Cyrillic=0, Latin-1 recorded
3. **Quality**: non-empty rate >95%, extraction rate reported, file sizes normal
4. **Sampling**: 25 random entries → basic Lean validity check

Result classification:
- **PASS** → next model or COMPLETE
- **WARN** → note issue, continue to next model, fix later
- **ERROR** → pause, report to user, ask: continue / fix / skip
- **FATAL** → stop pipeline, mark run failed

See [[workflows/debug]] for diagnosing verification failures.

## Recovery Paths

| Failure | When | Recovery |
|---------|------|----------|
| Machine shutdown | Any time | Restart with same `RUN_ID_PREFIX` → checkpoint auto-resumes |
| vLLM OOM / crash | Mid-model | Reduce `--parallel` → restart same `RUN_ID_PREFIX` |
| Model load failure | Backend start | Check HF_TOKEN, disk space, model files exist |
| Encoding corruption found | Phase 3 verify | Complete pipeline → post-processing → see [[workflows/debug]] |
| 0% extraction rate | Phase 3 verify | Sample raw_output → diagnose → fix code → re-run model |
| Verification ERROR | Phase 3 verify | Report to user → decide: continue / fix-code / skip-model |
| tmux session killed | Mid-model | Worker continues? If yes → done. If no → checkpoint resume. |
