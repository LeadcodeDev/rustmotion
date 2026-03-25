use rayon::prelude::*;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::engine::prefetch_icons;
use crate::error::{Result, RustmotionError};
use crate::schema::ResolvedScenario as Scenario;
use crate::tui::TuiProgress;

use super::tasks::{build_frame_tasks, render_frame_task};

/// Encode frames as a PNG sequence (one PNG file per frame)
pub fn encode_png_sequence(scenario: &Scenario, output_dir: &str, quiet: bool, _transparent: bool) -> Result<()> {
    let config = &scenario.video;
    let width = config.width;
    let height = config.height;

    for view in &scenario.views {
        prefetch_icons(&view.scenes);
    }

    let tasks = build_frame_tasks(scenario);
    let total_frames = tasks.len() as u32;

    if total_frames == 0 {
        return Err(RustmotionError::NoFrames.into());
    }

    std::fs::create_dir_all(output_dir)?;

    let mut tui = if !quiet {
        Some(TuiProgress::new(total_frames, output_dir, width, height, config.fps, "png")?)
    } else {
        None
    };

    let batch_size = (rayon::current_num_threads() * 2).max(4);
    let counter = AtomicU32::new(0);

    for batch in tasks.chunks(batch_size) {
        let results: Vec<Result<(u32, Vec<u8>)>> = batch
            .par_iter()
            .map(|task| {
                let frame_num = counter.fetch_add(1, Ordering::Relaxed);
                let rgba = render_frame_task(config, scenario, task)?;
                Ok((frame_num, rgba))
            })
            .collect();

        if let Some(ref mut tui) = tui {
            tui.set_progress(counter.load(Ordering::Relaxed));
        }

        for result in results {
            let (frame_num, rgba) = result?;
            let path = format!("{}/frame_{:05}.png", output_dir, frame_num);
            let img = image::RgbaImage::from_raw(width, height, rgba)
                .ok_or(RustmotionError::PixelImage)?;
            img.save(&path)?;
        }
    }

    if let Some(tui) = tui {
        tui.finish("Done!");
    }

    Ok(())
}

/// Encode frames as an animated GIF
pub fn encode_gif(scenario: &Scenario, output_path: &str, quiet: bool) -> Result<()> {
    let config = &scenario.video;
    let width = config.width;
    let height = config.height;
    let fps = config.fps;

    for view in &scenario.views {
        prefetch_icons(&view.scenes);
    }

    let tasks = build_frame_tasks(scenario);
    let total_frames = tasks.len() as u32;

    if total_frames == 0 {
        return Err(RustmotionError::NoFrames.into());
    }

    let mut tui = if !quiet {
        Some(TuiProgress::new(total_frames, output_path, width, height, fps, "gif")?)
    } else {
        None
    };

    let gif_w = width.min(65535) as u16;
    let gif_h = height.min(65535) as u16;

    let file = File::create(output_path)?;
    let mut encoder = gif::Encoder::new(BufWriter::new(file), gif_w, gif_h, &[])
        .map_err(|e| RustmotionError::GifEncoder { reason: e.to_string() })?;

    encoder.set_repeat(gif::Repeat::Infinite)
        .map_err(|e| RustmotionError::GifRepeat { reason: e.to_string() })?;

    let delay = (100.0 / fps as f64).round() as u16;

    let batch_size = (rayon::current_num_threads() * 2).max(4);
    let counter = AtomicU32::new(0);

    for batch in tasks.chunks(batch_size) {
        let results: Vec<Result<Vec<u8>>> = batch
            .par_iter()
            .map(|task| {
                let rgba = render_frame_task(config, scenario, task)?;
                counter.fetch_add(1, Ordering::Relaxed);
                Ok(rgba)
            })
            .collect();

        if let Some(ref mut tui) = tui {
            tui.set_progress(counter.load(Ordering::Relaxed));
        }

        for result in results {
            let rgba = result?;
            let mut frame = gif::Frame::from_rgba_speed(gif_w, gif_h, &mut rgba.clone(), 10);
            frame.delay = delay;
            encoder.write_frame(&frame)
                .map_err(|e| RustmotionError::GifFrame { reason: e.to_string() })?;
        }
    }

    if let Some(tui) = tui {
        tui.finish("Done!");
    }

    Ok(())
}

/// Stream raw RGBA pixel data to stdout for piping to external tools
pub fn encode_raw_stdout(scenario: &Scenario, quiet: bool) -> Result<()> {
    let config = &scenario.video;
    let fps = config.fps;

    let mut stdout = std::io::stdout().lock();

    let mut frame_offset = 0u32;
    for view in &scenario.views {
        for scene in &view.scenes {
            let scene_frames = (scene.duration * fps as f64).round() as u32;

            for local_frame in 0..scene_frames {
                let rgba = crate::engine::render::render_scene_frame(
                    config, scene, local_frame, scene_frames,
                )?;
                stdout.write_all(&rgba)?;

                if !quiet {
                    let global_frame = frame_offset + local_frame;
                    eprint!("\rFrame {}", global_frame);
                }
            }
            frame_offset += scene_frames;
        }
    }

    if !quiet {
        eprintln!("\nDone: {} frames streamed to stdout", frame_offset);
    }

    Ok(())
}
