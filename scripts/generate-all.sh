#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

PROJECT_DIR="$(pwd)"
BOLD='\033[1m'; CYAN='\033[0;36m'; GREEN='\033[0;32m'; RED='\033[0;31m'; NC='\033[0m'

ATTEMPTS="${ATTEMPTS:-128}"
PARALLEL="${PARALLEL:-3}"
BASE_PORT="${BASE_PORT:-8080}"
SESSION="minif2f-gen"
RUN_ID_PREFIX="v128-$(date +%Y%m%d)"

declare -A MODELS
MODELS["kimina-prover-rl-1.7b"]="models/kimina-1.7b.gguf"
MODELS["goedel-prover-dpo"]="models/goedel-prover-dpo.gguf"
MODELS["goedel-prover-v2-8b"]="models/goedel-prover-v2-8b.gguf"
MODELS["deepseek-prover-v2-7b"]="models/deepseek-prover-v2-7b.gguf"
MODELS["kimina-prover-distill-8b"]="models/kimina-prover-distill-8b.gguf"
MODELS["stp-model-lean"]="models/stp-model-lean.gguf"

main() {
    echo -e "${BOLD}╔══════════════════════════════════════╗${NC}"
    echo -e "${BOLD}║  Generate All 6 Models (${ATTEMPTS} attempts)  ║${NC}"
    echo -e "${BOLD}╚══════════════════════════════════════╝${NC}"
    echo ""

    for cmd in tmux cargo; do
        if ! command -v "$cmd" &>/dev/null; then
            echo -e "  ${RED}$cmd is required but not installed.${NC}"
            exit 1
        fi
    done

    # Check models
    local ready=0 total=0
    for name in "${!MODELS[@]}"; do
        total=$((total + 1))
        if [[ -f "${MODELS[$name]}" ]]; then ready=$((ready + 1)); fi
    done
    echo "  Ready: $ready/$total models, ${ATTEMPTS} attempts each, ${PARALLEL} parallel"
    echo ""

    # Build the parallel run script (runs inside tmux)
    # Export config so the generated script picks them up
    export ATTEMPTS PARALLEL BASE_PORT RUN_ID_PREFIX

    local run_script="/tmp/minif2f-gen-parallel.sh"
    cat > "$run_script" << 'RUNEOF'
#!/usr/bin/env bash
set -euo pipefail
cd "PROJECT_DIR_PLACEHOLDER"

BOLD='\033[1m'; GREEN='\033[0;32m'; RED='\033[0;31m'; CYAN='\033[0;36m'; NC='\033[0m'

ATTEMPTS="${ATTEMPTS:-128}"
MAX_PARALLEL="${PARALLEL:-3}"
BASE_PORT="${BASE_PORT:-8080}"

MODELS=(
  "kimina-prover-rl-1.7b|models/kimina-1.7b.gguf"
  "goedel-prover-dpo|models/goedel-prover-dpo.gguf"
  "goedel-prover-v2-8b|models/goedel-prover-v2-8b.gguf"
  "deepseek-prover-v2-7b|models/deepseek-prover-v2-7b.gguf"
  "kimina-prover-distill-8b|models/kimina-prover-distill-8b.gguf"
  "stp-model-lean|models/stp-model-lean.gguf"
)

total_models=${#MODELS[@]}
done_count=0
failed_count=0
active=0
port=$BASE_PORT

echo -e "${BOLD}Starting (up to $MAX_PARALLEL parallel, $ATTEMPTS attempts each)${NC}"
echo ""

for entry in "${MODELS[@]}"; do
    IFS='|' read -r name gguf <<< "$entry"

    # Wait for a free slot if all are busy
    while ((active >= MAX_PARALLEL)); do
        wait -n
        active=$((active - 1))
    done

    # Skip missing GGUF
    if [[ ! -f "$gguf" ]]; then
        echo -e "${RED}SKIP $name — GGUF not found${NC}"
        done_count=$((done_count + 1))
        continue
    fi

    local_port=$port
    port=$((port + 1))
    run_id="${RUN_ID_PREFIX}-${name}"

    (
        echo -e "${CYAN}╔══ START: $name (port $local_port) ══╗${NC}"
        if cargo run --release -- generate \
            -m "$name" -p "$gguf" \
            --port "$local_port" -n "$ATTEMPTS" \
            --parallel 128 \
            --run-id "$run_id" 2>&1; then
            echo -e "${GREEN}╚══ DONE:  $name ══╝${NC}"
        else
            echo -e "${RED}╚══ FAIL:  $name ══╝${NC}"
            mkdir -p /tmp/minif2f-failures
            touch "/tmp/minif2f-failures/${name}"
        fi
    ) &

    active=$((active + 1))
done

# Wait for remaining jobs
wait

# Count failures
for entry in "${MODELS[@]}"; do
    IFS='|' read -r name gguf <<< "$entry"
    if [[ -f "/tmp/minif2f-failures/${name}" ]]; then
        failed_count=$((failed_count + 1))
    fi
done
rm -rf /tmp/minif2f-failures

echo ""
echo -e "${BOLD}═══════════════════════════════════════${NC}"
echo -e "${BOLD}  Complete: $((total_models - failed_count))/$total_models models${NC}"
if ((failed_count > 0)); then
    echo -e "${RED}  Failed: $failed_count models${NC}"
fi
echo -e "  Output: output/*.json"
echo -e "${BOLD}═══════════════════════════════════════${NC}"
RUNEOF

    # Inject project directory
    sed -i "s|PROJECT_DIR_PLACEHOLDER|$PROJECT_DIR|" "$run_script"
    chmod +x "$run_script"

    # Create tmux session
    tmux kill-session -t "$SESSION" 2>/dev/null || true
    if ! tmux new-session -d -s "$SESSION" -c "$PROJECT_DIR" -n "generate-all" 2>/tmp/tmux-err.log; then
        echo -e "  ${RED}Failed to create tmux session.${NC}"
        sed 's/^/    /' /tmp/tmux-err.log
        exit 1
    fi

    tmux send-keys -t "$SESSION:0" \
        "echo 'Parallel: up to ${PARALLEL} models at once. Ctrl-B d to detach.'" Enter
    tmux send-keys -t "$SESSION:0" "echo ''" Enter
    tmux send-keys -t "$SESSION:0" "bash '$run_script'" Enter

    echo -e "  ${BOLD}Started in tmux session '${SESSION}' (${PARALLEL} parallel)${NC}"
    echo ""
    echo "  tmux attach -t ${SESSION}     # view progress"
    echo "  Ctrl-B d                       # detach (keeps running)"
    echo ""

    if [[ -n "${TMUX:-}" ]]; then
        echo "  Already in tmux — Ctrl-B w → select '${SESSION}'"
        echo "  Or: tmux switch-client -t ${SESSION}"
    elif [[ -t 0 ]]; then
        tmux attach -t "$SESSION"
    else
        echo "  (non-interactive — generation continues in background)"
    fi
}

main
