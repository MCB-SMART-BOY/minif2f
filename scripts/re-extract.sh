#!/usr/bin/env bash
# Re-extract Lean proofs from existing raw_output using current extraction logic.
# Use when extraction/validation code has been fixed but raw model outputs are OK.
# Avoids expensive GPU re-inference.
#
# Usage: bash scripts/re-extract.sh <model>
# Example: bash scripts/re-extract.sh goedel-prover-dpo

set -euo pipefail
PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$PROJECT_DIR"

MODEL="${1:?Usage: $0 <model-name>}"
RAW_INPUT="output/raw_output/${MODEL}.json"
LEAN_OUTPUT="output/lean_code/${MODEL}.json"
TMP_OUTPUT="${LEAN_OUTPUT}.tmp"

if [ ! -f "$RAW_INPUT" ]; then
    echo "ERROR: Raw output not found: $RAW_INPUT"
    exit 1
fi

echo "Re-extracting proofs for $MODEL..."
echo "  Input:  $RAW_INPUT ($(du -h "$RAW_INPUT" | cut -f1))"
echo "  Output: $LEAN_OUTPUT"

cargo run --release -- re-extract --raw-input "$RAW_INPUT" --lean-output "$TMP_OUTPUT" -m "$MODEL" \
    && mv "$TMP_OUTPUT" "$LEAN_OUTPUT" \
    && echo "Done: $LEAN_OUTPUT ($(du -h "$LEAN_OUTPUT" | cut -f1))"
