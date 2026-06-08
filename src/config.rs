use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub name: String,
    pub hf_repo: String,
    /// Prompt wrapper: `"qwen3"` | `"deepseek_v2"` | `"deepseek_coder"` | `"raw"`
    pub architecture: String,
    /// `"kimina"` | `"goedel_v2"` | `"simple"` | `"deepseek_prover"`
    pub prompt_format: String,
    pub param_count_b: Option<f64>,
    pub quantization: Option<String>,
    pub max_model_len: u32,
    pub temperature: f64,
    pub top_p: f64,
    pub max_tokens: u32,
    pub seed: u64,
    pub stop_sequences: Vec<String>,
    pub system_prompt: String,
}

#[derive(Debug, Clone)]
pub struct PipelineConfig {
    pub project_root: PathBuf,
    /// Path to the uv project directory containing vLLM dependencies
    pub uv_project_dir: String,
    pub port: u16,
    pub completion_attempts: usize,
    pub parallel: u32,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            project_root: PathBuf::from("."),
            uv_project_dir: "tools/vllm".into(),
            port: 8080,
            completion_attempts: 128,
            parallel: 8,
        }
    }
}

impl PipelineConfig {
    #[must_use]
    pub fn data_path(&self) -> PathBuf {
        self.project_root.join("data")
    }

    #[must_use]
    pub fn output_dir(&self) -> PathBuf {
        self.project_root.join("output")
    }

    #[must_use]
    pub fn checkpoint_dir(&self) -> PathBuf {
        self.project_root.join("results").join("checkpoints")
    }
}
