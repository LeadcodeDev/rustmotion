use anyhow::Result;
use minimp4::Mp4Muxer;
use openh264::encoder::{Encoder, EncoderConfig};
use openh264::formats::YUVBuffer;
use openh264::OpenH264API;
use rayon::prelude::*;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::engine::transition::{apply_transition, camera_pan_transition};
use crate::engine::{rgba_to_yuv420, preextract_video_frames, prefetch_icons};
use crate::error::RustmotionError;
use crate::schema::{EasingType, ResolvedView, Scene, ResolvedScenario as Scenario, TransitionType, VideoConfig, ViewType};
use crate::tui::TuiProgress;

/// Progress events emitted by encode_video_incremental
pub enum EncodeProgress {
    /// Rendering frames: (current, total)
    Rendering(u32, u32),
    /// Encoding H.264: (current, total)
    Encoding(u32, u32),
    /// Muxing MP4
    Muxing,
}

/// Description of what to render for a specific frame
#[derive(Clone)]
#[allow(dead_code)]
pub enum FrameTask {
    Normal {
        view_idx: usize,
        scene_idx: usize,
        frame_in_scene: u32,
        scene_total_frames: u32,
    },
    SlideTransition {
        view_idx: usize,
        scene_a_idx: usize,
        scene_b_idx: usize,
        frame_in_transition: u32,
        scene_a_frame_offset: u32,
        scene_a_total_frames: u32,
        scene_b_total_frames: u32,
        transition_type: TransitionType,
        transition_duration: f64,
        easing: EasingType,
    },
    WorldFrame {
        view_idx: usize,
        frame_in_view: u32,
        view_total_frames: u32,
    },
    ViewTransition {
        view_a_idx: usize,
        view_b_idx: usize,
        frame_in_transition: u32,
        transition_type: TransitionType,
        transition_duration: f64,
        easing: EasingType,
    },
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

    // Pre-extract video frames in bulk (if any VideoLayer present)
    for view in &scenario.views {
        preextract_video_frames(&view.scenes, fps);
        prefetch_icons(&view.scenes);
    }

    // Build a flat list of frame tasks
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

    // Render frames in parallel batches, then encode sequentially
    let batch_size = (rayon::current_num_threads() * 2).max(4);
    let counter = AtomicU32::new(0);

    let api = OpenH264API::from_source();
    let pixels = (width * height) as u32;
    let target_bitrate = (pixels as f64 * fps as f64 * 0.1) as u32;
    let encoder_config = EncoderConfig::new()
        .set_bitrate_bps(target_bitrate.max(3_000_000))
        .max_frame_rate(fps as f32);
    let mut encoder = Encoder::with_api_config(api, encoder_config)?;
    let mut h264_data: Vec<u8> = Vec::new();

    for batch in tasks.chunks(batch_size) {
        // Render batch in parallel → Vec<YUV bytes>
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

        // Encode sequentially (H.264 requires frame order)
        for yuv_result in yuv_frames {
            let yuv = yuv_result?;

            // Force every frame as I-frame to prevent inter-frame prediction
            // artifacts (ghost pixels at moving edges like growing codeblocks)
            encoder.force_intra_frame();

            let yuv_buf = YUVBuffer::from_vec(yuv, width as usize, height as usize);
            let bitstream = encoder.encode(&yuv_buf)?;
            bitstream.write_vec(&mut h264_data);
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
    if let Some(cb) = &on_progress {
        cb(EncodeProgress::Muxing);
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

pub fn render_frame_task(config: &VideoConfig, scenario: &Scenario, task: &FrameTask) -> Result<Vec<u8>> {
    render_frame_task_scaled(config, scenario, task, 1.0)
}

pub fn render_frame_task_scaled(
    config: &VideoConfig,
    scenario: &Scenario,
    task: &FrameTask,
    scale_factor: f32,
) -> Result<Vec<u8>> {
    use crate::engine::render_v2::{render_scene_frame, render_scene_frame_scaled, render_scene_frame_scaled_with_prev_bg, render_scene_bg_scaled, render_scene_fg_scaled};

    match task {
        FrameTask::Normal {
            view_idx,
            scene_idx,
            frame_in_scene,
            scene_total_frames,
        } => {
            let view = &scenario.views[*view_idx];
            let scene = &view.scenes[*scene_idx];
            let prev_bg = if *scene_idx > 0 {
                let prev = &view.scenes[*scene_idx - 1];
                Some((&prev.resolved_background, prev.duration))
            } else {
                None
            };
            render_scene_frame_scaled_with_prev_bg(config, scene, *frame_in_scene, *scene_total_frames, scale_factor, prev_bg)
        }
        FrameTask::SlideTransition {
            view_idx,
            scene_a_idx,
            scene_b_idx,
            frame_in_transition,
            scene_a_frame_offset,
            scene_a_total_frames,
            scene_b_total_frames,
            transition_type,
            transition_duration,
            easing,
        } => {
            let scenes = &scenario.views[*view_idx].scenes;
            let scaled_w = (config.width as f32 * scale_factor) as u32;
            let scaled_h = (config.height as f32 * scale_factor) as u32;
            let fps = config.fps;
            let progress = *frame_in_transition as f64 / (transition_duration * fps as f64);
            let frame_a_idx = scene_a_frame_offset + frame_in_transition;

            // Camera pan: static bg + sliding foreground children
            if matches!(transition_type, TransitionType::CameraPan) {
                let (ax, ay) = scenes[*scene_a_idx].world_position.as_ref().map(|p| (p.x, p.y)).unwrap_or((0.0, 0.0));
                let (bx, by) = scenes[*scene_b_idx].world_position.as_ref().map(|p| (p.x, p.y)).unwrap_or((0.0, 0.0));
                let dx = bx - ax;
                let dy = by - ay;
                let bg = render_scene_bg_scaled(config, &scenes[*scene_a_idx], frame_a_idx, scale_factor)?;
                let fg_a = render_scene_fg_scaled(config, &scenes[*scene_a_idx], frame_a_idx, *scene_a_total_frames, scale_factor)?;
                let fg_b = render_scene_fg_scaled(config, &scenes[*scene_b_idx], *frame_in_transition, *scene_b_total_frames, scale_factor)?;
                return Ok(camera_pan_transition(
                    &bg, &fg_a, &fg_b,
                    scaled_w, scaled_h,
                    progress,
                    dx * scale_factor, dy * scale_factor,
                    easing,
                ));
            }

            // Other transitions: render full frames
            let (frame_a, frame_b) = if scale_factor == 1.0 {
                let a = render_scene_frame(config, &scenes[*scene_a_idx], frame_a_idx, *scene_a_total_frames)?;
                let b = render_scene_frame(config, &scenes[*scene_b_idx], *frame_in_transition, *scene_b_total_frames)?;
                (a, b)
            } else {
                let a = render_scene_frame_scaled(config, &scenes[*scene_a_idx], frame_a_idx, *scene_a_total_frames, scale_factor)?;
                let b = render_scene_frame_scaled(config, &scenes[*scene_b_idx], *frame_in_transition, *scene_b_total_frames, scale_factor)?;
                (a, b)
            };

            Ok(apply_transition(
                &frame_a,
                &frame_b,
                scaled_w,
                scaled_h,
                progress,
                transition_type,
            ))
        }
        FrameTask::WorldFrame {
            view_idx,
            frame_in_view,
            view_total_frames: _,
        } => {
            use crate::engine::world::WorldTimeline;
            let view = &scenario.views[*view_idx];
            let timeline = WorldTimeline::build(view, config.fps, config.width, config.height);
            crate::engine::render_v2::render_world_frame_scaled(
                config, view, &timeline, *frame_in_view, scale_factor,
            )
        }
        FrameTask::ViewTransition {
            view_a_idx,
            view_b_idx,
            frame_in_transition,
            transition_type,
            transition_duration,
            easing: _,
        } => {
            let scaled_w = (config.width as f32 * scale_factor) as u32;
            let scaled_h = (config.height as f32 * scale_factor) as u32;
            let fps = config.fps;
            let progress = *frame_in_transition as f64 / (transition_duration * fps as f64);

            // Render last frame of view_a and first frame of view_b
            let view_a = &scenario.views[*view_a_idx];
            let view_b = &scenario.views[*view_b_idx];

            let frame_a = render_last_frame_of_view(config, view_a, fps, scale_factor)?;
            let frame_b = render_first_frame_of_view(config, view_b, fps, scale_factor)?;

            Ok(apply_transition(
                &frame_a,
                &frame_b,
                scaled_w,
                scaled_h,
                progress,
                transition_type,
            ))
        }
    }
}

/// Render the last frame of a view (used for ViewTransition)
fn render_last_frame_of_view(
    config: &VideoConfig,
    view: &ResolvedView,
    fps: u32,
    scale_factor: f32,
) -> Result<Vec<u8>> {
    use crate::engine::render_v2::render_scene_frame_scaled;
    match view.view_type {
        ViewType::Slide => {
            if let Some(last_scene) = view.scenes.last() {
                let scene_frames = (last_scene.duration * fps as f64).round() as u32;
                render_scene_frame_scaled(config, last_scene, scene_frames.saturating_sub(1), scene_frames, scale_factor)
            } else {
                Ok(vec![0u8; (config.width as f32 * scale_factor) as usize * (config.height as f32 * scale_factor) as usize * 4])
            }
        }
        ViewType::World => {
            let timeline = crate::engine::world::WorldTimeline::build(view, fps, config.width, config.height);
            let total_frames = timeline.total_frames(fps);
            crate::engine::render_v2::render_world_frame_scaled(
                config, view, &timeline, total_frames.saturating_sub(1), scale_factor,
            )
        }
    }
}

/// Render the first frame of a view (used for ViewTransition)
fn render_first_frame_of_view(
    config: &VideoConfig,
    view: &ResolvedView,
    fps: u32,
    scale_factor: f32,
) -> Result<Vec<u8>> {
    use crate::engine::render_v2::render_scene_frame_scaled;
    match view.view_type {
        ViewType::Slide => {
            if let Some(first_scene) = view.scenes.first() {
                let scene_frames = (first_scene.duration * fps as f64).round() as u32;
                render_scene_frame_scaled(config, first_scene, 0, scene_frames, scale_factor)
            } else {
                Ok(vec![0u8; (config.width as f32 * scale_factor) as usize * (config.height as f32 * scale_factor) as usize * 4])
            }
        }
        ViewType::World => {
            let timeline = crate::engine::world::WorldTimeline::build(view, fps, config.width, config.height);
            crate::engine::render_v2::render_world_frame_scaled(
                config, view, &timeline, 0, scale_factor,
            )
        }
    }
}

pub fn build_frame_tasks(scenario: &Scenario) -> Vec<FrameTask> {
    let fps = scenario.video.fps;
    let mut tasks = Vec::new();

    for (view_idx, view) in scenario.views.iter().enumerate() {
        // Inter-view transition: if this view has a transition and it's not the first view
        if view_idx > 0 {
            if let Some(ref transition) = view.transition {
                let transition_frames = (transition.duration * fps as f64).round() as u32;
                for f in 0..transition_frames {
                    tasks.push(FrameTask::ViewTransition {
                        view_a_idx: view_idx - 1,
                        view_b_idx: view_idx,
                        frame_in_transition: f,
                        transition_type: transition.transition_type.clone(),
                        transition_duration: transition.duration,
                        easing: transition.easing.clone(),
                    });
                }
            }
        }

        match view.view_type {
            ViewType::Slide => build_slide_view_tasks(&mut tasks, view_idx, view, fps),
            ViewType::World => build_world_view_tasks(&mut tasks, view_idx, view, fps, scenario.video.width, scenario.video.height),
        }
    }

    tasks
}

fn build_slide_view_tasks(tasks: &mut Vec<FrameTask>, view_idx: usize, view: &ResolvedView, fps: u32) {
    let scenes = &view.scenes;

    for (i, scene) in scenes.iter().enumerate() {
        let scene_frames = (scene.duration * fps as f64).round() as u32;
        let next_transition = scenes.get(i + 1).and_then(|s| s.transition.as_ref());
        let outgoing_transition_frames = next_transition
            .map(|t| (t.duration * fps as f64).round() as u32)
            .unwrap_or(0);

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

        for f in normal_start..normal_end {
            tasks.push(FrameTask::Normal {
                view_idx,
                scene_idx: i,
                frame_in_scene: f,
                scene_total_frames: scene_frames,
            });
        }

        if let Some(transition) = next_transition {
            let actual_transition_frames = outgoing_transition_frames.min(scene_frames);
            let scene_b_frames = (scenes[i + 1].duration * fps as f64).round() as u32;
            let easing = transition.easing.clone();
            for f in 0..actual_transition_frames {
                tasks.push(FrameTask::SlideTransition {
                    view_idx,
                    scene_a_idx: i,
                    scene_b_idx: i + 1,
                    frame_in_transition: f,
                    scene_a_frame_offset: scene_frames - actual_transition_frames,
                    scene_a_total_frames: scene_frames,
                    scene_b_total_frames: scene_b_frames,
                    transition_type: transition.transition_type.clone(),
                    transition_duration: transition.duration,
                    easing: easing.clone(),
                });
            }
        }
    }
}

fn build_world_view_tasks(tasks: &mut Vec<FrameTask>, view_idx: usize, view: &ResolvedView, fps: u32, video_width: u32, video_height: u32) {
    let timeline = crate::engine::world::WorldTimeline::build(view, fps, video_width, video_height);
    let total_frames = timeline.total_frames(fps);
    for f in 0..total_frames {
        tasks.push(FrameTask::WorldFrame {
            view_idx,
            frame_in_view: f,
            view_total_frames: total_frames,
        });
    }
}

/// Cached H.264 data for a single scene segment
pub struct SceneSegment {
    pub h264_data: Vec<u8>,
    pub scene_hash: u64,
}

pub fn hash_scene(scene: &Scene) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let json = serde_json::to_string(scene).unwrap_or_default();
    let mut hasher = DefaultHasher::new();
    json.hash(&mut hasher);
    hasher.finish()
}

pub fn hash_video_config(config: &VideoConfig) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let json = serde_json::to_string(config).unwrap_or_default();
    let mut hasher = DefaultHasher::new();
    json.hash(&mut hasher);
    hasher.finish()
}

/// Build frame tasks for a single scene (by index) within a slide view.
/// Used by incremental encoding (operates on view 0 only for backward compat).
fn build_scene_frame_tasks(scenario: &Scenario, scene_idx: usize) -> Vec<FrameTask> {
    let fps = scenario.video.fps;
    // Incremental encoding only works with simple single-slide-view scenarios
    let view_idx = 0;
    let scenes = &scenario.views[view_idx].scenes;
    let scene = &scenes[scene_idx];
    let mut tasks = Vec::new();

    let scene_frames = (scene.duration * fps as f64).round() as u32;
    let next_transition = scenes.get(scene_idx + 1).and_then(|s| s.transition.as_ref());
    let outgoing_transition_frames = next_transition
        .map(|t| (t.duration * fps as f64).round() as u32)
        .unwrap_or(0);

    let incoming_transition_frames = if scene_idx > 0 {
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

    for f in normal_start..normal_end {
        tasks.push(FrameTask::Normal {
            view_idx,
            scene_idx,
            frame_in_scene: f,
            scene_total_frames: scene_frames,
        });
    }

    if let Some(transition) = next_transition {
        let actual_transition_frames = outgoing_transition_frames.min(scene_frames);
        let scene_b_frames = (scenes[scene_idx + 1].duration * fps as f64).round() as u32;
        let easing = transition.easing.clone();
        for f in 0..actual_transition_frames {
            tasks.push(FrameTask::SlideTransition {
                view_idx,
                scene_a_idx: scene_idx,
                scene_b_idx: scene_idx + 1,
                frame_in_transition: f,
                scene_a_frame_offset: scene_frames - actual_transition_frames,
                scene_a_total_frames: scene_frames,
                scene_b_total_frames: scene_b_frames,
                transition_type: transition.transition_type.clone(),
                transition_duration: transition.duration,
                easing: easing.clone(),
            });
        }
    }

    tasks
}

/// Encode video incrementally, reusing cached segments for unchanged scenes.
/// Returns the new scene segments for use in subsequent incremental renders.
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

    // Incremental encoding operates on view 0's scenes (backward compat for single slide view)
    let num_scenes = scenario.views.get(0).map(|v| v.scenes.len()).unwrap_or(0);

    // Hash all scenes
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
    let scene_tasks: Vec<Vec<FrameTask>> = (0..num_scenes)
        .map(|i| build_scene_frame_tasks(scenario, i))
        .collect();

    let total_frames: u32 = scene_tasks.iter().map(|t| t.len() as u32).sum();

    if total_frames == 0 {
        return Err(RustmotionError::NoFrames.into());
    }

    // Flatten all tasks that need rendering, tagged with scene index
    let mut flat_tasks: Vec<(usize, &FrameTask)> = Vec::new();
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

    // Render in parallel batches with per-frame progress
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

    // Signal encoding phase
    if let Some(ref mut cb) = on_progress {
        cb(EncodeProgress::Encoding(0, frames_to_render));
    }

    // Encode sequentially with a SINGLE encoder, splitting output by scene
    let api = OpenH264API::from_source();
    let pixels = (width * height) as u32;
    let target_bitrate = (pixels as f64 * fps as f64 * 0.1) as u32;
    let encoder_config = EncoderConfig::new()
        .set_bitrate_bps(target_bitrate.max(3_000_000))
        .max_frame_rate(fps as f32);
    let mut encoder = Encoder::with_api_config(api, encoder_config)?;

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

    // Assemble final segments in order (cached + freshly rendered)
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

    // Concatenate all H.264 segments
    let total_h264_size: usize = new_segments.iter().map(|s| s.h264_data.len()).sum();
    let mut h264_data: Vec<u8> = Vec::with_capacity(total_h264_size);
    for seg in &new_segments {
        h264_data.extend_from_slice(&seg.h264_data);
    }

    // Process audio
    let total_duration = total_frames as f64 / fps as f64;
    let pcm_data = if !scenario.audio.is_empty() {
        super::audio::mix_audio_tracks(&scenario.audio, total_duration)?
    } else {
        None
    };

    // Signal muxing phase
    if let Some(ref mut cb) = on_progress {
        cb(EncodeProgress::Muxing);
    }

    // Mux
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

    if !quiet && on_progress.is_none() {
        eprintln!("Done!");
    }

    Ok(new_segments)
}

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

    // GIF requires width/height to fit in u16
    let gif_w = width.min(65535) as u16;
    let gif_h = height.min(65535) as u16;

    let file = File::create(output_path)?;
    let mut encoder = gif::Encoder::new(BufWriter::new(file), gif_w, gif_h, &[])
        .map_err(|e| RustmotionError::GifEncoder { reason: e.to_string() })?;

    encoder.set_repeat(gif::Repeat::Infinite)
        .map_err(|e| RustmotionError::GifRepeat { reason: e.to_string() })?;

    let delay = (100.0 / fps as f64).round() as u16; // GIF delay in 1/100 seconds

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
    use std::io::Write;

    let config = &scenario.video;
    let fps = config.fps;

    let mut stdout = std::io::stdout().lock();

    let mut frame_offset = 0u32;
    for view in &scenario.views {
        for scene in &view.scenes {
            let scene_frames = (scene.duration * fps as f64).round() as u32;

            for local_frame in 0..scene_frames {
                let rgba = crate::engine::render_v2::render_scene_frame(
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

/// Encode using FFmpeg subprocess (for h265, vp9, prores, webm, mov, transparency)
pub fn encode_with_ffmpeg(
    scenario: &Scenario,
    output_path: &str,
    quiet: bool,
    codec: &str,
    crf: Option<u8>,
    transparent: bool,
) -> Result<()> {
    encode_with_ffmpeg_progress(scenario, output_path, quiet, codec, crf, transparent, None)
}

pub fn encode_with_ffmpeg_progress(
    scenario: &Scenario,
    output_path: &str,
    quiet: bool,
    codec: &str,
    crf: Option<u8>,
    transparent: bool,
    on_progress: Option<&(dyn Fn(EncodeProgress) + Send + Sync)>,
) -> Result<()> {
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
        Some(TuiProgress::new(total_frames, output_path, width, height, fps, codec)?)
    } else {
        None
    };

    // Process audio first (needs a temp file for FFmpeg -i)
    let total_duration = total_frames as f64 / fps as f64;
    let audio_tmp_dir = if !scenario.audio.is_empty() {
        Some(std::env::temp_dir().join(format!("rustmotion_audio_{}", std::process::id())))
    } else {
        None
    };
    let pcm_data = if !scenario.audio.is_empty() {
        if let Some(ref tmp_dir) = audio_tmp_dir {
            std::fs::create_dir_all(tmp_dir)?;
        }
        super::audio::mix_audio_tracks(&scenario.audio, total_duration)?
    } else {
        None
    };

    // Build FFmpeg command with rawvideo pipe input
    let crf_val = crf.unwrap_or(23);
    let pix_fmt = if transparent { "rgba" } else { "rgba" };

    let mut cmd = std::process::Command::new("ffmpeg");
    cmd.args([
        "-y",
        "-loglevel", "error",
        "-f", "rawvideo",
        "-pixel_format", pix_fmt,
        "-video_size", &format!("{}x{}", width, height),
        "-framerate", &fps.to_string(),
        "-i", "pipe:0",
    ]);

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
            // h264 — use 10-bit for smoother dark gradients
            cmd.args(["-c:v", "libx264", "-crf", &crf_val.to_string(), "-preset", "medium", "-profile:v", "high10", "-pix_fmt", "yuv420p10le"]);
        }
    }

    // Add audio input if present
    if let Some(ref pcm) = pcm_data {
        let audio_path = audio_tmp_dir.as_ref().unwrap().join("audio.raw");
        std::fs::write(&audio_path, pcm)?;
        cmd.args([
            "-f", "s16le", "-ar", "44100", "-ac", "2", "-i",
            audio_path.to_str().unwrap(),
            "-c:a", "aac", "-b:a", "128k",
        ]);
    }

    cmd.arg(output_path);
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(if quiet { std::process::Stdio::null() } else { std::process::Stdio::inherit() });

    let mut child = cmd
        .spawn()
        .map_err(|e| RustmotionError::FfmpegSpawn { reason: e.to_string() })?;

    let mut stdin = child.stdin.take()
        .ok_or(RustmotionError::FfmpegPipe)?;

    // Render frames in parallel batches, pipe RGBA sequentially to FFmpeg
    let batch_size = (rayon::current_num_threads() * 2).max(4);
    let counter = AtomicU32::new(0);
    let mut pipe_error: Option<anyhow::Error> = None;

    for batch in tasks.chunks(batch_size) {
        if pipe_error.is_some() {
            break;
        }

        // Render batch in parallel
        let results: Vec<Result<Vec<u8>>> = batch
            .par_iter()
            .map(|task| {
                let rgba = render_frame_task(config, scenario, task)?;
                counter.fetch_add(1, Ordering::Relaxed);
                Ok(rgba)
            })
            .collect();

        let rendered = counter.load(Ordering::Relaxed);
        if let Some(ref mut tui) = tui {
            tui.set_progress(rendered);
        }
        if let Some(cb) = &on_progress {
            cb(EncodeProgress::Rendering(rendered, total_frames));
        }

        // Write RGBA sequentially to FFmpeg pipe (preserves frame order)
        for result in results {
            match result {
                Ok(rgba) => {
                    if let Err(e) = stdin.write_all(&rgba) {
                        pipe_error = Some(RustmotionError::FfmpegWrite { reason: e.to_string() }.into());
                        break;
                    }
                }
                Err(e) => {
                    pipe_error = Some(e);
                    break;
                }
            }
        }
    }

    // Close stdin to signal end of input
    drop(stdin);

    if let Some(cb) = &on_progress {
        cb(EncodeProgress::Muxing);
    }

    let status = child.wait()
        .map_err(|e| RustmotionError::FfmpegWait { reason: e.to_string() })?;

    // Cleanup audio temp directory
    if let Some(ref tmp_dir) = audio_tmp_dir {
        let _ = std::fs::remove_dir_all(tmp_dir);
    }

    if let Some(e) = pipe_error {
        return Err(e);
    }

    if !status.success() {
        return Err(RustmotionError::FfmpegFailed.into());
    }

    if let Some(tui) = tui {
        tui.finish("Done!");
    }

    Ok(())
}
