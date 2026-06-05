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

        match arch.as_str() {
            // ── Qwen3 ChatML — Kimina-Prover, Goedel-Prover-V2 ──────────
            // Kimina-Prover was RL-trained with a format reward requiring:
            //   <think>reasoning + optional Lean snippets</think>
            //   ```lean4
            //   complete proof
            //   ```
            // We let the model generate <think> naturally; do NOT prepopulate
            // an empty think block — that breaks the reasoning chain.
            "qwen3" => format!(
                "<|im_start|>system\n{}<|im_end|>\n<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n",
                self.config.system_prompt, user
            ),

            // ── DeepSeek V2 — Unicode fullwidth ｜ ────────────────────────
            "deepseek_v2" => format!(
                "<｜begin▁of▁sentence｜>{}<｜User｜>{}<｜Assistant｜>",
                self.config.system_prompt, user
            ),

            // ── DeepSeek Coder / V1 — ### Instruction: / ### Response: ───
            "deepseek_coder" => format!(
                "{}### Instruction:\n{}\n### Response:\n",
                self.config.system_prompt, user
            ),

            // ── Generic fallback ──────────────────────────────────────────
            _ => format!(
                "System: {}\n\nUser: {}\n\nAssistant: ",
                self.config.system_prompt, user
            ),
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
    /// The model is expected to output `<think>...</think>` followed by ` ```lean4 ` block.
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

    /// Goedel V2 / DeepSeek Prover V2 format (official):
    /// "Complete the following Lean 4 code: ... provide a detailed proof plan ..."
    /// The formal statement includes `sorry` as a placeholder — matching official format.
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
        // Add `sorry` placeholder matching official Goedel/DeepSeek format
        let formal_with_sorry = format!("{formal_block}\n  sorry");

        format!(
            "Complete the following Lean 4 code:\n\n```lean4\n{formal_with_sorry}\n```\n\n\
             Before producing the Lean 4 code to formally prove the given theorem, provide \
             a detailed proof plan outlining the main proof steps and strategies.\n\
             The plan should highlight key ideas, intermediate lemmas, and proof structures \
             that will guide the construction of the final formal proof."
        )
    }

    /// Simple format (DeepSeek Coder, Goedel-DPO, STP): plain theorem statement.
    /// Includes `sorry` placeholder matching official format.
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
        // Add `sorry` placeholder
        let formal_with_sorry = format!("{formal_block}\n  sorry");

        format!(
            "This is a theorem written in Lean 4. Please complete the corresponding proof code to formalize the argument:\n\n{formal_with_sorry}"
        )
    }

    /// Extract the complete Lean file from model output.
    ///
    /// Strategy (prioritised):
    /// 1. Find ```lean4 block after </think> — the primary output format for Kimina
    /// 2. For non-think architectures, find any ```lean4 block
    /// 3. Validate that the extracted code has actual proof content (not just header)
    /// 4. Fallback: strip think blocks and chat tokens, return raw text
    #[must_use]
    pub fn extract_proof(&self, raw: &str) -> String {
        // Find the ```lean4 block AFTER </think> (if present)
        let search_from = raw.find("</think>").map_or(0, |p| p + "</think>".len());
        let after_think = &raw[search_from..];

        // Try to find ```lean4 block after </think>
        if let Some(code) = extract_fenced_code(after_think) {
            let cleaned = strip_markdown_from_proof(&code);
            if has_proof_body(&cleaned) {
                return cleaned;
            }
        }

        // Fallback: search entire text for any fenced code
        if let Some(code) = extract_fenced_code(raw) {
            let cleaned = strip_markdown_from_proof(&code);
            if has_proof_body(&cleaned) {
                return cleaned;
            }
        }

        // Second fallback: try to extract Lean code from the raw text
        // (model might have output the proof without proper fencing)
        if let Some(code) = extract_lean_from_text(raw) {
            return code;
        }

        // Last resort: return raw text (stripped of think blocks, chat tokens, and markdown)
        let text = strip_think_blocks(raw);
        let cleaned = strip_chat_tokens(&text);
        strip_markdown_from_proof(&cleaned)
    }
}

// ── Prompt helpers ──────────────────────────────────────────────────────

/// Extract natural language problem description from `informal_prefix` (/-- ... -/).
fn extract_problem_desc(prefix: &str) -> String {
    if prefix.starts_with("/--") && prefix.ends_with("-/") {
        prefix[3..prefix.len().saturating_sub(2)].trim().to_string()
    } else {
        prefix.trim().to_string()
    }
}

// ── Proof extraction helpers ────────────────────────────────────────────

/// Find the best fenced code block in the text.
/// Returns the block with the most content after stripping the theorem header.
fn extract_fenced_code(text: &str) -> Option<String> {
    let mut best: Option<String> = None;
    let mut best_len = 0;

    for fence_start in [
        "```lean4\n",
        "```lean4",
        "```tactics\n",
        "```tactics",
        "```\n",
        "```",
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

/// Check if code has actual proof content beyond the theorem header.
fn has_proof_body(code: &str) -> bool {
    let stripped = strip_theorem_header(code);
    // Even a single tactic like `rfl` is a valid proof
    stripped.trim().len() >= 2
}

/// Strip markdown commentary lines from extracted proof code.
/// The model sometimes outputs markdown headers/comments mixed with Lean code.
fn strip_markdown_from_proof(code: &str) -> String {
    let lines: Vec<&str> = code
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            // Filter out markdown headers and obvious commentary
            !trimmed.starts_with("# ") && !trimmed.starts_with("## ") && !trimmed.starts_with("**")
        })
        .collect();
    lines.join("\n")
}

/// Try to extract Lean proof code from raw text (no fenced block found).
/// Looks for patterns like `:= by\n  ...` or tactic blocks.
fn extract_lean_from_text(text: &str) -> Option<String> {
    // Look for a theorem-like pattern with tactics after := by
    if let Some(pos) = text.find(":= by") {
        let after = &text[pos + ":=".len() + " by".len()..];
        let tactics: String = after
            .lines()
            .take_while(|line| {
                let trimmed = line.trim();
                !trimmed.is_empty()
                    && (trimmed.starts_with("  ")
                        || trimmed.starts_with('\t')
                        || trimmed.starts_with("·")
                        || trimmed.starts_with('.')
                        || trimmed.starts_with("--"))
            })
            .collect::<Vec<_>>()
            .join("\n");

        if !tactics.trim().is_empty() {
            // Reconstruct: find the full theorem context
            let before = &text[..pos + ":=".len() + " by".len()];
            // Find the start of the theorem (from the last import/empty line before)
            let clean_before = before
                .lines()
                .rev()
                .skip_while(|l| l.trim().is_empty())
                .collect::<Vec<_>>();
            let context_start = clean_before
                .iter()
                .enumerate()
                .find(|(_, l)| l.contains("import ") || l.contains("theorem "))
                .map(|(i, _)| clean_before.len() - i - 1)
                .unwrap_or(0);

            let context: String = clean_before
                .iter()
                .rev()
                .take(clean_before.len() - context_start)
                .rev()
                .copied()
                .collect::<Vec<_>>()
                .join("\n");

            return Some(format!("{context}\n  {tactics}"));
        }
    }
    None
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
        // Qwen3: no pre-populated think block — model generates it naturally
        assert!(!prompt.contains("<think>"));
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
        // Goedel-V2 format includes `sorry` placeholder
        assert!(prompt.contains("sorry"));
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
        // Simple format includes `sorry` placeholder
        assert!(prompt.contains("sorry"));
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
        // DeepSeek V2 format includes `sorry` placeholder
        assert!(prompt.contains("sorry"));
    }

    // ── Proof extraction tests ─────────────────────────────────────────

    #[test]
    fn test_extract_lean4_block_after_think() {
        let cfg = crate::models::find_model("kimina-prover-rl-1.7b").unwrap();
        let pb = PromptBuilder::new(cfg);
        let raw =
            "<think>\nsome reasoning\n</think>\n\n```lean4\nimport Mathlib\ntheorem foo : 1 = 1 := by rfl\n```";
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

    #[test]
    fn test_rejects_header_only_proof() {
        let cfg = crate::models::find_model("kimina-prover-rl-1.7b").unwrap();
        let pb = PromptBuilder::new(cfg);
        // Only header, no actual proof body
        let raw = "<think>\n\n</think>\n\n```lean4\nimport Mathlib\nopen Nat\n\ntheorem foo : 1 = 1 := by\n```\n\n# Some markdown commentary";
        let proof = pb.extract_proof(raw);
        // Should fall back to extracting from raw text (stripping think blocks and chat tokens)
        // The header-only code block is rejected because has_proof_body returns false
        assert!(!proof.contains("# Some markdown commentary"));
    }

    #[test]
    fn test_strips_markdown_headers() {
        let cfg = crate::models::find_model("kimina-prover-rl-1.7b").unwrap();
        let pb = PromptBuilder::new(cfg);
        let raw =
            "```lean4\nimport Mathlib\ntheorem foo : 1 = 1 := by rfl\n# This is markdown\n```";
        let proof = pb.extract_proof(raw);
        assert!(!proof.contains("# This is markdown"));
        assert!(proof.contains("rfl"));
    }
}
