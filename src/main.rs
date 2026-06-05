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
    /// Generate proofs for a model (all 488 theorems)
    Generate {
        /// Model name
        #[arg(short, long)]
        model: String,
        /// Path to GGUF model file
        #[arg(short = 'p', long)]
        model_path: String,
        /// Run ID for checkpointing
        #[arg(long, default_value = "default")]
        run_id: String,
        /// Port for llama-server (use different ports for parallel runs)
        #[arg(long, default_value = "8080")]
        port: u16,
        /// Number of attempts per theorem [default: 128]
        #[arg(short = 'n', long, default_value = "128")]
        attempts: usize,
        /// llama-server parallel slots [default: 8]
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
    }

    Ok(())
}
