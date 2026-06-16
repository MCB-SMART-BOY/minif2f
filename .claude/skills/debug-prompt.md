# debug-prompt

Test and debug prompt formats for individual models via vLLM. Use when prompt changes are made, a model produces empty outputs, or you need to verify prompt correctness before a full pipeline run.

## Quick test: single completion via curl

```bash
# vLLM should already be running on port 8080 (check with: curl http://localhost:8080/health)
# If not, start it manually or with: cargo run -- generate -m <model> -p data/models/<model> --port 8080 -n 1 --parallel 1

# Send test prompt (adjust format per model)
curl -s --noproxy '*' \
  -H "Content-Type: application/json" \
  -d '{
    "prompt": "<|im_start|>user\nThink about and solve the following problem step by step in Lean 4.\n\n# Formal statement:\n```lean4\nimport Mathlib\n\ntheorem test : 1 + 1 = 2 := by\n```\n<|im_end|>\n<|im_start|>assistant\n",
    "max_tokens": 256,
    "temperature": 0.6,
    "top_p": 0.95,
    "seed": 42,
    "stop": ["<|im_end|>", "</s>"]
  }' http://localhost:8080/v1/completions | python3 -c "
import json,sys
d=json.load(sys.stdin)
print(f'tokens: {d.get(\"usage\",{}).get(\"completion_tokens\",\"?\")}')
print(f'content: [{d.get(\"choices\",[{}])[0].get(\"text\",\"\")[:500]}]')
print(f'finish_reason: {d.get(\"choices\",[{}])[0].get(\"finish_reason\",\"?\")}')
"
```

## Extract the actual prompt being sent

```bash
# Build a prompt the same way the Rust code does
python3 << 'PYEOF'
# Simulate Kimina prompt format
header = "import Mathlib\nimport Aesop\n\nset_option maxHeartbeats 0\n\nopen BigOperators Real Nat Topology Rat\n\n"
informal = "/-- Test theorem -/\n"
formal = "theorem test_thm : 1 + 1 = 2 := by\n"

# Kimina format (Qwen3 ChatML)
user = f"Think about and solve the following problem step by step in Lean 4.\n# Problem:Test theorem\n# Formal statement:\n```lean4\n{header}{informal}{formal}```"
prompt = f"<|im_start|>system\nYou are an expert in mathematics and proving theorems in Lean 4.<|im_end|>\n<|im_start|>user\n{user}<|im_end|>\n<|im_start|>assistant\n"
print("=== Kimina RL ===")
print(repr(prompt))

# DeepSeek non-CoT format
formal_block = header + informal + formal + "  sorry"
user = f"Complete the following Lean 4 code:\n\n```lean4\n{formal_block}\n```"
prompt = f"<｜User｜>{user}<｜Assistant｜>"
print("\n=== DeepSeek non-CoT ===")
print(repr(prompt))
PYEOF
```

## Common debug patterns

```bash
# Check what model vLLM is serving
curl -s http://localhost:8080/v1/models | python3 -m json.tool | head -10

# Kill and restart vLLM on a different port
fuser -k 8080/tcp
# Then restart via cargo run or manual uv command
```
