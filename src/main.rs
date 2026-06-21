use clap::{Parser, Subcommand};
use minif2f_lib::config::PipelineConfig;
use minif2f_lib::models::{find_model, list_model_names};
use minif2f_lib::pipeline::EvaluationPipeline;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "minif2f",
    about = "Generate theorem proofs using LLMs for miniF2F"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List available models
    ListModels,
    /// Generate proofs for a model (all loaded miniF2F theorems)
    Generate {
        /// Model name
        #[arg(short, long)]
        model: String,
        /// Path to model directory (HuggingFace safetensors)
        #[arg(short = 'p', long)]
        model_path: String,
        /// Run ID for checkpointing
        #[arg(long, default_value = "default")]
        run_id: String,
        /// Port for vLLM server (use different ports for parallel runs)
        #[arg(long, default_value = "8080")]
        port: u16,
        /// Number of attempts per theorem [default: 128]
        #[arg(short = 'n', long, default_value = "128")]
        attempts: usize,
        /// vLLM max concurrent sequences [default: 8]
        #[arg(long, default_value = "8")]
        parallel: u32,
    },
    /// Generate report from existing results (not yet implemented)
    Report {
        /// Model name
        #[arg(short, long)]
        model: String,
        /// Run ID
        #[arg(long, default_value = "default")]
        run_id: String,
    },
    /// Show checkpoint progress
    Status {
        /// Run ID
        #[arg(long, default_value = "default")]
        run_id: String,
    },
    /// Re-extract lean_code from existing raw_output (no GPU / no inference)
    ReExtract {
        /// Model name
        #[arg(short, long)]
        model: String,
        /// Directory holding raw_output/<model>.json
        #[arg(long, default_value = "output/raw_output")]
        raw_dir: String,
        /// Directory to write lean_code/<model>.json
        #[arg(long, default_value = "output/lean_code")]
        lean_dir: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::ListModels => {
            println!("Available models:");
            for name in list_model_names() {
                println!("  {name}");
            }
        }
        Commands::Generate {
            model,
            model_path,
            run_id,
            port,
            attempts,
            parallel,
        } => {
            let model_cfg = find_model(&model).ok_or_else(|| {
                anyhow::anyhow!("Model '{model}' not found. Use 'list-models' to see available.")
            })?;

            let config = PipelineConfig {
                project_root: PathBuf::from("."),
                port,
                completion_attempts: attempts,
                parallel,
                ..PipelineConfig::default()
            };

            println!("=== minif2f Proof Generation ===");
            println!("Model: {} ({})", model_cfg.name, model_cfg.hf_repo);
            println!("Model path: {model_path}");
            println!("Architecture: {}", model_cfg.architecture);
            println!("Port: {port}");

            let pipeline = EvaluationPipeline::new(config, &run_id);
            pipeline.run(&model_cfg, &model_path).await?;

            println!("\n╔══════════════════════════════════╗");
            println!("║  Generation complete!            ║");
            println!("╚══════════════════════════════════╝");
        }
        Commands::Report { .. } => {
            println!("Report generation is not available. Check output/*.json for results.");
        }
        Commands::Status { run_id } => {
            println!("Checkpoint status for run '{run_id}':");
            for model_name in list_model_names() {
                let ck = minif2f_lib::checkpoint::CheckpointManager::new(
                    &PathBuf::from("results/checkpoints"),
                    &model_name,
                    &run_id,
                );
                match ck {
                    Ok(c) => println!(
                        "  {model_name}: {} done ({})",
                        c.total_done(),
                        c.initial_skipped
                    ),
                    Err(_) => println!("  {model_name}: no checkpoint"),
                }
            }
        }
        Commands::ReExtract {
            model,
            raw_dir,
            lean_dir,
        } => {
            let model_cfg = find_model(&model).ok_or_else(|| {
                anyhow::anyhow!("Model '{model}' not found. Use 'list-models' to see available.")
            })?;

            println!("=== Re-extracting lean_code (no GPU) ===");
            println!("Model: {} ({})", model_cfg.name, model_cfg.architecture);
            println!("Raw input:  {raw_dir}/{model}.json");
            println!("Lean output: {lean_dir}/{model}.json");

            let (total, recovered) = minif2f_lib::pipeline::re_extract_model(
                &model_cfg,
                &PathBuf::from(&raw_dir),
                &PathBuf::from(&lean_dir),
                &PathBuf::from("data"),
            )?;

            let rate = if total > 0 {
                recovered as f64 / total as f64 * 100.0
            } else {
                0.0
            };
            println!("\n✅ Re-extracted {total} attempts");
            println!("   Non-empty lean_code: {recovered} ({rate:.1}%)");
        }
    }

    Ok(())
}
