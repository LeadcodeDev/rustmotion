mod commands;
mod skills;
pub mod tui;

use clap::{CommandFactory, Parser, Subcommand};
use rustmotion::error::{Result, RustmotionError};
use rustmotion::loader::load_input;
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "rustmotion",
    version,
    about = "Render motion design videos from JSON scenarios"
)]
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

        /// Skip the implicit validation pass (schema + geometry + variables).
        /// Use only when the scenario was validated upstream (e.g. CI).
        #[arg(long)]
        no_validate: bool,

        /// Treat geometry violations as warnings instead of errors during the
        /// implicit validation pass.
        #[arg(long)]
        lenient: bool,

        /// Sample animated frames and apply renderer transforms to detect
        /// per-frame viewport overflow during the implicit validation pass.
        #[arg(long)]
        strict_anim: bool,

        /// Load variable overrides from a JSON object file (e.g. {"color":"#f00"}).
        /// Keys must match variables declared in the scenario's `config` block.
        #[arg(long, value_name = "FILE")]
        props: Option<PathBuf>,

        /// Set a single variable override as key=value (repeatable).
        /// The value is parsed as JSON if valid, otherwise treated as a string.
        /// --var takes precedence over --props for the same key.
        #[arg(long, value_name = "KEY=VALUE", number_of_values = 1)]
        var: Vec<String>,
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

        /// Load variable overrides from a JSON object file.
        #[arg(long, value_name = "FILE")]
        props: Option<PathBuf>,

        /// Set a single variable override as key=value (repeatable).
        #[arg(long, value_name = "KEY=VALUE", number_of_values = 1)]
        var: Vec<String>,
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

        /// Treat unknown component attributes as errors instead of warnings.
        #[arg(long)]
        strict_attrs: bool,

        /// Treat geometry violations as warnings instead of errors.
        #[arg(long)]
        lenient: bool,

        /// Load variable overrides from a JSON object file.
        #[arg(long, value_name = "FILE")]
        props: Option<PathBuf>,

        /// Set a single variable override as key=value (repeatable).
        #[arg(long, value_name = "KEY=VALUE", number_of_values = 1)]
        var: Vec<String>,
    },

    /// Render one video per line of a JSONL data file
    Batch {
        /// Path to the scenario template file (JSON or HTML dialect)
        #[arg(short = 'f', long)]
        file: PathBuf,

        /// Path to a JSONL file where each line is a JSON object of variable overrides
        #[arg(long)]
        data: PathBuf,

        /// Directory to write output files into
        #[arg(long)]
        output_dir: PathBuf,

        /// Output filename template. Use {field} for values from each data line
        /// and {index} for the 0-based line number. Default: "{index}.mp4".
        #[arg(long, default_value = "{index}.mp4")]
        name_template: String,

        /// Video codec (h264, h265, vp9, prores)
        #[arg(long)]
        codec: Option<String>,

        /// Constant Rate Factor (0-51, lower = better quality)
        #[arg(long)]
        crf: Option<u8>,

        /// Output file format (mp4, webm, mov, gif, png-seq)
        #[arg(long)]
        format: Option<String>,

        /// Enable transparent background
        #[arg(long)]
        transparent: bool,

        /// Number of videos to render in parallel (default: 1 / sequential).
        /// Note: the render itself already uses all cores via rayon; --jobs
        /// parallelises across videos, which may saturate the machine.
        #[arg(long, default_value = "1")]
        jobs: usize,
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

/// Parse `--var key=value` flags into a map. Values that parse as valid JSON
/// scalars or objects are stored as their JSON type; bare strings that are not
/// valid JSON are stored as JSON strings.
///
/// Parsing rules (applied in order):
///   1. Split on the first `=`. Keys without `=` are an error.
///   2. Try `serde_json::from_str` on the value part.
///   3. If that fails, treat the raw string as a JSON string value.
///
/// Examples:
///   `--var count=42`        → `count: Number(42)`
///   `--var flag=true`       → `flag: Bool(true)`
///   `--var name=hello`      → `name: String("hello")`  (bare string, not valid JSON)
///   `--var name='"hello"'`  → `name: String("hello")`  (explicit JSON string)
fn parse_var_flags(vars: &[String]) -> Result<HashMap<String, serde_json::Value>> {
    let mut map = HashMap::new();
    for entry in vars {
        let (key, raw_val) = entry.split_once('=').ok_or_else(|| {
            RustmotionError::Generic(format!(
                "--var '{}' is missing '=': expected KEY=VALUE",
                entry
            ))
        })?;
        if key.is_empty() {
            return Err(RustmotionError::Generic(format!(
                "--var '{}': key must not be empty",
                entry
            )));
        }
        let value: serde_json::Value = serde_json::from_str(raw_val)
            .unwrap_or_else(|_| serde_json::Value::String(raw_val.to_string()));
        map.insert(key.to_string(), value);
    }
    Ok(map)
}

/// Load a `--props <file.json>` JSON object into a variable map.
fn load_props_file(path: &PathBuf) -> Result<HashMap<String, serde_json::Value>> {
    let text = std::fs::read_to_string(path).map_err(|e| RustmotionError::FileRead {
        path: path.display().to_string(),
        source: e,
    })?;
    let val: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
        RustmotionError::Generic(format!(
            "--props file '{}' is not valid JSON: {}",
            path.display(),
            e
        ))
    })?;
    match val {
        serde_json::Value::Object(map) => Ok(map.into_iter().collect()),
        _ => Err(RustmotionError::Generic(format!(
            "--props file '{}' must be a JSON object {{...}}, got a different type",
            path.display()
        ))),
    }
}

/// Merge `--props` and `--var` flags into a single override map.
/// `--var` takes precedence over `--props` for the same key.
/// Returns `None` if neither flag was supplied (no-override fast path).
fn build_overrides(
    props: Option<&PathBuf>,
    var_flags: &[String],
) -> Result<Option<HashMap<String, serde_json::Value>>> {
    let has_props = props.is_some();
    let has_vars = !var_flags.is_empty();
    if !has_props && !has_vars {
        return Ok(None);
    }
    let mut map = if let Some(p) = props {
        load_props_file(p)?
    } else {
        HashMap::new()
    };
    // --var wins over --props
    for (k, v) in parse_var_flags(var_flags)? {
        map.insert(k, v);
    }
    Ok(Some(map))
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
        Commands::Studio { file } => match load_input(&file) {
            Ok(scenario) => rustmotion_studio::run_preview(scenario, Some(file), true),
            Err(e) => rustmotion_studio::run_preview_with_error(format!("{}", e), Some(file), true),
        },
        Commands::Render {
            file,
            json,
            output,
            frame,
            output_format,
            codec,
            crf,
            format,
            transparent,
            watch,
            no_validate,
            lenient,
            strict_anim,
            props,
            var,
        } => {
            // Validate codec / CRF up-front so we never spawn an encoder with bad args.
            commands::validation::check_codec(codec.as_deref())?;
            commands::validation::check_crf(crf)?;

            let overrides = build_overrides(props.as_ref(), &var)?;

            if watch {
                if overrides.is_some() {
                    return Err(RustmotionError::Generic(
                        "--props / --var are not supported in --watch mode".to_string(),
                    ));
                }
                let input_path = file.ok_or(RustmotionError::WatchRequiresFile)?;
                commands::cmd_watch(
                    &input_path,
                    &output,
                    frame,
                    output_format.as_ref(),
                    cli.quiet,
                    codec,
                    crf,
                    format,
                    transparent,
                    no_validate,
                    lenient,
                    strict_anim,
                )
            } else {
                let source = match (file.as_ref(), json.as_deref()) {
                    (Some(_), Some(_)) => return Err(RustmotionError::ConflictingInput),
                    (Some(p), None) => commands::validation::ValidationSource::File(p),
                    (None, Some(j)) => commands::validation::ValidationSource::Inline(j),
                    (None, None) => return Err(RustmotionError::MissingInput),
                };

                let loaded = commands::validation::load_with_vars(source, overrides.as_ref())?;

                if !cli.quiet {
                    commands::validation::warn_on_silent_defaults(&loaded);
                }

                if !no_validate {
                    let report = commands::validation::run_checks(&loaded, strict_anim);
                    if !report.is_clean() {
                        let label = loaded
                            .source_path
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|| "<inline>".to_string());
                        commands::validation::print_report(&report, &label);
                    }
                    if report.is_blocking(lenient) {
                        return Err(report.to_error());
                    }
                }

                commands::cmd_render(
                    loaded.scenario,
                    &output,
                    frame,
                    output_format.as_ref(),
                    cli.quiet,
                    codec,
                    crf,
                    format,
                    transparent,
                )
            }
        }
        Commands::Still {
            file,
            output,
            time,
            format,
            quality,
            props,
            var,
        } => {
            let overrides = build_overrides(props.as_ref(), &var)?;
            let scenario = rustmotion::loader::load_input_with_vars(&file, overrides.as_ref())?;
            commands::cmd_still(scenario, &output, time, format, quality)
        }
        Commands::Validate {
            file,
            report,
            fix,
            strict_anim,
            strict_attrs,
            lenient,
            props,
            var,
        } => {
            let overrides = build_overrides(props.as_ref(), &var)?;
            commands::cmd_validate(
                &file,
                report.as_deref(),
                fix,
                strict_anim,
                strict_attrs,
                lenient,
                overrides.as_ref(),
            )
        }
        Commands::Batch {
            file,
            data,
            output_dir,
            name_template,
            codec,
            crf,
            format,
            transparent,
            jobs,
        } => {
            commands::validation::check_codec(codec.as_deref())?;
            commands::validation::check_crf(crf)?;
            commands::cmd_batch(
                &file,
                &data,
                &output_dir,
                &name_template,
                codec,
                crf,
                format,
                transparent,
                jobs,
                cli.quiet,
            )
        }
        Commands::Schema { output } => commands::cmd_schema(output.as_deref()),
        Commands::Info { file } => commands::cmd_info(&file),
        Commands::Skills { action } => match action {
            SkillsAction::Install { global } => skills::install(global),
            SkillsAction::Uninstall { global } => skills::uninstall(global),
            SkillsAction::List => {
                skills::list();
                Ok(())
            }
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
        },
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
        RustmotionError::Generic(
            "Could not detect shell. Use 'completions generate <shell>' instead.".into(),
        )
    })?;

    let mut buf = Vec::new();
    clap_complete::generate(shell, &mut Cli::command(), "rustmotion", &mut buf);
    clap_complete::generate(
        shell,
        &mut rustmotion_studio::command(),
        "rustmotion-studio",
        &mut buf,
    );
    let completions = String::from_utf8(buf).unwrap();

    let home = dirs::home_dir()
        .ok_or_else(|| RustmotionError::Generic("Could not find home directory".into()))?;

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
                let mut f = std::fs::OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(&zshrc)?;
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
            return Err(RustmotionError::Generic(format!(
                "Unsupported shell for install: {:?}. Use 'completions generate' instead.",
                shell
            )));
        }
    }

    Ok(())
}

fn uninstall_completions() -> Result<()> {
    let shell =
        detect_shell().ok_or_else(|| RustmotionError::Generic("Could not detect shell.".into()))?;

    let home = dirs::home_dir()
        .ok_or_else(|| RustmotionError::Generic("Could not find home directory".into()))?;

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
                let filtered: Vec<&str> = content
                    .lines()
                    .filter(|line| {
                        let trimmed = line.trim();
                        trimmed != "# rustmotion completions"
                            && !(trimmed.contains(".zfunc") && trimmed.starts_with("fpath="))
                            && !(trimmed.contains("compinit") && trimmed.starts_with("autoload"))
                    })
                    .collect();
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
            return Err(RustmotionError::Generic(format!(
                "Unsupported shell: {:?}",
                shell
            )));
        }
    }

    eprintln!("Completions uninstalled. Restart your shell.");
    Ok(())
}

#[cfg(test)]
mod var_parsing_tests {
    use super::*;
    use serde_json::json;

    /// "42" parses as Number, not String.
    #[test]
    fn var_flag_numeric_string_parses_as_number() {
        let flags = vec!["count=42".to_string()];
        let map = parse_var_flags(&flags).unwrap();
        assert_eq!(map["count"], json!(42));
    }

    /// "true" parses as Bool.
    #[test]
    fn var_flag_true_parses_as_bool() {
        let flags = vec!["flag=true".to_string()];
        let map = parse_var_flags(&flags).unwrap();
        assert_eq!(map["flag"], json!(true));
    }

    /// A bare word that is not valid JSON becomes a String.
    #[test]
    fn var_flag_bare_word_is_string() {
        let flags = vec!["name=hello".to_string()];
        let map = parse_var_flags(&flags).unwrap();
        assert_eq!(map["name"], json!("hello"));
    }

    /// An explicitly-quoted JSON string `'"42"'` becomes String("42").
    #[test]
    fn var_flag_quoted_number_is_string() {
        let flags = vec!["name=\"42\"".to_string()];
        let map = parse_var_flags(&flags).unwrap();
        assert_eq!(map["name"], json!("42"));
    }

    /// --var wins over --props for the same key.
    #[test]
    fn var_wins_over_props_for_same_key() {
        // Build a fake props map directly (no file I/O needed for unit test)
        let mut props_map: HashMap<String, serde_json::Value> = HashMap::new();
        props_map.insert("color".to_string(), json!("#000000"));

        // Simulate merge logic that build_overrides does
        let var_flags = vec!["color=#ffffff".to_string()];
        let var_map = parse_var_flags(&var_flags).unwrap();

        // --var must overwrite --props
        let mut merged = props_map;
        for (k, v) in var_map {
            merged.insert(k, v);
        }
        assert_eq!(merged["color"], json!("#ffffff"));
    }

    /// Missing '=' in --var is an error.
    #[test]
    fn var_flag_missing_eq_is_error() {
        let flags = vec!["noequals".to_string()];
        let result = parse_var_flags(&flags);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("KEY=VALUE"));
    }
}
