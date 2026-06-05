#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

PROJECT_DIR="$(pwd)"
BOLD='\033[1m'; CYAN='\033[0;36m'; GREEN='\033[0;32m'; RED='\033[0;31m'; NC='\033[0m'

ATTEMPTS="${ATTEMPTS:-128}"
PARALLEL="${PARALLEL:-2}"
SESSION="minif2f-gen"
RUN_ID_PREFIX="v128-$(date +%Y%m%d)"

MODELS=(
  "kimina-prover-rl-1.7b|models/kimina-1.7b.gguf"
  "stp-model-lean|models/stp-model-lean.gguf"
  "goedel-prover-dpo|models/goedel-prover-dpo.gguf"
  "deepseek-prover-v2-7b|models/deepseek-prover-v2-7b.gguf"
  "goedel-prover-v2-8b|models/goedel-prover-v2-8b.gguf"
  "kimina-prover-distill-8b|models/kimina-prover-distill-8b.gguf"
)

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

    # Build the queue file (one model per line: name|gguf)
    local queue="/tmp/minif2f-queue.txt"
    : > "$queue"  # truncate
    for entry in "${MODELS[@]}"; do
        IFS='|' read -r name gguf <<< "$entry"
        if [[ -f "$gguf" ]]; then
            echo "$name|$gguf" >> "$queue"
        fi
    done

    # Build the worker script (each tmux window runs this)
    local worker="/tmp/minif2f-worker.sh"
    cat > "$worker" << 'WORKEREOF'
#!/usr/bin/env bash
set -euo pipefail
cd "PROJECT_DIR_PLACEHOLDER"

BOLD='\033[1m'; GREEN='\033[0;32m'; RED='\033[0;31m'; CYAN='\033[0;36m'; NC='\033[0m'

ATTEMPTS="${ATTEMPTS:-128}"
QUEUE="$1"
PORT="$2"
RUN_ID_PREFIX="${RUN_ID_PREFIX:-v128-$(date +%Y%m%d)}"

slot_name="${3:-worker}"

while true; do
    # Atomically pop the first line using mkdir as mutex (no flock dependency)
    while ! mkdir /tmp/minif2f-pop-lock 2>/dev/null; do sleep 0.1; done
    line=$(head -1 "$QUEUE" 2>/dev/null || true)
    [[ -n "$line" ]] && sed -i '1d' "$QUEUE"
    rmdir /tmp/minif2f-pop-lock
    line=$(echo "$line" | xargs)  # trim

    if [[ -z "$line" ]]; then
        echo -e "${GREEN}[$slot_name] Queue empty — done.${NC}"
        break
    fi

    IFS='|' read -r name gguf <<< "$line"
    run_id="${RUN_ID_PREFIX}-${name}"

    echo ""
    echo -e "${CYAN}╔══════════════════════════════════════════════════╗${NC}"
    echo -e "${CYAN}║  [$slot_name] START: $name (port $PORT)${NC}"
    echo -e "${CYAN}╚══════════════════════════════════════════════════╝${NC}"
    echo ""

    if cargo run --release -- generate \
        -m "$name" -p "$gguf" \
        --port "$PORT" -n "$ATTEMPTS" \
        --parallel 128 \
        --run-id "$run_id"; then
        echo -e "${GREEN}╚══ [$slot_name] DONE:  $name ══╝${NC}"
    else
        echo -e "${RED}╚══ [$slot_name] FAIL:  $name ══╝${NC}"
    fi
done
WORKEREOF

    # Inject paths
    sed -i "s|PROJECT_DIR_PLACEHOLDER|$PROJECT_DIR|" "$worker"
    chmod +x "$worker"

    # Export config so worker picks them up
    export ATTEMPTS RUN_ID_PREFIX

    # Create tmux session with PARALLEL windows
    tmux kill-session -t "$SESSION" 2>/dev/null || true
    if ! tmux new-session -d -s "$SESSION" -c "$PROJECT_DIR" -n "slot-1" 2>/tmp/tmux-err.log; then
        echo -e "  ${RED}Failed to create tmux session.${NC}"
        sed 's/^/    /' /tmp/tmux-err.log
        exit 1
    fi

    local port=$BASE_PORT
    # Window 0 (slot-1) — already created
    tmux send-keys -t "$SESSION:0" \
        "echo 'Slot 1 — popping jobs from queue'" Enter
    tmux send-keys -t "$SESSION:0" \
        "bash '$worker' '$queue' '$port' 'slot-1'" Enter

    # Additional windows
    for ((i=1; i<PARALLEL; i++)); do
        port=$((port + 1))
        tmux new-window -t "$SESSION" -n "slot-$((i+1))" -c "$PROJECT_DIR"
        tmux send-keys -t "$SESSION:$i" \
            "echo 'Slot $((i+1)) — popping jobs from queue'" Enter
        tmux send-keys -t "$SESSION:$i" \
            "bash '$worker' '$queue' '$port' 'slot-$((i+1))'" Enter
    done

    echo -e "  ${BOLD}Started in tmux session '${SESSION}' (${PARALLEL} parallel slots)${NC}"
    echo ""
    echo "  tmux attach -t ${SESSION}     # view progress"
    echo "  Ctrl-B n                       # next slot"
    echo "  Ctrl-B d                       # detach"
    echo ""

    if [[ -n "${TMUX:-}" ]]; then
        echo "  Already in tmux — Ctrl-B w → select '${SESSION}'"
    elif [[ -t 0 ]]; then
        tmux attach -t "$SESSION"
    else
        echo "  (non-interactive — generation continues in background)"
    fi
}

main
