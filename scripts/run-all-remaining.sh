#!/usr/bin/env bash
# Unattended orchestration: finish ALL remaining miniF2F work in one hands-off run.
#
# Runs, in dependency order, every remaining step so the user can start it once and
# walk away (no network needed):
#   1. rerun goedel-prover-dpo      (vLLM)  — decoder-corrupted raw, full regenerate
#   2. rerun deepseek-prover-v2-7b  (vLLM)  — decoder-corrupted raw, full regenerate
#   3. run STP                      (HF)    — first run ever, separate backend
#   4. resume goedel-prover-v2-8b   (vLLM)  — capped 16384 window, resumes from checkpoint
#   5. re-extract the 3 qwen3 models (CPU)  — refresh lean_code with fixed logic
#
# Per-step failures are logged but do NOT abort the chain (each step has independent
# value). On completion, writes results/ALL_DONE.flag.
#
# Usage: bash scripts/run-all-remaining.sh        (run inside the tmux session)
#        tmux new-session -d -s minif2f-all -c <proj> 'bash scripts/run-all-remaining.sh'

set -uo pipefail   # NOT -e: a failing step must not kill the whole chain
PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_DIR"

GREEN='\033[0;32m'; RED='\033[0;31m'; CYAN='\033[0;36m'; BOLD='\033[1m'; NC='\033[0m'

ATTEMPTS=128
PORT=8080
RUN_ID_PREFIX="v128-20260613-fix2"   # MUST match for goedel-v2 resume
LOG="results/run-all-remaining.log"
mkdir -p results output/raw_output output/lean_code

log() { echo -e "$(date '+%H:%M:%S') $*" | tee -a "$LOG"; }

free_gpu() {
    # Kill any orphan vLLM + free port 8080 (one model per GPU).
    fuser -k "$PORT/tcp" 2>/dev/null || true
    pkill -f "VLLM::EngineCore" 2>/dev/null || true
    pkill -f "vllm.entrypoints" 2>/dev/null || true
    for _ in $(seq 1 30); do
        fuser "$PORT/tcp" 2>/dev/null || break
        sleep 2
    done
    sleep 3
}

# Run one vLLM model with retry (mirrors generate-all.sh worker logic).
run_vllm() {
    local name="$1" parallel="$2" runid="${RUN_ID_PREFIX}-$1"
    log "${CYAN}=== vLLM: $name (parallel=$parallel) ===${NC}"
    free_gpu
    local attempt=1 max=5
    while ((attempt <= max)); do
        log "  $name attempt $attempt/$max"
        if cargo run --release -- generate \
            -m "$name" -p "data/models/$name" \
            --port "$PORT" -n "$ATTEMPTS" --parallel "$parallel" \
            --run-id "$runid" 2>&1 | tee -a "$LOG"; then
            log "${GREEN}  DONE: $name${NC}"
            return 0
        fi
        ((attempt < max)) && sleep $((2 ** (attempt - 1)))
        ((attempt++))
    done
    log "${RED}  FAIL: $name after $max attempts — continuing chain${NC}"
    return 1
}

log "${BOLD}╔══ Unattended run starting ══╗${NC}"

# ── Build release binary with ALL fixes baked in ──────────────────────
log "Building release binary..."
if ! cargo build --release 2>&1 | tee -a "$LOG"; then
    log "${RED}FATAL: release build failed — aborting${NC}"
    exit 1
fi
log "${GREEN}Build OK${NC}"

# ── Step 1-2: rerun decoder-corrupted LLaMA models (fresh, from 0) ─────
run_vllm "goedel-prover-dpo" 40
run_vllm "deepseek-prover-v2-7b" 32

# ── Step 3: STP (HF generate, separate backend, own checkpoint) ───────
log "${CYAN}=== STP: scripts/stp_runner.py ===${NC}"
free_gpu
if uv run --directory tools/vllm python "$PROJECT_DIR/scripts/stp_runner.py" --attempts "$ATTEMPTS" --batch 4 2>&1 | tee -a "$LOG"; then
    log "${GREEN}  DONE: STP${NC}"
else
    log "${RED}  FAIL: STP — continuing chain${NC}"
fi

# ── Step 4: resume goedel-v2 (capped 16384 window via models.rs) ──────
run_vllm "goedel-prover-v2-8b" 16

# ── Step 5: offline re-extract qwen3 models (no GPU) ──────────────────
free_gpu
for m in kimina-prover-rl-1.7b kimina-prover-distill-8b goedel-prover-v2-8b; do
    log "${CYAN}=== re-extract: $m ===${NC}"
    cargo run --release -- re-extract -m "$m" 2>&1 | tee -a "$LOG" \
        && log "${GREEN}  DONE: re-extract $m${NC}" \
        || log "${RED}  FAIL: re-extract $m${NC}"
done

# ── Final summary ─────────────────────────────────────────────────────
log "${BOLD}╔══ ALL STEPS COMPLETE ══╗${NC}"
for m in goedel-prover-dpo deepseek-prover-v2-7b stp-model-lean \
         goedel-prover-v2-8b kimina-prover-rl-1.7b kimina-prover-distill-8b; do
    if [[ -f "output/raw_output/$m.json" ]]; then
        n=$(python3 -c "import json;d=json.load(open('output/raw_output/$m.json'));print(len(d[list(d)[0]]))" 2>/dev/null || echo "?")
        log "  $m: $n/488 theorems"
    else
        log "  $m: ${RED}no output${NC}"
    fi
done

echo "ALL DONE $(date)" > results/ALL_DONE.flag
log "${GREEN}${BOLD}Wrote results/ALL_DONE.flag — safe to disconnect.${NC}"
