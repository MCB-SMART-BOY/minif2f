#!/usr/bin/env bash
set -euo pipefail
FAIL=0
echo "=== Pre-Generate Checks ==="

echo "[1] GPU free?"
USED=$(nvidia-smi --query-gpu=memory.used --format=csv,noheader 2>/dev/null | head -1 | sed 's/ MiB//')
[ -n "$USED" ] && [ "$USED" -lt 500 ] && echo "  OK (${USED} MiB)" || { echo "  FAIL (${USED:-unknown} MiB)"; FAIL=1; }

echo "[2] Port 8080 free?"
! fuser 8080/tcp 2>/dev/null && echo "  OK" || { echo "  FAIL"; FAIL=1; }

echo "[3] Release current?"
cargo build --release 2>&1 | tail -1

echo "[4] Orphans?"
pgrep -f "vllm.entrypoints" >/dev/null 2>&1 && { echo "  Cleaning..."; pkill -f "vllm.entrypoints"; sleep 2; }
echo "  OK"

[ "$FAIL" -eq 1 ] && { echo "=== FAILED ==="; exit 1; } || echo "=== All OK ==="
