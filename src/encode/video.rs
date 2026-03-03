use anyhow::Result;
use minimp4::Mp4Muxer;
use openh264::encoder::{Encoder, EncoderConfig};
use openh264::formats::YUVBuffer;
use openh264::OpenH264API;
use rayon::prelude::*;
use std::fs::File;
use std::io::BufWriter;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::engine::transition::apply_transition;
use crate::engine::{render_frame, rgba_to_yuv420};
use crate::schema::{Scene, Scenario, TransitionType, VideoConfig};
use crate::tui::TuiProgress;

/// Description of what to render for a specific frame
#[derive(Clone)]
enum FrameTask {
    Normal {
        scene_idx: usize,
        frame_in_scene: u32,
        scene_total_frames: u32,
    },
    Transition {
        scene_a_idx: usize,
        scene_b_idx: usize,
        frame_in_transition: u32,
        scene_a_frame_offset: u32,
        scene_a_total_frames: u32,
        scene_b_total_frames: u32,
        transition_type: TransitionType,
        transition_duration: f64,
    },
}

pub fn encode_video(scenario: &Scenario, output_path: &str, quiet: bool) -> Result<()> {
    let config = &scenario.video;
    let width = config.width;
    let height = config.height;
    let fps = config.fps;

    // Build a flat list of frame tasks
    let tasks = build_frame_tasks(scenario);
    let total_frames = tasks.len() as u32;

    if total_frames == 0 {
        anyhow::bail!("No frames to render (total duration is 0)");
    }

    let mut tui = if !quiet {
        Some(TuiProgress::new(total_frames, output_path, width, height, fps, "h264")?)
    } else {
        None
    };

    // Render frames in parallel batches, then encode sequentially
    let batch_size = (rayon::current_num_threads() * 2).max(4);
    let counter = AtomicU32::new(0);

    let api = OpenH264API::from_source();
    let pixels = (width * height) as u32;
    let target_bitrate = (pixels as f64 * fps as f64 * 0.3) as u32;
    let encoder_config = EncoderConfig::new()
        .set_bitrate_bps(target_bitrate.max(10_000_000))
        .max_frame_rate(fps as f32);
    let mut encoder = Encoder::with_api_config(api, encoder_config)?;
    let mut h264_data: Vec<u8> = Vec::new();

    for batch in tasks.chunks(batch_size) {
        // Render batch in parallel → Vec<YUV bytes>
        let yuv_frames: Vec<Result<Vec<u8>>> = batch
            .par_iter()
            .map(|task| {
                let rgba = render_frame_task(config, &scenario.scenes, task)?;
                let yuv = rgba_to_yuv420(&rgba, width, height);
                counter.fetch_add(1, Ordering::Relaxed);
                Ok(yuv)
            })
            .collect();

        if let Some(ref mut tui) = tui {
            tui.set_progress(counter.load(Ordering::Relaxed));
        }

        // Encode sequentially (H.264 requires frame order)
        for yuv_result in yuv_frames {
            let yuv = yuv_result?;

            // Force every frame as I-frame to prevent inter-frame artifacts
            encoder.force_intra_frame();

            let yuv_buf = YUVBuffer::from_vec(yuv, width as usize, height as usize);
            let bitstream = encoder.encode(&yuv_buf)?;
            h264_data.extend_from_slice(&bitstream.to_vec());
        }
    }

    // Process audio
    let total_duration = total_frames as f64 / fps as f64;
    let pcm_data = if !scenario.audio.is_empty() {
        if let Some(ref mut tui) = tui {
            tui.set_status("Processing audio");
        }
        super::audio::mix_audio_tracks(&scenario.audio, total_duration)?
    } else {
        None
    };

    // Mux
    if let Some(ref mut tui) = tui {
        tui.set_status("Muxing to MP4");
    }
    let file = File::create(output_path)?;
    let writer = BufWriter::new(file);
    let mut muxer = Mp4Muxer::new(writer);
    muxer.init_video(width as i32, height as i32, false, "rustmotion");
    if let Some(ref pcm) = pcm_data {
        muxer.init_audio(128000, 44100, 2);
        muxer.write_video_with_audio(&h264_data, fps, pcm);
    } else {
        muxer.write_video_with_fps(&h264_data, fps);
    }
    muxer.close();

    if let Some(tui) = tui {
        tui.finish("Done!");
    }
    Ok(())
}

fn render_frame_task(config: &VideoConfig, scenes: &[Scene], task: &FrameTask) -> Result<Vec<u8>> {
    match task {
        FrameTask::Normal {
            scene_idx,
            frame_in_scene,
            scene_total_frames,
        } => render_frame(config, &scenes[*scene_idx], *frame_in_scene, *scene_total_frames),
        FrameTask::Transition {
            scene_a_idx,
            scene_b_idx,
            frame_in_transition,
            scene_a_frame_offset,
            scene_a_total_frames,
            scene_b_total_frames,
            transition_type,
            transition_duration,
        } => {
            let frame_a = render_frame(
                config,
                &scenes[*scene_a_idx],
                scene_a_frame_offset + frame_in_transition,
                *scene_a_total_frames,
            )?;
            let frame_b = render_frame(
                config,
                &scenes[*scene_b_idx],
                *frame_in_transition,
                *scene_b_total_frames,
            )?;

            let fps = config.fps;
            let progress = *frame_in_transition as f64 / (transition_duration * fps as f64);
            Ok(apply_transition(
                &frame_a,
                &frame_b,
                config.width,
                config.height,
                progress,
                transition_type,
            ))
        }
    }
}

fn build_frame_tasks(scenario: &Scenario) -> Vec<FrameTask> {
    let fps = scenario.video.fps;
    let scenes = &scenario.scenes;
    let mut tasks = Vec::new();

    for (i, scene) in scenes.iter().enumerate() {
        let scene_frames = (scene.duration * fps as f64).round() as u32;
        let next_transition = scenes.get(i + 1).and_then(|s| s.transition.as_ref());
        let outgoing_transition_frames = next_transition
            .map(|t| (t.duration * fps as f64).round() as u32)
            .unwrap_or(0);

        // Skip frames already rendered by the incoming transition from the previous scene
        let incoming_transition_frames = if i > 0 {
            scene
                .transition
                .as_ref()
                .map(|t| (t.duration * fps as f64).round() as u32)
                .unwrap_or(0)
        } else {
            0
        };

        let normal_start = incoming_transition_frames;
        let normal_end = scene_frames.saturating_sub(outgoing_transition_frames);

        // Normal frames
        for f in normal_start..normal_end {
            tasks.push(FrameTask::Normal {
                scene_idx: i,
                frame_in_scene: f,
                scene_total_frames: scene_frames,
            });
        }

        // Transition frames
        if let Some(transition) = next_transition {
            let actual_transition_frames = outgoing_transition_frames.min(scene_frames);
            let scene_b_frames = (scenes[i + 1].duration * fps as f64).round() as u32;
            for f in 0..actual_transition_frames {
                tasks.push(FrameTask::Transition {
                    scene_a_idx: i,
                    scene_b_idx: i + 1,
                    frame_in_transition: f,
                    scene_a_frame_offset: scene_frames - actual_transition_frames,
                    scene_a_total_frames: scene_frames,
                    scene_b_total_frames: scene_b_frames,
                    transition_type: transition.transition_type.clone(),
                    transition_duration: transition.duration,
                });
            }
        }
    }

    tasks
}

/// Encode frames as a PNG sequence (one PNG file per frame)
pub fn encode_png_sequence(scenario: &Scenario, output_dir: &str, quiet: bool, _transparent: bool) -> Result<()> {
    let config = &scenario.video;
    let width = config.width;
    let height = config.height;

    let tasks = build_frame_tasks(scenario);
    let total_frames = tasks.len() as u32;

    if total_frames == 0 {
        anyhow::bail!("No frames to render");
    }

    // Create output directory
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
                let rgba = render_frame_task(config, &scenario.scenes, task)?;
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
                .ok_or_else(|| anyhow::anyhow!("Failed to create image"))?;
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

    let tasks = build_frame_tasks(scenario);
    let total_frames = tasks.len() as u32;

    if total_frames == 0 {
        anyhow::bail!("No frames to render");
    }

    let mut tui = if !quiet {
        Some(TuiProgress::new(total_frames, output_path, width, height, fps, "gif")?)
    } else {
        None
    };

    // GIF requires width/height to fit in u16
    let gif_w = width.min(65535) as u16;
    let gif_h = height.min(65535) as u16;

    let file = File::create(output_path)?;
    let mut encoder = gif::Encoder::new(BufWriter::new(file), gif_w, gif_h, &[])
        .map_err(|e| anyhow::anyhow!("Failed to create GIF encoder: {}", e))?;

    encoder.set_repeat(gif::Repeat::Infinite)
        .map_err(|e| anyhow::anyhow!("Failed to set GIF repeat: {}", e))?;

    let delay = (100.0 / fps as f64).round() as u16; // GIF delay in 1/100 seconds

    let batch_size = (rayon::current_num_threads() * 2).max(4);
    let counter = AtomicU32::new(0);

    for batch in tasks.chunks(batch_size) {
        let results: Vec<Result<Vec<u8>>> = batch
            .par_iter()
            .map(|task| {
                let rgba = render_frame_task(config, &scenario.scenes, task)?;
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
                .map_err(|e| anyhow::anyhow!("Failed to write GIF frame: {}", e))?;
        }
    }

    if let Some(tui) = tui {
        tui.finish("Done!");
    }

    Ok(())
}

/// Encode using FFmpeg subprocess (for h265, vp9, prores, webm, mov, transparency)
pub fn encode_with_ffmpeg(
    scenario: &Scenario,
    output_path: &str,
    quiet: bool,
    codec: &str,
    crf: Option<u8>,
    transparent: bool,
) -> Result<()> {
    let config = &scenario.video;
    let width = config.width;
    let height = config.height;
    let fps = config.fps;

    let tasks = build_frame_tasks(scenario);
    let total_frames = tasks.len() as u32;

    if total_frames == 0 {
        anyhow::bail!("No frames to render");
    }

    let mut tui = if !quiet {
        Some(TuiProgress::new(total_frames, output_path, width, height, fps, codec)?)
    } else {
        None
    };

    // First render to a temporary PNG sequence
    let tmp_dir = std::env::temp_dir().join(format!("rustmotion_{}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir)?;

    let batch_size = (rayon::current_num_threads() * 2).max(4);
    let counter = AtomicU32::new(0);

    for batch in tasks.chunks(batch_size) {
        let results: Vec<Result<(u32, Vec<u8>)>> = batch
            .par_iter()
            .map(|task| {
                let frame_num = counter.fetch_add(1, Ordering::Relaxed);
                let rgba = render_frame_task(config, &scenario.scenes, task)?;
                Ok((frame_num, rgba))
            })
            .collect();

        if let Some(ref mut tui) = tui {
            tui.set_progress(counter.load(Ordering::Relaxed));
        }

        for result in results {
            let (frame_num, rgba) = result?;
            let path = tmp_dir.join(format!("frame_{:05}.png", frame_num));
            let img = image::RgbaImage::from_raw(width, height, rgba)
                .ok_or_else(|| anyhow::anyhow!("Failed to create image"))?;
            img.save(&path)?;
        }
    }

    if let Some(ref mut tui) = tui {
        tui.set_status("Encoding with FFmpeg");
    }

    // Build FFmpeg command
    let input_pattern = tmp_dir.join("frame_%05d.png");
    let crf_val = crf.unwrap_or(23);

    let mut cmd = std::process::Command::new("ffmpeg");
    cmd.args(["-y", "-framerate", &fps.to_string(), "-i", input_pattern.to_str().unwrap()]);

    match codec {
        "h265" | "hevc" => {
            cmd.args(["-c:v", "libx265", "-crf", &crf_val.to_string(), "-preset", "medium"]);
            if transparent {
                cmd.args(["-pix_fmt", "yuva420p"]);
            } else {
                cmd.args(["-pix_fmt", "yuv420p"]);
            }
        }
        "vp9" => {
            cmd.args(["-c:v", "libvpx-vp9", "-crf", &crf_val.to_string(), "-b:v", "0"]);
            if transparent {
                cmd.args(["-pix_fmt", "yuva420p"]);
            } else {
                cmd.args(["-pix_fmt", "yuv420p"]);
            }
        }
        "prores" => {
            cmd.args(["-c:v", "prores_ks", "-profile:v", "4"]);
            if transparent {
                cmd.args(["-pix_fmt", "yuva444p10le"]);
            } else {
                cmd.args(["-pix_fmt", "yuv422p10le"]);
            }
        }
        _ => {
            // h264
            cmd.args(["-c:v", "libx264", "-crf", &crf_val.to_string(), "-preset", "medium", "-pix_fmt", "yuv420p"]);
        }
    }

    // Process audio if present
    let total_duration = total_frames as f64 / fps as f64;
    if !scenario.audio.is_empty() {
        if let Some(pcm_data) = super::audio::mix_audio_tracks(&scenario.audio, total_duration)? {
            let audio_path = tmp_dir.join("audio.raw");
            std::fs::write(&audio_path, &pcm_data)?;
            cmd.args([
                "-f", "s16le", "-ar", "44100", "-ac", "2", "-i",
                audio_path.to_str().unwrap(),
                "-c:a", "aac", "-b:a", "128k",
            ]);
        }
    }

    cmd.arg(output_path);

    let output = cmd
        .stdout(std::process::Stdio::null())
        .stderr(if quiet { std::process::Stdio::null() } else { std::process::Stdio::inherit() })
        .status()
        .map_err(|e| anyhow::anyhow!("Failed to run ffmpeg: {}. Is ffmpeg installed?", e))?;

    // Cleanup temp directory
    let _ = std::fs::remove_dir_all(&tmp_dir);

    if !output.success() {
        anyhow::bail!("FFmpeg encoding failed");
    }

    if let Some(tui) = tui {
        tui.finish("Done!");
    }

    Ok(())
}
