use crate::config::ModelConfig;
use crate::data::Theorem;

pub struct PromptBuilder {
    pub config: ModelConfig,
}

impl PromptBuilder {
    #[must_use]
    pub fn new(config: ModelConfig) -> Self {
        Self { config }
    }

    /// Build the full prompt string with model-specific chat template and user message format.
    #[must_use]
    pub fn build(&self, theorem: &Theorem) -> String {
        let user = self.build_user_message(theorem);
        let arch = self.config.architecture.to_lowercase();

        if arch.contains("qwen") {
            // Qwen3 ChatML with enable_thinking=false: empty <think> block tells
            // the model to skip reasoning and go directly to the answer.
            format!(
                "<|im_start|>system\n{}<|im_end|>\n<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n",
                self.config.system_prompt, user
            )
        } else if arch.contains("deepseek_v2") {
            // DeepSeek V2/V3: <｜User｜>content<｜Assistant｜> (Unicode fullwidth ｜ U+FF5C)
            format!(
                "<｜begin▁of▁sentence｜>{}<｜User｜>{}<｜Assistant｜>",
                self.config.system_prompt, user
            )
        } else if arch.contains("deepseek_coder") {
            // DeepSeek Coder / V1: ### Instruction: / ### Response: / <|EOT|>
            format!(
                "{}### Instruction:\n{}\n### Response:\n",
                self.config.system_prompt, user
            )
        } else {
            // Generic fallback
            format!(
                "System: {}\n\nUser: {}\n\nAssistant: ",
                self.config.system_prompt, user
            )
        }
    }

    fn build_user_message(&self, theorem: &Theorem) -> String {
        let fmt = self.config.prompt_format.to_lowercase();
        match fmt.as_str() {
            "goedel_v2" => Self::build_goedel_v2(theorem),
            "simple" => Self::build_simple(theorem),
            _ => Self::build_kimina(theorem), // default: kimina format
        }
    }

    /// Kimina format (official): "Think about and solve the following problem step by step in Lean 4."
    fn build_kimina(theorem: &Theorem) -> String {
        use std::fmt::Write;

        let mut parts: Vec<&str> = vec![];
        if !theorem.header.is_empty() {
            parts.push(&theorem.header);
        }
        if !theorem.informal_prefix.is_empty() {
            parts.push(&theorem.informal_prefix);
        }
        parts.push(&theorem.formal_statement);
        let formal_block = parts.join("\n");

        let problem_desc = extract_problem_desc(&theorem.informal_prefix);

        let mut msg =
            "Think about and solve the following problem step by step in Lean 4.".to_string();
        if !problem_desc.is_empty() {
            let _ = write!(msg, "\n# Problem: {problem_desc}");
        }
        let _ = write!(msg, "\n# Formal statement:\n```lean4\n{formal_block}\n```");
        msg
    }

    /// Goedel V2 / `DeepSeek` Prover V2 format (official):
    /// "Complete the following Lean 4 code: ... provide a detailed proof plan ..."
    fn build_goedel_v2(theorem: &Theorem) -> String {
        let mut parts: Vec<&str> = vec![];
        if !theorem.header.is_empty() {
            parts.push(&theorem.header);
        }
        if !theorem.informal_prefix.is_empty() {
            parts.push(&theorem.informal_prefix);
        }
        parts.push(&theorem.formal_statement);
        let formal_block = parts.join("\n");

        format!(
            "Complete the following Lean 4 code:\n\n```lean4\n{formal_block}\n```\n\n\
             Before producing the Lean 4 code to formally prove the given theorem, provide \
             a detailed proof plan outlining the main proof steps and strategies.\n\
             The plan should highlight key ideas, intermediate lemmas, and proof structures \
             that will guide the construction of the final formal proof."
        )
    }

    /// Simple format (user's default): plain theorem statement, no extra structure.
    fn build_simple(theorem: &Theorem) -> String {
        let mut parts: Vec<&str> = vec![];
        if !theorem.header.is_empty() {
            parts.push(&theorem.header);
        }
        if !theorem.informal_prefix.is_empty() {
            parts.push(&theorem.informal_prefix);
        }
        parts.push(&theorem.formal_statement);
        let formal_block = parts.join("\n");

        format!(
            "This is a theorem written in Lean 4. Please complete the corresponding proof code to formalize the argument:\n\n{formal_block}"
        )
    }

    /// Extract the complete Lean file from model output.
    /// With `enable_thinking=false`, the model outputs a Lean 4 code block after `</think>`.
    #[must_use]
    pub fn extract_proof(&self, raw: &str) -> String {
        // Find the ```lean4 block AFTER </think> (or after the empty <think> if thinking disabled)
        let search_from = raw.find("</think>").map_or(0, |p| p + "</think>".len());
        let after_think = &raw[search_from..];

        // Try to find ```lean4 block
        if let Some(code) = extract_fenced_code(after_think) {
            return code;
        }

        // Fallback: search entire text for any fenced code
        if let Some(code) = extract_fenced_code(raw) {
            return code;
        }

        // Last resort: return raw text (stripped of think blocks and chat tokens)
        let text = strip_think_blocks(raw);
        strip_chat_tokens(&text)
    }
}

/// Extract natural language problem description from `informal_prefix` (/-- ... -/).
fn extract_problem_desc(prefix: &str) -> String {
    if prefix.starts_with("/--") && prefix.ends_with("-/") {
        prefix[3..prefix.len().saturating_sub(2)].trim().to_string()
    } else {
        prefix.trim().to_string()
    }
}

// ── Proof extraction helpers ──────────────────────────────────────────

fn extract_fenced_code(text: &str) -> Option<String> {
    // Collect ALL code blocks, return the one with most content after stripping header.
    // The model often generates multiple blocks — the first may only contain the theorem
    // statement, while later blocks have the actual proof.
    let mut best: Option<String> = None;
    let mut best_len = 0;

    for (fence_start, _fence_label) in [
        ("```lean4\n", "lean4\n"),
        ("```lean4", "lean4"),
        ("```tactics\n", "tactics\n"),
        ("```tactics", "tactics"),
        ("```\n", "\n"),
        ("```", ""),
    ] {
        let mut search_from = 0;
        while let Some(pos) = text[search_from..].find(fence_start) {
            let abs_pos = search_from + pos;
            let start = abs_pos + fence_start.len();
            let rest = &text[start..];
            if let Some(end) = rest.find("```") {
                let code = rest[..end].trim().to_string();
                let stripped = strip_theorem_header(&code);
                let stripped_len = stripped.trim().len();
                if stripped_len > best_len {
                    best_len = stripped_len;
                    best = Some(code);
                }
                search_from = start + end + 3; // continue past this block
            } else {
                break;
            }
        }
    }
    best
}

fn strip_think_blocks(text: &str) -> String {
    let mut result = text.to_string();
    loop {
        let s = result.find("<think>");
        let e = result.find("</think>");
        match (s, e) {
            (Some(start), Some(end)) if start < end => {
                let after_end = end + "</think>".len();
                result.replace_range(start..after_end, "");
            }
            (Some(start), _) => {
                // Incomplete <think> — model ran out of tokens.
                // Remove just the <think> tag, keep the reasoning (may contain Lean code).
                result.replace_range(start..start + "<think>".len(), "");
                break;
            }
            _ => break,
        }
    }
    result
}

fn strip_chat_tokens(text: &str) -> String {
    let mut s = text.to_string();
    for tok in [
        // Qwen / general
        "<|im_end|>",
        "<|im_start|>",
        "<|endoftext|>",
        "<|begin_of_text|>",
        "<|end_of_text|>",
        "<|eot_id|>",
        "</s>",
        // DeepSeek V2 (Unicode fullwidth ｜)
        "<｜User｜>",
        "<｜Assistant｜>",
        "<｜begin▁of▁sentence｜>",
        "<｜end▁of▁sentence｜>",
        // DeepSeek Coder / V1
        "### Instruction:",
        "### Response:",
        "<|EOT|>",
        // DeepSeek ASCII variants
        "<|User|>",
        "<|Assistant|>",
    ] {
        s = s.replace(tok, "");
    }
    s
}

fn strip_theorem_header(code: &str) -> String {
    // Use rfind (last occurrence) — the model often outputs the theorem
    // statement inside a code fence followed by actual proof code.
    if let Some(pos) = code.rfind(":= by\n") {
        return code[pos + ":=".len() + " by\n".len()..].trim().to_string();
    }
    if let Some(pos) = code.rfind(":= by") {
        let after_pos = pos + ":=".len();
        let rest = code[after_pos..].trim();
        // Remove leading "by" (with optional space)
        let after_by = rest.strip_prefix("by").map_or("", str::trim);
        return after_by.to_string();
    }
    code.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_theorem() -> Theorem {
        Theorem {
            name: "test_thm".into(),
            split: "test".into(),
            informal_prefix: "/-- The sum of two even numbers is even -/".into(),
            formal_statement: "theorem test_thm (a b : Nat) : a + b = b + a := by".into(),
            header: "import Mathlib\nopen Nat".into(),
            goal: String::new(),
        }
    }

    // ── Prompt format tests ───────────────────────────────────────────

    #[test]
    fn test_kimina_format() {
        let cfg = crate::models::find_model("kimina-prover-rl-1.7b").unwrap();
        let pb = PromptBuilder::new(cfg);
        let t = make_theorem();
        let prompt = pb.build(&t);
        assert!(prompt.contains("Think about and solve"));
        assert!(prompt.contains("import Mathlib"));
        assert!(prompt.contains("theorem test_thm"));
        assert!(prompt.contains("<|im_start|>system"));
        assert!(prompt.contains("<|im_start|>user"));
        assert!(prompt.contains("<think>\n\n</think>"));
    }

    #[test]
    fn test_goedel_v2_format() {
        let cfg = crate::models::find_model("goedel-prover-v2-8b").unwrap();
        let pb = PromptBuilder::new(cfg);
        let t = make_theorem();
        let prompt = pb.build(&t);
        assert!(prompt.contains("Complete the following Lean 4 code"));
        assert!(prompt.contains("proof plan"));
        assert!(prompt.contains("import Mathlib"));
        assert!(prompt.contains("<|im_start|>system"));
    }

    #[test]
    fn test_simple_format() {
        let cfg = crate::models::find_model("goedel-prover-dpo").unwrap();
        let pb = PromptBuilder::new(cfg);
        let t = make_theorem();
        let prompt = pb.build(&t);
        assert!(prompt.contains("This is a theorem written in Lean 4"));
        assert!(prompt.contains("### Instruction:"));
        assert!(prompt.contains("### Response:"));
        assert!(prompt.contains("theorem test_thm"));
    }

    #[test]
    fn test_deepseek_v2_format() {
        let cfg = crate::models::find_model("deepseek-prover-v2-7b").unwrap();
        let pb = PromptBuilder::new(cfg);
        let t = make_theorem();
        let prompt = pb.build(&t);
        assert!(prompt.contains("<｜begin▁of▁sentence｜>"));
        assert!(prompt.contains("<｜User｜>"));
        assert!(prompt.contains("<｜Assistant｜>"));
        assert!(prompt.contains("Complete the following Lean 4 code"));
    }

    // ── Proof extraction tests ─────────────────────────────────────────

    #[test]
    fn test_extract_lean4_block_after_think() {
        let cfg = crate::models::find_model("kimina-prover-rl-1.7b").unwrap();
        let pb = PromptBuilder::new(cfg);
        let raw =
            "<think>\n\n</think>\n\n```lean4\nimport Mathlib\ntheorem foo : 1 = 1 := by rfl\n```";
        let proof = pb.extract_proof(raw);
        assert!(proof.contains("import Mathlib"));
        assert!(proof.contains("theorem foo"));
        assert!(proof.contains("rfl"));
    }

    #[test]
    fn test_extract_proof_no_think_block() {
        let cfg = crate::models::find_model("kimina-prover-rl-1.7b").unwrap();
        let pb = PromptBuilder::new(cfg);
        let raw = "```lean4\ntheorem foo : 1 = 1 := by rfl\n```";
        let proof = pb.extract_proof(raw);
        assert!(proof.contains("theorem foo"));
        assert!(proof.contains("rfl"));
    }

    #[test]
    fn test_extract_proof_fallback_to_raw() {
        let cfg = crate::models::find_model("kimina-prover-rl-1.7b").unwrap();
        let pb = PromptBuilder::new(cfg);
        let raw = "theorem foo : 1 = 1 := by rfl";
        let proof = pb.extract_proof(raw);
        assert!(proof.contains("theorem foo"));
        assert!(proof.contains("rfl"));
        // Chat tokens should be stripped
        assert!(!proof.contains("<|im_end|>"));
    }

    #[test]
    fn test_strip_chat_tokens() {
        let cfg = crate::models::find_model("kimina-prover-rl-1.7b").unwrap();
        let pb = PromptBuilder::new(cfg);
        let raw = "<|im_end|>theorem foo : 1 = 1 := by rfl<|im_start|>";
        let proof = pb.extract_proof(raw);
        assert!(!proof.contains("<|im_end|>"));
        assert!(!proof.contains("<|im_start|>"));
        assert!(proof.contains("theorem foo"));
    }
}
