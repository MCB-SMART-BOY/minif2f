# Debug

Triggers: "乱码" "空输出" "提取率低" "检查质量" "U+FFFD"

## Decision Tree

### "输出乱码" → Encoding Corruption

Step 1: Identify model architecture
```
# Check architecture
python3 -c "from src.models import find_model; m=find_model('<name>'); print(m.architecture if m else '?')"
```

Step 2: Architecture-specific diagnostic
```
Qwen3 (qwen3):
  - Should have ZERO byte-fallback decoding applied
  - If U+FFFD > 0: decoder is being incorrectly applied → fix: architecture match in generate_one_with_retry
  - Normal: 0 U+FFFD, 0 Cyrillic

LLaMA (raw / deepseek_v2):
  - Should have decode_llama_byte_fallback applied
  - U+FFFD < 1%: expected (vLLM tokenizer leakage)
  - U+FFFD > 1%: check if Latin-1 range (U+0080-U+00FF) is being incorrectly decoded
  - Cyrillic: must be 0. If >0 → Latin-1 bytes being wrongly converted
  - Latin-1 chars (U+0080-U+00FF, excl. U+00B7·): record but don't block.
    These are vLLM 0.22.1 known issue — raw bytes mapped to wrong Unicode range.
    Fix requires vLLM upgrade or post-processing script.
```

Step 3: Run encoding scan
```python
import json
with open('output/raw_output/<model>.json') as f: data = json.load(f)
m = list(data.keys())[0]
fffd = cyrillic = latin1 = nonempty = 0
for t in data[m].values():
    for v in t.values():
        if not v: continue
        nonempty += 1
        if chr(0xFFFD) in v: fffd += 1
        if any(0x0400 <= ord(c) <= 0x04FF for c in v): cyrillic += 1
        # Latin-1 non-control (excl. U+00B7 Lean bullet)
        latin1 += sum(1 for c in v if 0x80 <= ord(c) <= 0xFF and ord(c) != 0xB7)
print(f'Non-empty: {nonempty}, U+FFFD: {fffd} ({fffd/max(nonempty,1)*100:.1f}%), Cyrillic: {cyrillic}, Latin-1: {latin1}')
```

### "0% 提取 / 空输出" → Model Output Issue

Step 1: Check raw_output emptiness
```
All empty: model generates nothing or only EOS
  → LLaMA: check if begin_suppress_tokens is in config.json (vLLM ignores it)
  → Qwen3: check if prompt format is correct
  → Test: send single prompt manually via curl

Some empty (< 5%): normal — some seeds produce degenerate output
  → Check if empty rate increased vs previous run
  → If temperature=1.0, some variance expected

Many empty (> 50%): serious problem
  → Sample non-empty entries → are they valid? Or truncated?
  → Check max_tokens config — too small?
  → Check stop_sequences — stopping prematurely?
```

Step 2: Raw output has content but lean extraction fails
```
Sample failing case → trace through extract_proof manually:

1. extract_fenced_code(raw) → found ```lean4 block?
2. strip_theorem_header(code) → found := by?
3. is_proof_body(stripped) → pass?
4. validate_lean_code(assembled) → which check fails?

Common failures:
- Model outputs English commentary instead of Lean → is_proof_body rejects
- Model outputs only "sorry" → has_proof_body rejects
- Model outputs markdown headers "### Proof" → strip_markdown_from_proof removes
- DeepSeek single-line ":=by" format → strip_theorem_header fallback handles this
```

### "提取率突然下降" → Regression

1. Compare old vs new: `diff old_output/lean_code/<model>.json new_output/`
2. Check if prompt format changed (models.rs diff)
3. Check if extract_proof or validate_lean_code changed (prompts.rs diff)
4. Sample failing entries from both runs → compare extraction path

### Post-Processing: Latin-1 Leak Fix

After pipeline completes, run post-processing on LLaMA models with significant Latin-1 leakage:
```python
# For each raw_output entry: convert U+0080-U+00FF (excl. U+00B7·) to raw bytes
# Then String::from_utf8_lossy to reconstruct broken UTF-8 sequences
# This partially recovers multi-byte chars split by vLLM's incorrect tokenization
```
