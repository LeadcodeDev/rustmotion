mod commands;
mod skills;
pub mod tui;

use clap::{CommandFactory, Parser, Subcommand};
use rustmotion::error::{Result, RustmotionError};
use rustmotion::loader::{load_scenario, load_scenario_from_source};
use std::io::Write;
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

        /// Write a machine-readable JSON report of all violations to this path
        #[arg(long)]
        report: Option<PathBuf>,

        /// Auto-fix safe violations in place (clamp positions, set wrap=true,
        /// enable auto_scroll). The original file is rewritten.
        #[arg(long)]
        fix: bool,

        /// Sample animated frames and reapply renderer transforms to detect
        /// per-frame viewport overflow (slower).
        #[arg(long)]
        strict_anim: bool,

        /// Treat geometry violations as warnings instead of errors.
        #[arg(long)]
        lenient: bool,
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

    /// Generate or install shell completions
    Completions {
        #[command(subcommand)]
        action: CompletionsAction,
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

#[derive(Subcommand)]
enum CompletionsAction {
    /// Install completions to the current shell config
    Install,
    /// Remove installed completions
    Uninstall,
    /// Print completions to stdout
    Generate {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
}

#[derive(Clone, clap::ValueEnum)]
pub(crate) enum OutputFormat {
    Json,
}

pub fn run() -> Result<()> {
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
                Ok(scenario) => rustmotion_studio::run_preview(scenario, Some(file), true),
                Err(e) => rustmotion_studio::run_preview_with_error(format!("{}", e), Some(file), true),
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
        Commands::Validate { file, report, fix, strict_anim, lenient } => {
            commands::cmd_validate(&file, report.as_deref(), fix, strict_anim, lenient)
        }
        Commands::Schema { output } => commands::cmd_schema(output.as_deref()),
        Commands::Info { file } => commands::cmd_info(&file),
        Commands::Skills { action } => match action {
            SkillsAction::Install { global } => skills::install(global),
            SkillsAction::Uninstall { global } => skills::uninstall(global),
            SkillsAction::List => { skills::list(); Ok(()) }
            SkillsAction::Show { name } => skills::show(&name),
        },
        Commands::Completions { action } => match action {
            CompletionsAction::Generate { shell } => {
                clap_complete::generate(
                    shell,
                    &mut Cli::command(),
                    "rustmotion",
                    &mut std::io::stdout(),
                );
                clap_complete::generate(
                    shell,
                    &mut rustmotion_studio::command(),
                    "rustmotion-studio",
                    &mut std::io::stdout(),
                );
                Ok(())
            }
            CompletionsAction::Install => install_completions(),
            CompletionsAction::Uninstall => uninstall_completions(),
        }
    }
}

fn detect_shell() -> Option<clap_complete::Shell> {
    let shell = std::env::var("SHELL").ok()?;
    if shell.ends_with("/zsh") {
        Some(clap_complete::Shell::Zsh)
    } else if shell.ends_with("/bash") {
        Some(clap_complete::Shell::Bash)
    } else if shell.ends_with("/fish") {
        Some(clap_complete::Shell::Fish)
    } else {
        None
    }
}

fn install_completions() -> Result<()> {
    let shell = detect_shell().ok_or_else(|| {
        RustmotionError::Generic("Could not detect shell. Use 'completions generate <shell>' instead.".into())
    })?;

    let mut buf = Vec::new();
    clap_complete::generate(shell, &mut Cli::command(), "rustmotion", &mut buf);
    clap_complete::generate(shell, &mut rustmotion_studio::command(), "rustmotion-studio", &mut buf);
    let completions = String::from_utf8(buf).unwrap();

    let home = dirs::home_dir().ok_or_else(|| RustmotionError::Generic("Could not find home directory".into()))?;

    match shell {
        clap_complete::Shell::Zsh => {
            // Write to ~/.zfunc/_rustmotion
            let dir = home.join(".zfunc");
            std::fs::create_dir_all(&dir)?;
            let path = dir.join("_rustmotion");
            std::fs::write(&path, &completions)?;

            // Check if fpath is already configured in .zshrc
            let zshrc = home.join(".zshrc");
            let zshrc_content = std::fs::read_to_string(&zshrc).unwrap_or_default();
            if !zshrc_content.contains(".zfunc") {
                let mut f = std::fs::OpenOptions::new().append(true).create(true).open(&zshrc)?;
                writeln!(f, "\n# rustmotion completions")?;
                writeln!(f, "fpath=(~/.zfunc $fpath)")?;
                writeln!(f, "autoload -Uz compinit && compinit")?;
                eprintln!("Added fpath to ~/.zshrc");
            }
            eprintln!("Installed completions to {}", path.display());
            eprintln!("Restart your shell or run: source ~/.zshrc");
        }
        clap_complete::Shell::Bash => {
            let dir = home.join(".local/share/bash-completion/completions");
            std::fs::create_dir_all(&dir)?;
            let path = dir.join("rustmotion");
            std::fs::write(&path, &completions)?;
            eprintln!("Installed completions to {}", path.display());
            eprintln!("Restart your shell or run: source {}", path.display());
        }
        clap_complete::Shell::Fish => {
            let dir = home.join(".config/fish/completions");
            std::fs::create_dir_all(&dir)?;
            let path = dir.join("rustmotion.fish");
            std::fs::write(&path, &completions)?;
            eprintln!("Installed completions to {}", path.display());
        }
        _ => {
            return Err(RustmotionError::Generic(format!("Unsupported shell for install: {:?}. Use 'completions generate' instead.", shell)).into());
        }
    }

    Ok(())
}

fn uninstall_completions() -> Result<()> {
    let shell = detect_shell().ok_or_else(|| {
        RustmotionError::Generic("Could not detect shell.".into())
    })?;

    let home = dirs::home_dir().ok_or_else(|| RustmotionError::Generic("Could not find home directory".into()))?;

    match shell {
        clap_complete::Shell::Zsh => {
            let path = home.join(".zfunc/_rustmotion");
            if path.exists() {
                std::fs::remove_file(&path)?;
                eprintln!("Removed {}", path.display());
            }

            // Remove fpath lines from .zshrc
            let zshrc = home.join(".zshrc");
            if zshrc.exists() {
                let content = std::fs::read_to_string(&zshrc)?;
                let filtered: Vec<&str> = content.lines().filter(|line| {
                    let trimmed = line.trim();
                    trimmed != "# rustmotion completions"
                        && !(trimmed.contains(".zfunc") && trimmed.starts_with("fpath="))
                        && !(trimmed.contains("compinit") && trimmed.starts_with("autoload"))
                }).collect();
                std::fs::write(&zshrc, filtered.join("\n") + "\n")?;
                eprintln!("Cleaned ~/.zshrc");
            }
        }
        clap_complete::Shell::Bash => {
            let path = home.join(".local/share/bash-completion/completions/rustmotion");
            if path.exists() {
                std::fs::remove_file(&path)?;
                eprintln!("Removed {}", path.display());
            }
        }
        clap_complete::Shell::Fish => {
            let path = home.join(".config/fish/completions/rustmotion.fish");
            if path.exists() {
                std::fs::remove_file(&path)?;
                eprintln!("Removed {}", path.display());
            }
        }
        _ => {
            return Err(RustmotionError::Generic(format!("Unsupported shell: {:?}", shell)).into());
        }
    }

    eprintln!("Completions uninstalled. Restart your shell.");
    Ok(())
}
