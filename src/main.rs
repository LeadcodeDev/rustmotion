mod encode;
mod engine;
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
        #[arg(long)]
        watch: bool,
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
        } => {
            if watch {
                let input_path = input.ok_or_else(|| anyhow::anyhow!("--watch requires an input file path (cannot use --json or stdin)"))?;
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

fn load_scenario(input: &PathBuf) -> Result<schema::Scenario> {
    let json_str = std::fs::read_to_string(input)
        .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", input.display(), e))?;
    let scenario: schema::Scenario = serde_json::from_str(&json_str)
        .map_err(|e| anyhow::anyhow!("Failed to parse JSON: {}", e))?;
    Ok(scenario)
}

fn load_scenario_from_source(
    input: Option<&PathBuf>,
    json: Option<&str>,
) -> Result<schema::Scenario> {
    match (input, json) {
        (Some(_), Some(_)) => {
            anyhow::bail!("Cannot use both input file and --json")
        }
        (Some(path), None) => load_scenario(path),
        (None, Some(json_str)) => {
            let scenario: schema::Scenario = serde_json::from_str(json_str)
                .map_err(|e| anyhow::anyhow!("Failed to parse JSON: {}", e))?;
            Ok(scenario)
        }
        (None, None) => {
            anyhow::bail!("Provide either an input file or --json")
        }
    }
}

fn cmd_render(
    scenario: schema::Scenario,
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
                // Check if we need FFmpeg (for h265, vp9, prores, webm, mov)
                let use_ffmpeg = codec.as_deref().map_or(false, |c| c != "h264")
                    || matches!(fmt, "webm" | "mov")
                    || transparent;

                if use_ffmpeg {
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

    eprintln!("Watching {} for changes... (Ctrl+C to stop)", input.display());

    // Initial render
    match load_scenario(input) {
        Ok(scenario) => {
            engine::clear_asset_cache();
            if let Err(e) = cmd_render(scenario, output, frame, output_format, quiet, codec.clone(), crf, format.clone(), transparent) {
                eprintln!("Render error: {}", e);
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
        rx.recv().map_err(|_| anyhow::anyhow!("File watcher channel closed"))?;

        // Drain any additional events (debounce)
        std::thread::sleep(std::time::Duration::from_millis(100));
        while rx.try_recv().is_ok() {}

        eprintln!("\nFile changed, re-rendering...");

        match load_scenario(input) {
            Ok(scenario) => {
                engine::clear_asset_cache();
                if let Err(e) = cmd_render(scenario, output, frame, output_format, quiet, codec.clone(), crf, format.clone(), transparent) {
                    eprintln!("Render error: {}", e);
                }
            }
            Err(e) => eprintln!("Load error: {}", e),
        }
    }
}

fn render_single_frame(scenario: &schema::Scenario, frame_num: u32, output: &PathBuf) -> Result<()> {
    let config = &scenario.video;
    let fps = config.fps;

    // Find which scene this frame belongs to
    let mut frame_offset = 0u32;
    for scene in &scenario.scenes {
        let scene_frames = (scene.duration * fps as f64).round() as u32;
        if frame_num < frame_offset + scene_frames {
            let local_frame = frame_num - frame_offset;

            let rgba = engine::render_v2::render_scene_frame(
                config, scene, local_frame, scene_frames,
            )?;

            // Save as PNG using the image crate
            let img = image::RgbaImage::from_raw(config.width, config.height, rgba)
                .ok_or_else(|| anyhow::anyhow!("Failed to create image from pixels"))?;
            img.save(output)?;
            return Ok(());
        }
        frame_offset += scene_frames;
    }

    anyhow::bail!(
        "Frame {} is out of range (total frames: {})",
        frame_num,
        frame_offset
    );
}

fn cmd_validate(input: &PathBuf) -> Result<()> {
    let json_str = std::fs::read_to_string(input)
        .map_err(|e| anyhow::anyhow!("Failed to read {}: {}", input.display(), e))?;

    // Parse JSON
    let scenario: Result<schema::Scenario, _> = serde_json::from_str(&json_str);

    match scenario {
        Ok(scenario) => {
            // Semantic validation
            let errors = validate_scenario(&scenario);
            if errors.is_empty() {
                eprintln!("Valid scenario: {} scene(s)", scenario.scenes.len());
                let total_duration: f64 = scenario.scenes.iter().map(|s| s.duration).sum();
                eprintln!(
                    "  Resolution: {}x{} @ {}fps",
                    scenario.video.width, scenario.video.height, scenario.video.fps
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
            eprintln!("JSON parse error: {}", e);
            std::process::exit(1);
        }
    }
}

fn validate_scenario(scenario: &schema::Scenario) -> Vec<String> {
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
    if scenario.scenes.is_empty() {
        errors.push("At least one scene is required".to_string());
    }

    for (i, scene) in scenario.scenes.iter().enumerate() {
        if scene.duration <= 0.0 {
            errors.push(format!("scenes[{}].duration must be > 0", i));
        }

        for (j, layer) in scene.children.iter().enumerate() {
            match layer {
                schema::Layer::Image(img) => {
                    if !std::path::Path::new(&img.src).exists() {
                        errors.push(format!(
                            "scenes[{}].children[{}].src: file not found '{}'",
                            i, j, img.src
                        ));
                    }
                    if let (Some(start), Some(end)) = (img.start_at, img.end_at) {
                        if start >= end {
                            errors.push(format!(
                                "scenes[{}].children[{}]: start_at ({}) must be < end_at ({})",
                                i, j, start, end
                            ));
                        }
                    }
                }
                schema::Layer::Text(t) => {
                    if let (Some(start), Some(end)) = (t.start_at, t.end_at) {
                        if start >= end {
                            errors.push(format!(
                                "scenes[{}].children[{}]: start_at ({}) must be < end_at ({})",
                                i, j, start, end
                            ));
                        }
                    }
                }
                schema::Layer::Shape(s) => {
                    if let (Some(start), Some(end)) = (s.start_at, s.end_at) {
                        if start >= end {
                            errors.push(format!(
                                "scenes[{}].children[{}]: start_at ({}) must be < end_at ({})",
                                i, j, start, end
                            ));
                        }
                    }
                }
                schema::Layer::Svg(svg) => {
                    if svg.src.is_none() && svg.data.is_none() {
                        errors.push(format!(
                            "scenes[{}].children[{}]: SVG layer must have 'src' or 'data'",
                            i, j
                        ));
                    }
                    if let Some(ref src) = svg.src {
                        if !std::path::Path::new(src).exists() {
                            errors.push(format!(
                                "scenes[{}].children[{}].src: file not found '{}'",
                                i, j, src
                            ));
                        }
                    }
                    if let (Some(start), Some(end)) = (svg.start_at, svg.end_at) {
                        if start >= end {
                            errors.push(format!(
                                "scenes[{}].children[{}]: start_at ({}) must be < end_at ({})",
                                i, j, start, end
                            ));
                        }
                    }
                }
                schema::Layer::Gif(g) => {
                    if !std::path::Path::new(&g.src).exists() {
                        errors.push(format!(
                            "scenes[{}].children[{}].src: file not found '{}'",
                            i, j, g.src
                        ));
                    }
                    if let (Some(start), Some(end)) = (g.start_at, g.end_at) {
                        if start >= end {
                            errors.push(format!(
                                "scenes[{}].children[{}]: start_at ({}) must be < end_at ({})",
                                i, j, start, end
                            ));
                        }
                    }
                }
                schema::Layer::Codeblock(cb) => {
                    if let (Some(start), Some(end)) = (cb.start_at, cb.end_at) {
                        if start >= end {
                            errors.push(format!(
                                "scenes[{}].children[{}]: start_at ({}) must be < end_at ({})",
                                i, j, start, end
                            ));
                        }
                    }
                }
                schema::Layer::Video(v) => {
                    if !std::path::Path::new(&v.src).exists() {
                        errors.push(format!(
                            "scenes[{}].children[{}].src: file not found '{}'",
                            i, j, v.src
                        ));
                    }
                    if let (Some(start), Some(end)) = (v.start_at, v.end_at) {
                        if start >= end {
                            errors.push(format!(
                                "scenes[{}].children[{}]: start_at ({}) must be < end_at ({})",
                                i, j, start, end
                            ));
                        }
                    }
                }
                schema::Layer::Counter(ct) => {
                    if let (Some(start), Some(end)) = (ct.start_at, ct.end_at) {
                        if start >= end {
                            errors.push(format!(
                                "scenes[{}].children[{}]: start_at ({}) must be < end_at ({})",
                                i, j, start, end
                            ));
                        }
                    }
                }
                schema::Layer::Card(card) | schema::Layer::Flex(card) => {
                    if let (Some(start), Some(end)) = (card.start_at, card.end_at) {
                        if start >= end {
                            errors.push(format!(
                                "scenes[{}].children[{}]: start_at ({}) must be < end_at ({})",
                                i, j, start, end
                            ));
                        }
                    }
                    // Grid validation
                    if matches!(card.style.display, Some(schema::CardDisplay::Grid)) && card.style.grid_template_columns.is_none() {
                        errors.push(format!(
                            "scenes[{}].children[{}]: grid display without grid-template-columns",
                            i, j
                        ));
                    }
                }
                schema::Layer::Icon(icon) => {
                    if let Some((prefix, name)) = icon.icon.split_once(':') {
                        if prefix.is_empty() || name.is_empty() {
                            errors.push(format!(
                                "scenes[{}].children[{}]: icon '{}' has empty prefix or name (expected 'prefix:name')",
                                i, j, icon.icon
                            ));
                        }
                    } else {
                        errors.push(format!(
                            "scenes[{}].children[{}]: invalid icon format '{}' (expected 'prefix:name', e.g. 'lucide:home')",
                            i, j, icon.icon
                        ));
                    }
                    if let (Some(start), Some(end)) = (icon.start_at, icon.end_at) {
                        if start >= end {
                            errors.push(format!(
                                "scenes[{}].children[{}]: start_at ({}) must be < end_at ({})",
                                i, j, start, end
                            ));
                        }
                    }
                }
                schema::Layer::Caption(_) | schema::Layer::Group(_) => {}
                schema::Layer::ProgressBar(pb) => {
                    if let (Some(start), Some(end)) = (pb.start_at, pb.end_at) {
                        if start >= end {
                            errors.push(format!(
                                "scenes[{}].children[{}]: start_at ({}) must be < end_at ({})",
                                i, j, start, end
                            ));
                        }
                    }
                }
                schema::Layer::QrCode(qr) => {
                    if qr.content.is_empty() {
                        errors.push(format!(
                            "scenes[{}].children[{}]: QR code content must not be empty",
                            i, j
                        ));
                    }
                    if let (Some(start), Some(end)) = (qr.start_at, qr.end_at) {
                        if start >= end {
                            errors.push(format!(
                                "scenes[{}].children[{}]: start_at ({}) must be < end_at ({})",
                                i, j, start, end
                            ));
                        }
                    }
                }
            }
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
    scenario: schema::Scenario,
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
    let mut scene_start = 0.0f64;
    for scene in &scenario.scenes {
        let scene_end = scene_start + scene.duration;
        if time < scene_end || std::ptr::eq(scene, scenario.scenes.last().unwrap()) {
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
                .ok_or_else(|| anyhow::anyhow!("Failed to create image from pixels"))?;

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

    anyhow::bail!("Time {:.2}s is beyond video duration", time);
}

fn cmd_info(input: &PathBuf) -> Result<()> {
    let scenario = load_scenario(input)?;
    let fps = scenario.video.fps;
    let total_duration: f64 = scenario.scenes.iter().map(|s| s.duration).sum();
    let total_frames: u32 = scenario
        .scenes
        .iter()
        .map(|s| (s.duration * fps as f64).round() as u32)
        .sum();

    let total_layers: usize = scenario.scenes.iter().map(|s| s.children.len()).sum();

    println!("File: {}", input.display());
    println!("Resolution: {}x{}", scenario.video.width, scenario.video.height);
    println!("FPS: {}", fps);
    println!("Duration: {:.1}s ({} frames)", total_duration, total_frames);
    println!("Scenes: {}", scenario.scenes.len());
    println!("Total layers: {}", total_layers);
    println!("Audio tracks: {}", scenario.audio.len());

    for (i, scene) in scenario.scenes.iter().enumerate() {
        let scene_frames = (scene.duration * fps as f64).round() as u32;
        println!(
            "  Scene {}: {:.1}s ({} frames, {} layers{})",
            i + 1,
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

    Ok(())
}
