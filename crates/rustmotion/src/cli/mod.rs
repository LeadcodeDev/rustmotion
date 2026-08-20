//! Le binaire `rustmotion` : analyse des arguments et aiguillage vers
//! `commands`.
//!
//! Il n'y a pas de sous-commande `studio` ici. Le studio est une app Dioxus
//! qui dépend de cette crate ; l'appeler depuis ce module ferait dépendre
//! `rustmotion` de `rustmotion-studio`, donc d'elle-même, et cargo refuse le
//! cycle. Le studio s'ouvre par son propre binaire, `rustmotion-studio -f
//! scenario.json`.

mod claude_md;
mod commands;
mod skills;
mod tui;

use clap::{CommandFactory, Parser, Subcommand};
use rustmotion::error::{Result, RustmotionError};
use rustmotion::schema::ResolvedScenario;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

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

        /// Render only frames START..=END (0-indexed, inclusive) as a
        /// standalone segment instead of the full video — e.g. `0-149` for
        /// this scenario's first 150 frames. Bounds are validated against
        /// the scenario's actual total frame count once it is loaded.
        /// Segments carry their own windowed slice of the scenario's audio
        /// (they do not restart every track from t=0), so segments from the
        /// same scenario can be joined with `rustmotion concat` afterwards.
        /// Mutually exclusive with --frame and --watch. Only mp4/webm/mov
        /// output is implemented for a range today — png-seq/gif/raw are
        /// not (see --format).
        #[arg(long, value_name = "START-END", conflicts_with_all = ["frame", "watch"])]
        frames: Option<FrameRangeArg>,

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

        /// Try this machine's hardware video encoder (VideoToolbox on macOS,
        /// NVENC/QSV/AMF elsewhere) for h264/h265, probed live via `ffmpeg
        /// -encoders` — never assumed from the platform this binary was
        /// built for. Falls back to the software encoder, with a message,
        /// when unavailable, unsupported for the chosen codec, or combined
        /// with --transparent (no hardware encoder here produces an alpha
        /// channel). `--crf` has no effect once a hardware encoder is
        /// actually used; see the warning `check_crf` prints for that case.
        #[arg(long)]
        hardware_acceleration: bool,

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

        /// Deprecated. Unknown component attributes are errors by default, so
        /// this flag no longer changes anything; it is accepted so existing
        /// scripts keep working, and prints a notice when used.
        #[arg(long)]
        strict_attrs: bool,

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

    /// Join MP4 segments — e.g. ones produced by several `render --frames
    /// a-b` calls against the same scenario — into one file. Remuxes via
    /// ffmpeg's concat demuxer (`-c copy`, no re-encoding); every input must
    /// share codec, resolution, and pixel format, which segments of the
    /// same scenario rendered with the same `render` flags always do.
    /// Requires ffmpeg on PATH.
    Concat {
        /// Segment files to join, in order.
        #[arg(required = true, num_args = 1..)]
        inputs: Vec<PathBuf>,

        /// Output file path
        #[arg(short, long, default_value = "concat.mp4")]
        output: PathBuf,
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

    /// Generate word-level caption timings from audio (whisper.cpp) or subtitles
    #[command(after_help = CAPTIONS_EXAMPLES)]
    Captions {
        /// Audio file to transcribe (wav/mp3/flac — requires a whisper.cpp
        /// binary in PATH: `brew install whisper-cpp`)
        #[arg(
            required_unless_present_any = ["from_srt", "from_vtt"],
            conflicts_with_all = ["from_srt", "from_vtt"]
        )]
        audio: Option<PathBuf>,

        /// Output JSON file (prints to stdout if omitted)
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Whisper model: a name (tiny, base, small, medium, large-v3)
        /// resolved from ~/.cache/whisper/ggml-<name>.bin and next to the
        /// binary, or a direct path to a .bin file
        #[arg(long, default_value = "base", conflicts_with_all = ["from_srt", "from_vtt"])]
        model: String,

        /// Spoken language code (e.g. "en", "fr"); auto-detected if omitted
        #[arg(long, conflicts_with_all = ["from_srt", "from_vtt"])]
        lang: Option<String>,

        /// Import cues from an SRT subtitle file instead of transcribing.
        /// Word timing is spread uniformly over each cue's duration.
        #[arg(long, value_name = "FILE", conflicts_with = "from_vtt")]
        from_srt: Option<PathBuf>,

        /// Import cues from a WebVTT subtitle file instead of transcribing.
        /// Word timing is spread uniformly over each cue's duration.
        #[arg(long, value_name = "FILE")]
        from_vtt: Option<PathBuf>,
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

        /// Deprecated. Unknown component attributes are errors by default, so
        /// this flag no longer changes anything; it is accepted so existing
        /// scripts keep working, and prints a notice when used.
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

const CAPTIONS_EXAMPLES: &str = "\
Examples:
  # Transcribe audio into word-level timings (requires whisper.cpp)
  rustmotion captions voice.mp3 -o words.json

  # Import subtitles instead (word timing spread uniformly inside each cue)
  rustmotion captions --from-srt subs.srt -o words.json

  # Inject the output into a scenario through the variables system.
  # In scenario.json:
  #   \"config\": { \"words\": { \"type\": \"array\", \"default\": [] } }
  #   ... { \"type\": \"caption\", \"mode\": \"word_pop\", \"words\": \"$words\" } ...
  rustmotion render -f scenario.json --props words.json";

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

/// `--frames START-END`: an inclusive, 0-indexed frame range. Parsed eagerly
/// (format + `start <= end`) by clap via `FromStr`; whether `end` actually
/// fits the scenario's total frame count can only be checked once the
/// scenario is loaded, so that half lives in
/// `RustmotionError::FrameRangeOutOfRange` instead.
#[derive(Clone, Copy, Debug)]
pub(crate) struct FrameRangeArg {
    start: u32,
    end: u32,
}

impl std::str::FromStr for FrameRangeArg {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        let (a, b) = s.split_once('-').ok_or_else(|| {
            format!("--frames '{s}' must look like START-END (e.g. 0-149), got no '-'")
        })?;
        let start: u32 = a
            .trim()
            .parse()
            .map_err(|_| format!("--frames '{s}': '{a}' is not a valid frame number"))?;
        let end: u32 = b
            .trim()
            .parse()
            .map_err(|_| format!("--frames '{s}': '{b}' is not a valid frame number"))?;
        if start > end {
            return Err(format!(
                "--frames '{s}': start ({start}) must be <= end ({end})"
            ));
        }
        Ok(FrameRangeArg { start, end })
    }
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

/// Render frames `[frame_range.0, frame_range.1]` (inclusive, 0-indexed) of
/// `scenario` as a standalone segment file, instead of the full video.
///
/// Deliberately separate from `commands::cmd_render` rather than an added
/// parameter on it: `cmd_render` is also called from `commands::batch`,
/// outside this change's file scope, so its signature stays untouched.
/// Only the two output kinds `render --frames` actually supports
/// (mp4/webm/mov, native or ffmpeg-driven) are implemented here —
/// png-seq/gif/raw frame-range support does not exist yet (see the
/// `--frames` help text) and this function says so instead of silently
/// ignoring the range for those formats.
#[allow(clippy::too_many_arguments)]
fn render_frame_range(
    scenario: ResolvedScenario,
    output: &Path,
    frame_range: (u32, u32),
    output_format: Option<&OutputFormat>,
    quiet: bool,
    codec: Option<String>,
    crf: Option<u8>,
    format: Option<String>,
    transparent: bool,
    hardware_acceleration: bool,
) -> Result<()> {
    let start_time = std::time::Instant::now();

    if !scenario.fonts.is_empty() {
        rustmotion::engine::renderer::load_custom_fonts(&scenario.fonts);
    }

    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    let fmt = format
        .as_deref()
        .unwrap_or_else(|| output.extension().and_then(|e| e.to_str()).unwrap_or("mp4"));

    if matches!(fmt, "png-seq" | "gif" | "raw") {
        return Err(RustmotionError::Generic(format!(
            "--frames does not support --format {fmt} yet; only mp4/webm/mov segment output is \
             implemented. Render the full video in that format instead, or drop --format for the \
             default mp4 container."
        )));
    }

    let output_str = output
        .to_str()
        .ok_or_else(|| RustmotionError::NonUtf8Path {
            path: output.to_string_lossy().into_owned(),
        })?;

    let ffmpeg_available = std::process::Command::new("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    let codec_str = codec.as_deref().unwrap_or("h264");

    let mut cb = |p: rustmotion::encode::EncodeProgress| {
        if quiet {
            return;
        }
        match p {
            rustmotion::encode::EncodeProgress::Rendering(c, t) => {
                eprint!(
                    "\rRendering frames {}-{}: {}/{}",
                    frame_range.0, frame_range.1, c, t
                );
            }
            rustmotion::encode::EncodeProgress::Encoding(c, t) => {
                eprint!("\rEncoding: {}/{}          ", c, t);
            }
            rustmotion::encode::EncodeProgress::Muxing => {
                eprint!("\rMuxing...                          ");
            }
        }
    };

    if ffmpeg_available {
        rustmotion::encode::video::encode_with_ffmpeg_hw_range(
            &scenario,
            output_str,
            quiet,
            codec_str,
            crf,
            transparent,
            hardware_acceleration,
            frame_range,
            Some(&mut cb),
        )?;
    } else {
        if hardware_acceleration && !quiet {
            eprintln!(
                "Hardware acceleration requested but ffmpeg was not found on PATH (only ffmpeg \
                 can drive a hardware encoder); continuing with the bundled software encoder."
            );
        }
        rustmotion::encode::video::encode_video_range(
            &scenario,
            output_str,
            quiet,
            frame_range,
            Some(&mut cb),
        )?;
    }

    if !quiet {
        eprintln!();
        eprintln!(
            "Frames {}-{} saved to {}",
            frame_range.0,
            frame_range.1,
            output.display()
        );
    }

    let elapsed = start_time.elapsed();
    if let Some(OutputFormat::Json) = output_format {
        let result = serde_json::json!({
            "status": "success",
            "output": output.to_string_lossy(),
            "frame_start": frame_range.0,
            "frame_end": frame_range.1,
            "duration_ms": elapsed.as_millis(),
        });
        println!("{}", serde_json::to_string(&result)?);
    }

    Ok(())
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
        Commands::Render {
            file,
            json,
            output,
            frame,
            frames,
            output_format,
            codec,
            crf,
            format,
            transparent,
            hardware_acceleration,
            watch,
            no_validate,
            lenient,
            strict_anim,
            strict_attrs,
            props,
            var,
        } => {
            // Validate codec / CRF up-front so we never spawn an encoder with bad args.
            commands::validation::check_codec(codec.as_deref())?;
            if let Some(warning) = commands::validation::check_crf(crf, hardware_acceleration)? {
                if !cli.quiet {
                    eprintln!("Warning: {warning}");
                }
            }

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
                    hardware_acceleration,
                    no_validate,
                    lenient,
                    strict_anim,
                    strict_attrs,
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
                    let mut report = commands::validation::run_checks(&loaded, strict_anim);
                    if strict_attrs {
                        commands::validation::warn_strict_attrs_is_now_default();
                        report.promote_attr_warnings();
                    }
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

                if let Some(range) = frames {
                    render_frame_range(
                        loaded.scenario,
                        &output,
                        (range.start, range.end),
                        output_format.as_ref(),
                        cli.quiet,
                        codec,
                        crf,
                        format,
                        transparent,
                        hardware_acceleration,
                    )
                } else {
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
                        hardware_acceleration,
                    )
                }
            }
        }
        Commands::Concat { inputs, output } => {
            let output_str = output
                .to_str()
                .ok_or_else(|| RustmotionError::NonUtf8Path {
                    path: output.to_string_lossy().into_owned(),
                })?;
            rustmotion::encode::video::concat_mp4_segments(&inputs, output_str)?;
            if !cli.quiet {
                eprintln!(
                    "Joined {} segment(s) into {}",
                    inputs.len(),
                    output.display()
                );
            }
            Ok(())
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
        Commands::Captions {
            audio,
            output,
            model,
            lang,
            from_srt,
            from_vtt,
        } => commands::cmd_captions(
            audio.as_deref(),
            output.as_deref(),
            &model,
            lang.as_deref(),
            from_srt.as_deref(),
            from_vtt.as_deref(),
            cli.quiet,
        ),
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
            // `batch` has no `--hardware-acceleration` flag of its own (out of
            // this workstream's file scope: wiring it into `cmd_batch` touches
            // commands/batch.rs), so this combination can never fire here.
            commands::validation::check_crf(crf, false)?;
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
