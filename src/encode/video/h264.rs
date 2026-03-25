use openh264::encoder::{Encoder, EncoderConfig};
use openh264::formats::YUVBuffer;
use openh264::OpenH264API;
use rayon::prelude::*;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::engine::{rgba_to_yuv420, preextract_video_frames, prefetch_icons};
use crate::error::{Result, RustmotionError};
use crate::schema::ResolvedScenario as Scenario;
use crate::tui::TuiProgress;

use super::mux::mux_h264_to_mp4;
use super::tasks::{build_frame_tasks, build_scene_frame_tasks, hash_scene, render_frame_task, SceneSegment};
use super::EncodeProgress;

/// Create an OpenH264 encoder with standard settings for the given video dimensions.
fn create_encoder(width: u32, height: u32, fps: u32) -> Result<Encoder> {
    let api = OpenH264API::from_source();
    let pixels = (width * height) as u32;
    let target_bitrate = (pixels as f64 * fps as f64 * 0.1) as u32;
    let config = EncoderConfig::new()
        .set_bitrate_bps(target_bitrate.max(3_000_000))
        .max_frame_rate(fps as f32);
    Ok(Encoder::with_api_config(api, config)?)
}

pub fn encode_video(scenario: &Scenario, output_path: &str, quiet: bool) -> Result<()> {
    encode_video_with_progress(scenario, output_path, quiet, None)
}

pub fn encode_video_with_progress(
    scenario: &Scenario,
    output_path: &str,
    quiet: bool,
    on_progress: Option<&(dyn Fn(EncodeProgress) + Send + Sync)>,
) -> Result<()> {
    let config = &scenario.video;
    let width = config.width;
    let height = config.height;
    let fps = config.fps;

    for view in &scenario.views {
        preextract_video_frames(&view.scenes, fps);
        prefetch_icons(&view.scenes);
    }

    let tasks = build_frame_tasks(scenario);
    let total_frames = tasks.len() as u32;

    if total_frames == 0 {
        return Err(RustmotionError::NoFrames.into());
    }

    let mut tui = if !quiet {
        Some(TuiProgress::new(total_frames, output_path, width, height, fps, "h264")?)
    } else {
        None
    };

    let batch_size = (rayon::current_num_threads() * 2).max(4);
    let counter = AtomicU32::new(0);

    let mut encoder = create_encoder(width, height, fps)?;
    let mut h264_data: Vec<u8> = Vec::new();

    for batch in tasks.chunks(batch_size) {
        let yuv_frames: Vec<Result<Vec<u8>>> = batch
            .par_iter()
            .map(|task| {
                let rgba = render_frame_task(config, scenario, task)?;
                let yuv = rgba_to_yuv420(&rgba, width, height);
                counter.fetch_add(1, Ordering::Relaxed);
                Ok(yuv)
            })
            .collect();

        let rendered = counter.load(Ordering::Relaxed);
        if let Some(ref mut tui) = tui {
            tui.set_progress(rendered);
        }
        if let Some(cb) = &on_progress {
            cb(EncodeProgress::Rendering(rendered, total_frames));
        }

        for yuv_result in yuv_frames {
            let yuv = yuv_result?;
            encoder.force_intra_frame();
            let yuv_buf = YUVBuffer::from_vec(yuv, width as usize, height as usize);
            let bitstream = encoder.encode(&yuv_buf)?;
            bitstream.write_vec(&mut h264_data);
        }
    }

    // Process audio
    if let Some(ref mut tui) = tui {
        tui.set_status("Processing audio");
    }

    if let Some(ref mut tui) = tui {
        tui.set_status("Muxing to MP4");
    }
    if let Some(cb) = &on_progress {
        cb(EncodeProgress::Muxing);
    }

    let total_duration = total_frames as f64 / fps as f64;
    mux_h264_to_mp4(&h264_data, output_path, width, height, fps, scenario, total_duration)?;

    if let Some(tui) = tui {
        tui.finish("Done!");
    }
    Ok(())
}

/// Encode video incrementally, reusing cached segments for unchanged scenes.
pub fn encode_video_incremental(
    scenario: &Scenario,
    output_path: &str,
    quiet: bool,
    prev_segments: Option<&[SceneSegment]>,
    mut on_progress: Option<&mut dyn FnMut(EncodeProgress)>,
) -> Result<Vec<SceneSegment>> {
    let config = &scenario.video;
    let width = config.width;
    let height = config.height;
    let fps = config.fps;

    for view in &scenario.views {
        preextract_video_frames(&view.scenes, fps);
        prefetch_icons(&view.scenes);
    }

    let num_scenes = scenario.views.get(0).map(|v| v.scenes.len()).unwrap_or(0);

    let scene_hashes: Vec<u64> = scenario.views.get(0)
        .map(|v| v.scenes.iter().map(hash_scene).collect())
        .unwrap_or_default();

    // Determine which scenes need re-rendering
    let mut needs_render = vec![true; num_scenes];
    if let Some(prev) = prev_segments {
        if prev.len() == num_scenes {
            let scenes = &scenario.views[0].scenes;
            for i in 0..num_scenes {
                let hash_changed = scene_hashes[i] != prev[i].scene_hash;
                let next_changed_with_transition = if i + 1 < num_scenes {
                    scene_hashes[i + 1] != prev[i + 1].scene_hash
                        && scenes[i + 1].transition.is_some()
                } else {
                    false
                };
                needs_render[i] = hash_changed || next_changed_with_transition;
            }
        }
    }

    let scenes_to_render: usize = needs_render.iter().filter(|&&r| r).count();

    if !quiet && on_progress.is_none() {
        eprintln!("Re-rendering {}/{} scenes...", scenes_to_render, num_scenes);
    }

    // Build per-scene tasks
    let scene_tasks: Vec<Vec<super::tasks::FrameTask>> = (0..num_scenes)
        .map(|i| build_scene_frame_tasks(scenario, i))
        .collect();

    let total_frames: u32 = scene_tasks.iter().map(|t| t.len() as u32).sum();

    if total_frames == 0 {
        return Err(RustmotionError::NoFrames.into());
    }

    // Flatten tasks that need rendering
    let mut flat_tasks: Vec<(usize, &super::tasks::FrameTask)> = Vec::new();
    let mut scene_frame_counts: Vec<(usize, u32)> = Vec::new();
    for i in 0..num_scenes {
        if needs_render[i] {
            let tasks = &scene_tasks[i];
            scene_frame_counts.push((i, tasks.len() as u32));
            for task in tasks {
                flat_tasks.push((i, task));
            }
        }
    }

    let frames_to_render = flat_tasks.len() as u32;

    // Render in parallel batches
    let batch_size = (rayon::current_num_threads() * 2).max(4);
    let counter = AtomicU32::new(0);
    let mut all_yuv: Vec<Result<Vec<u8>>> = Vec::with_capacity(flat_tasks.len());

    for batch in flat_tasks.chunks(batch_size) {
        let batch_results: Vec<Result<Vec<u8>>> = batch
            .par_iter()
            .map(|(_, task)| {
                let rgba = render_frame_task(config, scenario, task)?;
                let yuv = rgba_to_yuv420(&rgba, width, height);
                counter.fetch_add(1, Ordering::Relaxed);
                Ok(yuv)
            })
            .collect();

        if let Some(ref mut cb) = on_progress {
            cb(EncodeProgress::Rendering(counter.load(Ordering::Relaxed), frames_to_render));
        }

        all_yuv.extend(batch_results);
    }

    // Encode phase
    if let Some(ref mut cb) = on_progress {
        cb(EncodeProgress::Encoding(0, frames_to_render));
    }

    let mut encoder = create_encoder(width, height, fps)?;
    let mut yuv_iter = all_yuv.into_iter();
    let mut rendered_segments: std::collections::HashMap<usize, Vec<u8>> = std::collections::HashMap::new();
    let mut encoded_count: u32 = 0;

    for &(scene_idx, frame_count) in &scene_frame_counts {
        let mut segment_h264: Vec<u8> = Vec::new();

        for _ in 0..frame_count {
            let yuv = yuv_iter.next().unwrap()?;
            encoder.force_intra_frame();
            let yuv_buf = YUVBuffer::from_vec(yuv, width as usize, height as usize);
            let bitstream = encoder.encode(&yuv_buf)?;
            bitstream.write_vec(&mut segment_h264);
            encoded_count += 1;
            if let Some(ref mut cb) = on_progress {
                cb(EncodeProgress::Encoding(encoded_count, frames_to_render));
            }
        }

        rendered_segments.insert(scene_idx, segment_h264);
    }

    // Assemble final segments
    let mut new_segments = Vec::with_capacity(num_scenes);
    for i in 0..num_scenes {
        if let Some(h264_data) = rendered_segments.remove(&i) {
            new_segments.push(SceneSegment {
                h264_data,
                scene_hash: scene_hashes[i],
            });
        } else if let Some(prev) = prev_segments {
            new_segments.push(SceneSegment {
                h264_data: prev[i].h264_data.clone(),
                scene_hash: prev[i].scene_hash,
            });
        }
    }

    // Concatenate and mux
    let total_h264_size: usize = new_segments.iter().map(|s| s.h264_data.len()).sum();
    let mut h264_data: Vec<u8> = Vec::with_capacity(total_h264_size);
    for seg in &new_segments {
        h264_data.extend_from_slice(&seg.h264_data);
    }

    if let Some(ref mut cb) = on_progress {
        cb(EncodeProgress::Muxing);
    }

    let total_duration = total_frames as f64 / fps as f64;
    mux_h264_to_mp4(&h264_data, output_path, width, height, fps, scenario, total_duration)?;

    if !quiet && on_progress.is_none() {
        eprintln!("Done!");
    }

    Ok(new_segments)
}
