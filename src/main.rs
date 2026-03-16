mod encode;
mod engine;
mod error;
mod include;
mod preview;
mod schema;
mod tui;

// v2 architecture (M1: Foundation)
#[macro_use]
mod macros;
mod components;
mod layout;
mod traits;

use anyhow::Result;
use clap::{Parser, Subcommand};
use components::{ChildComponent, Component};
use error::RustmotionError;
use schema::{CardDisplay, ResolvedScenario, Scenario};
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
    /// Render a JSON scenario to MP4
    Render {
        /// Path to the JSON scenario file
        input: Option<PathBuf>,

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

        /// Open a preview window instead of encoding
        #[arg(short = 'p', long)]
        preview: bool,
    },

    /// Export a single frame as a still image (PNG, JPEG, WebP)
    Still {
        /// Path to the JSON scenario file
        input: PathBuf,

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
        input: PathBuf,
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
        input: PathBuf,
    },
}

#[derive(Clone, clap::ValueEnum)]
enum OutputFormat {
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
        Commands::Render {
            input,
            json,
            output,
            frame,
            output_format,
            codec,
            crf,
            format,
            transparent,
            watch,
            preview,
        } => {
            if preview {
                let input_path = input.ok_or(RustmotionError::PreviewRequiresFile)?;
                let scenario = load_scenario(&input_path)?;
                preview::run_preview(scenario, Some(input_path), watch)
            } else if watch {
                let input_path = input.ok_or(RustmotionError::WatchRequiresFile)?;
                cmd_watch(&input_path, &output, frame, output_format.as_ref(), cli.quiet, codec, crf, format, transparent)
            } else {
                let scenario = load_scenario_from_source(input.as_ref(), json.as_deref())?;
                cmd_render(scenario, &output, frame, output_format.as_ref(), cli.quiet, codec, crf, format, transparent)
            }
        }
        Commands::Still { input, output, time, format, quality } => {
            let scenario = load_scenario(&input)?;
            cmd_still(scenario, &output, time, format, quality)
        }
        Commands::Validate { input } => cmd_validate(&input),
        Commands::Schema { output } => cmd_schema(output.as_deref()),
        Commands::Info { input } => cmd_info(&input),
    }
}

fn load_scenario(input: &PathBuf) -> Result<ResolvedScenario> {
    let json_str = std::fs::read_to_string(input)
        .map_err(|e| RustmotionError::FileRead { path: input.display().to_string(), source: e })?;
    let scenario: Scenario = serde_json::from_str(&json_str)
        .map_err(RustmotionError::from)?;
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
            let scenario: Scenario = serde_json::from_str(json_str)
                .map_err(RustmotionError::from)?;
            include::resolve_includes(scenario, &include::IncludeSource::Inline)
        }
        (None, None) => {
            Err(RustmotionError::MissingInput.into())
        }
    }
}

fn cmd_render(
    scenario: ResolvedScenario,
    output: &PathBuf,
    frame: Option<u32>,
    output_format: Option<&OutputFormat>,
    quiet: bool,
    codec: Option<String>,
    crf: Option<u8>,
    format: Option<String>,
    transparent: bool,
) -> Result<()> {
    let start = std::time::Instant::now();

    // Load custom fonts if defined
    if !scenario.fonts.is_empty() {
        engine::renderer::load_custom_fonts(&scenario.fonts);
    }

    // Create parent directories if they don't exist
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    if let Some(frame_num) = frame {
        // Single frame render to PNG
        let png_path = if output.extension().map(|e| e == "mp4").unwrap_or(false) {
            output.with_extension("png")
        } else {
            output.clone()
        };
        render_single_frame(&scenario, frame_num, &png_path)?;
        if !quiet {
            eprintln!("Frame {} saved to {}", frame_num, png_path.display());
        }
    } else {
        // Determine output format
        let fmt = format.as_deref().unwrap_or_else(|| {
            output.extension()
                .and_then(|e| e.to_str())
                .unwrap_or("mp4")
        });

        match fmt {
            "png-seq" => {
                encode::encode_png_sequence(&scenario, output.to_str().unwrap(), quiet, transparent)?;
            }
            "gif" => {
                encode::encode_gif(&scenario, output.to_str().unwrap(), quiet)?;
            }
            "raw" => {
                encode::encode_raw_stdout(&scenario, false)?;
            }
            _ => {
                // Try FFmpeg first (supports 10-bit H.264, all codecs)
                // Fall back to built-in openh264 encoder if FFmpeg is not available
                let ffmpeg_available = std::process::Command::new("ffmpeg")
                    .arg("-version")
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);

                if ffmpeg_available {
                    encode::encode_with_ffmpeg(
                        &scenario,
                        output.to_str().unwrap(),
                        quiet,
                        codec.as_deref().unwrap_or("h264"),
                        crf,
                        transparent,
                    )?;
                } else {
                    encode::encode_video(&scenario, output.to_str().unwrap(), quiet)?;
                }
            }
        }
    }

    let elapsed = start.elapsed();

    if let Some(OutputFormat::Json) = output_format {
        let result = serde_json::json!({
            "status": "success",
            "output": output.to_str().unwrap(),
            "duration_ms": elapsed.as_millis(),
        });
        println!("{}", serde_json::to_string(&result)?);
    }

    Ok(())
}

fn cmd_watch(
    input: &PathBuf,
    output: &PathBuf,
    frame: Option<u32>,
    output_format: Option<&OutputFormat>,
    quiet: bool,
    codec: Option<String>,
    crf: Option<u8>,
    format: Option<String>,
    transparent: bool,
) -> Result<()> {
    use notify::{Watcher, RecursiveMode};
    use std::sync::mpsc;

    // Determine if we can use incremental rendering (native h264 only)
    let fmt = format.as_deref().unwrap_or_else(|| {
        output.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("mp4")
    });
    let use_ffmpeg = codec.as_deref().map_or(false, |c| c != "h264")
        || matches!(fmt, "webm" | "mov")
        || transparent;
    let can_incremental = frame.is_none()
        && !matches!(fmt, "png-seq" | "gif" | "raw")
        && !use_ffmpeg;

    // State for incremental rendering
    let mut prev_segments: Option<Vec<encode::SceneSegment>> = None;
    let mut prev_config_hash: Option<u64> = None;

    // Initialize TUI for watch mode
    let codec_label = codec.as_deref().unwrap_or("h264");
    let mut tui_watch: Option<tui::TuiWatch> = None;

    // Initial render
    match load_scenario(input) {
        Ok(scenario) => {
            // Load custom fonts if defined
            if !scenario.fonts.is_empty() {
                engine::renderer::load_custom_fonts(&scenario.fonts);
            }

            if !quiet {
                tui_watch = tui::TuiWatch::new(
                    &input.display().to_string(),
                    &output.display().to_string(),
                    scenario.video.width,
                    scenario.video.height,
                    scenario.video.fps,
                    codec_label,
                ).ok();
            }

            if can_incremental {
                let config_hash = encode::hash_video_config(&scenario.video);
                if let Some(ref mut tui) = tui_watch {
                    tui.set_phase(tui::WatchPhase::InitialRender);
                }
                let mut encoding_started = false;
                let mut cb = |progress: encode::EncodeProgress| {
                    if let Some(ref mut tui) = tui_watch {
                        match progress {
                            encode::EncodeProgress::Rendering(current, total) => tui.set_frame_progress(current, total),
                            encode::EncodeProgress::Encoding(current, total) => {
                                if !encoding_started {
                                    tui.set_phase(tui::WatchPhase::Encoding);
                                    encoding_started = true;
                                }
                                tui.set_frame_progress(current, total);
                            }
                            encode::EncodeProgress::Muxing => tui.set_phase(tui::WatchPhase::Muxing),
                        }
                    }
                };
                match encode::encode_video_incremental(&scenario, output.to_str().unwrap(), quiet, None, Some(&mut cb)) {
                    Ok(segments) => {
                        prev_segments = Some(segments);
                        prev_config_hash = Some(config_hash);
                        if let Some(ref mut tui) = tui_watch {
                            tui.finish_render();
                        }
                    }
                    Err(e) => eprintln!("Render error: {}", e),
                }
            } else {
                engine::clear_asset_cache();
                if let Err(e) = cmd_render(scenario, output, frame, output_format, quiet, codec.clone(), crf, format.clone(), transparent) {
                    eprintln!("Render error: {}", e);
                }
                if let Some(ref mut tui) = tui_watch {
                    tui.finish_render();
                }
            }
        }
        Err(e) => eprintln!("Load error: {}", e),
    }

    let (tx, rx) = mpsc::channel();

    let mut watcher = notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
        if let Ok(event) = res {
            if event.kind.is_modify() || event.kind.is_create() {
                let _ = tx.send(());
            }
        }
    })?;

    watcher.watch(input.as_ref(), RecursiveMode::NonRecursive)?;

    // Debounce: wait for changes, then re-render
    loop {
        // Block until a change event
        rx.recv().map_err(|_| RustmotionError::WatcherClosed)?;

        // Drain any additional events (debounce)
        std::thread::sleep(std::time::Duration::from_millis(100));
        while rx.try_recv().is_ok() {}

        match load_scenario(input) {
            Ok(scenario) => {
                if can_incremental {
                    let config_hash = encode::hash_video_config(&scenario.video);
                    // If video config changed (resolution/fps), do full re-render
                    let use_prev = if prev_config_hash == Some(config_hash) {
                        prev_segments.as_deref()
                    } else {
                        engine::clear_asset_cache();
                        None
                    };

                    // Count changed scenes for the TUI
                    let view0_scenes = scenario.views.get(0).map(|v| &v.scenes[..]).unwrap_or(&[]);
                    let num_scenes = view0_scenes.len();
                    let scene_hashes: Vec<u64> = view0_scenes.iter()
                        .map(|s| encode::hash_video_config_scene(s))
                        .collect();
                    let changed = if let Some(ref prev) = use_prev {
                        if prev.len() == num_scenes {
                            (0..num_scenes).filter(|&i| {
                                let hash_changed = scene_hashes[i] != prev[i].scene_hash;
                                let next_changed = if i + 1 < num_scenes {
                                    scene_hashes[i + 1] != prev[i + 1].scene_hash
                                        && view0_scenes[i + 1].transition.is_some()
                                } else { false };
                                hash_changed || next_changed
                            }).count()
                        } else { num_scenes }
                    } else { num_scenes };

                    if let Some(ref mut tui) = tui_watch {
                        tui.set_phase(tui::WatchPhase::Rerendering { changed, total: num_scenes });
                    }

                    let mut encoding_started = false;
                    let mut cb = |progress: encode::EncodeProgress| {
                        if let Some(ref mut tui) = tui_watch {
                            match progress {
                                encode::EncodeProgress::Rendering(current, total) => tui.set_frame_progress(current, total),
                                encode::EncodeProgress::Encoding(current, total) => {
                                    if !encoding_started {
                                        tui.set_phase(tui::WatchPhase::Encoding);
                                        encoding_started = true;
                                    }
                                    tui.set_frame_progress(current, total);
                                }
                                encode::EncodeProgress::Muxing => tui.set_phase(tui::WatchPhase::Muxing),
                            }
                        }
                    };
                    match encode::encode_video_incremental(&scenario, output.to_str().unwrap(), quiet, use_prev, Some(&mut cb)) {
                        Ok(segments) => {
                            prev_segments = Some(segments);
                            prev_config_hash = Some(config_hash);
                            if let Some(ref mut tui) = tui_watch {
                                tui.finish_render();
                            }
                        }
                        Err(e) => eprintln!("Render error: {}", e),
                    }
                } else {
                    engine::clear_asset_cache();
                    if let Err(e) = cmd_render(scenario, output, frame, output_format, quiet, codec.clone(), crf, format.clone(), transparent) {
                        eprintln!("Render error: {}", e);
                    }
                    if let Some(ref mut tui) = tui_watch {
                        tui.finish_render();
                    }
                }
            }
            Err(e) => eprintln!("Load error: {}", e),
        }
    }
}

fn render_single_frame(scenario: &ResolvedScenario, frame_num: u32, output: &PathBuf) -> Result<()> {
    use crate::schema::ViewType;

    let config = &scenario.video;
    let fps = config.fps;

    // Find which view and frame this belongs to
    let mut frame_offset = 0u32;
    for view in &scenario.views {
        match view.view_type {
            ViewType::World => {
                // World view: all scenes share a single continuous timeline
                let timeline = engine::world::WorldTimeline::build(view, fps, config.width, config.height);
                let view_frames = timeline.total_frames(fps);
                if frame_num < frame_offset + view_frames {
                    let frame_in_view = frame_num - frame_offset;
                    let rgba = engine::render_v2::render_world_frame_scaled(
                        config, view, &timeline, frame_in_view, 1.0,
                    )?;

                    let img = image::RgbaImage::from_raw(config.width, config.height, rgba)
                        .ok_or(RustmotionError::PixelImage)?;
                    img.save(output)?;
                    return Ok(());
                }
                frame_offset += view_frames;
            }
            ViewType::Slide => {
                // Slide view: each scene is independent
                for scene in &view.scenes {
                    let scene_frames = (scene.duration * fps as f64).round() as u32;
                    if frame_num < frame_offset + scene_frames {
                        let local_frame = frame_num - frame_offset;

                        let rgba = engine::render_v2::render_scene_frame(
                            config, scene, local_frame, scene_frames,
                        )?;

                        let img = image::RgbaImage::from_raw(config.width, config.height, rgba)
                            .ok_or(RustmotionError::PixelImage)?;
                        img.save(output)?;
                        return Ok(());
                    }
                    frame_offset += scene_frames;
                }
            }
        }
    }

    Err(RustmotionError::FrameOutOfRange { frame: frame_num, total: frame_offset }.into())
}

fn cmd_validate(input: &PathBuf) -> Result<()> {
    let json_str = std::fs::read_to_string(input)
        .map_err(|e| RustmotionError::FileRead { path: input.display().to_string(), source: e })?;

    // Parse JSON
    let scenario: Result<Scenario, _> = serde_json::from_str(&json_str);

    match scenario {
        Ok(scenario) => {
            // Resolve includes before validating
            let resolved = include::resolve_includes(
                scenario,
                &include::IncludeSource::File(input.clone()),
            );
            match resolved {
                Ok(resolved) => {
                    let errors = validate_scenario(&resolved);
                    let all_scenes: Vec<_> = resolved.all_scenes().collect();
                    if errors.is_empty() {
                        eprintln!("Valid scenario: {} scene(s) in {} view(s)", all_scenes.len(), resolved.views.len());
                        let total_duration: f64 =
                            all_scenes.iter().map(|s| s.duration).sum();
                        eprintln!(
                            "  Resolution: {}x{} @ {}fps",
                            resolved.video.width, resolved.video.height, resolved.video.fps
                        );
                        eprintln!("  Duration: {:.1}s", total_duration);
                        Ok(())
                    } else {
                        for err in &errors {
                            eprintln!("Error: {}", err);
                        }
                        std::process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("Include resolution error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("JSON parse error: {}", e);
            std::process::exit(1);
        }
    }
}

fn validate_children(
    children: &[ChildComponent],
    path: &str,
    errors: &mut Vec<String>,
) {
    for (j, child) in children.iter().enumerate() {
        let p = format!("{}.children[{}]", path, j);

        // Timing validation (common to all timed components)
        if let Some(timed) = child.component.as_timed() {
            let (start, end) = timed.timing();
            if let (Some(s), Some(e)) = (start, end) {
                if s >= e {
                    errors.push(format!("{}: start_at ({}) must be < end_at ({})", p, s, e));
                }
            }
        }

        // Component-specific validation
        match &child.component {
            Component::Image(img) => {
                if !std::path::Path::new(&img.src).exists() {
                    errors.push(format!("{}.src: file not found '{}'", p, img.src));
                }
            }
            Component::Video(v) => {
                if !std::path::Path::new(&v.src).exists() {
                    errors.push(format!("{}.src: file not found '{}'", p, v.src));
                }
            }
            Component::Gif(g) => {
                if !std::path::Path::new(&g.src).exists() {
                    errors.push(format!("{}.src: file not found '{}'", p, g.src));
                }
            }
            Component::Svg(svg) => {
                if svg.src.is_none() && svg.data.is_none() {
                    errors.push(format!("{}: SVG must have 'src' or 'data'", p));
                }
                if let Some(ref src) = svg.src {
                    if !std::path::Path::new(src).exists() {
                        errors.push(format!("{}.src: file not found '{}'", p, src));
                    }
                }
            }
            Component::Icon(icon) => {
                if let Some((prefix, name)) = icon.icon.split_once(':') {
                    if prefix.is_empty() || name.is_empty() {
                        errors.push(format!(
                            "{}: icon '{}' has empty prefix or name (expected 'prefix:name')",
                            p, icon.icon
                        ));
                    }
                } else {
                    errors.push(format!(
                        "{}: invalid icon format '{}' (expected 'prefix:name', e.g. 'lucide:home')",
                        p, icon.icon
                    ));
                }
            }
            Component::QrCode(qr) => {
                if qr.content.is_empty() {
                    errors.push(format!("{}: QR code content must not be empty", p));
                }
            }
            Component::Mockup(m) => {
                if !std::path::Path::new(&m.src).exists() {
                    errors.push(format!("{}.src: file not found '{}'", p, m.src));
                }
            }
            Component::Card(card) => {
                if matches!(card.style.display, Some(CardDisplay::Grid))
                    && card.style.grid_template_columns.is_none()
                {
                    errors.push(format!("{}: grid display without grid-template-columns", p));
                }
                validate_children(&card.children, &p, errors);
            }
            Component::Flex(flex) => {
                validate_children(&flex.children, &p, errors);
            }
            Component::Grid(grid) => {
                if grid.style.grid_template_columns.is_none() {
                    errors.push(format!("{}: grid without grid-template-columns", p));
                }
                validate_children(&grid.children, &p, errors);
            }
            Component::Positioned(pos) => {
                validate_children(&pos.children, &p, errors);
            }
            _ => {}
        }
    }
}

fn validate_scenario(scenario: &ResolvedScenario) -> Vec<String> {
    let mut errors = Vec::new();

    if scenario.video.width == 0 || scenario.video.height == 0 {
        errors.push("video.width and video.height must be > 0".to_string());
    }
    if scenario.video.width % 2 != 0 || scenario.video.height % 2 != 0 {
        errors.push("video.width and video.height must be even (required for H.264)".to_string());
    }
    if scenario.video.fps == 0 {
        errors.push("video.fps must be > 0".to_string());
    }

    let all_scenes: Vec<_> = scenario.all_scenes().collect();
    if all_scenes.is_empty() {
        errors.push("At least one scene is required".to_string());
    }

    for (vi, view) in scenario.views.iter().enumerate() {
        for (si, scene) in view.scenes.iter().enumerate() {
            if scene.duration <= 0.0 {
                errors.push(format!("views[{}].scenes[{}].duration must be > 0", vi, si));
            }

            validate_children(&scene.children, &format!("views[{}].scenes[{}]", vi, si), &mut errors);
        }
    }

    for (i, audio) in scenario.audio.iter().enumerate() {
        if !std::path::Path::new(&audio.src).exists() {
            errors.push(format!(
                "audio[{}].src: file not found '{}'",
                i, audio.src
            ));
        }
    }

    errors
}

fn cmd_schema(output: Option<&std::path::Path>) -> Result<()> {
    let schema = schema::generate_json_schema();
    let json = serde_json::to_string_pretty(&schema)?;

    if let Some(path) = output {
        std::fs::write(path, &json)?;
        eprintln!("Schema written to {}", path.display());
    } else {
        println!("{}", json);
    }

    Ok(())
}

fn cmd_still(
    scenario: ResolvedScenario,
    output: &PathBuf,
    time: f64,
    format: Option<String>,
    quality: u8,
) -> Result<()> {
    // Load custom fonts if defined
    if !scenario.fonts.is_empty() {
        engine::renderer::load_custom_fonts(&scenario.fonts);
    }

    let config = &scenario.video;
    let fps = config.fps;

    // Find which scene contains this time
    let all_scenes: Vec<_> = scenario.all_scenes().collect();
    let mut scene_start = 0.0f64;
    for (idx, scene) in all_scenes.iter().enumerate() {
        let scene_end = scene_start + scene.duration;
        if time < scene_end || idx == all_scenes.len() - 1 {
            let local_time = (time - scene_start).max(0.0);
            let frame_index = (local_time * fps as f64).round() as u32;
            let scene_frames = (scene.duration * fps as f64).round() as u32;

            let rgba = engine::render_v2::render_scene_frame(
                config, scene, frame_index.min(scene_frames.saturating_sub(1)), scene_frames,
            )?;

            // Create parent directories
            if let Some(parent) = output.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent)?;
                }
            }

            let img = image::RgbaImage::from_raw(config.width, config.height, rgba)
                .ok_or(RustmotionError::PixelImage)?;

            let fmt = format.as_deref().unwrap_or_else(|| {
                output.extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("png")
            });

            match fmt {
                "jpeg" | "jpg" => {
                    use image::ImageEncoder;
                    let file = std::fs::File::create(output)?;
                    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(file, quality);
                    encoder.write_image(
                        img.as_raw(),
                        config.width,
                        config.height,
                        image::ExtendedColorType::Rgba8,
                    )?;
                }
                "webp" => {
                    use image::ImageEncoder;
                    let file = std::fs::File::create(output)?;
                    let encoder = image::codecs::webp::WebPEncoder::new_lossless(file);
                    encoder.write_image(
                        img.as_raw(),
                        config.width,
                        config.height,
                        image::ExtendedColorType::Rgba8,
                    )?;
                }
                _ => {
                    img.save(output)?;
                }
            }

            eprintln!("Still image saved to {}", output.display());
            return Ok(());
        }
        scene_start = scene_end;
    }

    Err(RustmotionError::TimeOutOfRange { time }.into())
}

fn cmd_info(input: &PathBuf) -> Result<()> {
    let scenario = load_scenario(input)?;
    let fps = scenario.video.fps;
    let all_scenes: Vec<_> = scenario.all_scenes().collect();
    let total_duration: f64 = all_scenes.iter().map(|s| s.duration).sum();
    let total_frames: u32 = all_scenes
        .iter()
        .map(|s| (s.duration * fps as f64).round() as u32)
        .sum();

    let total_layers: usize = all_scenes.iter().map(|s| s.children.len()).sum();

    println!("File: {}", input.display());
    println!("Resolution: {}x{}", scenario.video.width, scenario.video.height);
    println!("FPS: {}", fps);
    println!("Duration: {:.1}s ({} frames)", total_duration, total_frames);
    println!("Views: {}", scenario.views.len());
    println!("Scenes: {}", all_scenes.len());
    println!("Total layers: {}", total_layers);
    println!("Audio tracks: {}", scenario.audio.len());

    for (vi, view) in scenario.views.iter().enumerate() {
        let vtype = match view.view_type {
            schema::ViewType::Slide => "Slide",
            schema::ViewType::World => "World",
        };
        println!("  View {}: {} ({} scenes)", vi + 1, vtype, view.scenes.len());
        for (si, scene) in view.scenes.iter().enumerate() {
            let scene_frames = (scene.duration * fps as f64).round() as u32;
            println!(
                "    Scene {}: {:.1}s ({} frames, {} layers{})",
                si + 1,
                scene.duration,
                scene_frames,
                scene.children.len(),
                scene
                    .transition
                    .as_ref()
                    .map(|t| format!(", transition: {:?} {:.1}s", t.transition_type, t.duration))
                    .unwrap_or_default()
            );
        }
    }

    Ok(())
}
