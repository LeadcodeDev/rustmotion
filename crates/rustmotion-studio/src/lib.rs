mod components;
mod preview;

pub use preview::{run_preview, run_preview_with_error};

use std::path::PathBuf;

use clap::{CommandFactory, Parser};
use rustmotion::error::Result;
use rustmotion::loader::load_scenario;

#[derive(Parser)]
#[command(name = "rustmotion-studio", about = "Rustmotion live preview studio")]
pub struct Cli {
    /// Path to a JSON scenario to open directly in the editor (optional).
    #[arg(short, long)]
    file: Option<PathBuf>,
    /// Workspace directory to scan for scenarios (default: current directory).
    #[arg(short, long)]
    dir: Option<PathBuf>,
}

/// Returns the clap Command for shell completion generation.
pub fn command() -> clap::Command {
    Cli::command()
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let workspace = cli
        .dir
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    match cli.file {
        Some(f) => match load_scenario(&f) {
            Ok(scenario) => {
                preview::run_preview_root(scenario, None, Some(f), workspace, true, true)
            }
            Err(e) => preview::run_preview_root(
                preview::empty_scenario(),
                Some(format!("{}", e)),
                Some(f),
                workspace,
                true,
                true,
            ),
        },
        None => preview::run_preview_root(
            preview::empty_scenario(),
            None,
            None,
            workspace,
            false,
            true,
        ),
    }
}
