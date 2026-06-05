#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; BOLD='\033[1m'; NC='\033[0m'

# ── Config ────────────────────────────────────────────────────────────
LLAMA_CPP_REPO="https://github.com/ggml-org/llama.cpp.git"
MINIF2F_URL="https://raw.githubusercontent.com/openai/miniF2F/main/minif2f.jsonl"
HF_TOKEN="${HF_TOKEN:-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}"
HF_HOME="${HF_HOME:-$PWD/data/models}"
# Default to huggingface.co (works with proxy). Use hf-mirror.com if no proxy.
HF_ENDPOINT="${HF_ENDPOINT:-https://huggingface.co}"

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

    # Check Vulkan SDK version + glslc — need >= 1.3.275 for cooperative matrix features
    local vulkan_ver
    vulkan_ver=$(pkg-config --modversion vulkan 2>/dev/null || echo "0")
    local use_vulkan=OFF
    if command -v glslc &>/dev/null && [[ "$(printf '%s\n' "1.3.275" "$vulkan_ver" | sort -V | head -1)" = "1.3.275" ]]; then
        use_vulkan=ON
        echo "  Vulkan SDK $vulkan_ver + glslc detected — enabling GPU backend"
    else
        if ! command -v glslc &>/dev/null; then
            warn "glslc not found — install shaderc for GPU acceleration"
        fi
        warn "Vulkan SDK $vulkan_ver is too old (need >= 1.3.275). Using CPU backend."
        warn "Install a newer Vulkan SDK for GPU acceleration:"
        warn "  https://vulkan.lunarg.com/sdk/home"
    fi

    echo "  Cloning llama.cpp (depth=1)..."
    git clone --depth 1 "$LLAMA_CPP_REPO" tools/llama.cpp

    # Check CUDA: auto-detect from common install paths
    local use_cuda=OFF
    if [[ -d /usr/local/cuda ]] && [[ -x /usr/local/cuda/bin/nvcc ]]; then
        export PATH="/usr/local/cuda/bin:$PATH"
        use_cuda=ON
        echo "  CUDA: $(/usr/local/cuda/bin/nvcc --version | grep release | awk '{print $5,$6}') — enabling GPU backend"
    elif [[ -d /usr/local/cuda-12.8 ]] && [[ -x /usr/local/cuda-12.8/bin/nvcc ]]; then
        export PATH="/usr/local/cuda-12.8/bin:$PATH"
        use_cuda=ON
        echo "  CUDA 12.8 detected — enabling GPU backend"
    fi

    echo "  Building with CUDA=$use_cuda Vulkan=OFF backend..."
    cmake -B tools/llama.cpp/build -S tools/llama.cpp \
        -DGGML_CUDA="$use_cuda" -DGGML_VULKAN=OFF -DCMAKE_BUILD_TYPE=Release
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

    # Ensure Python venv (using uv)
    local VENV_PYTHON="tools/venv/bin/python3"
    if [[ ! -d tools/venv ]]; then
        uv venv tools/venv --python 3.10
        uv pip install --python "$VENV_PYTHON" transformers torch sentencepiece
    fi

    export HF_HOME HF_TOKEN HF_ENDPOINT
    local model_dir="data/models/${name}"

    # Step 1: Download model files (with retry for transient network errors)
    echo "    Downloading from ${repo}..."
    export HF_HUB_ENABLE_HF_TRANSFER=0
    local attempt=1 max_attempts=5
    while ((attempt <= max_attempts)); do
        if "$VENV_PYTHON" -c "
from huggingface_hub import snapshot_download
snapshot_download('${repo}', local_dir='${model_dir}', resume_download=True)
print('Download complete')
"; then
            break
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

    # Step 2: Convert to GGUF (always f16 first; quantize after if needed)
    echo "    Converting to GGUF (f16)..."
    mkdir -p "$(dirname "$outfile")"
    local f16_tmp="models/.tmp-${name}-f16.gguf"
    if ! "$VENV_PYTHON" tools/llama.cpp/convert_hf_to_gguf.py "$model_dir" \
        --outfile "$f16_tmp" --outtype f16 2>&1 | tail -3; then
        warn "$name GGUF conversion failed"
        rm -f "$f16_tmp"
        return
    fi

    # Step 3: Quantize (skip if target is already f16)
    if [[ "$outtype" == "f16" ]]; then
        mv "$f16_tmp" "$outfile"
    else
        echo "    Quantizing to ${outtype}..."
        local qtype
        qtype=$(echo "$outtype" | tr '[:lower:]' '[:upper:]')
        tools/llama.cpp/build/bin/llama-quantize "$f16_tmp" "$outfile" "$qtype" 2>&1 | tail -3
        rm -f "$f16_tmp"
    fi

    if [[ -f "$outfile" ]]; then
        ok "$name -> $outfile ($(du -h "$outfile" | cut -f1))"
    else
        warn "$name GGUF conversion/quantization failed"
    fi
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
            "SCP from another machine (fastest — copy pre-built GGUF)" \
            "Download + convert from HuggingFace (30-60 min per model)" \
            "Skip — I will handle models myself")
    fi

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
            # MODEL_INDEX: "a" for all, or 0-5 for a specific model
            local model_choice="${MODEL_INDEX:-}"
            if [[ -z "$model_choice" ]]; then
                echo ""
                echo "  Which models to download?"
                for i in "${!MODEL_LIST[@]}"; do
                    IFS='|' read -r name repo outfile outtype <<< "${MODEL_LIST[$i]}"
                    echo -e "    ${BOLD}$((i+1)))${NC} $name ($outtype)"
                done
                echo -e "    ${BOLD}a)${NC} All of them"
                read -r -p "  Choose [1-${#MODEL_LIST[@]}/a]: " model_choice
            fi

            if [[ "$model_choice" == "a" ]]; then
                for entry in "${MODEL_LIST[@]}"; do
                    IFS='|' read -r name repo outfile outtype <<< "$entry"
                    step_model_single "$name" "$repo" "$outfile" "$outtype"
                done
            elif [[ "$model_choice" =~ ^[0-9]+$ ]] && ((model_choice >= 0 && model_choice < ${#MODEL_LIST[@]})); then
                IFS='|' read -r name repo outfile outtype <<< "${MODEL_LIST[$model_choice]}"
                step_model_single "$name" "$repo" "$outfile" "$outtype"
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

# Only run main when executed directly (not sourced)
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main
fi
