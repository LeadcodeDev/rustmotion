mod app;
pub mod editor;
mod library;
pub mod scenario;
mod theme;

pub use app::{run_preview, run_preview_with_error};

use std::path::PathBuf;

use clap::Parser;
use rustmotion::error::Result;
use rustmotion::loader::load_scenario;

#[derive(Parser)]
#[command(name = "rustmotion-studio", about = "Rustmotion live preview studio")]
pub struct Cli {
    #[arg(
        short,
        long,
        help = "Path to a JSON scenario to open directly in the editor (optional)."
    )]
    file: Option<PathBuf>,
    #[arg(
        short,
        long,
        help = "Workspace directory to scan for scenarios (default: current directory)."
    )]
    dir: Option<PathBuf>,
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    run_with(cli.file, cli.dir)
}

pub fn run_with(file: Option<PathBuf>, dir: Option<PathBuf>) -> Result<()> {
    let workspace =
        dir.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    match file {
        Some(f) => match load_scenario(&f) {
            Ok(scenario) => app::run_preview_root(scenario, None, Some(f), workspace, true, true),
            Err(e) => app::run_preview_root(
                scenario::empty_scenario(),
                Some(format!("{}", e)),
                Some(f),
                workspace,
                true,
                true,
            ),
        },
        None => app::run_preview_root(
            scenario::empty_scenario(),
            None,
            None,
            workspace,
            false,
            true,
        ),
    }
}
