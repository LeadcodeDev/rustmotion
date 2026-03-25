mod commands;
mod encode;
mod engine;
mod error;
mod include;
mod preview;
mod schema;
mod skills;
mod tui;
mod variables;

// v2 architecture (M1: Foundation)
#[macro_use]
mod macros;
mod components;
mod layout;
mod traits;
#[cfg(test)]
mod tests;

use crate::error::Result;
use clap::{CommandFactory, Parser, Subcommand};
use error::RustmotionError;
use schema::{ResolvedScenario, Scenario};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "rustmotion", version, about = "Render motion design videos from JSON scenarios")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Suppress all output except errors
    #[arg(short, long, global = true)]
    quiet: bool,

    /// Number of parallel rendering threads (defaults to all cores)
    #[arg(long, global = true)]
    threads: Option<usize>,
}

#[derive(Subcommand)]
enum Commands {
    /// Open a live preview window with hot-reload
    Studio {
        /// Path to the JSON scenario file
        #[arg(short, long)]
        file: PathBuf,
    },

    /// Render a JSON scenario to video or a single frame
    Render {
        /// Path to the JSON scenario file
        #[arg(short, long)]
        file: Option<PathBuf>,

        /// Inline JSON scenario string
        #[arg(long)]
        json: Option<String>,

        /// Output file path
        #[arg(short, long, default_value = "output.mp4")]
        output: PathBuf,

        /// Render a single frame instead of full video (0-indexed)
        #[arg(long)]
        frame: Option<u32>,

        /// Output format for machine consumption
        #[arg(long, value_enum)]
        output_format: Option<OutputFormat>,

        /// Video codec (h264, h265, vp9, prores)
        #[arg(long)]
        codec: Option<String>,

        /// Constant Rate Factor (0-51, lower = better quality)
        #[arg(long)]
        crf: Option<u8>,

        /// Output file format (mp4, webm, mov, gif, png-seq)
        #[arg(long)]
        format: Option<String>,

        /// Enable transparent background (for PNG sequence, WebM, ProRes 4444)
        #[arg(long)]
        transparent: bool,

        /// Watch the input file for changes and re-render automatically
        #[arg(short, long)]
        watch: bool,
    },

    /// Export a single frame as a still image (PNG, JPEG, WebP)
    Still {
        /// Path to the JSON scenario file
        #[arg(short, long)]
        file: PathBuf,

        /// Output file path
        #[arg(short, long, default_value = "still.png")]
        output: PathBuf,

        /// Time in seconds to capture
        #[arg(long, default_value = "0.0")]
        time: f64,

        /// Image format (png, jpeg, webp)
        #[arg(long)]
        format: Option<String>,

        /// JPEG quality (1-100)
        #[arg(long, default_value = "90")]
        quality: u8,
    },

    /// Validate a JSON scenario without rendering
    Validate {
        /// Path to the JSON scenario file
        #[arg(short, long)]
        file: PathBuf,
    },

    /// Print the JSON Schema for scenario files
    Schema {
        /// Output file path (prints to stdout if omitted)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Show information about a scenario
    Info {
        /// Path to the JSON scenario file
        #[arg(short, long)]
        file: PathBuf,
    },

    /// Manage Claude Code skills for rustmotion
    Skills {
        #[command(subcommand)]
        action: SkillsAction,
    },

    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
}

#[derive(Subcommand)]
enum SkillsAction {
    /// Install skills to current project or globally
    Install {
        /// Install to ~/.claude/skills/ instead of ./.claude/skills/
        #[arg(long)]
        global: bool,
    },
    /// Remove all rustmotion skills from current project or globally
    Uninstall {
        /// Remove from ~/.claude/skills/ instead of ./.claude/skills/
        #[arg(long)]
        global: bool,
    },
    /// List all available skills and rules
    List,
    /// Show the content of a specific rule
    Show {
        /// Rule name (e.g. "hex-colors", "paint-context")
        name: String,
    },
}

#[derive(Clone, clap::ValueEnum)]
pub(crate) enum OutputFormat {
    Json,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Configure rayon thread pool
    if let Some(threads) = cli.threads {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build_global()
            .ok(); // Ignore error if already initialized
    }

    match cli.command {
        Commands::Studio { file } => {
            match load_scenario(&file) {
                Ok(scenario) => preview::run_preview(scenario, Some(file), true),
                Err(e) => preview::run_preview_with_error(format!("{}", e), Some(file), true),
            }
        }
        Commands::Render {
            file, json, output, frame, output_format,
            codec, crf, format, transparent, watch,
        } => {
            if watch {
                let input_path = file.ok_or(RustmotionError::WatchRequiresFile)?;
                commands::cmd_watch(&input_path, &output, frame, output_format.as_ref(), cli.quiet, codec, crf, format, transparent)
            } else {
                let scenario = load_scenario_from_source(file.as_ref(), json.as_deref())?;
                commands::cmd_render(scenario, &output, frame, output_format.as_ref(), cli.quiet, codec, crf, format, transparent)
            }
        }
        Commands::Still { file, output, time, format, quality } => {
            let scenario = load_scenario(&file)?;
            commands::cmd_still(scenario, &output, time, format, quality)
        }
        Commands::Validate { file } => commands::cmd_validate(&file),
        Commands::Schema { output } => commands::cmd_schema(output.as_deref()),
        Commands::Info { file } => commands::cmd_info(&file),
        Commands::Skills { action } => match action {
            SkillsAction::Install { global } => skills::install(global),
            SkillsAction::Uninstall { global } => skills::uninstall(global),
            SkillsAction::List => { skills::list(); Ok(()) }
            SkillsAction::Show { name } => skills::show(&name),
        },
        Commands::Completions { shell } => {
            clap_complete::generate(
                shell,
                &mut Cli::command(),
                "rustmotion",
                &mut std::io::stdout(),
            );
            Ok(())
        }
    }
}

pub(crate) fn load_scenario(input: &PathBuf) -> Result<ResolvedScenario> {
    let json_str = std::fs::read_to_string(input)
        .map_err(|e| RustmotionError::FileRead { path: input.display().to_string(), source: e })?;
    let mut json_value: serde_json::Value =
        serde_json::from_str(&json_str).map_err(RustmotionError::from)?;

    // Apply variable defaults for standalone rendering
    variables::apply_defaults(&mut json_value)?;

    let scenario: Scenario =
        serde_json::from_value(json_value).map_err(RustmotionError::from)?;
    include::resolve_includes(scenario, &include::IncludeSource::File(input.clone()))
}

fn load_scenario_from_source(
    input: Option<&PathBuf>,
    json: Option<&str>,
) -> Result<ResolvedScenario> {
    match (input, json) {
        (Some(_), Some(_)) => {
            Err(RustmotionError::ConflictingInput.into())
        }
        (Some(path), None) => load_scenario(path),
        (None, Some(json_str)) => {
            let mut json_value: serde_json::Value =
                serde_json::from_str(json_str).map_err(RustmotionError::from)?;
            variables::apply_defaults(&mut json_value)?;
            let scenario: Scenario =
                serde_json::from_value(json_value).map_err(RustmotionError::from)?;
            include::resolve_includes(scenario, &include::IncludeSource::Inline)
        }
        (None, None) => {
            Err(RustmotionError::MissingInput.into())
        }
    }
}
