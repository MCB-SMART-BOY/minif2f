#!/usr/bin/env bash
set -euo pipefail
PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_DIR"
BOLD='\033[1m'; CYAN='\033[0;36m'; GREEN='\033[0;32m'; RED='\033[0;31m'; NC='\033[0m'

ATTEMPTS="${ATTEMPTS:-128}"
SESSION="minif2f-gen"
RUN_ID_PREFIX="${RUN_ID_PREFIX:-v128-$(date +%Y%m%d)}"

# --parallel tuned for optimal throughput (not max VRAM):
#   More slots = more memory bandwidth contention → per-slot t/s drops.
#   Q4_K_M models are memory-bandwidth bound (~1.7 TB/s RTX 5090).
#   7B Q4_K_M ~4.5 GB → single-stream max ~378 t/s, 16-way ~65-78 t/s each.
#   LLaMA-7B (no GQA, kv=256KB/tok): p=16 is the sweet spot.
#   Qwen3 (GQA, kv=64KB/tok): can push higher, less memory pressure.
MODELS=(
  "goedel-prover-dpo|models/goedel-prover-dpo.gguf|16"                        # LLaMA-7B, ctx=65536, ~70 t/s per slot
  "deepseek-prover-v2-7b|models/deepseek-prover-v2-7b.gguf|7"                # LLaMA-7B, ctx=86016, per_slot=12288
  "kimina-prover-rl-1.7b|models/kimina-1.7b.gguf|24"                         # Qwen3-1.7B, ctx=292608, per_slot=12192
  "goedel-prover-v2-8b|models/goedel-prover-v2-8b.gguf|8"                    # Qwen3-8B, ctx=294912, per_slot=36864
  "kimina-prover-distill-8b|models/kimina-prover-distill-8b.gguf|24"         # Qwen3-8B, ctx=292608, per_slot=12192
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

    # Print summary
    local ready=0 total=0
    for entry in "${MODELS[@]}"; do
        total=$((total + 1))
        IFS='|' read -r _ gguf _parallel <<< "$entry"
        if [[ -f "$gguf" ]]; then ready=$((ready + 1)); fi
    done
    echo "  Ready: $ready/$total models, ${ATTEMPTS} attempts each, sequential"
    echo "  --parallel: per-model (64/32/16 based on ctx + q8_0 KV cache)"
    echo ""

    # Build worker script — sequential, single port
    local worker="/tmp/minif2f-worker.sh"
    cat > "$worker" << 'WORKEREOF'
#!/usr/bin/env bash
set -euo pipefail
cd "PROJECT_DIR_PLACEHOLDER"

BOLD='\033[1m'; GREEN='\033[0;32m'; RED='\033[0;31m'; CYAN='\033[0;36m'; NC='\033[0m'

ATTEMPTS="${ATTEMPTS:-128}"
PORT=8080
RUN_ID_PREFIX="${RUN_ID_PREFIX:-v128}"

models=("$@")

for entry in "${models[@]}"; do
    IFS='|' read -r name gguf parallel <<< "$entry"
    parallel="${parallel:-8}"
    if [[ ! -f "$gguf" ]]; then
        echo -e "${RED}SKIP $name — GGUF not found${NC}"
        continue
    fi
    run_id="${RUN_ID_PREFIX}-${name}"

    echo ""
    echo -e "${CYAN}╔══════════════════════════════════════════════════╗${NC}"
    echo -e "${CYAN}║  START: $name${NC}"
    echo -e "${CYAN}╚══════════════════════════════════════════════════╝${NC}"
    echo ""

    # Kill any orphaned llama-server
    fuser -k "$PORT/tcp" 2>/dev/null || true
    sleep 2

    # Retry loop — transient failures self-heal
    attempt=1; max_attempts=5
    while ((attempt <= max_attempts)); do
        echo -e "  Attempt $attempt/$max_attempts..."
        if cargo run --release -- generate \
            -m "$name" -p "$gguf" \
            --port "$PORT" -n "$ATTEMPTS" \
            --parallel "$parallel" \
            --run-id "$run_id"; then
            echo -e "${GREEN}╚══ DONE:  $name ══╝${NC}"
            break
        fi
        if ((attempt < max_attempts)); then
            wait=$((2 ** (attempt - 1)))
            echo -e "${RED}  Attempt $attempt failed — retrying in ${wait}s...${NC}"
            sleep "$wait"
        else
            echo -e "${RED}╚══ FAIL:  $name (after $max_attempts attempts) ══╝${NC}"
        fi
        ((attempt++))
    done
done
echo ""
echo -e "${GREEN}${BOLD}All models done.${NC}"
WORKEREOF

    sed -i "s|PROJECT_DIR_PLACEHOLDER|$PROJECT_DIR|" "$worker"
    chmod +x "$worker"
    export ATTEMPTS RUN_ID_PREFIX

    # Create tmux session — single window, sequential execution
    tmux kill-session -t "$SESSION" 2>/dev/null || true
    if ! tmux new-session -d -s "$SESSION" -c "$PROJECT_DIR" 2>/tmp/tmux-err.log; then
        echo -e "  ${RED}Failed to create tmux session.${NC}"
        sed 's/^/    /' /tmp/tmux-err.log
        exit 1
    fi

    # Collect available models
    local model_args=()
    for entry in "${MODELS[@]}"; do
        IFS='|' read -r _ gguf _parallel <<< "$entry"
        if [[ -f "$gguf" ]]; then
            model_args+=("$entry")
        fi
    done

    tmux send-keys -t "$SESSION:0" \
        "echo 'Sequential — models: ${model_args[*]}'" Enter
    tmux send-keys -t "$SESSION:0" \
        "bash '$worker' ${model_args[*]@Q}" Enter

    echo -e "  ${BOLD}Started in tmux session '${SESSION}'${NC}"
    echo ""
    echo "  tmux attach -t ${SESSION}     # view progress"
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
