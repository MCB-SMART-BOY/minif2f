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
        ModelConfig {
            name: "goedel-prover-dpo".into(),
            hf_repo: "Goedel-LM/Goedel-Prover-DPO".into(),
            architecture: "deepseek_coder".into(),
            prompt_format: "simple".into(), // no official prompt format
            param_count_b: Some(7.0),
            quantization: Some("awq".into()),
            stop_sequences: vec!["<|EOT|>".into(), "### Instruction:".into(), "</s>".into()],
            system_prompt: "You are an expert in mathematics and proving theorems in Lean 4.\n\n"
                .into(),
            ..defaults()
        },
        // 2. Kimina-Prover-RL-1.7B — Qwen3 ChatML, official max_tokens=8096
        ModelConfig {
            name: "kimina-prover-rl-1.7b".into(),
            hf_repo: "AI-MO/Kimina-Prover-RL-1.7B".into(),
            architecture: "qwen3".into(),
            prompt_format: "kimina".into(),
            max_model_len: 8192,
            max_tokens: 8192,
            ..defaults()
        },
        // 3. Goedel-Prover-V2-8B — Qwen3 ChatML, official max_new_tokens=32768
        ModelConfig {
            name: "goedel-prover-v2-8b".into(),
            hf_repo: "Goedel-LM/Goedel-Prover-V2-8B".into(),
            architecture: "qwen3".into(),
            prompt_format: "goedel_v2".into(),
            param_count_b: Some(8.0),
            quantization: Some("awq".into()),
            max_model_len: 8192,
            max_tokens: 8192,
            ..defaults()
        },
        // 4. DeepSeek-Prover-V2-7B — DeepSeek V2, official context=32K
        ModelConfig {
            name: "deepseek-prover-v2-7b".into(),
            hf_repo: "deepseek-ai/DeepSeek-Prover-V2-7B".into(),
            architecture: "deepseek_v2".into(),
            prompt_format: "goedel_v2".into(),
            param_count_b: Some(7.0),
            quantization: Some("awq".into()),
            max_model_len: 8192,
            max_tokens: 8192,
            stop_sequences: vec![
                "<｜end▁of▁sentence｜>".into(),
                "<｜Assistant｜>".into(),
                "<｜User｜>".into(),
                "</s>".into(),
            ],
            ..defaults()
        },
        // 5. Kimina-Prover-Distill-8B — Qwen3 ChatML, official max_tokens=8096
        ModelConfig {
            name: "kimina-prover-distill-8b".into(),
            hf_repo: "AI-MO/Kimina-Prover-Distill-8B".into(),
            architecture: "qwen3".into(),
            prompt_format: "kimina".into(),
            param_count_b: Some(8.0),
            quantization: Some("awq".into()),
            max_model_len: 8192,
            max_tokens: 8192,
            system_prompt: "You are an expert in mathematics and Lean 4.".into(),
            ..defaults()
        },
        // 6. STP_model_Lean — DeepSeek Coder (finetuned from DeepSeek-Prover-V1.5-SFT)
        ModelConfig {
            name: "stp-model-lean".into(),
            hf_repo: "kfdong/STP_model_Lean".into(),
            architecture: "deepseek_coder".into(),
            prompt_format: "simple".into(), // no official prompt format
            quantization: Some("awq".into()),
            stop_sequences: vec!["<|EOT|>".into(), "### Instruction:".into(), "</s>".into()],
            system_prompt: "You are an expert in mathematics and proving theorems in Lean 4.\n\n"
                .into(),
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
        assert_eq!(m.max_model_len, 8192);
        assert_eq!(m.max_tokens, 8192);
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
                !m.system_prompt.is_empty(),
                "{} has empty system_prompt",
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
