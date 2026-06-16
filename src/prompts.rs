use crate::config::ModelConfig;
use crate::data::Theorem;

#[derive(Clone)]
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
            // Official Goedel-V2: user message only, no system prompt.
            // Kimina models: system + user.
            "qwen3" => {
                if self.config.system_prompt.is_empty() {
                    format!(
                        "<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n",
                        user
                    )
                } else {
                    format!(
                        "<|im_start|>system\n{}<|im_end|>\n<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n",
                        self.config.system_prompt, user
                    )
                }
            }

            // ── DeepSeek V2 — Unicode fullwidth ｜ ────────────────────────
            // NOTE: BOS (<｜begin▁of▁sentence｜>) is added automatically by
            // vLLM via tokenizer config (add_bos_token).  Including it
            // here produces a double BOS and a warning at every request.
            "deepseek_v2" => format!(
                "{}<｜User｜>{}<｜Assistant｜>",
                self.config.system_prompt, user
            ),

            // ── DeepSeek Coder / V1 — ### Instruction: / ### Response: ───
            // Legacy DeepSeek Coder chat support: prepopulate response with
            // ```lean4 + header so the model generates proof code inside the block.
            // CRITICAL: strip trailing ``` from the prepopulated content.
            // If the model sees a closed ```lean4 block, it outputs EOS or
            // English prose instead of Lean tactics (72% empty in testing).
            "deepseek_coder" => {
                if self.config.prompt_format == "simple" {
                    let prepop = user.split("```lean4\n").nth(1).unwrap_or("").trim_end();
                    // Strip closing ``` so model continues inside open code block
                    let prepop = prepop.strip_suffix("```").unwrap_or(prepop).trim_end();
                    let instruction = if user.trim_end().ends_with("```") {
                        user.clone()
                    } else {
                        format!("{user}\n```")
                    };
                    format!(
                        "{}### Instruction:\n{}\n### Response:\n```lean4\n{}",
                        self.config.system_prompt, instruction, prepop
                    )
                } else {
                    format!(
                        "{}### Instruction:\n{}\n### Response:\n",
                        self.config.system_prompt, user
                    )
                }
            }

            // ── Raw — no chat template (official DPO/STP completion prompts) ──
            "raw" => user,

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
            "goedel_v2" => Self::build_goedel_v2(theorem, true),
            "goedel_v2_nocot" => Self::build_goedel_v2(theorem, false),
            "simple" => Self::build_simple(theorem),
            "deepseek_prover" => Self::build_deepseek_prover(theorem),
            _ => Self::build_kimina(theorem), // default: kimina format
        }
    }

    /// Kimina format (official): "Think about and solve the following problem
    /// step by step in Lean 4." Model expected to output `<think>...</think>`
    /// followed by a ```lean4 block.
    fn build_kimina(theorem: &Theorem) -> String {
        use std::fmt::Write;

        let formal_block = theorem_block(theorem, true);

        let problem_desc = extract_problem_desc(&theorem.informal_prefix);

        let mut msg =
            "Think about and solve the following problem step by step in Lean 4.".to_string();
        if !problem_desc.is_empty() {
            let _ = write!(msg, "\n# Problem:{problem_desc}");
        }
        let _ = write!(msg, "\n# Formal statement:\n```lean4\n{formal_block}\n```");
        msg
    }

    /// Goedel V2 / DeepSeek Prover V2 format.
    ///
    /// Both modes from the official papers (Goedel-Prover-V2 arXiv 2508.03613,
    /// DeepSeek-Prover-V2 arXiv 2504.21801 Appendix A).
    ///
    /// CoT mode: closed code block + proof plan request.  Model outputs a
    /// natural-language proof plan followed by a new ```lean4 code block.
    ///
    /// Non-CoT mode: closed code block only.  Model outputs a new ```lean4
    /// code block directly (avg 443 tokens for 7B — Table 3 of DS paper).
    fn build_goedel_v2(theorem: &Theorem, cot: bool) -> String {
        let formal_block = theorem_block(theorem, true);
        let formal_with_sorry = format!("{}\n  sorry", formal_block.trim_end());

        let base =
            format!("Complete the following Lean 4 code:\n\n```lean4\n{formal_with_sorry}\n```");

        if cot {
            format!(
                "{base}\nBefore producing the Lean 4 code to formally prove the given theorem, \
                 provide a detailed proof plan outlining the main proof steps and strategies.\n\
                 The plan should highlight key ideas, intermediate lemmas, and proof structures \
                 that will guide the construction of the final formal proof."
            )
        } else {
            base
        }
    }

    /// Goedel-Prover-DPO official format:
    /// Raw completion prompt with an open ```lean4 block.
    /// Model generates proof code inside the block, then may close it with ```.
    fn build_simple(theorem: &Theorem) -> String {
        let formal_block = theorem_block(theorem, true);

        format!(
            "Complete the following Lean 4 code with explanatory comments preceding each line of code:\n\n```lean4\n{formal_block}"
        )
    }

    /// STP model format (official eval script):
    /// Completion (NOT chat). "Complete the following Lean 4 code:" + open ```lean4.
    /// Statement = formal_statement with last "sorry" stripped.
    /// Informal prefix is excluded — STP max_model_len is only 1024.
    /// Model generates Lean tactics from `:= by`.
    fn build_deepseek_prover(theorem: &Theorem) -> String {
        // Strip trailing "sorry" (official: rsplit("sorry", 1)[0].strip()).
        // No-op for minif2f data (no "sorry" in formal_statements).
        let statement = match theorem.formal_statement.rsplit_once("sorry") {
            Some((before, _)) => before,
            None => &theorem.formal_statement,
        };

        let mut formal_block = String::new();
        append_section(&mut formal_block, &theorem.header);
        // NOTE: informal_prefix is intentionally excluded — STP has 1024 ctx
        append_section(&mut formal_block, statement.trim());

        format!("Complete the following Lean 4 code:\n\n```lean4\n{formal_block}")
    }

    /// Extract the proof body from model output.
    ///
    /// Strategy (prioritised):
    /// 1. Find ```lean4 block after </think> — primary format for Kimina
    /// 2. Fallback: any ```lean4 block in raw text
    /// 3. Fallback: extract Lean tactics from raw text (no fenced block)
    /// 4. Last resort: strip think/chat/markdown, validate body exists
    #[must_use]
    pub fn extract_proof(&self, raw: &str) -> String {
        // Find the ```lean4 block AFTER </think> (if present — Kimina models)
        let search_from = raw.find("</think>").map_or(0, |p| p + "</think>".len());
        let after_think = &raw[search_from..];

        // Try to find ```lean4 block after </think>
        if let Some(code) = extract_fenced_code(after_think) {
            let cleaned = strip_markdown_from_proof(&code);
            if has_proof_body(&cleaned) {
                return cleaned;
            }
        }

        // For chat models, the raw text includes the prompt (with its own
        // ```lean4 block containing `sorry`). Search only the model's output —
        // after the last assistant marker. Skip the full-text fallback since
        // it would match the prompt's code block, not the model's.
        let after_assistant = find_model_output_start(raw);
        let is_chat = after_assistant > 0;
        if is_chat {
            if let Some(code) = extract_fenced_code(&raw[after_assistant..]) {
                let cleaned = strip_markdown_from_proof(&code);
                if has_proof_body(&cleaned) {
                    return cleaned;
                }
            }
        } else {
            // Raw architecture — entire text is model output
            if let Some(code) = extract_fenced_code(raw) {
                let cleaned = strip_markdown_from_proof(&code);
                if has_proof_body(&cleaned) {
                    return cleaned;
                }
            }
        }

        // Third fallback: try to extract Lean code from raw text
        // (model might have output the proof without proper fencing)
        if let Some(code) = extract_lean_from_text(raw) {
            let cleaned = strip_chat_tokens(&code);
            if has_proof_body(&cleaned) {
                return cleaned;
            }
        }

        // Last resort: strip think/chat/markdown, validate proof body
        let text = strip_think_blocks(raw);
        let cleaned = strip_chat_tokens(&text);
        let stripped = strip_markdown_from_proof(&cleaned);
        if has_proof_body(&stripped) {
            strip_trailing_fence(&stripped)
        } else {
            String::new()
        }
    }
    /// Validate that assembled lean_code is a complete Lean proof file.
    /// Returns true if the code looks compilable (has tactics, no `sorry`,
    /// no markdown/chat artefacts).
    #[must_use]
    pub fn validate_lean_code(&self, code: &str) -> bool {
        if code.is_empty() {
            return false;
        }
        // Must contain := by
        if !code.contains(":= by") {
            return false;
        }
        // Must NOT contain sorry
        if code.contains("sorry") {
            return false;
        }
        // Must have tactics after := by
        if let Some(pos) = code.find(":= by") {
            let after = &code[pos + ":=".len() + " by".len()..];
            let body = after.trim();
            if body.len() < 2 {
                return false;
            }
            // Reject markdown artefacts in proof body
            if body.contains("```") || body.contains("**") {
                return false;
            }
            // Reject chat tokens
            if body.contains("<|im_start|>")
                || body.contains("<|im_end|>")
                || body.contains("<｜User｜>")
                || body.contains("<｜Assistant｜>")
            {
                return false;
            }
            // Reject natural language commentary (not Lean tactics)
            if !is_proof_body(body) {
                return false;
            }
            // Strip block comments (/- ... -/) — if nothing remains,
            // the model generated only commentary, no actual tactics.
            let without_comments = strip_block_comments(body);
            if without_comments.trim().len() < 2 {
                return false;
            }
        }
        true
    }
}

// ── Prompt helpers ──────────────────────────────────────────────────────

fn theorem_block(theorem: &Theorem, include_informal: bool) -> String {
    let mut out = String::new();
    append_section(&mut out, &theorem.header);
    if include_informal {
        append_section(&mut out, &theorem.informal_prefix);
    }
    append_section(&mut out, &theorem.formal_statement);
    out
}

fn append_section(out: &mut String, section: &str) {
    if section.is_empty() {
        return;
    }
    if !out.is_empty() && !out.ends_with('\n') && !section.starts_with('\n') {
        out.push('\n');
    }
    out.push_str(section);
}

/// Extract natural language problem description from `informal_prefix` (/-- ... -/).
fn extract_problem_desc(prefix: &str) -> String {
    if prefix.starts_with("/--") && prefix.ends_with("-/") {
        prefix[3..prefix.len().saturating_sub(2)].trim().to_string()
    } else {
        prefix.trim().to_string()
    }
}

// ── Proof extraction helpers ────────────────────────────────────────────

/// Find where the model's output begins in the raw text.
/// For chat models, returns the byte position after the last assistant marker.
/// For raw architectures (no chat markers), returns 0 (search entire text).
fn find_model_output_start(raw: &str) -> usize {
    // Qwen3 ChatML
    if let Some(pos) = raw.rfind("<|im_start|>assistant\n") {
        return pos + "<|im_start|>assistant\n".len();
    }
    // DeepSeek V2 (Unicode fullwidth ｜)
    if let Some(pos) = raw.rfind("<｜Assistant｜>") {
        return pos + "<｜Assistant｜>".len();
    }
    // DeepSeek Coder
    if let Some(pos) = raw.rfind("### Response:\n") {
        return pos + "### Response:\n".len();
    }
    // Raw architecture — entire text is model output
    0
}

/// Find the best fenced code block in the text.
/// Returns the block with the most content after stripping the theorem header.
/// Language-agnostic fence openers (` ``` `, ` ```\n`) have their first-line
/// language specifier stripped to avoid including "tactics" etc. as code.
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
        let is_bare_fence = fence_start == "```" || fence_start == "```\n";
        let mut search_from = 0;
        while let Some(pos) = text[search_from..].find(fence_start) {
            let abs_pos = search_from + pos;
            let start = abs_pos + fence_start.len();
            let rest = &text[start..];
            if let Some(end) = rest.find("```") {
                let raw_code = rest[..end].trim().to_string();
                // Bare ``` fences include the language specifier (e.g. "tactics")
                // as the first line. Strip it so it doesn't become "proof" content.
                let code = if is_bare_fence {
                    strip_fence_lang_specifier(&raw_code)
                } else {
                    raw_code
                };
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

/// Strip the language specifier from the first line of a fenced code block.
/// Bare ``` fences (without a language tag) include the tag as the first line.
/// e.g. "tactics\nimport Mathlib..." → "import Mathlib..."
fn strip_fence_lang_specifier(code: &str) -> String {
    if let Some(first_newline) = code.find('\n') {
        let first_line = &code[..first_newline].trim();
        // Language specifiers are single words (no spaces, no Lean syntax)
        if !first_line.is_empty()
            && !first_line.contains(' ')
            && !first_line.contains('/')
            && first_line
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return code[first_newline + 1..].trim().to_string();
        }
    }
    code.to_string()
}

/// Check if the text looks like a proof body (starts with tactic-like content,
/// not another theorem/definition/import statement or natural language commentary).
fn is_proof_body(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    let first_word = trimmed.split_whitespace().next().unwrap_or("");
    // Reject theorem/lemma/import headers
    if matches!(
        first_word,
        "theorem" | "lemma" | "import" | "open" | "set_option" | "noncomputable" | "variable"
    ) {
        return false;
    }
    if trimmed.starts_with("```") || trimmed.starts_with('#') {
        return false;
    }
    // Reject natural language commentary: first word is English prose starter
    // with capital letter, and the line has many words (prose) vs few (tactics).
    if let Some(first_char) = first_word.chars().next() {
        if first_char.is_ascii_uppercase() {
            let word_count = trimmed.split_whitespace().count();
            // Lean comments: "-- ..." or "/- ... -/"
            // Prose commentary: "The product of ..." (many words, natural language)
            if word_count > 4 && !trimmed.starts_with("--") && !trimmed.starts_with("/-") {
                return false;
            }
        }
    }
    true
}

/// Check if code has actual proof content beyond the theorem header.
fn has_proof_body(code: &str) -> bool {
    let stripped = strip_theorem_header(code);
    let trimmed = stripped.trim();
    // Reject empty, fence-only, backtick-only, or markdown artefacts
    if trimmed.len() < 2 || trimmed.starts_with('`') {
        return false;
    }
    // Reject `sorry` — indicates the prompt's code block was extracted
    // instead of the model's output (models should replace `sorry`, not
    // leave it in place).
    if trimmed.contains("sorry") {
        return false;
    }
    // Reject natural language commentary (model explains the proof instead of
    // writing Lean tactics).  Commentary starts with an English sentence
    // (capital letter + many prose words).  Valid Lean tactics are lowercase
    // or symbols.  Lean comments (--, /-) are allowed through.
    if !is_proof_body(trimmed) {
        return false;
    }
    true
}

/// Strip markdown commentary lines from extracted proof code.
fn strip_markdown_from_proof(code: &str) -> String {
    let lines: Vec<&str> = code
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            // Reject all markdown heading levels (#, ##, ###, ...) and bold markers
            !trimmed.starts_with('#') && !trimmed.starts_with("**")
        })
        .collect();
    lines.join("\n")
}

/// Try to extract Lean proof code from raw text (no fenced block found).
/// Looks for `:= by` followed by indented tactic lines.
fn extract_lean_from_text(text: &str) -> Option<String> {
    if let Some(pos) = text.find(":= by") {
        let after = &text[pos + ":=".len() + " by".len()..];
        let lines: Vec<&str> = after.lines().collect();

        // Skip leading blank lines after `:= by`
        let start_idx = lines.iter().position(|l| !l.trim().is_empty())?;

        // Collect tactic lines — stop at blank line or new definition
        let mut tactics: Vec<&str> = Vec::new();
        for &line in &lines[start_idx..] {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                break;
            }
            // Stop at new theorem/lemma/definition boundaries
            if trimmed.starts_with("theorem ")
                || trimmed.starts_with("lemma ")
                || trimmed.starts_with("import ")
            {
                break;
            }
            // Accept indented lines and common continuation patterns.
            // `line.starts_with(' ')` checks the RAW line (before trim) —
            // this preserves indented tactics that `trimmed.starts_with("  ")` would miss.
            if line.starts_with(' ')
                || line.starts_with('\t')
                || trimmed.starts_with("·")
                || trimmed.starts_with('.')
                || trimmed.starts_with("--")
            {
                tactics.push(line);
            } else if tactics.is_empty() {
                // First non-indented line — accept short tactics (e.g. `rfl`, `simp_all`)
                tactics.push(line);
            } else {
                break;
            }
        }

        let tactics_str = tactics.join("\n").trim().to_string();
        if tactics_str.is_empty() {
            return None;
        }

        // Reconstruct context: find the theorem statement leading up to `:= by`
        let before = &text[..pos + ":=".len() + " by".len()];
        let clean_before: Vec<&str> = before
            .lines()
            .rev()
            .skip_while(|l| l.trim().is_empty())
            .collect();
        let context_start = clean_before
            .iter()
            .enumerate()
            .rfind(|(_, l)| l.contains("import ") || l.contains("theorem "))
            .map(|(i, _)| clean_before.len() - i - 1)
            .unwrap_or(0);

        let context = clean_before
            .iter()
            .rev()
            .take(clean_before.len() - context_start)
            .rev()
            .copied()
            .collect::<Vec<_>>()
            .join("\n");

        return Some(format!("{context}\n  {tactics_str}"));
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

/// Strip Lean block comments (/- ... -/) from proof body.
/// Returns the text with all block comments removed.
fn strip_block_comments(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.char_indices().peekable();
    while let Some((_, c)) = chars.next() {
        if c == '/' {
            // Peek ahead for "/-"
            let mut lookahead = chars.clone();
            if let Some((_, '-')) = lookahead.next() {
                // Found "/-", skip until "-/"
                chars = lookahead;
                let mut depth: u32 = 1;
                while let Some((_, ch)) = chars.next() {
                    if ch == '-' {
                        let mut la2 = chars.clone();
                        if let Some((_, '/')) = la2.next() {
                            depth = depth.saturating_sub(1);
                            chars = la2;
                            if depth == 0 {
                                break;
                            }
                        } else {
                            result.push('-');
                            result.push(ch);
                        }
                    } else if ch == '/' {
                        let mut la2 = chars.clone();
                        if let Some((_, '-')) = la2.next() {
                            depth = depth.saturating_add(1);
                            chars = la2;
                        } else {
                            result.push(ch);
                        }
                    } else {
                        // keep scanning inside comment
                    }
                }
            } else {
                result.push(c);
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Strip trailing ``` (code fence closer) from proof body.
/// Open code block formats (Goedel-DPO, STP) let the model generate
/// inside the block; the model may close it with ```.
fn strip_trailing_fence(text: &str) -> String {
    text.trim_end()
        .strip_suffix("```")
        .map(str::trim_end)
        .unwrap_or(text)
        .to_string()
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

/// Strip the theorem header — returns only the proof body.
///
/// Uses `find` (first occurrence) rather than `rfind` so that nested
/// `have ... := by` blocks inside the proof are preserved intact.
fn strip_theorem_header(code: &str) -> String {
    if let Some(pos) = code.find(":= by\n") {
        let after = &code[pos + ":=".len() + " by\n".len()..];
        let trimmed = after.trim();
        if is_proof_body(trimmed) {
            return trimmed.to_string();
        }
    }
    if let Some(pos) = code.find(":= by") {
        let after_pos = pos + ":=".len();
        let rest = code[after_pos..].trim();
        let after_by = rest.strip_prefix("by").map_or("", str::trim);
        if !after_by.is_empty() && is_proof_body(after_by) {
            return after_by.to_string();
        }
    }
    // DeepSeek-Prover-V2 often outputs `:=by` without a space (single-line format).
    // This is common when the model generates code without newlines — the tokenizer
    // merges `:=` and `by` into a continuous string.  Without this fallback the
    // proof body is rejected because `is_proof_body` sees "theorem" at the start.
    if let Some(pos) = code.find(":=by") {
        let after_pos = pos + 4; // skip ":=by" (4 chars)
        let after_by = code[after_pos..].trim();
        if !after_by.is_empty() && is_proof_body(after_by) {
            return after_by.to_string();
        }
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
        // Official: # Problem:{desc} — no space after colon
        assert!(prompt.contains("# Problem:The sum of two even numbers is even"));
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
        assert!(prompt.contains("sorry"));
        // Official: user message only, no system prompt
        assert!(!prompt.contains("<|im_start|>system"));
        // Official: `sorry` and closing ``` on separate lines
        assert!(
            prompt.contains("sorry\n```"),
            "CoT: closing ``` must be on its own line after 'sorry'"
        );
        assert!(
            !prompt.contains(":= by\n\n  sorry"),
            "`sorry` should directly follow `:= by` without an extra blank line"
        );
    }

    #[test]
    fn test_goedel_v2_nocot_format() {
        let cfg = crate::models::find_model("deepseek-prover-v2-7b").unwrap();
        assert_eq!(cfg.prompt_format, "goedel_v2_nocot");
        let pb = PromptBuilder::new(cfg);
        let t = make_theorem();
        let prompt = pb.build(&t);
        assert!(prompt.contains("Complete the following Lean 4 code"));
        // Non-CoT: no proof plan request
        assert!(!prompt.contains("proof plan"));
        assert!(!prompt.contains("Before producing"));
        assert!(prompt.contains("import Mathlib"));
        assert!(prompt.contains("sorry"));
        // No system prompt
        assert!(!prompt.contains("<|im_start|>system"));
        // Non-CoT: closed code block per Appendix A.1 of DS paper
        assert!(
            prompt.contains("sorry\n```"),
            "non-CoT: closing ``` must be on its own line after 'sorry' (Appendix A.1)"
        );
        assert!(
            !prompt.contains("proof plan"),
            "non-CoT: no proof plan request"
        );
    }

    #[test]
    fn test_simple_format() {
        let cfg = crate::models::find_model("goedel-prover-dpo").unwrap();
        let pb = PromptBuilder::new(cfg);
        let t = make_theorem();
        let prompt = pb.build(&t);
        // Official Goedel-DPO eval format: raw completion, open code block
        assert!(!prompt.contains("### Instruction:"));
        assert!(!prompt.contains("### Response:"));
        assert!(prompt.contains("Complete the following Lean 4 code with explanatory comments"));
        assert!(prompt.contains("import Mathlib"));
        assert!(prompt.contains("theorem test_thm"));
        assert!(prompt.contains("```lean4\nimport Mathlib"));
        assert!(
            !prompt.trim_end().ends_with("```"),
            "Goedel-DPO prompt must leave the Lean code block open"
        );
        // No `sorry` in minif2f data
        assert!(!prompt.contains("sorry"));
    }

    #[test]
    fn test_deepseek_v2_format() {
        let cfg = crate::models::find_model("deepseek-prover-v2-7b").unwrap();
        let pb = PromptBuilder::new(cfg);
        let t = make_theorem();
        let prompt = pb.build(&t);
        // BOS (<｜begin▁of▁sentence｜>) is NOT in the template —
        // llama-server adds it automatically via tokenizer config.
        assert!(prompt.contains("<｜User｜>"));
        assert!(prompt.contains("<｜Assistant｜>"));
        assert!(prompt.contains("Complete the following Lean 4 code"));
        assert!(prompt.contains("sorry"));
    }

    #[test]
    fn test_deepseek_prover_format() {
        let cfg = crate::models::find_model("stp-model-lean").unwrap();
        let pb = PromptBuilder::new(cfg);
        let t = make_theorem();
        let prompt = pb.build(&t);
        // STP uses raw architecture — no chat template, no system prompt
        assert!(!prompt.contains("### Instruction:"));
        assert!(!prompt.contains("<|im_start|>"));
        assert!(!prompt.contains("System:"));
        // Should use "Complete the following Lean 4 code" format
        assert!(prompt.contains("Complete the following Lean 4 code"));
        assert!(prompt.contains("import Mathlib"));
        assert!(prompt.contains("theorem test_thm"));
        // STP format: NO `sorry` — model generates from `:= by`
        assert!(!prompt.contains("sorry"));
        // informal_prefix is excluded — STP has only 1024 ctx
        assert!(!prompt.contains("sum of two even"));
        assert!(
            !prompt.trim_end().ends_with("```"),
            "STP prompt must leave the Lean code block open"
        );
    }

    #[test]
    fn test_stp_raw_no_chat_template() {
        let cfg = crate::models::find_model("stp-model-lean").unwrap();
        assert_eq!(cfg.architecture, "raw");
        assert_eq!(cfg.prompt_format, "deepseek_prover");
        assert_eq!(cfg.system_prompt, "");
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
        // Should fall back to raw text (stripped of think blocks, chat tokens, markdown)
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

    #[test]
    fn test_extract_preserves_have_proof_body() {
        // Regression: rfind shredded proof bodies that contained `have ... := by`
        let cfg = crate::models::find_model("kimina-prover-rl-1.7b").unwrap();
        let pb = PromptBuilder::new(cfg);
        let raw = "```lean4\nimport Mathlib\ntheorem foo (a b : Nat) (h : a = b) : a + 1 = b + 1 := by\n  have h₁ : a + 1 = b + 1 := by\n    rw [h]\n  exact h₁\n```";
        let proof = pb.extract_proof(raw);
        assert!(proof.contains("have h₁"));
        assert!(proof.contains("rw [h]"));
        assert!(proof.contains("exact h₁"));
    }

    #[test]
    fn test_strip_theorem_header_preserves_have_block() {
        // find (not rfind) preserves nested `have ... := by`
        let code =
            "import Mathlib\ntheorem foo : bar := by\n  have h₁ : x = y := by\n    rfl\n  rw [h₁]";
        let stripped = strip_theorem_header(code);
        assert!(stripped.contains("have h₁"));
        assert!(stripped.contains("rw [h₁]"));
        assert!(!stripped.contains("theorem foo"));
    }

    #[test]
    fn test_strip_theorem_header_without_space() {
        // DeepSeek often outputs `:=by` without space (one-line format)
        let code = "import Mathlib\ntheorem foo : 1 = 1 :=by rfl";
        let stripped = strip_theorem_header(code);
        assert!(stripped.contains("rfl"));
        assert!(!stripped.contains("theorem foo"));
        assert!(!stripped.contains(":=by"));
    }

    #[test]
    fn test_strip_theorem_header_deepseek_oneline() {
        // Real deepseek output: everything on one line, `:=by` no space
        let code = "import Mathlib\ntheorem foo (a b : Nat) : a + b = b + a :=by rw [add_comm]";
        let stripped = strip_theorem_header(code);
        assert!(stripped.contains("rw [add_comm]"));
        assert!(!stripped.contains("theorem foo"));
    }

    #[test]
    fn test_strip_theorem_header_deepseek_oneline_have() {
        // DeepSeek one-line with nested `have ... :=by` — must preserve inner `have`
        let code = "import Mathlib\ntheorem foo (h : a = b) : a + 1 = b + 1 :=by have h1 : a + 1 = b + 1 :=by rw [h]; exact h1";
        let stripped = strip_theorem_header(code);
        assert!(stripped.contains("have h1"));
        assert!(stripped.contains("rw [h]"));
        assert!(stripped.contains("exact h1"));
        assert!(!stripped.contains("theorem foo"));
    }

    #[test]
    fn test_accepts_short_tactic_rw() {
        // Short tactics like `rw` (2 chars) are valid proof bodies
        let cfg = crate::models::find_model("kimina-prover-rl-1.7b").unwrap();
        let pb = PromptBuilder::new(cfg);
        let raw = "```lean4\nimport Mathlib\ntheorem foo : 1 = 1 := by rw\n```";
        let proof = pb.extract_proof(raw);
        assert!(proof.contains("rw"));
        assert!(!proof.is_empty());
    }

    #[test]
    fn test_has_proof_body_rejects_markdown() {
        // Code that's just a markdown fence should be rejected
        assert!(!has_proof_body("```"));
        assert!(!has_proof_body("``"));
        assert!(!has_proof_body("\n\n"));
        // But actual proof content should pass
        assert!(has_proof_body("theorem foo : 1 = 1 := by\n  rfl"));
        assert!(has_proof_body("rw [h]"));
    }

    #[test]
    fn test_strip_block_comments() {
        assert_eq!(strip_block_comments("  rfl"), "  rfl");
        assert_eq!(strip_block_comments("/- comment -/"), "");
        assert_eq!(strip_block_comments("/- comment -/ rfl"), " rfl");
        // Nested comments
        assert_eq!(
            strip_block_comments("/- outer /- inner -/ still outer -/ after"),
            " after"
        );
    }

    #[test]
    fn test_validate_lean_code_rejects_commentary_only() {
        let cfg = crate::models::find_model("goedel-prover-dpo").unwrap();
        let pb = PromptBuilder::new(cfg);
        // Just a comment, no tactics
        assert!(!pb.validate_lean_code(
            "import Mathlib\ntheorem foo : 1 = 1 := by\n  /- a comment explaining the proof -/"
        ));
        // Comment + actual tactic
        assert!(pb.validate_lean_code(
            "import Mathlib\ntheorem foo : 1 = 1 := by\n  /- trivial -/\n  rfl"
        ));
        // Only tactics
        assert!(pb.validate_lean_code("import Mathlib\ntheorem foo : 1 = 1 := by\n  rfl"));
        // Has sorry — reject
        assert!(!pb.validate_lean_code("import Mathlib\ntheorem foo : 1 = 1 := by\n  sorry"));
    }

    #[test]
    fn test_is_proof_body_rejects_commentary() {
        // Natural language commentary that the model might generate
        assert!(!is_proof_body(
            "The product of the first seven odd numbers modulo 10 equals 5"
        ));
        assert!(!is_proof_body(
            "This theorem can be proved by induction on n"
        ));
        assert!(!is_proof_body(
            "We will use the triangle inequality to bound the sum"
        ));
        // Short uppercase text (might be a variable name or short statement)
        assert!(is_proof_body("Nat.add_comm a b"));
        assert!(is_proof_body("S : Type _"));
        // Lean tactics (lowercase, symbols) always pass
        assert!(is_proof_body("  rfl"));
        assert!(is_proof_body("  nlinarith"));
        assert!(is_proof_body("  rw [h]"));
        // Comments pass
        assert!(is_proof_body(
            "-- This is a long explanatory comment about the proof"
        ));
        assert!(is_proof_body("/- multi-line\n   comment -/"));
    }

    #[test]
    fn test_is_proof_body_detection() {
        assert!(is_proof_body("  rfl"));
        assert!(is_proof_body("  simp_all"));
        assert!(is_proof_body("have h : x = y := by"));
        assert!(is_proof_body("rw [h]"));
        assert!(!is_proof_body("theorem foo : bar := by"));
        assert!(!is_proof_body("lemma baz : qux := by"));
        assert!(!is_proof_body("import Mathlib"));
        assert!(!is_proof_body("```"));
        // Reject markdown headers (any level)
        assert!(!is_proof_body("# Analysis"));
        assert!(!is_proof_body("## Step 1"));
        assert!(!is_proof_body("### Detailed Proof and Analysis"));
        assert!(!is_proof_body("#### Sub-step"));
        // /-- doc comments are valid Lean, but the strip_theorem_header
        // should have already removed them from the proof body
        assert!(is_proof_body("/-- a comment about the proof -/"));
        assert!(is_proof_body("-- a line comment"));
    }

    #[test]
    fn test_strip_fence_lang_specifier() {
        // Language specifiers are stripped
        assert_eq!(
            strip_fence_lang_specifier("tactics\nimport Mathlib\ntheorem foo := by\n  rfl"),
            "import Mathlib\ntheorem foo := by\n  rfl"
        );
        assert_eq!(
            strip_fence_lang_specifier("lean4\nimport Mathlib\n  rfl"),
            "import Mathlib\n  rfl"
        );
        // Non-language first lines are left intact
        assert_eq!(
            strip_fence_lang_specifier("import Mathlib\ntheorem foo := by\n  rfl"),
            "import Mathlib\ntheorem foo := by\n  rfl"
        );
        // Single word with spaces is NOT a language specifier
        assert_eq!(
            strip_fence_lang_specifier("have h : x = y := by\n  rfl"),
            "have h : x = y := by\n  rfl"
        );
    }

    #[test]
    fn test_strip_markdown_rejects_all_heading_levels() {
        // #, ##, ###, #### etc. all filtered
        let code = "# H1\n## H2\n### H3\n  rfl\n**bold**";
        let result = strip_markdown_from_proof(code);
        assert!(!result.contains('#'));
        assert!(!result.contains("**"));
        assert!(result.contains("rfl"));
    }

    #[test]
    fn test_extract_proof_rejects_markdown_only_output() {
        // deepseek style: outputs only "### Detailed Proof" with no Lean code
        let cfg = crate::models::find_model("deepseek-prover-v2-7b").unwrap();
        let pb = PromptBuilder::new(cfg);
        let raw = "<｜User｜>Complete the following Lean 4 code:\n\n```lean4\nimport Mathlib\ntheorem foo : 1 = 1 := by\n  sorry\n```<｜Assistant｜>\n\n### Detailed Proof and Analysis\n\nWe can prove this by...";
        let proof = pb.extract_proof(raw);
        // The English-only "proof" should be rejected
        assert!(
            proof.is_empty(),
            "English-only output must be rejected, got: {proof:?}"
        );
    }

    #[test]
    fn test_lean_fenced_block_with_tactics() {
        let cfg = crate::models::find_model("kimina-prover-rl-1.7b").unwrap();
        let pb = PromptBuilder::new(cfg);
        let raw = "```lean4\nimport Mathlib\ntheorem foo : 1 = 1 := by\n  simp\n```";
        let proof = pb.extract_proof(raw);
        assert!(proof.contains("import Mathlib"));
        assert!(proof.contains("simp"));
        assert!(!proof.contains("```"));
    }
}
