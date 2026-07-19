use openh264::encoder::{Encoder, EncoderConfig};
use openh264::formats::YUVBuffer;
use openh264::OpenH264API;
use rayon::prelude::*;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::encode::audio_analysis::analyze_scenario_audio;
use crate::engine::{preextract_video_frames, prefetch_icons, rgba_to_yuv420};
use crate::error::{Result, RustmotionError};
use crate::schema::ResolvedScenario as Scenario;

use super::mux::mux_h264_to_mp4;
use super::tasks::{build_frame_tasks, render_frame_task, SceneSegment};
use super::EncodeProgress;

/// Create an OpenH264 encoder with standard settings for the given video dimensions.
fn create_encoder(width: u32, height: u32, fps: u32) -> Result<Encoder> {
    let api = OpenH264API::from_source();
    let pixels = width * height;
    let target_bitrate = (pixels as f64 * fps as f64 * 0.1) as u32;
    let config = EncoderConfig::new()
        .set_bitrate_bps(target_bitrate.max(3_000_000))
        .max_frame_rate(fps as f32);
    Encoder::with_api_config(api, config).map_err(|e| RustmotionError::from(e.to_string()))
}

pub fn encode_video(
    scenario: &Scenario,
    output_path: &str,
    _quiet: bool,
    mut on_progress: Option<&mut dyn FnMut(EncodeProgress)>,
) -> Result<()> {
    let config = &scenario.video;
    let width = config.width;
    let height = config.height;
    let fps = config.fps;

    for view in &scenario.views {
        preextract_video_frames(&view.scenes, fps);
        prefetch_icons(&view.scenes);
    }
    analyze_scenario_audio(scenario);

    let tasks = build_frame_tasks(scenario);
    let total_frames = tasks.len() as u32;

    if total_frames == 0 {
        return Err(RustmotionError::NoFrames);
    }

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
        if let Some(ref mut cb) = on_progress {
            cb(EncodeProgress::Rendering(rendered, total_frames));
        }

        for yuv_result in yuv_frames {
            let yuv = yuv_result?;
            encoder.force_intra_frame();
            let yuv_buf = YUVBuffer::from_vec(yuv, width as usize, height as usize);
            let bitstream = encoder
                .encode(&yuv_buf)
                .map_err(|e| RustmotionError::from(e.to_string()))?;
            bitstream.write_vec(&mut h264_data);
        }
    }

    if let Some(ref mut cb) = on_progress {
        cb(EncodeProgress::Muxing);
    }

    let total_duration = total_frames as f64 / fps as f64;
    mux_h264_to_mp4(
        &h264_data,
        output_path,
        width,
        height,
        fps,
        scenario,
        total_duration,
    )?;

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

    if scenario.views.is_empty() {
        return Err(RustmotionError::IncrementalUnsupported {
            reason: "scenario has no views".to_string(),
        });
    }
    // All-slide compositions (any view count) segment cleanly: one slot per
    // scene plus one per inter-view transition. World views don't — camera
    // pans composite several scenes per frame.
    let Some(slots) = super::tasks::segment_slots(scenario) else {
        return Err(RustmotionError::IncrementalUnsupported {
            reason: "incremental encoding requires slide views (got a world view)".to_string(),
        });
    };

    for view in &scenario.views {
        preextract_video_frames(&view.scenes, fps);
        prefetch_icons(&view.scenes);
    }
    analyze_scenario_audio(scenario);

    let num_slots = slots.len();
    let scene_hashes: Vec<u64> = slots
        .iter()
        .map(|s| super::tasks::slot_hash(scenario, s))
        .collect();
    let needs_render = super::tasks::plan_dirty(scenario, &slots, &scene_hashes, prev_segments);

    let scenes_to_render: usize = needs_render.iter().filter(|&&r| r).count();

    if !quiet && on_progress.is_none() {
        eprintln!(
            "Re-rendering {}/{} segments...",
            scenes_to_render, num_slots
        );
    }

    // Build per-slot tasks
    let scene_tasks: Vec<Vec<super::tasks::FrameTask>> = slots
        .iter()
        .map(|s| super::tasks::build_slot_frame_tasks(scenario, s))
        .collect();
    let num_scenes = num_slots;

    let total_frames: u32 = scene_tasks.iter().map(|t| t.len() as u32).sum();

    if total_frames == 0 {
        return Err(RustmotionError::NoFrames);
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
            cb(EncodeProgress::Rendering(
                counter.load(Ordering::Relaxed),
                frames_to_render,
            ));
        }

        all_yuv.extend(batch_results);
    }

    // Encode phase
    if let Some(ref mut cb) = on_progress {
        cb(EncodeProgress::Encoding(0, frames_to_render));
    }

    let mut encoder = create_encoder(width, height, fps)?;
    let mut yuv_iter = all_yuv.into_iter();
    let mut rendered_segments: std::collections::HashMap<usize, Vec<u8>> =
        std::collections::HashMap::new();
    let mut encoded_count: u32 = 0;

    for &(scene_idx, frame_count) in &scene_frame_counts {
        let mut segment_h264: Vec<u8> = Vec::new();

        for _ in 0..frame_count {
            let yuv = yuv_iter.next().unwrap()?;
            encoder.force_intra_frame();
            let yuv_buf = YUVBuffer::from_vec(yuv, width as usize, height as usize);
            let bitstream = encoder
                .encode(&yuv_buf)
                .map_err(|e| RustmotionError::from(e.to_string()))?;
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
    mux_h264_to_mp4(
        &h264_data,
        output_path,
        width,
        height,
        fps,
        scenario,
        total_duration,
    )?;

    if !quiet && on_progress.is_none() {
        eprintln!("Done!");
    }

    Ok(new_segments)
}

#[cfg(test)]
mod incremental_tests {
    use super::*;
    use crate::loader::load_scenario_from_source;

    fn two_view_json(second_text: &str) -> String {
        format!(
            r##"{{
            "video": {{"width": 32, "height": 32, "fps": 10}},
            "composition": [
                {{"type": "slide", "scenes": [
                    {{"duration": 0.2, "children": [{{"type": "text", "content": "one"}}]}},
                    {{"duration": 0.2, "children": [{{"type": "text", "content": "{second_text}"}}]}}
                ]}},
                {{"type": "slide", "transition": {{"type": "fade", "duration": 0.2}}, "scenes": [
                    {{"duration": 0.2, "children": [{{"type": "text", "content": "three"}}]}}
                ]}}
            ]
        }}"##
        )
    }

    #[test]
    fn multi_view_slide_composition_encodes_incrementally() {
        // Used to fail with IncrementalUnsupported ("single slide view").
        // Second run with one scene changed must re-render only that scene
        // and the segments it feeds — strictly fewer frames than run one.
        let out = std::env::temp_dir().join("rustmotion_incr_multiview_test.mp4");
        let out_str = out.to_str().unwrap();

        let base = load_scenario_from_source(None, Some(&two_view_json("two"))).unwrap();
        let mut first_total = 0u32;
        let mut cb = |p: EncodeProgress| {
            if let EncodeProgress::Rendering(_, total) = p {
                first_total = total;
            }
        };
        let segments =
            encode_video_incremental(&base, out_str, true, None, Some(&mut cb)).expect("first run");
        assert_eq!(segments.len(), 4, "2 + VT + 1 slots");
        assert!(out.exists() && std::fs::metadata(&out).unwrap().len() > 0);

        let changed = load_scenario_from_source(None, Some(&two_view_json("TWO CHANGED"))).unwrap();
        let mut second_total = 0u32;
        let mut cb2 = |p: EncodeProgress| {
            if let EncodeProgress::Rendering(_, total) = p {
                second_total = total;
            }
        };
        let segments2 =
            encode_video_incremental(&changed, out_str, true, Some(&segments), Some(&mut cb2))
                .expect("second run");
        assert_eq!(segments2.len(), 4);
        assert!(
            second_total > 0 && second_total < first_total,
            "second run must re-render a strict subset (first={first_total}, second={second_total})"
        );
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn world_views_still_refuse_incremental_with_a_clear_reason() {
        let json = r##"{
            "video": {"width": 32, "height": 32, "fps": 10},
            "composition": [
                {"type": "world", "scenes": [
                    {"duration": 0.2, "children": [{"type": "text", "content": "w"}]}
                ]}
            ]
        }"##;
        let s = load_scenario_from_source(None, Some(json)).unwrap();
        let err = encode_video_incremental(&s, "/tmp/never.mp4", true, None, None).unwrap_err();
        assert!(
            err.to_string().contains("world"),
            "reason must name world views: {err}"
        );
    }
}
