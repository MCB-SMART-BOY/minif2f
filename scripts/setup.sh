#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; BOLD='\033[1m'; NC='\033[0m'

# ── Config ────────────────────────────────────────────────────────────
MINIF2F_URL="https://raw.githubusercontent.com/openai/miniF2F/main/minif2f.jsonl"
HF_TOKEN="${HF_TOKEN:-}"
HF_HOME="${HF_HOME:-$PWD/data/models}"
# Default to huggingface.co (works with proxy). Use hf-mirror.com if no proxy.
HF_ENDPOINT="${HF_ENDPOINT:-https://huggingface.co}"
VLLM_DIR="tools/vllm"

# ── Model download (HuggingFace safetensors → data/models/<name>/) ────
# vLLM loads the safetensors DIRECTORY directly with --quantization fp8 at
# load time. No GGUF conversion, no quantization step.
# Format: "name|repo"
MODEL_LIST=(
  "goedel-prover-dpo|Goedel-LM/Goedel-Prover-DPO"
  "kimina-prover-rl-1.7b|AI-MO/Kimina-Prover-RL-1.7B"
  "goedel-prover-v2-8b|Goedel-LM/Goedel-Prover-V2-8B"
  "deepseek-prover-v2-7b|deepseek-ai/DeepSeek-Prover-V2-7B"
  "kimina-prover-distill-8b|AI-MO/Kimina-Prover-Distill-8B"
  "stp-model-lean|kfdong/STP_model_Lean"
)

# ── Helpers ────────────────────────────────────────────────────────────
section()  { echo -e "\n${BOLD}═══ $1 ═══${NC}"; }
ok()       { echo -e "  ${GREEN}[OK]${NC} $1"; }
warn()     { echo -e "  ${YELLOW}[WARN]${NC} $1"; }
fail()     { echo -e "  ${RED}[FAIL]${NC} $1"; exit 1; }
check_cmd() { command -v "$1" &>/dev/null && ok "$1 found" || fail "$1 not found — please install"; }

choose() {
    local prompt="$1"; shift; local options=("$@")
    echo -e "${YELLOW}${prompt}${NC}" >&2
    for i in "${!options[@]}"; do printf "  ${BOLD}%d)${NC} %s\n" "$((i+1))" "${options[$i]}" >&2; done
    while true; do
        read -r -p "  Choose [1-${#options[@]}]: " choice
        if [[ "$choice" =~ ^[0-9]+$ ]] && ((choice >= 1 && choice <= ${#options[@]})); then
            echo "$((choice - 1))"; return
        fi
    done
}

# ── Steps ──────────────────────────────────────────────────────────────

step_prerequisites() {
    section "Checking prerequisites"
    check_cmd rustup
    check_cmd cargo
    check_cmd uv

    # NVIDIA GPU + CUDA: vLLM needs a CUDA-capable GPU.
    if command -v nvidia-smi &>/dev/null; then
        ok "nvidia-smi found ($(nvidia-smi --query-gpu=name --format=csv,noheader | head -1))"
    else
        warn "nvidia-smi not found — vLLM requires an NVIDIA GPU with CUDA"
    fi
}

step_vllm() {
    section "Setting up vLLM backend (tools/vllm)"

    if [[ ! -f "$VLLM_DIR/pyproject.toml" ]]; then
        fail "$VLLM_DIR/pyproject.toml missing — repo layout unexpected"
    fi

    if [[ -d "$VLLM_DIR/.venv" ]]; then
        ok "vLLM venv already present ($VLLM_DIR/.venv)"
        return
    fi

    echo "  Provisioning vLLM via uv (this downloads vLLM + CUDA wheels)..."
    if uv sync --directory "$VLLM_DIR"; then
        ok "vLLM environment ready at $VLLM_DIR/.venv"
    else
        fail "uv sync failed in $VLLM_DIR — check network and uv install"
    fi
}

step_dataset() {
    section "Downloading miniF2F dataset"

    mkdir -p data/raw
    if [[ -f data/raw/minif2f.jsonl ]]; then
        ok "minif2f.jsonl already exists ($(du -h data/raw/minif2f.jsonl | cut -f1))"
        return
    fi

    echo "  Downloading from openai/miniF2F..."
    curl -L --progress-bar -o data/raw/minif2f.jsonl "$MINIF2F_URL"
    ok "Downloaded ($(du -h data/raw/minif2f.jsonl | cut -f1))"
}

step_rust_build() {
    section "Building Rust project"
    cargo build --release
    ok "Build complete: target/release/minif2f"
}

step_model_single() {
    local name="$1" repo="$2"
    local model_dir="data/models/${name}"

    if [[ -f "$model_dir/config.json" ]] && compgen -G "$model_dir/*.safetensors" >/dev/null; then
        ok "$name already present ($(du -sh "$model_dir" | cut -f1))"
        return
    fi

    echo -e "  ${BOLD}Downloading: $name${NC} (safetensors → $model_dir)"

    export HF_HOME HF_TOKEN HF_ENDPOINT HF_HUB_ENABLE_HF_TRANSFER=1

    # Download the HF snapshot directly into data/models/<name>/.
    # vLLM consumes this directory as-is (FP8 quantization happens at load time).
    local attempt=1 max_attempts=5
    while ((attempt <= max_attempts)); do
        if uv run --directory "$VLLM_DIR" python -c "
from huggingface_hub import snapshot_download
snapshot_download('${repo}', local_dir='${PWD}/${model_dir}', resume_download=True)
print('Download complete')
"; then
            ok "$name -> $model_dir ($(du -sh "$model_dir" | cut -f1))"
            return
        fi
        if ((attempt < max_attempts)); then
            local wait=$((2 ** (attempt - 1)))
            warn "$name download attempt $attempt failed — retrying in ${wait}s..."
            sleep "$wait"
        else
            warn "$name download failed after $max_attempts attempts"
            return
        fi
        ((attempt++))
    done
}

step_models() {
    section "Models"

    # Non-interactive: respect MODEL_DOWNLOAD / MODEL_INDEX env vars
    # MODEL_DOWNLOAD=1 means HuggingFace, MODEL_INDEX sets which model (0-5, or "a" for all)
    if [[ ! -t 0 ]]; then
        local idx="${MODEL_DOWNLOAD:-2}"

        if [[ "$idx" != "1" ]]; then
            warn "Non-interactive mode — skipping model download."
            warn "Set MODEL_DOWNLOAD=1 MODEL_INDEX=0 to auto-download."
            warn "Or re-run ./setup.sh interactively."
            return
        fi
    else
        echo "  How do you want to get models?"
        local idx
        idx=$(choose "  " \
            "SCP from another machine (fastest — rsync pre-downloaded safetensors)" \
            "Download + convert from HuggingFace (30-60 min per model)" \
            "Skip — I will handle models myself")
    fi

    case "$idx" in
        0)
            echo ""
            read -r -p "  Source host [user@host]: " src_host
            for entry in "${MODEL_LIST[@]}"; do
                IFS='|' read -r name repo <<< "$entry"
                echo "  Copying $name..."
                mkdir -p "data/models/${name}"
                rsync -a "$src_host:projects/minif2f/data/models/${name}/" "data/models/${name}/" 2>/dev/null && \
                    ok "$name copied" || warn "$name failed — try download instead"
            done
            ;;
        1)
            # MODEL_INDEX: "a" for all, or 0-5 for a specific model
            local model_choice="${MODEL_INDEX:-}"
            if [[ -z "$model_choice" ]]; then
                echo ""
                echo "  Which models to download?"
                for i in "${!MODEL_LIST[@]}"; do
                    IFS='|' read -r name repo <<< "${MODEL_LIST[$i]}"
                    echo -e "    ${BOLD}$((i+1)))${NC} $name"
                done
                echo -e "    ${BOLD}a)${NC} All of them"
                read -r -p "  Choose [1-${#MODEL_LIST[@]}/a]: " model_choice
            fi

            if [[ "$model_choice" == "a" ]]; then
                for entry in "${MODEL_LIST[@]}"; do
                    IFS='|' read -r name repo <<< "$entry"
                    step_model_single "$name" "$repo"
                done
            elif [[ "$model_choice" =~ ^[0-9]+$ ]] && ((model_choice >= 0 && model_choice < ${#MODEL_LIST[@]})); then
                IFS='|' read -r name repo <<< "${MODEL_LIST[$model_choice]}"
                step_model_single "$name" "$repo"
            elif [[ "$model_choice" =~ ^[0-9]+$ ]] && ((model_choice >= 1 && model_choice <= ${#MODEL_LIST[@]})); then
                local i=$((model_choice - 1))
                IFS='|' read -r name repo <<< "${MODEL_LIST[$i]}"
                step_model_single "$name" "$repo"
            fi
            ;;
        2)
            warn "Skipping models — place safetensors dirs in data/models/<name>/ manually"
            ;;
    esac
}

step_done() {
    section "Setup complete"

    echo ""
    echo -e "  ${BOLD}Next:${NC}"
    echo ""
    echo "  ./run          # Interactive menu"
    echo ""
    echo "  # Or directly:"
    echo "  cargo run --release -- generate -m kimina-prover-rl-1.7b -p data/models/kimina-prover-rl-1.7b"
    echo ""
    echo "  # Check status:"
    echo "  cargo run --release -- list-models"
    echo "  cargo run --release -- status --run-id v1"
    echo ""
    echo -e "  ${BOLD}Output:${NC} output/raw_output/<model>.json + output/lean_code/<model>.json"
    echo ""
}

# ── Main ────────────────────────────────────────────────────────────────
main() {
    echo -e "${BOLD}╔══════════════════════════════════════╗${NC}"
    echo -e "${BOLD}║    minif2f — New Machine Setup       ║${NC}"
    echo -e "${BOLD}╚══════════════════════════════════════╝${NC}"

    step_prerequisites
    step_vllm
    step_dataset
    step_rust_build
    step_models
    step_done
}

# Only run main when executed directly (not sourced)
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main
fi
