#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")"

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; BOLD='\033[1m'; NC='\033[0m'

# ── Config ────────────────────────────────────────────────────────────
LLAMA_CPP_REPO="https://github.com/ggml-org/llama.cpp.git"
MINIF2F_URL="https://raw.githubusercontent.com/openai/miniF2F/main/minif2f.jsonl"
HF_TOKEN="${HF_TOKEN:-}"
HF_HOME="${HF_HOME:-$PWD/data/models}"

# ── Model download URLs (HuggingFace) ─────────────────────────────────
# Format: "name|repo|outfile|outtype"
# outtype: f16 for ≤3B, q4_k_m for 7-8B
MODEL_LIST=(
  "goedel-prover-dpo|Goedel-LM/Goedel-Prover-DPO|models/goedel-prover-dpo.gguf|q4_k_m"
  "kimina-prover-rl-1.7b|AI-MO/Kimina-Prover-RL-1.7B|models/kimina-1.7b.gguf|f16"
  "goedel-prover-v2-8b|Goedel-LM/Goedel-Prover-V2-8B|models/goedel-prover-v2-8b.gguf|q4_k_m"
  "deepseek-prover-v2-7b|deepseek-ai/DeepSeek-Prover-V2-7B|models/deepseek-prover-v2-7b.gguf|q4_k_m"
  "kimina-prover-distill-8b|AI-MO/Kimina-Prover-Distill-8B|models/kimina-prover-distill-8b.gguf|q4_k_m"
  "stp-model-lean|kfdong/STP_model_Lean|models/stp-model-lean.gguf|q4_k_m"
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
    check_cmd cmake
    check_cmd cargo

    # Vulkan: try to detect
    if pkg-config --exists vulkan 2>/dev/null || dpkg -l libvulkan-dev &>/dev/null || pacman -Q vulkan-headers &>/dev/null 2>&1; then
        ok "Vulkan headers found"
    else
        warn "Vulkan headers not found — install vulkan-headers / libvulkan-dev for GPU backend"
    fi
}

step_llama_cpp() {
    section "Setting up llama.cpp"

    if [[ -f tools/llama.cpp/build/bin/llama-server ]]; then
        ok "llama-server already built"
        return
    fi

    if [[ -d tools/llama.cpp ]]; then
        warn "tools/llama.cpp exists but no build — rebuilding"
        rm -rf tools/llama.cpp
    fi

    echo "  Cloning llama.cpp (depth=1)..."
    git clone --depth 1 "$LLAMA_CPP_REPO" tools/llama.cpp

    echo "  Building with Vulkan backend..."
    cmake -B tools/llama.cpp/build -S tools/llama.cpp \
        -DGGML_VULKAN=ON -DGGML_CUDA=OFF -DCMAKE_BUILD_TYPE=Release
    cmake --build tools/llama.cpp/build --config Release -j"$(nproc)"

    ok "llama-server built at tools/llama.cpp/build/bin/llama-server"
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
    local name="$1" repo="$2" outfile="$3" outtype="$4"

    if [[ -f "$outfile" ]]; then
        ok "$name already exists ($(du -h "$outfile" | cut -f1))"
        return
    fi

    echo -e "  ${BOLD}Processing: $name${NC}"

    # Ensure Python venv
    if [[ ! -d tools/venv ]]; then
        python3 -m venv tools/venv
        source tools/venv/bin/activate
        pip install -q transformers torch sentencepiece
    else
        source tools/venv/bin/activate
    fi

    export HF_HOME HF_TOKEN
    local model_dir="data/models/${name}"

    # Download
    echo "    Downloading from ${repo}..."
    python3 -c "
from transformers import AutoModelForCausalLM, AutoTokenizer
m = AutoModelForCausalLM.from_pretrained('${repo}')
t = AutoTokenizer.from_pretrained('${repo}')
m.save_pretrained('${model_dir}')
t.save_pretrained('${model_dir}')
" 2>&1 | tail -1

    # Convert
    echo "    Converting to GGUF (${outtype})..."
    mkdir -p "$(dirname "$outfile")"
    python3 tools/llama.cpp/convert_hf_to_gguf.py "$model_dir" \
        --outfile "$outfile" --outtype "$outtype" 2>&1 | tail -1

    ok "$name → $outfile ($(du -h "$outfile" | cut -f1))"
}

step_models() {
    section "Models"

    echo "  How do you want to get models?"
    local idx
    idx=$(choose "  " \
        "SCP from another machine (fastest — copy pre-built GGUF)" \
        "Download + convert from HuggingFace (30-60 min per model)" \
        "Skip — I will handle models myself")

    case "$idx" in
        0)
            echo ""
            read -r -p "  Source host [user@host]: " src_host
            for entry in "${MODEL_LIST[@]}"; do
                IFS='|' read -r name repo outfile outtype <<< "$entry"
                echo "  Copying $name..."
                scp "$src_host:projects/minif2f/$outfile" "$outfile" 2>/dev/null && \
                    ok "$name copied" || warn "$name failed — try download instead"
            done
            ;;
        1)
            echo ""
            echo "  Which models to download?"
            for i in "${!MODEL_LIST[@]}"; do
                IFS='|' read -r name repo outfile outtype <<< "${MODEL_LIST[$i]}"
                echo -e "    ${BOLD}$((i+1)))${NC} $name ($outtype)"
            done
            echo -e "    ${BOLD}a)${NC} All of them"
            read -r -p "  Choose [1-${#MODEL_LIST[@]}/a]: " model_choice

            if [[ "$model_choice" == "a" ]]; then
                for entry in "${MODEL_LIST[@]}"; do
                    IFS='|' read -r name repo outfile outtype <<< "$entry"
                    step_model_single "$name" "$repo" "$outfile" "$outtype"
                done
            elif [[ "$model_choice" =~ ^[0-9]+$ ]] && ((model_choice >= 1 && model_choice <= ${#MODEL_LIST[@]})); then
                local i=$((model_choice - 1))
                IFS='|' read -r name repo outfile outtype <<< "${MODEL_LIST[$i]}"
                step_model_single "$name" "$repo" "$outfile" "$outtype"
            fi
            ;;
        2)
            warn "Skipping models — place GGUF files in models/ manually"
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
    echo "  cargo run --release -- generate -m kimina-prover-rl-1.7b -p models/kimina-1.7b.gguf"
    echo ""
    echo "  # Check status:"
    echo "  cargo run --release -- list-models"
    echo "  cargo run --release -- status --run-id v1"
    echo ""
    echo -e "  ${BOLD}Output:${NC} output/<model>.json"
    echo ""
}

# ── Main ────────────────────────────────────────────────────────────────
main() {
    echo -e "${BOLD}╔══════════════════════════════════════╗${NC}"
    echo -e "${BOLD}║    minif2f — New Machine Setup       ║${NC}"
    echo -e "${BOLD}╚══════════════════════════════════════╝${NC}"

    step_prerequisites
    step_llama_cpp
    step_dataset
    step_rust_build
    step_models
    step_done
}

main
