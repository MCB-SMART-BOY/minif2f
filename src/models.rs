use crate::config::ModelConfig;

fn defaults() -> ModelConfig {
    ModelConfig {
        name: String::new(),
        hf_repo: String::new(),
        architecture: "qwen3".into(),
        prompt_format: "kimina".into(),
        param_count_b: None,
        quantization: None,
        max_model_len: 4096,
        temperature: 0.6,
        top_p: 0.95,
        max_tokens: 4096,
        seed: 42,
        stop_sequences: vec!["<|im_end|>".into(), "</s>".into()],
        system_prompt: "You are an expert in mathematics and proving theorems in Lean 4.".into(),
    }
}

#[must_use]
pub fn builtin_models() -> Vec<ModelConfig> {
    vec![
        // 1. Goedel-Prover-DPO — DeepSeek Coder (LLaMA-based)
        //    Official eval uses raw completion prompt, not chat:
        //    "Complete ... explanatory comments ..." + open ```lean4 block.
        //    Eval script: max_model_len=4096, max_tokens=2048,
        //    temperature=1.0, top_p=0.95, LLM seed=1.
        //    EOS=100001 (<｜end▁of▁sentence｜>)
        ModelConfig {
            name: "goedel-prover-dpo".into(),
            hf_repo: "Goedel-LM/Goedel-Prover-DPO".into(),
            architecture: "raw".into(),
            prompt_format: "simple".into(),
            param_count_b: Some(7.0),
            quantization: Some("awq".into()),
            max_model_len: 4096,
            temperature: 1.0,
            max_tokens: 2048,
            seed: 1,
            stop_sequences: vec![
                "<｜end▁of▁sentence｜>".into(),
                "<|EOT|>".into(),
                "### Instruction:".into(),
                "</s>".into(),
            ],
            system_prompt: String::new(),
            ..defaults()
        },
        // 2. Kimina-Prover-RL-1.7B — Qwen3 ChatML
        //    HF config.json: max_position_embeddings=40960
        //    HF quickstart: max_tokens=8096, temp=0.6, top_p=0.95
        //    Output: <think>...</think> + ```lean4 block
        //    EOS=151645 (<|im_end|>)
        ModelConfig {
            name: "kimina-prover-rl-1.7b".into(),
            hf_repo: "AI-MO/Kimina-Prover-RL-1.7B".into(),
            architecture: "qwen3".into(),
            prompt_format: "kimina".into(),
            max_model_len: 40960,
            max_tokens: 8096,
            ..defaults()
        },
        // 3. Goedel-Prover-V2-8B — Qwen3 ChatML
        //    HF config: max_position_embeddings=40960; quickstart: max_new_tokens=32768, seed=30
        //    Prompt: proof plan + ```lean4 block with sorry placeholder
        //    Chat: user message only — NO system prompt
        //    EOS=151645 (<|im_end|>)
        //    max_model_len kept at the HF 40960 (official). At parallel=16 the
        //    KV cache would be ~40 GB → preemption. Mitigated by running at
        //    parallel=8 (~20 GB KV, fits RTX 5090). max_tokens stays at the
        //    official 32768; vLLM validates max_tokens ≤ max_model_len.
        ModelConfig {
            name: "goedel-prover-v2-8b".into(),
            hf_repo: "Goedel-LM/Goedel-Prover-V2-8B".into(),
            architecture: "qwen3".into(),
            prompt_format: "goedel_v2".into(),
            param_count_b: Some(8.0),
            quantization: Some("awq".into()),
            max_model_len: 40960,
            max_tokens: 32768,
            seed: 30,
            system_prompt: String::new(), // official: no system message
            ..defaults()
        },
        // 4. DeepSeek-Prover-V2-7B — DeepSeek V2 (LLaMA-based)
        //    HF config.json: max_position_embeddings=65536
        //    Model card: "extended context length of up to 32K tokens"
        //    HF quickstart: max_new_tokens=8192, seed=30
        //    Prompt: "Complete the following Lean 4 code:" (non-CoT — no proof plan request)
        //    Chat: NO system prompt — only [{"role": "user", "content": prompt}]
        //    Unicode fullwidth tokens: <｜User｜>, <｜Assistant｜>
        //    EOS=100001, BOS=100000
        ModelConfig {
            name: "deepseek-prover-v2-7b".into(),
            hf_repo: "deepseek-ai/DeepSeek-Prover-V2-7B".into(),
            architecture: "deepseek_v2".into(),
            prompt_format: "goedel_v2_nocot".into(),
            param_count_b: Some(7.0),
            quantization: Some("awq".into()),
            max_model_len: 65536,
            max_tokens: 8192,
            seed: 30,
            stop_sequences: vec![
                "<｜end▁of▁sentence｜>".into(),
                "<｜Assistant｜>".into(),
                "<｜User｜>".into(),
                "</s>".into(),
            ],
            system_prompt: String::new(), // official: no system prompt
            ..defaults()
        },
        // 5. Kimina-Prover-Distill-8B — Qwen3 ChatML
        //    HF config.json: max_position_embeddings=40960
        //    HF quickstart: max_tokens=8096, temp=0.6, top_p=0.95
        //    System prompt: "You are an expert in mathematics and Lean 4."
        //    EOS=151645 (<|im_end|>)
        ModelConfig {
            name: "kimina-prover-distill-8b".into(),
            hf_repo: "AI-MO/Kimina-Prover-Distill-8B".into(),
            architecture: "qwen3".into(),
            prompt_format: "kimina".into(),
            param_count_b: Some(8.0),
            quantization: Some("awq".into()),
            max_model_len: 40960,
            max_tokens: 8096,
            system_prompt: "You are an expert in mathematics and Lean 4.".into(),
            ..defaults()
        },
        // 6. STP_model_Lean — based on DeepSeek-Prover-V1.5 (LLaMA-based)
        //    HF config.json: max_position_embeddings=4096
        //    Official eval: max_model_len=1024 (run_generation_and_test.sh)
        //    Paper §3.1: "Complete the following Lean 4 code:" + ```lean4
        //    run_generation_and_test.sh: temperature=1.0, seed=1
        //    model_utils.py: top_p=1.0, max_tokens=1024
        //    STP fine-tuning overrides the base chat template → raw text only.
        //    Format: statement = formal_statement.rsplit("sorry", 1)[0].strip()
        //    — model generates proof body from `:= by`
        //    EOS=100001 (<｜end▁of▁sentence｜>)
        ModelConfig {
            name: "stp-model-lean".into(),
            hf_repo: "kfdong/STP_model_Lean".into(),
            architecture: "raw".into(),
            prompt_format: "deepseek_prover".into(),
            max_model_len: 1024,
            max_tokens: 1024,
            temperature: 1.0,
            top_p: 1.0,
            seed: 1,
            quantization: Some("awq".into()),
            stop_sequences: vec!["<｜end▁of▁sentence｜>".into(), "</s>".into()],
            system_prompt: String::new(),
            ..defaults()
        },
    ]
}

#[must_use]
pub fn find_model(name: &str) -> Option<ModelConfig> {
    builtin_models().into_iter().find(|m| m.name == name)
}

#[must_use]
pub fn list_model_names() -> Vec<String> {
    builtin_models().iter().map(|m| m.name.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_6_models_registered() {
        let models = builtin_models();
        assert_eq!(models.len(), 6);
    }

    #[test]
    fn test_find_known_model() {
        let m = find_model("kimina-prover-rl-1.7b");
        assert!(m.is_some());
        let m = m.unwrap();
        assert_eq!(m.architecture, "qwen3");
        assert_eq!(m.prompt_format, "kimina");
        assert_eq!(m.max_model_len, 40960);
        assert_eq!(m.max_tokens, 8096);
    }

    #[test]
    fn test_official_context_limits() {
        // goedel-v2 uses the official HF 40960; VRAM controlled via parallel=8
        assert_eq!(
            find_model("goedel-prover-v2-8b").unwrap().max_model_len,
            40960
        );
        assert_eq!(
            find_model("deepseek-prover-v2-7b").unwrap().max_model_len,
            65536
        );
        assert_eq!(find_model("stp-model-lean").unwrap().max_model_len, 1024);
    }

    #[test]
    fn test_official_sampling_params() {
        let dpo = find_model("goedel-prover-dpo").unwrap();
        assert_eq!(dpo.temperature, 1.0);
        assert_eq!(dpo.top_p, 0.95);
        assert_eq!(dpo.max_tokens, 2048);
        assert_eq!(dpo.seed, 1);

        let kimina_rl = find_model("kimina-prover-rl-1.7b").unwrap();
        assert_eq!(kimina_rl.temperature, 0.6);
        assert_eq!(kimina_rl.top_p, 0.95);
        assert_eq!(kimina_rl.max_tokens, 8096);

        let stp = find_model("stp-model-lean").unwrap();
        assert_eq!(stp.temperature, 1.0);
        assert_eq!(stp.top_p, 1.0);
        assert_eq!(stp.max_tokens, 1024);
        assert_eq!(stp.seed, 1);
    }

    #[test]
    fn test_find_unknown_model_returns_none() {
        assert!(find_model("nonexistent-model").is_none());
    }

    #[test]
    fn test_list_model_names_count() {
        let names = list_model_names();
        assert_eq!(names.len(), 6);
    }

    #[test]
    fn test_each_model_has_required_fields() {
        for m in builtin_models() {
            assert!(!m.name.is_empty(), "model has empty name");
            assert!(!m.hf_repo.is_empty(), "{} has empty hf_repo", m.name);
            assert!(
                !m.architecture.is_empty(),
                "{} has empty architecture",
                m.name
            );
            assert!(
                !m.prompt_format.is_empty(),
                "{} has empty prompt_format",
                m.name
            );
            assert!(m.max_model_len > 0, "{} has zero max_model_len", m.name);
            assert!(m.max_tokens > 0, "{} has zero max_tokens", m.name);
            assert!(m.temperature > 0.0, "{} has zero temperature", m.name);
            assert!(
                !m.system_prompt.is_empty()
                    || m.architecture == "raw"
                    || m.architecture == "deepseek_v2"
                    || m.architecture == "deepseek_coder"
                    || m.name == "goedel-prover-v2-8b", // official: user message only, no system
                "{} has empty system_prompt (requires arch justification)",
                m.name
            );
        }
    }

    #[test]
    fn test_all_models_have_different_names() {
        let names = list_model_names();
        let unique: std::collections::HashSet<_> = names.iter().collect();
        assert_eq!(unique.len(), names.len(), "duplicate model names found");
    }
}
