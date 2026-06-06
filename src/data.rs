use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct Theorem {
    pub name: String,
    pub split: String,
    #[serde(default)]
    pub informal_prefix: String,
    pub formal_statement: String,
    #[serde(default)]
    pub header: String,
    #[serde(default)]
    pub goal: String,
}

impl Theorem {
    #[must_use]
    pub fn make_proof_file(&self, proof_body: &str) -> String {
        let mut code = String::new();
        append_section(&mut code, &self.header);
        append_section(&mut code, &self.informal_prefix);
        append_section(&mut code, &self.formal_statement);
        append_section(&mut code, proof_body);
        code
    }
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

/// Load theorems from a JSONL file, optionally filtering by split.
///
/// # Errors
///
/// Returns an error if the file cannot be read or contains invalid JSON.
pub fn load_jsonl(path: &Path, filter_split: Option<&str>) -> Result<Vec<Theorem>> {
    let content = std::fs::read_to_string(path).context("reading JSONL")?;
    let mut theorems = Vec::new();
    for line in content.lines().filter(|l| !l.trim().is_empty()) {
        let t: Theorem = serde_json::from_str(line).context("parsing theorem")?;
        if let Some(split) = filter_split {
            if t.split != split {
                continue;
            }
        }
        theorems.push(t);
    }
    Ok(theorems)
}

/// Load theorems for a specific split ("test" or "valid").
///
/// # Errors
///
/// Returns an error if no dataset file is found or parsing fails.
pub fn load_split(data_dir: &Path, split: &str) -> Result<Vec<Theorem>> {
    let raw = data_dir.join("raw");

    // Try per-split JSONL — only return if file exists
    let split_path = raw.join(format!("{split}.jsonl"));
    if split_path.exists() {
        let t = load_jsonl(&split_path, None)?;
        if !t.is_empty() {
            return Ok(t);
        }
    }

    // Try combined minif2f.jsonl, filtered by split
    let combined = raw.join("minif2f.jsonl");
    if combined.exists() {
        return load_jsonl(&combined, Some(split));
    }

    anyhow::bail!("No dataset found in {}/raw/", data_dir.display())
}

/// Load all theorems from both "test" and "valid" splits.
///
/// # Errors
///
/// Returns an error if neither split has a readable dataset.
pub fn load_all(data_dir: &Path) -> Result<Vec<Theorem>> {
    let mut all = Vec::new();
    let mut errors = Vec::new();
    for split in &["test", "valid"] {
        match load_split(data_dir, split) {
            Ok(t) => all.extend(t),
            Err(e) => errors.push(e),
        }
    }
    if all.is_empty() && !errors.is_empty() {
        anyhow::bail!(
            "Failed to load any theorems from {}: {}",
            data_dir.display(),
            errors
                .iter()
                .map(|e| format!("{e}"))
                .collect::<Vec<_>>()
                .join("; ")
        );
    }
    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_theorem() {
        let t = Theorem {
            name: "test_thm".into(),
            split: "test".into(),
            informal_prefix: String::new(),
            formal_statement: "theorem test_thm (n : Nat) : n = n := by".into(),
            header: "import Mathlib\nopen Nat".into(),
            goal: String::new(),
        };
        assert_eq!(t.name, "test_thm");
        assert_eq!(t.split, "test");
    }

    #[test]
    fn test_make_proof_file() {
        let t = Theorem {
            name: "test".into(),
            split: "test".into(),
            informal_prefix: String::new(),
            formal_statement: "theorem test (n : Nat) : n = n := by".into(),
            header: "import Mathlib\nopen Nat".into(),
            goal: String::new(),
        };
        let code = t.make_proof_file("  rfl");
        assert!(code.contains("import Mathlib"));
        assert!(code.contains("theorem test"));
        assert!(code.contains("rfl"));
        assert!(code.contains(":= by"));
    }

    #[test]
    fn test_make_proof_file_with_prefix() {
        let t = Theorem {
            name: "test".into(),
            split: "test".into(),
            informal_prefix: "/-- This is a test theorem -/".into(),
            formal_statement: "theorem test (n : Nat) : n = n := by".into(),
            header: "import Mathlib".into(),
            goal: String::new(),
        };
        let code = t.make_proof_file("  rfl");
        assert!(code.contains("/-- This is a test theorem -/"));
        assert!(code.contains("theorem test"));
        assert!(code.contains("rfl"));
    }
}
