use anyhow::Result;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub struct CheckpointManager {
    file: PathBuf,
    completed: HashSet<String>,
    pub initial_skipped: usize,
}

impl CheckpointManager {
    /// Create a new checkpoint manager, loading existing state if available.
    ///
    /// # Errors
    ///
    /// Returns an error if the checkpoint directory cannot be created or the
    /// checkpoint file cannot be read.
    pub fn new(checkpoint_dir: &Path, model_name: &str, run_id: &str) -> Result<Self> {
        std::fs::create_dir_all(checkpoint_dir)?;
        let safe = model_name.replace(['/', ' '], "_");
        let file = checkpoint_dir.join(format!("{safe}__{run_id}.json"));
        let completed: HashSet<String> = if file.exists() {
            serde_json::from_str(&std::fs::read_to_string(&file)?).unwrap_or_default()
        } else {
            HashSet::new()
        };
        let initial_skipped = completed.len();
        Ok(Self {
            file,
            completed,
            initial_skipped,
        })
    }

    #[must_use]
    pub fn is_done(&self, name: &str) -> bool {
        self.completed.contains(name)
    }

    /// Mark a theorem as completed and persist the checkpoint.
    ///
    /// # Errors
    ///
    /// Returns an error if the checkpoint cannot be serialized or written to disk.
    pub fn mark_done(&mut self, name: &str) -> Result<()> {
        self.completed.insert(name.to_string());
        let tmp = self.file.with_extension("tmp");
        std::fs::write(&tmp, &serde_json::to_string(&self.completed)?)?;
        std::fs::rename(&tmp, &self.file)?;
        Ok(())
    }

    #[must_use]
    pub fn total_done(&self) -> usize {
        self.completed.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("minif2f-test-{}", uuid()));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    fn uuid() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        format!("{nanos:x}")
    }

    fn cleanup(dir: &Path) {
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn test_new_empty_checkpoint() {
        let dir = temp_dir();
        let ck = CheckpointManager::new(&dir, "test-model", "v1").unwrap();
        assert_eq!(ck.total_done(), 0);
        assert_eq!(ck.initial_skipped, 0);
        assert!(!ck.is_done("theorem_1"));
        cleanup(&dir);
    }

    #[test]
    fn test_mark_done_and_check() {
        let dir = temp_dir();
        let mut ck = CheckpointManager::new(&dir, "test-model", "v1").unwrap();
        ck.mark_done("theorem_1").unwrap();
        assert!(ck.is_done("theorem_1"));
        assert!(!ck.is_done("theorem_2"));
        assert_eq!(ck.total_done(), 1);
        cleanup(&dir);
    }

    #[test]
    fn test_mark_multiple() {
        let dir = temp_dir();
        let mut ck = CheckpointManager::new(&dir, "test-model", "v1").unwrap();
        ck.mark_done("a").unwrap();
        ck.mark_done("b").unwrap();
        ck.mark_done("c").unwrap();
        assert_eq!(ck.total_done(), 3);
        assert!(ck.is_done("a"));
        assert!(ck.is_done("b"));
        assert!(ck.is_done("c"));
        cleanup(&dir);
    }

    #[test]
    fn test_resume_from_checkpoint() {
        let dir = temp_dir();
        // First session: complete 2 theorems
        {
            let mut ck = CheckpointManager::new(&dir, "test-model", "v1").unwrap();
            ck.mark_done("thm_a").unwrap();
            ck.mark_done("thm_b").unwrap();
            assert_eq!(ck.initial_skipped, 0);
        }
        // Second session: resume
        {
            let ck = CheckpointManager::new(&dir, "test-model", "v1").unwrap();
            assert_eq!(ck.total_done(), 2);
            assert_eq!(ck.initial_skipped, 2);
            assert!(ck.is_done("thm_a"));
            assert!(ck.is_done("thm_b"));
            assert!(!ck.is_done("thm_c"));
        }
        cleanup(&dir);
    }
}
