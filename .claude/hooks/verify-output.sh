#!/usr/bin/env bash
set -euo pipefail
MODEL="${1:?Usage: verify-output.sh <model_name>}"
echo "=== Verifying: $MODEL ==="

python3 -c "
import json, os, sys

raw_path = f'output/raw_output/{sys.argv[1]}.json'
lean_path = f'output/lean_code/{sys.argv[1]}.json'

# Structure
with open(raw_path) as f: raw = json.load(f)
with open(lean_path) as f: lean = json.load(f)
rm = list(raw.keys())[0]
lm = list(lean.keys())[0]
print(f'Raw: {len(raw[rm])} theorems, {os.path.getsize(raw_path)/1e6:.1f} MB')
print(f'Lean: {len(lean[lm])} theorems, {os.path.getsize(lean_path)/1e6:.1f} MB')

# Encoding
fffd=0; cyrillic=0; nonempty=0
for t in raw[rm].values():
    for v in t.values():
        if not v: continue
        nonempty+=1
        if chr(0xFFFD) in v: fffd+=1
        if any(chr(0x0400)<=c<=chr(0x04FF) for c in v): cyrillic+=1
print(f'Non-empty: {nonempty}, U+FFFD: {fffd}, Cyrillic: {cyrillic}')

# Extraction rate
l_nonempty=sum(1 for t in lean[lm].values() for v in t.values() if v)
rate=l_nonempty/nonempty*100 if nonempty else 0
print(f'Extraction: {l_nonempty}/{nonempty} ({rate:.1f}%)')
" "$MODEL"
echo "=== Done ==="
