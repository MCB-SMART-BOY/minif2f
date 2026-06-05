#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

BOLD='\033[1m'; CYAN='\033[0;36m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'

# ── Config ────────────────────────────────────────────────────────────
ATTEMPTS="${ATTEMPTS:-128}"
PARALLEL="${PARALLEL:-4}"
SESSION="minif2f-gen"
RUN_ID_PREFIX="v128-$(date +%Y%m%d)"

declare -A MODELS
MODELS["kimina-prover-rl-1.7b"]="models/kimina-1.7b.gguf"
MODELS["goedel-prover-dpo"]="models/goedel-prover-dpo.gguf"
MODELS["goedel-prover-v2-8b"]="models/goedel-prover-v2-8b.gguf"
MODELS["deepseek-prover-v2-7b"]="models/deepseek-prover-v2-7b.gguf"
MODELS["kimina-prover-distill-8b"]="models/kimina-prover-distill-8b.gguf"
MODELS["stp-model-lean"]="models/stp-model-lean.gguf"

# ── Helpers ───────────────────────────────────────────────────────────
banner() {
    echo -e "${BOLD}╔══════════════════════════════════════╗${NC}"
    echo -e "${BOLD}║    Generate All 6 Models (${ATTEMPTS}×/theorem) ║${NC}"
    echo -e "${BOLD}╚══════════════════════════════════════╝${NC}"
    echo ""
}

# ── Main ──────────────────────────────────────────────────────────────
main() {
    banner

    # Check tmux
    if ! command -v tmux &>/dev/null; then
        echo "ERROR: tmux is required. Install it first."
        exit 1
    fi

    # Check models exist
    local missing=0
    for name in "${!MODELS[@]}"; do
        if [[ ! -f "${MODELS[$name]}" ]]; then
            echo -e "  ${YELLOW}MISSING${NC}: ${MODELS[$name]} — $name will be skipped"
            ((missing++))
        fi
    done
    if ((missing > 0)); then
        echo ""
        echo "  Missing $missing GGUF files. Run setup.sh first."
        echo ""
    fi

    echo -e "  ${BOLD}${#MODELS[@]} models${NC}, ${PARALLEL} parallel, ${ATTEMPTS} attempts/theorem"
    echo -e "  ${CYAN}tmux session: ${SESSION}${NC}"
    echo ""

    # Kill existing session if any
    tmux kill-session -t "$SESSION" 2>/dev/null || true

    # Create tmux session
    tmux new-session -d -s "$SESSION" -n "overview"

    # Overview window: show status of all models
    tmux send-keys -t "$SESSION:0" \
        "echo 'Generating all 6 models (${ATTEMPTS} attempts × 488 theorems each)'" Enter
    tmux send-keys -t "$SESSION:0" \
        "echo 'Attach to any window: tmux attach -t ${SESSION}'" Enter
    tmux send-keys -t "$SESSION:0" \
        "echo 'List windows: Ctrl-B w'" Enter

    local i=1
    local port=8080
    for name in "${!MODELS[@]}"; do
        local gguf="${MODELS[$name]}"
        [[ -f "$gguf" ]] || continue

        local run_id="${RUN_ID_PREFIX}"

        tmux new-window -t "$SESSION" -n "$name"
        tmux send-keys -t "$SESSION:$i" \
            "echo '=== ${name} | port ${port} | run ${run_id} ==='" Enter
        tmux send-keys -t "$SESSION:$i" \
            "cargo run --release -- generate -m '${name}' -p '${gguf}' --port ${port} -n ${ATTEMPTS} --run-id '${run_id}'" Enter

        echo -e "  ${GREEN}win $i${NC}: ${name} (port ${port})"
        ((i++))
        ((port++))
    done

    echo ""
    echo -e "  ${BOLD}Commands:${NC}"
    echo "    tmux attach -t ${SESSION}     # view progress"
    echo "    Ctrl-B w                       # list windows"
    echo "    Ctrl-B <n>                     # switch to window n"
    echo ""
    echo -e "  ${BOLD}Start ${PARALLEL} models at a time:${NC} enter each window and press Enter"
    echo ""

    # Attach to session
    tmux attach -t "$SESSION"
}

main
