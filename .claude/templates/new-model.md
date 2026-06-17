# New Model Checklist

Use this template when adding a new model to the pipeline.

## 1. Research

- [ ] HuggingFace model card read and understood
- [ ] Official eval script located (GitHub repo)
- [ ] Paper/technical report read if available
- [ ] Architecture determined (qwen3 / deepseek_v2 / raw)
- [ ] Tokenizer class identified (for byte-fallback decision)

## 2. Configuration (models.rs)

```rust
ModelConfig {
    name: "model-name".into(),
    hf_repo: "org/model-name".into(),
    architecture: "qwen3|deepseek_v2|raw".into(),
    prompt_format: "kimina|goedel_v2|simple|deepseek_prover".into(),
    max_model_len: NNNNN,    // from official config/eval
    max_tokens: NNNNN,       // from official eval
    temperature: N.N,        // from official eval
    top_p: N.NN,             // from official eval
    seed: NN,                // from official eval
    stop_sequences: vec![...],
    system_prompt: "...".into(),
    ..defaults()
}
```

## 3. Prompt Format (prompts.rs)

- [ ] New format function added (or reuse existing)
- [ ] Chat template architecture handled
- [ ] `sorry` inclusion matches official
- [ ] Code block open/closed matches official

## 4. Documentation

- [ ] Added to [[02-models]] with complete official config
- [ ] Added to CLAUDE.md models table
- [ ] Added to README.md Supported Models
- [ ] Added to generate-all.sh MODELS array with parallel value

## 5. Testing

- [ ] Prompt format test added
- [ ] Sampling params test in models.rs
- [ ] Extract proof works with sample output
- [ ] Full quality gates: `bash .claude/hooks/quality.sh`
