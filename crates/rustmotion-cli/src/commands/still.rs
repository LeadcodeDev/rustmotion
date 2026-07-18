use rustmotion::engine;
use rustmotion::error::{Result, RustmotionError};
use rustmotion::schema::ResolvedScenario;
use std::path::PathBuf;

pub fn cmd_still(
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

            let rgba = engine::render::render_scene_frame(
                config,
                scene,
                frame_index.min(scene_frames.saturating_sub(1)),
                scene_frames,
            )?;

            // Create parent directories
            if let Some(parent) = output.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent)?;
                }
            }

            let img = image::RgbaImage::from_raw(config.width, config.height, rgba)
                .ok_or(RustmotionError::PixelImage)?;

            let fmt = format
                .as_deref()
                .unwrap_or_else(|| output.extension().and_then(|e| e.to_str()).unwrap_or("png"));

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
