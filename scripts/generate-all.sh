#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

PROJECT_DIR="$(pwd)"
BOLD='\033[1m'; CYAN='\033[0;36m'; GREEN='\033[0;32m'; RED='\033[0;31m'; NC='\033[0m'

ATTEMPTS="${ATTEMPTS:-128}"
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

    # Build per-slot model lists (round-robin, skip missing GGUF)
    local slot1_models=()
    local slot2_models=()
    for i in "${!MODELS[@]}"; do
        IFS='|' read -r _ gguf <<< "${MODELS[$i]}"
        if [[ ! -f "$gguf" ]]; then continue; fi
        if ((i % 2 == 0)); then
            slot1_models+=("${MODELS[$i]}")
        else
            slot2_models+=("${MODELS[$i]}")
        fi
    done

    # Print summary
    local ready=0 total=0
    for entry in "${MODELS[@]}"; do
        total=$((total + 1))
        IFS='|' read -r _ gguf <<< "$entry"
        if [[ -f "$gguf" ]]; then ready=$((ready + 1)); fi
    done
    echo "  Ready: $ready/$total models, ${ATTEMPTS} attempts each, 2 parallel"
    echo ""

    # Build worker script
    local worker="/tmp/minif2f-worker.sh"
    cat > "$worker" << 'WORKEREOF'
#!/usr/bin/env bash
set -euo pipefail
cd "PROJECT_DIR_PLACEHOLDER"

BOLD='\033[1m'; GREEN='\033[0;32m'; RED='\033[0;31m'; CYAN='\033[0;36m'; NC='\033[0m'

ATTEMPTS="${ATTEMPTS:-128}"
SLOT="$1"
PORT="$2"
RUN_ID_PREFIX="${RUN_ID_PREFIX:-v128}"

# Models are passed as remaining args: "name|gguf" "name|gguf" ...
shift 2
models=("$@")

for entry in "${models[@]}"; do
    IFS='|' read -r name gguf <<< "$entry"
    if [[ ! -f "$gguf" ]]; then
        echo -e "${RED}[$SLOT] SKIP $name — GGUF not found${NC}"
        continue
    fi
    run_id="${RUN_ID_PREFIX}-${name}"

    echo ""
    echo -e "${CYAN}╔══════════════════════════════════════════════════╗${NC}"
    echo -e "${CYAN}║  [$SLOT] START: $name (port $PORT)${NC}"
    echo -e "${CYAN}╚══════════════════════════════════════════════════╝${NC}"
    echo ""

    if cargo run --release -- generate \
        -m "$name" -p "$gguf" \
        --port "$PORT" -n "$ATTEMPTS" \
        --parallel 128 \
        --run-id "$run_id"; then
        echo -e "${GREEN}╚══ [$SLOT] DONE:  $name ══╝${NC}"
    else
        echo -e "${RED}╚══ [$SLOT] FAIL:  $name ══╝${NC}"
    fi
done
echo -e "${GREEN}[$SLOT] All models in this slot done.${NC}"
WORKEREOF

    sed -i "s|PROJECT_DIR_PLACEHOLDER|$PROJECT_DIR|" "$worker"
    chmod +x "$worker"
    export ATTEMPTS RUN_ID_PREFIX

    # Create tmux session
    tmux kill-session -t "$SESSION" 2>/dev/null || true
    if ! tmux new-session -d -s "$SESSION" -c "$PROJECT_DIR" -n "slot-1" 2>/tmp/tmux-err.log; then
        echo -e "  ${RED}Failed to create tmux session.${NC}"
        sed 's/^/    /' /tmp/tmux-err.log
        exit 1
    fi

    # Window 0: slot-1 (port 8080, models 0,2,4)
    tmux send-keys -t "$SESSION:0" \
        "echo 'Slot 1 — models: ${slot1_models[*]}'" Enter
    tmux send-keys -t "$SESSION:0" \
        "bash '$worker' 'slot-1' 8080 ${slot1_models[*]@Q}" Enter

    # Window 1: slot-2 (port 8081, models 1,3,5)
    tmux new-window -t "$SESSION" -n "slot-2" -c "$PROJECT_DIR"
    tmux send-keys -t "$SESSION:1" \
        "echo 'Slot 2 — models: ${slot2_models[*]}'" Enter
    tmux send-keys -t "$SESSION:1" \
        "bash '$worker' 'slot-2' 8081 ${slot2_models[*]@Q}" Enter

    echo -e "  ${BOLD}Started in tmux session '${SESSION}'${NC}"
    echo ""
    echo "  tmux attach -t ${SESSION}     # view progress"
    echo "  Ctrl-B n                       # switch slot (0/1)"
    echo "  Ctrl-B d                       # detach"
    echo ""

    if [[ -n "${TMUX:-}" ]]; then
        echo "  Already in tmux — Ctrl-B w → select '${SESSION}'"
    elif [[ -t 0 ]]; then
        tmux attach -t "$SESSION"
    else
        echo "  (non-interactive)"
    fi
}

main
