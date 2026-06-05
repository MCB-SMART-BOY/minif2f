#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

BOLD='\033[1m'; CYAN='\033[0;36m'; GREEN='\033[0;32m'; RED='\033[0;31m'; NC='\033[0m'

ATTEMPTS="${ATTEMPTS:-128}"
SESSION="minif2f-gen"
PORT=8080
RUN_ID_PREFIX="v128-$(date +%Y%m%d)"

declare -A MODELS
MODELS["kimina-prover-rl-1.7b"]="models/kimina-1.7b.gguf"
MODELS["goedel-prover-dpo"]="models/goedel-prover-dpo.gguf"
MODELS["goedel-prover-v2-8b"]="models/goedel-prover-v2-8b.gguf"
MODELS["deepseek-prover-v2-7b"]="models/deepseek-prover-v2-7b.gguf"
MODELS["kimina-prover-distill-8b"]="models/kimina-prover-distill-8b.gguf"
MODELS["stp-model-lean"]="models/stp-model-lean.gguf"

banner() {
    echo -e "${BOLD}╔══════════════════════════════════════╗${NC}"
    echo -e "${BOLD}║  Generate All 6 Models (${ATTEMPTS}×/theorem)  ║${NC}"
    echo -e "${BOLD}╚══════════════════════════════════════╝${NC}"
    echo ""
}

main() {
    banner

    if ! command -v tmux &>/dev/null; then
        echo "ERROR: tmux is required."
        exit 1
    fi

    # Check models
    local missing=0 total=0
    for name in "${!MODELS[@]}"; do
        ((total++))
        if [[ ! -f "${MODELS[$name]}" ]]; then
            echo -e "  ${RED}MISSING${NC}: ${MODELS[$name]} — $name skipped"
            ((missing++))
        fi
    done
    echo "  Ready: $((total - missing))/$total models, ${ATTEMPTS} attempts each"
    echo ""

    # Build the sequential run script
    local run_script="/tmp/minif2f-gen-sequence.sh"
    cat > "$run_script" << 'RUNEOF'
#!/usr/bin/env bash
set -euo pipefail

BOLD='\033[1m'; GREEN='\033[0;32m'; RED='\033[0;31m'; CYAN='\033[0;36m'; NC='\033[0m'

MODELS=(
  "kimina-prover-rl-1.7b|models/kimina-1.7b.gguf"
  "goedel-prover-dpo|models/goedel-prover-dpo.gguf"
  "goedel-prover-v2-8b|models/goedel-prover-v2-8b.gguf"
  "deepseek-prover-v2-7b|models/deepseek-prover-v2-7b.gguf"
  "kimina-prover-distill-8b|models/kimina-prover-distill-8b.gguf"
  "stp-model-lean|models/stp-model-lean.gguf"
)

ATTEMPTS="${ATTEMPTS:-128}"
PORT=8080
RUN_ID_PREFIX="$(date +%Y%m%d)"

total=${#MODELS[@]}
current=0
failed=0

echo -e "${BOLD}Starting sequential generation ($total models, ${ATTEMPTS} attempts each)${NC}"
echo ""

for entry in "${MODELS[@]}"; do
    IFS='|' read -r name gguf <<< "$entry"
    ((current++))

    if [[ ! -f "$gguf" ]]; then
        echo -e "${RED}[$current/$total] SKIP $name — GGUF not found${NC}"
        continue
    fi

    run_id="${RUN_ID_PREFIX}-${name}"

    echo -e "${CYAN}╔══════════════════════════════════════════════════╗${NC}"
    echo -e "${CYAN}║  [$current/$total] $name${NC}"
    echo -e "${CYAN}╚══════════════════════════════════════════════════╝${NC}"
    echo ""

    if cargo run --release -- generate \
        -m "$name" -p "$gguf" \
        --port "$PORT" -n "$ATTEMPTS" \
        --run-id "$run_id"; then
        echo -e "${GREEN}[$current/$total] DONE: $name${NC}"
    else
        echo -e "${RED}[$current/$total] FAILED: $name${NC}"
        ((failed++))
    fi
    echo ""
done

echo ""
echo -e "${BOLD}═══════════════════════════════════════${NC}"
echo -e "${BOLD}  Complete: $((total - failed))/$total models${NC}"
if ((failed > 0)); then
    echo -e "${RED}  Failed: $failed models${NC}"
fi
echo -e "  Output: output/*.json"
echo -e "${BOLD}═══════════════════════════════════════${NC}"
RUNEOF

    chmod +x "$run_script"

    # Kill existing session, start new one
    tmux kill-session -t "$SESSION" 2>/dev/null || true
    tmux new-session -d -s "$SESSION" -n "generate-all"

    tmux send-keys -t "$SESSION:0" \
        "echo 'Models run sequentially: one loaded, generates, unloads, next starts.'" Enter
    tmux send-keys -t "$SESSION:0" \
        "echo 'Detach with Ctrl-B d — generation continues in background.'" Enter
    tmux send-keys -t "$SESSION:0" \
        "echo ''" Enter
    tmux send-keys -t "$SESSION:0" \
        "bash '$run_script'" Enter

    echo -e "  ${BOLD}Started in tmux session '${SESSION}'${NC}"
    echo ""
    echo "  tmux attach -t ${SESSION}     # view progress"
    echo "  Ctrl-B d                       # detach (keeps running)"
    echo ""

    tmux attach -t "$SESSION"
}

main
