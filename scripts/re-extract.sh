#!/usr/bin/env bash
# Re-extract Lean proofs from existing raw_output using current extraction logic.
# Use when extraction/validation code has been fixed but raw model outputs are OK.
# Avoids expensive GPU re-inference.
#
# ONLY valid for qwen3 models (kimina-*, goedel-v2) — their raw_output is clean.
# LLaMA models (goedel-dpo, deepseek-v2) have decoder-corrupted raw that cannot
# be recovered offline; they must be regenerated on GPU with the fixed binary.
#
# Usage: bash scripts/re-extract.sh <model>
# Example: bash scripts/re-extract.sh kimina-prover-rl-1.7b

set -euo pipefail
PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_DIR"

MODEL="${1:?Usage: $0 <model-name>}"
RAW_INPUT="output/raw_output/${MODEL}.json"
LEAN_OUTPUT="output/lean_code/${MODEL}.json"

if [ ! -f "$RAW_INPUT" ]; then
    echo "ERROR: Raw output not found: $RAW_INPUT"
    exit 1
fi

echo "Re-extracting proofs for $MODEL..."
echo "  Input:  $RAW_INPUT ($(du -h "$RAW_INPUT" | cut -f1))"
echo "  Output: $LEAN_OUTPUT"

# Back up the existing lean_code before overwriting (safety net).
if [ -f "$LEAN_OUTPUT" ]; then
    cp "$LEAN_OUTPUT" "${LEAN_OUTPUT}.bak"
    echo "  Backup: ${LEAN_OUTPUT}.bak"
fi

cargo run --release -- re-extract -m "$MODEL"

