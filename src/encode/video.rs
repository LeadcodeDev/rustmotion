use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
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

    if !quiet {
        eprintln!(
            "Rendering {}x{} @ {}fps — {} frames ({:.1}s) [{}]",
            width, height, fps, total_frames,
            total_frames as f64 / fps as f64,
            format!("{} threads", rayon::current_num_threads()),
        );
    }

    let pb = if !quiet {
        let pb = ProgressBar::new(total_frames as u64);
        pb.set_style(
            ProgressStyle::with_template("  {bar:40.cyan/blue} {pos}/{len} frames ({eta} remaining)")
                .unwrap()
                .progress_chars("##-"),
        );
        Some(pb)
    } else {
        None
    };

    // Render frames in parallel batches, then encode sequentially
    let batch_size = (rayon::current_num_threads() * 2).max(4);
    let counter = AtomicU32::new(0);

    let api = OpenH264API::from_source();
    let mut encoder = Encoder::with_api_config(api, EncoderConfig::new())?;
    let mut h264_data: Vec<u8> = Vec::new();

    for batch in tasks.chunks(batch_size) {
        // Render batch in parallel → Vec<YUV bytes>
        let yuv_frames: Vec<Result<Vec<u8>>> = batch
            .par_iter()
            .map(|task| {
                let rgba = render_frame_task(config, &scenario.scenes, task)?;
                let yuv = rgba_to_yuv420(&rgba, width, height);

                let count = counter.fetch_add(1, Ordering::Relaxed) + 1;
                if let Some(ref pb) = pb {
                    pb.set_position(count as u64);
                }

                Ok(yuv)
            })
            .collect();

        // Encode sequentially (H.264 requires frame order)
        for yuv_result in yuv_frames {
            let yuv = yuv_result?;
            let yuv_buf = YUVBuffer::from_vec(yuv, width as usize, height as usize);
            let bitstream = encoder.encode(&yuv_buf)?;
            h264_data.extend_from_slice(&bitstream.to_vec());
        }
    }

    if let Some(ref pb) = pb {
        pb.finish_and_clear();
    }

    // Process audio
    let total_duration = total_frames as f64 / fps as f64;
    let pcm_data = if !scenario.audio.is_empty() {
        if !quiet {
            eprintln!("Processing audio...");
        }
        super::audio::mix_audio_tracks(&scenario.audio, total_duration)?
    } else {
        None
    };

    // Mux
    if !quiet {
        eprintln!("Muxing to MP4: {}", output_path);
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

    if !quiet {
        eprintln!("Done! Output: {}", output_path);
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
