# Debug

Triggers: "乱码" "空输出" "提取率低" "检查质量" "U+FFFD"

## Decision Tree

### "输出乱码"
- Determine architecture (LLaMA or Qwen3)
- LLaMA: check `decode_llama_byte_fallback` applied correctly; scan U+FFFD/Cyrillic/Latin-1 counts
- Qwen3: check decoder is NOT applied; if it is → fix architecture match
- Latin-1 leak (e.g. `hâ$+`): record count, plan post-processing after pipeline completes

### "0% 提取 / 空输出"
- Check raw_output: all empty? some empty?
- All empty → model generates EOS immediately (vLLM doesn't support `begin_suppress_tokens`)
- Some empty → check temperature / seed / max_tokens
- Has content but lean empty → debug extract_proof with sample text

### "提取率突然下降"
- Compare old vs new checkpoint: raw → lean mapping
- Sample failing cases, trace through extract_proof step by step
- Check if prompt format was changed

### Encoding Scan (standard diagnostic)
```python
import json
with open('output/raw_output/<model>.json') as f:
    data = json.load(f)
# Count U+FFFD, Cyrillic, Latin-1 non-control
# Report: total, non-empty, U+FFFD count/rate, Cyrillic count, top Latin-1 chars
```
