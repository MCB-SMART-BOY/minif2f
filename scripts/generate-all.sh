#!/usr/bin/env bash
set -euo pipefail
PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_DIR"
BOLD='\033[1m'; CYAN='\033[0;36m'; GREEN='\033[0;32m'; RED='\033[0;31m'; NC='\033[0m'

ATTEMPTS="${ATTEMPTS:-128}"
SESSION="minif2f-gen"
RUN_ID_PREFIX="${RUN_ID_PREFIX:-v128-$(date +%Y%m%d)}"

# --parallel = vLLM --max-num-seqs (empirically tested on RTX 5090 32 GB).
#   Goedel-DPO: tested p=8✅ p=10✅ p=12⚠️(8/50 marginal fails)
#   All other models estimated from Goedel-DPO baseline:
#     available_KV = 30GB - model_weights, per_seq_KV = max_tokens * kv_per_tok
#     p ≈ available_KV / (per_seq_KV * safety_factor)
#   LLaMA-7B (no GQA, kv=512KB/tok FP16): KV-heavy, conservative.
#   Qwen3 (GQA, kv=128KB/tok FP16): 4× lighter KV, can push higher.
MODELS=(
  "goedel-prover-dpo|data/models/goedel-prover-dpo|10"                        # LLaMA-7B, 13GB, tested p=10✅ p=12⚠️
  "deepseek-prover-v2-7b|data/models/deepseek-prover-v2-7b|6"                # LLaMA-7B, 13GB+long ctx(8192) → conservative p=6
  "kimina-prover-rl-1.7b|data/models/kimina-prover-rl-1.7b|38"               # Qwen3-1.7B, 3.4GB+GQA, light model → p=38
  "goedel-prover-v2-8b|data/models/goedel-prover-v2-8b|8"                   # Qwen3-8B, 16GB+very long ctx(32768) → p=8
  "kimina-prover-distill-8b|data/models/kimina-prover-distill-8b|30"         # Qwen3-8B, 16GB+GQA → p=30
  "stp-model-lean|data/models/stp-model-lean|16"                             # DS-Prover-V1.5, 13GB+short ctx(1024) → p=16
)

main() {
    echo -e "${BOLD}╔══════════════════════════════════════╗${NC}"
    echo -e "${BOLD}║  Generate Models (${ATTEMPTS} attempts)  ║${NC}"
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
        IFS='|' read -r _ model_path _parallel <<< "$entry"
        if [[ -d "$model_path" ]]; then ready=$((ready + 1)); fi
    done
    echo "  Ready: $ready/$total models, ${ATTEMPTS} attempts each, sequential"
    echo "  --parallel: per-model values from MODELS array"
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
    IFS='|' read -r name model_path parallel <<< "$entry"
    parallel="${parallel:-8}"
    if [[ ! -d "$model_path" ]]; then
        echo -e "${RED}SKIP $name — model directory not found: $model_path${NC}"
        continue
    fi
    run_id="${RUN_ID_PREFIX}-${name}"

    echo ""
    echo -e "${CYAN}╔══════════════════════════════════════════════════╗${NC}"
    echo -e "${CYAN}║  START: $name${NC}"
    echo -e "${CYAN}╚══════════════════════════════════════════════════╝${NC}"
    echo ""

    # Kill any orphaned vLLM server and all subprocesses
    # vLLM's EngineCore is a separate process that may hold GPU memory + port
    # after the parent exits. We aggressively clean everything.
    fuser -k "$PORT/tcp" 2>/dev/null || true
    # Kill all vLLM/EngineCore orphan processes
    pkill -f "VLLM::EngineCore" 2>/dev/null || true
    pkill -f "server.py" 2>/dev/null || true
    # Wait until port is truly free (up to 60s)
    for _ in $(seq 1 30); do
        if ! fuser "$PORT/tcp" 2>/dev/null; then
            break
        fi
        sleep 2
    done
    sleep 2

    # Retry loop — transient failures self-heal
    attempt=1; max_attempts=5
    while ((attempt <= max_attempts)); do
        echo -e "  Attempt $attempt/$max_attempts..."
        if cargo run --release -- generate \
            -m "$name" -p "$model_path" \
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
        IFS='|' read -r _ model_path _parallel <<< "$entry"
        if [[ -d "$model_path" ]]; then
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
