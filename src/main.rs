mod encode;
mod engine;
mod schema;

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
}

#[derive(Subcommand)]
enum Commands {
    /// Render a JSON scenario to MP4
    Render {
        /// Path to the JSON scenario file
        input: PathBuf,

        /// Output MP4 file path
        #[arg(short, long, default_value = "output.mp4")]
        output: PathBuf,

        /// Render a single frame instead of full video (0-indexed)
        #[arg(long)]
        frame: Option<u32>,

        /// Output format for machine consumption
        #[arg(long, value_enum)]
        output_format: Option<OutputFormat>,
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

    match cli.command {
        Commands::Render {
            input,
            output,
            frame,
            output_format,
        } => cmd_render(&input, &output, frame, output_format.as_ref(), cli.quiet),
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

fn cmd_render(
    input: &PathBuf,
    output: &PathBuf,
    frame: Option<u32>,
    output_format: Option<&OutputFormat>,
    quiet: bool,
) -> Result<()> {
    let scenario = load_scenario(input)?;
    let start = std::time::Instant::now();

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
        encode::encode_video(&scenario, output.to_str().unwrap(), quiet)?;
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

fn render_single_frame(scenario: &schema::Scenario, frame_num: u32, output: &PathBuf) -> Result<()> {
    let config = &scenario.video;
    let fps = config.fps;

    // Find which scene this frame belongs to
    let mut frame_offset = 0u32;
    for scene in &scenario.scenes {
        let scene_frames = (scene.duration * fps as f64).round() as u32;
        if frame_num < frame_offset + scene_frames {
            let local_frame = frame_num - frame_offset;
            let rgba = engine::render_frame(config, scene, local_frame, scene_frames)?;

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

        for (j, layer) in scene.layers.iter().enumerate() {
            if let schema::Layer::Image(img) = layer {
                if !std::path::Path::new(&img.src).exists() {
                    errors.push(format!(
                        "scenes[{}].layers[{}].src: file not found '{}'",
                        i, j, img.src
                    ));
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

fn cmd_info(input: &PathBuf) -> Result<()> {
    let scenario = load_scenario(input)?;
    let fps = scenario.video.fps;
    let total_duration: f64 = scenario.scenes.iter().map(|s| s.duration).sum();
    let total_frames: u32 = scenario
        .scenes
        .iter()
        .map(|s| (s.duration * fps as f64).round() as u32)
        .sum();

    let total_layers: usize = scenario.scenes.iter().map(|s| s.layers.len()).sum();

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
            scene.layers.len(),
            scene
                .transition
                .as_ref()
                .map(|t| format!(", transition: {:?} {:.1}s", t.transition_type, t.duration))
                .unwrap_or_default()
        );
    }

    Ok(())
}
