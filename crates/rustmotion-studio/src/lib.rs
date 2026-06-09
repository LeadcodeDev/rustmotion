mod preview;

pub use preview::{run_bench, run_preview, run_preview_with_error};

use std::path::PathBuf;

use clap::{CommandFactory, Parser};
use rustmotion::error::Result;
use rustmotion::loader::load_scenario;

#[derive(Parser)]
#[command(name = "rustmotion-studio", about = "Rustmotion live preview studio")]
pub struct Cli {
    /// Path to the JSON scenario file
    #[arg(short, long)]
    file: PathBuf,
    /// Run a headless render/encode benchmark and exit (no window).
    #[arg(long, default_value_t = false)]
    bench: bool,
}

/// Returns the clap Command for shell completion generation.
pub fn command() -> clap::Command {
    Cli::command()
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    match load_scenario(&cli.file) {
        Ok(scenario) => {
            if cli.bench {
                run_bench(scenario);
                Ok(())
            } else {
                run_preview(scenario, Some(cli.file), true)
            }
        }
        Err(e) => run_preview_with_error(format!("{}", e), Some(cli.file), true),
    }
}
