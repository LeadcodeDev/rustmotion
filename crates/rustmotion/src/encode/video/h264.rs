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
use super::tasks::{build_frame_tasks, build_frame_tasks_range, render_frame_task, SceneSegment};
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
    quiet: bool,
    on_progress: Option<&mut dyn FnMut(EncodeProgress)>,
) -> Result<()> {
    encode_video_impl(scenario, output_path, quiet, None, on_progress)
}

/// Same as [`encode_video`], restricted to the inclusive frame index range
/// `[frame_range.0, frame_range.1]` — the same index space `--frame N`
/// already addresses via `build_frame_tasks(...).get(N)`. Produces a
/// standalone MP4 covering only that range, independently muxed; its
/// embedded audio is windowed to match via `mix_audio_tracks_segment` (see
/// that function's doc), so a segment starting at frame 300 carries the
/// audio that plays at that point in the *full* scenario rather than audio
/// restarted from t=0.
pub fn encode_video_range(
    scenario: &Scenario,
    output_path: &str,
    quiet: bool,
    frame_range: (u32, u32),
    on_progress: Option<&mut dyn FnMut(EncodeProgress)>,
) -> Result<()> {
    encode_video_impl(scenario, output_path, quiet, Some(frame_range), on_progress)
}

fn encode_video_impl(
    scenario: &Scenario,
    output_path: &str,
    _quiet: bool,
    frame_range: Option<(u32, u32)>,
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
    for failure in analyze_scenario_audio(scenario) {
        eprintln!("rustmotion: audio-reactive: {failure} — waveform/audio_spectrum will render flat for this track.");
    }

    let (tasks, full_total_frames, segment_start_frame) = match frame_range {
        Some((start, end)) => {
            let (tasks, total) = build_frame_tasks_range(scenario, start, end)?;
            (tasks, total, start)
        }
        None => {
            let tasks = build_frame_tasks(scenario);
            let total = tasks.len() as u32;
            if total == 0 {
                return Err(RustmotionError::NoFrames);
            }
            (tasks, total, 0)
        }
    };
    let total_frames = tasks.len() as u32;

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

    let scenario_total_duration = full_total_frames as f64 / fps as f64;
    let segment_duration = total_frames as f64 / fps as f64;
    let segment_start = segment_start_frame as f64 / fps as f64;
    mux_h264_to_mp4(
        &h264_data,
        output_path,
        width,
        height,
        fps,
        scenario,
        segment_duration,
        scenario_total_duration,
        segment_start,
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
    for failure in analyze_scenario_audio(scenario) {
        eprintln!("rustmotion: audio-reactive: {failure} — waveform/audio_spectrum will render flat for this track.");
    }

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
    // Slots are built independently but their tasks form one stream, so each
    // starts where the previous ended — a task's `global_frame` must mean the
    // same thing here as in a full encode.
    let mut next_frame = 0u32;
    let scene_tasks: Vec<Vec<super::tasks::FrameTask>> = slots
        .iter()
        .map(|s| {
            let tasks = super::tasks::build_slot_frame_tasks(scenario, s, next_frame);
            next_frame += tasks.len() as u32;
            tasks
        })
        .collect();
    let num_scenes = num_slots;

    let total_frames: u32 = scene_tasks.iter().map(|t| t.len() as u32).sum();

    if total_frames == 0 {
        return Err(RustmotionError::NoFrames);
    }

    // Flatten tasks that need rendering. Kept in increasing scene-index
    // order (the encode loop below relies on that to detect scene
    // boundaries by watching for a change in `scene_idx`, rather than
    // pre-summing per-scene frame counts).
    let mut flat_tasks: Vec<(usize, &super::tasks::FrameTask)> = Vec::new();
    for i in 0..num_scenes {
        if needs_render[i] {
            for task in &scene_tasks[i] {
                flat_tasks.push((i, task));
            }
        }
    }

    let frames_to_render = flat_tasks.len() as u32;

    // Render and encode one batch at a time instead of materialising every
    // dirty frame's YUV buffer before encoding a single one (constat #9).
    // The old two-phase shape — render ALL batches into `all_yuv`, only then
    // start encoding — meant a `--watch` re-render of a long, high-res
    // scenario held the *entire* duration's worth of YUV resident before
    // the encoder even started: at 1080x1920 a YUV420 frame is ~3.11MB, so
    // 60s @ 30fps (1800 frames) peaked at ~5.6GB before any bytes were
    // encoded. Rendering still happens in parallel batches (unchanged
    // throughput), but each batch is encoded immediately after it renders,
    // so at most one batch's worth of YUV (a few dozen frames) is ever
    // resident at once.
    let batch_size = (rayon::current_num_threads() * 2).max(4);
    let counter = AtomicU32::new(0);

    let mut encoder = create_encoder(width, height, fps)?;
    let mut rendered_segments: std::collections::HashMap<usize, Vec<u8>> =
        std::collections::HashMap::new();
    let mut current_scene: Option<usize> = None;
    let mut current_segment: Vec<u8> = Vec::new();
    let mut encoded_count: u32 = 0;

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

        for ((scene_idx, _), yuv_result) in batch.iter().zip(batch_results) {
            let yuv = yuv_result?;

            // `flat_tasks` groups frames by scene in increasing scene-index
            // order (built by iterating `0..num_scenes` and pushing each
            // dirty scene's tasks contiguously), so a scene index only ever
            // changes when the previous scene's segment is complete.
            if current_scene != Some(*scene_idx) {
                if let Some(prev_idx) = current_scene {
                    rendered_segments.insert(prev_idx, std::mem::take(&mut current_segment));
                }
                current_scene = Some(*scene_idx);
            }

            encoder.force_intra_frame();
            let yuv_buf = YUVBuffer::from_vec(yuv, width as usize, height as usize);
            let bitstream = encoder
                .encode(&yuv_buf)
                .map_err(|e| RustmotionError::from(e.to_string()))?;
            bitstream.write_vec(&mut current_segment);
            encoded_count += 1;
            if let Some(ref mut cb) = on_progress {
                cb(EncodeProgress::Encoding(encoded_count, frames_to_render));
            }
        }
    }
    if let Some(prev_idx) = current_scene {
        rendered_segments.insert(prev_idx, std::mem::take(&mut current_segment));
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

    // Incremental encoding always covers the full scenario — there is no
    // sub-range concept here, so the segment IS the whole scenario, same as
    // every pre-frame-range caller of `mux_h264_to_mp4`.
    let total_duration = total_frames as f64 / fps as f64;
    mux_h264_to_mp4(
        &h264_data,
        output_path,
        width,
        height,
        fps,
        scenario,
        total_duration,
        total_duration,
        0.0,
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

    /// Constat #9: the old code rendered *every* dirty frame into `all_yuv`
    /// before encoding a single one — a full render phase, then a full
    /// encode phase, back to back. That is what let a `--watch` re-render of
    /// a long, high-resolution scenario hold the entire duration's worth of
    /// YUV buffers in memory before the encoder even started.
    ///
    /// Peak RSS isn't observable from a unit test, but the *causal* symptom
    /// is: `Encoding` progress events can only start after every `Rendering`
    /// event has fired, because rendering must finish in full before
    /// encoding begins. With batch-interleaved render+encode, an `Encoding`
    /// event fires after each batch — including before the *last* batch has
    /// even been rendered, as long as there is more than one batch. This
    /// test forces >=4 batches (independent of the machine's core count, by
    /// reading `rayon::current_num_threads()` the same way production code
    /// does) and asserts that interleaving.
    #[test]
    fn incremental_encode_interleaves_rendering_and_encoding_instead_of_buffering_everything() {
        let batch_size = (rayon::current_num_threads() * 2).max(4);
        let total_frames = batch_size * 3 + 1; // guarantee >= 4 batches
        let fps = 10u32;
        let duration = total_frames as f64 / fps as f64;

        let json = format!(
            r#"{{"video": {{"width": 16, "height": 16, "fps": {fps}}},
                 "scenes": [{{"duration": {duration}, "children": []}}]}}"#
        );
        let scenario = load_scenario_from_source(None, Some(&json)).unwrap();

        let mut events: Vec<(&'static str, u32)> = Vec::new();
        let mut cb = |p: EncodeProgress| match p {
            EncodeProgress::Rendering(cur, _) => events.push(("render", cur)),
            EncodeProgress::Encoding(cur, _) => events.push(("encode", cur)),
            EncodeProgress::Muxing => events.push(("mux", 0)),
        };

        let out = std::env::temp_dir().join(format!(
            "rustmotion_incr_mem_test_{}.mp4",
            std::process::id()
        ));
        encode_video_incremental(&scenario, out.to_str().unwrap(), true, None, Some(&mut cb))
            .expect("encode");

        let last_render_idx = events
            .iter()
            .rposition(|(k, _)| *k == "render")
            .expect("at least one render event");
        let first_encode_idx = events
            .iter()
            .position(|(k, _)| *k == "encode")
            .expect("at least one encode event");

        assert!(
            first_encode_idx < last_render_idx,
            "encoding must start before rendering finishes (interleaved batches), \
             got event sequence: {:?}",
            events
        );

        let _ = std::fs::remove_file(&out);
    }
}
