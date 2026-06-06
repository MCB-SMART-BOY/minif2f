# debug-prompt

Test and debug prompt formats for individual models. Use when prompt changes are made, a model produces empty outputs, or you need to verify prompt correctness before a full pipeline run.

## Quick test: single completion via curl

```bash
# Start a test llama-server (adjust model/port as needed)
./tools/llama.cpp/build/bin/llama-server \
  -m models/goedel-prover-dpo.gguf \
  --port 8099 -ngl 99 --ctx-size 4096 --parallel 1 \
  --no-warmup --cache-type-k q8_0 --cache-type-v q8_0 \
  --flash-attn on --api-key minif2f &

# Wait for health
until curl -s -H "Authorization: Bearer minif2f" http://localhost:8099/health | grep -q ok; do sleep 1; done

# Send test prompt (adjust to model's format)
curl -s --noproxy '*' \
  -H "Authorization: Bearer minif2f" \
  -H "Content-Type: application/json" \
  -d '{
    "prompt": "### Instruction:\nComplete the following Lean 4 code:\n\n```lean4\nimport Mathlib\n\ntheorem test : 1 + 1 = 2 := by\n```\n\n### Response:\n```lean4\nimport Mathlib\n\ntheorem test : 1 + 1 = 2 := by\n",
    "n_predict": 128,
    "temperature": 0.6,
    "top_p": 0.95,
    "seed": 42,
    "stop": ["<｜end▁of▁sentence｜>", "<|EOT|>", "### Instruction:", "</s>"],
    "n_probs": 0
  }' \
  http://localhost:8099/completion | python3 -c "
import json, sys
d = json.load(sys.stdin)
print(f'tokens_predicted: {d.get(\"tokens_predicted\", \"?\")}')
print(f'stop_type: {d.get(\"stop_type\", \"?\")}')
print(f'content ({len(d.get(\"content\",\"\"))} chars):')
print(d.get('content', '')[:500])
"
```

## Dump all theorem prompts

```bash
# Print prompt for a specific theorem
python3 -c "
import json
with open('data/raw/minif2f.jsonl') as f:
    for line in f:
        obj = json.loads(line)
        if obj['name'] == 'THEOREM_NAME_HERE':
            print(json.dumps(obj, indent=2))
            break
"
```

## Common prompt issues

| Symptom | Likely Cause | Check |
|---------|-------------|-------|
| All outputs empty (``) | Stop token in prompt start / closed code block | Does prompt end with ``` inside a code block? |
| Output is English prose, not Lean | Model generating outside code block | Is prepopulated response block open or closed? |
| Truncated output (truncated=1) | max_tokens too low for theorem | Check n_tokens vs max_tokens |
| Very short outputs (<10 tokens) | Model hitting EOS immediately | Check stop_sequences don't match prompt content |
| Model ignores system prompt | Empty system_prompt → block omitted | Qwen3: check `system_prompt.is_empty()` logic |

## Per-model prompt format reference

| Model | Architecture | Prompt Format | Key Gotcha |
|-------|-------------|---------------|------------|
| goedel-prover-dpo | deepseek_coder | simple | Strip trailing ``` from prepopulated response |
| kimina-prover-rl-1.7b | qwen3 | kimina | Don't prepopulate `<think>` |
| goedel-prover-v2-8b | qwen3 | goedel_v2 | User msg only, no system prompt |
| deepseek-prover-v2-7b | deepseek_v2 | goedel_v2 | Unicode ｜, no system prompt |
| kimina-prover-distill-8b | qwen3 | kimina | Same as kimina-rl |
| stp-model-lean | raw | deepseek_prover | No chat template, max 1024 ctx |

## Verify prompt fix: Goedel-DPO trailing ```

The `deepseek_coder` + `simple` template now strips the closing ```:
```rust
let prepop = prepop.strip_suffix("```").unwrap_or(prepop).trim_end();
```
Verify: prompt should end with `:= by` (open code block), NOT with `````.
