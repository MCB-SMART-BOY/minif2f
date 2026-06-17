#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
echo "=== Quality Gates ==="
echo "[1/3] cargo fmt --check"
cargo fmt --check || { cargo fmt; echo "Fixed formatting - review and re-add"; exit 1; }
echo "[2/3] cargo clippy -- -D warnings"
cargo clippy -- -D warnings
echo "[3/3] cargo test"
cargo test
echo "=== All gates passed ==="
