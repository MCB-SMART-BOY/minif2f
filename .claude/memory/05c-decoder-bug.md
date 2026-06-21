---
name: decoder-gpt2-bytefallback-bug
description: decode_llama_byte_fallback uses (cp-0x100) but LLaMA/DeepSeek BPE uses GPT-2 bytes_to_unicode table. Corrupts multibyte math symbols (ℤℕℝ) in raw/deepseek_v2 archs. IRREVERSIBLE — needs rerun.
metadata:
  type: project
---

## Decoder Byte-Fallback Bug (root cause, 2026-06-21)

**THE most serious finding — overrides earlier "raw_output is clean" claim.**
raw_output is clean ONLY for qwen3 models. LLaMA-based archs are corrupted at write time.

### Root cause
`inference.rs:227 decode_llama_byte_fallback()` reverses byte-fallback with
`byte = codepoint - 0x100` for chars in U+0100..U+01FF. This is WRONG.

LLaMA/DeepSeek tokenizers use the **GPT-2 `bytes_to_unicode` table** (same as
Qwen BPE), which is NOT a flat +0x100 offset. The two agree only on ASCII bytes;
they diverge for the 35 continuation-bytes that GPT-2 maps into U+0100..U+0145:

```
byte 0x7f → ġ (U+0121)   our decoder: 0x121-0x100 = 0x21 '!'   WRONG (should stay 0x7f)
byte 0x80 → Ģ (U+0122)   our decoder: 0x22 '"'                 WRONG
byte 0x84 → Ħ (U+0126)   our decoder: 0x26 '&'                 WRONG  ← breaks ℤ
... 35 bytes total mis-decoded
```

### Mechanism (end-to-end, verified by simulation)
```
ℤ  (U+2124, utf-8 = e2 84 a4)
 → model token string (GPT-2 byte-encoded): â Ħ ¤   (e2→â, 84→Ħ, a4→¤)
 → our buggy decoder: â & ¤   (Ħ→0x26 '&', the middle byte 0x84 PERMANENTLY → 0x26)
 → written to disk as "â&¤"
```
The CORRECT decoder (`char → gpt2_u2b[char] → byte`) would have produced `ℤ` directly.

### Corruption rate (sampled ~20k attempts each)
| arch | model | passes decoder? | corruption |
|------|-------|-----------------|------------|
| qwen3 | goedel-v2, kimina-rl, kimina-distill | NO (`_ => raw`) | **0.0%** |
| raw | goedel-prover-dpo | YES | **83.2%** |
| deepseek_v2 | deepseek-prover-v2 | YES | **46.7%** |

NOTE: qwen3 "Latin-1" chars flagged earlier (²³¹×÷±, é ö) are LEGITIMATE Unicode
(superscripts, math operators, names) in the model's natural-language reasoning —
NOT corruption. The earlier blanket "Latin-1 dirty" metric conflated these with
real corruption. Real corruption = `â`/`Ã` followed by a broken continuation byte.

### IRREVERSIBLE — cannot be fixed by post-processing
The mis-decode already happened BEFORE disk write. `Ħ`(0x84)→`&`(0x26) on disk is
now indistinguishable from a genuine `&`. 35 byte values collapse ambiguously.
Verified: `correct_decoder(stored_corrupted)` yields U+FFFD, not ℤ. Recovery rate 0.3%.

⇒ goedel-prover-dpo and deepseek-prover-v2 MUST be regenerated after fixing the
decoder. This is the ONE finding that forces GPU rerun. qwen3 models are fine.

### Fix
Replace flat-offset decode with the real GPT-2 `bytes_to_unicode` inverse table.
OR: drop the custom decoder entirely and let vLLM detokenize (it already handles
this correctly — that's why we should test whether the decoder is needed at all
on current vLLM 0.22.1). Re-run DPO + DeepSeek-V2 with the fix.

Supersedes the "raw is clean" line in [[extraction-double-header-bug]].
Related: [[stp-runner-walrus-bug]], [[02-models]], [[04-hardware]]
