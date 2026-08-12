use crate::engine::transition::{apply_transition, camera_pan_transition};
use crate::error::{Result, RustmotionError};
use crate::schema::{
    EasingType, ResolvedScenario as Scenario, ResolvedView, Scene, TransitionType, VideoConfig,
    ViewType,
};

/// Description of what to render for a specific frame
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum FrameTask {
    Normal {
        /// This frame's index in the *whole* schedule.
        ///
        /// The audio track is muxed on the scenario's timeline, so anything
        /// reading the audio analysis needs this rather than the scene-local
        /// counter beside it. Carried on the task rather than re-derived from
        /// the scenario, which would be a second copy of the scheduler's
        /// arithmetic that can drift from it.
        global_frame: u32,
        view_idx: usize,
        scene_idx: usize,
        frame_in_scene: u32,
        scene_total_frames: u32,
    },
    SlideTransition {
        /// This frame's index in the *whole* schedule.
        ///
        /// The audio track is muxed on the scenario's timeline, so anything
        /// reading the audio analysis needs this rather than the scene-local
        /// counter beside it. Carried on the task rather than re-derived from
        /// the scenario, which would be a second copy of the scheduler's
        /// arithmetic that can drift from it.
        global_frame: u32,
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
        /// This frame's index in the *whole* schedule.
        ///
        /// The audio track is muxed on the scenario's timeline, so anything
        /// reading the audio analysis needs this rather than the scene-local
        /// counter beside it. Carried on the task rather than re-derived from
        /// the scenario, which would be a second copy of the scheduler's
        /// arithmetic that can drift from it.
        global_frame: u32,
        view_idx: usize,
        frame_in_view: u32,
        view_total_frames: u32,
    },
    ViewTransition {
        /// This frame's index in the *whole* schedule.
        ///
        /// The audio track is muxed on the scenario's timeline, so anything
        /// reading the audio analysis needs this rather than the scene-local
        /// counter beside it. Carried on the task rather than re-derived from
        /// the scenario, which would be a second copy of the scheduler's
        /// arithmetic that can drift from it.
        global_frame: u32,
        view_a_idx: usize,
        view_b_idx: usize,
        frame_in_transition: u32,
        transition_type: TransitionType,
        transition_duration: f64,
        easing: EasingType,
    },
}

pub fn render_frame_task(
    config: &VideoConfig,
    scenario: &Scenario,
    task: &FrameTask,
) -> Result<Vec<u8>> {
    render_frame_task_scaled(config, scenario, task, 1.0)
}

/// Per-frame enriched hit-map for the studio overlay. Only `Normal` frames
/// produce hits; transitions/world frames return an empty Vec for now.
pub fn render_frame_task_hits(
    scenario: &Scenario,
    task: &FrameTask,
) -> Vec<rustmotion_core::engine::paint_pass::EnrichedHit> {
    use crate::engine::render::render_scene_hits;
    match task {
        FrameTask::Normal {
            view_idx,
            scene_idx,
            frame_in_scene,
            ..
        } => {
            let scene = &scenario.views[*view_idx].scenes[*scene_idx];
            render_scene_hits(&scenario.video, scene, *frame_in_scene)
        }
        _ => Vec::new(),
    }
}

pub fn render_frame_task_scaled(
    config: &VideoConfig,
    scenario: &Scenario,
    task: &FrameTask,
    scale_factor: f32,
) -> Result<Vec<u8>> {
    use crate::engine::render::{
        post_effects::apply_post_effects, render_scene_bg_scaled, render_scene_fg_scaled,
        render_scene_frame, render_scene_frame_scaled, render_scene_frame_scaled_with_prev_bg,
    };

    match task {
        FrameTask::Normal {
            global_frame,
            view_idx,
            scene_idx,
            frame_in_scene,
            scene_total_frames,
        } => {
            let scenario_time = *global_frame as f64 / config.fps as f64;
            let view = &scenario.views[*view_idx];
            let scene = &view.scenes[*scene_idx];
            let prev_bg = if *scene_idx > 0 {
                let prev = &view.scenes[*scene_idx - 1];
                Some((&prev.resolved_background, prev.duration))
            } else {
                None
            };
            let mut pixels = render_scene_frame_scaled_with_prev_bg(
                config,
                scene,
                *frame_in_scene,
                scenario_time,
                *scene_total_frames,
                scale_factor,
                prev_bg,
            )?;
            let scaled_w = (config.width as f32 * scale_factor) as u32;
            let scaled_h = (config.height as f32 * scale_factor) as u32;
            apply_post_effects(
                &mut pixels,
                scaled_w,
                scaled_h,
                &scene.effects,
                *frame_in_scene,
            );
            Ok(pixels)
        }
        FrameTask::SlideTransition {
            global_frame,
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
            let scenario_time = *global_frame as f64 / config.fps as f64;
            let scenes = &scenario.views[*view_idx].scenes;
            let scaled_w = (config.width as f32 * scale_factor) as u32;
            let scaled_h = (config.height as f32 * scale_factor) as u32;
            let fps = config.fps;
            let progress = transition_progress(*frame_in_transition, *transition_duration, fps);
            let frame_a_idx = scene_a_frame_offset + frame_in_transition;

            if matches!(transition_type, TransitionType::CameraPan) {
                let (ax, ay) = scenes[*scene_a_idx]
                    .world_position
                    .as_ref()
                    .map(|p| (p.x, p.y))
                    .unwrap_or((0.0, 0.0));
                let (bx, by) = scenes[*scene_b_idx]
                    .world_position
                    .as_ref()
                    .map(|p| (p.x, p.y))
                    .unwrap_or((0.0, 0.0));
                let dx = bx - ax;
                let dy = by - ay;
                let bg_a = render_scene_bg_scaled(
                    config,
                    &scenes[*scene_a_idx],
                    frame_a_idx,
                    scale_factor,
                )?;
                // The transition belongs to the scene being entered, so that is
                // where the background mode lives.
                let pan_bg = scenes[*scene_b_idx]
                    .transition
                    .as_ref()
                    .map(|t| t.background)
                    .unwrap_or_default();
                // Rendered in both modes: `Static` now crossfades in place too
                // (see camera_pan_transition's doc), so it needs scene B's
                // background just as `Travel` does.
                let bg_b = render_scene_bg_scaled(
                    config,
                    &scenes[*scene_b_idx],
                    *frame_in_transition,
                    scale_factor,
                )?;
                let fg_a = render_scene_fg_scaled(
                    config,
                    &scenes[*scene_a_idx],
                    frame_a_idx,
                    scenario_time,
                    *scene_a_total_frames,
                    scale_factor,
                )?;
                let fg_b = render_scene_fg_scaled(
                    config,
                    &scenes[*scene_b_idx],
                    *frame_in_transition,
                    scenario_time,
                    *scene_b_total_frames,
                    scale_factor,
                )?;
                // CameraPan composites two scenes; apply scene_b effects to the composited result.
                let mut composited = camera_pan_transition(
                    &bg_a,
                    &bg_b,
                    &fg_a,
                    &fg_b,
                    scaled_w,
                    scaled_h,
                    progress,
                    dx * scale_factor,
                    dy * scale_factor,
                    easing,
                    pan_bg,
                );
                apply_post_effects(
                    &mut composited,
                    scaled_w,
                    scaled_h,
                    &scenes[*scene_b_idx].effects,
                    *frame_in_transition,
                );
                return Ok(composited);
            }

            let (frame_a, frame_b) = if scale_factor == 1.0 {
                let a = render_scene_frame(
                    config,
                    &scenes[*scene_a_idx],
                    frame_a_idx,
                    scenario_time,
                    *scene_a_total_frames,
                )?;
                let b = render_scene_frame(
                    config,
                    &scenes[*scene_b_idx],
                    *frame_in_transition,
                    scenario_time,
                    *scene_b_total_frames,
                )?;
                (a, b)
            } else {
                let a = render_scene_frame_scaled(
                    config,
                    &scenes[*scene_a_idx],
                    frame_a_idx,
                    scenario_time,
                    *scene_a_total_frames,
                    scale_factor,
                )?;
                let b = render_scene_frame_scaled(
                    config,
                    &scenes[*scene_b_idx],
                    *frame_in_transition,
                    scenario_time,
                    *scene_b_total_frames,
                    scale_factor,
                )?;
                (a, b)
            };

            // For slide transitions, apply effects of scene_b to the composited result.
            // Rationale: the transition is the "entry" of scene_b; its post-effects
            // (e.g. vignette) should appear on the blended frames to avoid a
            // jarring pop when the transition ends and Normal frames begin.
            let mut composited = apply_transition(
                &frame_a,
                &frame_b,
                scaled_w,
                scaled_h,
                progress,
                transition_type,
            );
            apply_post_effects(
                &mut composited,
                scaled_w,
                scaled_h,
                &scenes[*scene_b_idx].effects,
                *frame_in_transition,
            );
            Ok(composited)
        }
        FrameTask::WorldFrame {
            global_frame,
            view_idx,
            frame_in_view,
            view_total_frames: _,
        } => {
            let scenario_time = *global_frame as f64 / config.fps as f64;
            use crate::engine::world::WorldTimeline;
            let view = &scenario.views[*view_idx];
            let timeline = WorldTimeline::build(view, config.fps, config.width, config.height);
            let mut pixels = crate::engine::render::render_world_frame_scaled(
                config,
                view,
                &timeline,
                *frame_in_view,
                scenario_time,
                scale_factor,
            )?;
            // `apply_post_effects` runs for every other frame kind (Normal,
            // the camera-pan and slide-transition composites) but was never
            // called on a `WorldFrame` — a scene's `effects` (e.g. vignette)
            // silently never rendered inside a `world` view. Apply the
            // active scene's effects, by symmetry with how `Normal` applies
            // that scene's own effects.
            let scaled_w = (config.width as f32 * scale_factor) as u32;
            let scaled_h = (config.height as f32 * scale_factor) as u32;
            let time = *frame_in_view as f64 / config.fps as f64;
            if let Some(active_idx) = timeline.active_scene_idx(time, &view.scenes, config.fps) {
                apply_post_effects(
                    &mut pixels,
                    scaled_w,
                    scaled_h,
                    &view.scenes[active_idx].effects,
                    *frame_in_view,
                );
            }
            Ok(pixels)
        }
        FrameTask::ViewTransition {
            global_frame,
            view_a_idx,
            view_b_idx,
            frame_in_transition,
            transition_type,
            transition_duration,
            easing: _,
        } => {
            let scenario_time = *global_frame as f64 / config.fps as f64;
            let scaled_w = (config.width as f32 * scale_factor) as u32;
            let scaled_h = (config.height as f32 * scale_factor) as u32;
            let fps = config.fps;
            // Open-interval progress (never exactly 0.0 or 1.0) — see
            // `view_transition_progress`'s doc for why a `ViewTransition`
            // needs this and a `SlideTransition` (via `transition_progress`)
            // does not.
            let progress =
                view_transition_progress(*frame_in_transition, *transition_duration, fps);

            let view_a = &scenario.views[*view_a_idx];
            let view_b = &scenario.views[*view_b_idx];

            let frame_a =
                render_last_frame_of_view(config, view_a, fps, scenario_time, scale_factor)?;
            let frame_b =
                render_first_frame_of_view(config, view_b, fps, scenario_time, scale_factor)?;

            let mut composited = apply_transition(
                &frame_a,
                &frame_b,
                scaled_w,
                scaled_h,
                progress,
                transition_type,
            );
            // By symmetry with `SlideTransition` (which applies scene_b's
            // effects to the blended result, "the transition is the entry
            // of scene_b"): apply the incoming view's first scene's effects,
            // so an effect present on both sides' Normal frames doesn't
            // disappear for the transition's duration and pop back.
            if let Some(first_scene) = view_b.scenes.first() {
                apply_post_effects(
                    &mut composited,
                    scaled_w,
                    scaled_h,
                    &first_scene.effects,
                    *frame_in_transition,
                );
            }
            Ok(composited)
        }
    }
}

/// Frame index within a transition → progress in `[0.0, 1.0]`.
///
/// `build_frame_tasks` emits exactly `(transition_duration * fps).round()`
/// frames for a transition (`frame_in_transition` in `0..transition_frames`).
/// Dividing by the raw, unrounded `transition_duration * fps` instead of by
/// that same emitted frame count can disagree once rounding is involved —
/// e.g. 23 emitted frames for a 22.5-frame duration — which left `progress`
/// maxing out at 22/22.5 ≈ 0.978 on the last emitted frame and discharging
/// the residual as a snap in the very next (non-transition) frame: measured
/// at 22.6px of foreground displacement in one frame with linear easing.
/// Dividing by `transition_frames - 1` instead makes `frame_in_transition ==
/// transition_frames - 1` land on exactly `1.0`, so the transition's last
/// frame is the fully-completed state and there is nothing left to snap.
fn transition_progress(frame_in_transition: u32, transition_duration: f64, fps: u32) -> f64 {
    let transition_frames = (transition_duration * fps as f64).round() as u32;
    frame_in_transition as f64 / transition_frames.saturating_sub(1).max(1) as f64
}

/// Frame index within a *view* transition → progress in the OPEN interval
/// `(0.0, 1.0)`, excluding both endpoints.
///
/// A `SlideTransition` (via `transition_progress` above) is deliberately
/// allowed to hit exactly 0.0/1.0 at its ends, because at those points it
/// renders each side at scene-frame indices that were never emitted as a
/// `Normal` frame — `frame_a_idx`/`frame_in_transition` continue exactly
/// where `normal_start`/`normal_end` left off, so a 0.0-progress transition
/// frame is new content, not a repeat.
///
/// A `ViewTransition` is different: `frame_a`/`frame_b` are
/// `render_last_frame_of_view`/`render_first_frame_of_view` — the outgoing
/// view's own last `Normal` frame and the incoming view's own first
/// `Normal` frame, at the exact same time point, re-rendered byte-for-byte.
/// A blend weight of exactly 0.0 or 1.0 there reproduces one of those
/// frames identically, placed directly next to the frame it duplicates in
/// the output stream — a still frame sitting inside otherwise continuous
/// motion. Mapping onto the open interval instead — `(f + 1) / (N + 1)`
/// instead of `f / (N - 1)` — keeps every `ViewTransition` frame a genuine
/// blend of both sides, so no frame in the stream is ever byte-identical to
/// its neighbour.
fn view_transition_progress(frame_in_transition: u32, transition_duration: f64, fps: u32) -> f64 {
    let transition_frames = (transition_duration * fps as f64).round().max(1.0);
    (frame_in_transition as f64 + 1.0) / (transition_frames + 1.0)
}

fn render_last_frame_of_view(
    config: &VideoConfig,
    view: &ResolvedView,
    fps: u32,
    // Where that last frame sits on the scenario's own timeline — the view
    // transition that needs it is itself somewhere in the middle of a render.
    scenario_time: f64,
    scale_factor: f32,
) -> Result<Vec<u8>> {
    use crate::engine::render::render_scene_frame_scaled;
    match view.view_type {
        ViewType::Slide => {
            if let Some(last_scene) = view.scenes.last() {
                let scene_frames = (last_scene.duration * fps as f64).round() as u32;
                render_scene_frame_scaled(
                    config,
                    last_scene,
                    scene_frames.saturating_sub(1),
                    scenario_time,
                    scene_frames,
                    scale_factor,
                )
            } else {
                Ok(vec![
                    0u8;
                    (config.width as f32 * scale_factor) as usize
                        * (config.height as f32 * scale_factor) as usize
                        * 4
                ])
            }
        }
        ViewType::World => {
            let timeline =
                crate::engine::world::WorldTimeline::build(view, fps, config.width, config.height);
            let total_frames = timeline.total_frames(fps);
            crate::engine::render::render_world_frame_scaled(
                config,
                view,
                &timeline,
                total_frames.saturating_sub(1),
                scenario_time,
                scale_factor,
            )
        }
    }
}

fn render_first_frame_of_view(
    config: &VideoConfig,
    view: &ResolvedView,
    fps: u32,
    // See `render_last_frame_of_view`.
    scenario_time: f64,
    scale_factor: f32,
) -> Result<Vec<u8>> {
    use crate::engine::render::render_scene_frame_scaled;
    match view.view_type {
        ViewType::Slide => {
            if let Some(first_scene) = view.scenes.first() {
                let scene_frames = (first_scene.duration * fps as f64).round() as u32;
                render_scene_frame_scaled(
                    config,
                    first_scene,
                    0,
                    scenario_time,
                    scene_frames,
                    scale_factor,
                )
            } else {
                Ok(vec![
                    0u8;
                    (config.width as f32 * scale_factor) as usize
                        * (config.height as f32 * scale_factor) as usize
                        * 4
                ])
            }
        }
        ViewType::World => {
            let timeline =
                crate::engine::world::WorldTimeline::build(view, fps, config.width, config.height);
            crate::engine::render::render_world_frame_scaled(
                config,
                view,
                &timeline,
                0,
                scenario_time,
                scale_factor,
            )
        }
    }
}

pub fn build_frame_tasks(scenario: &Scenario) -> Vec<FrameTask> {
    let fps = scenario.video.fps;
    let mut tasks = Vec::new();

    for (view_idx, view) in scenario.views.iter().enumerate() {
        if view_idx > 0 {
            if let Some(ref transition) = view.transition {
                let transition_frames = (transition.duration * fps as f64).round() as u32;
                for f in 0..transition_frames {
                    tasks.push(FrameTask::ViewTransition {
                        global_frame: tasks.len() as u32,
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
            ViewType::World => build_world_view_tasks(
                &mut tasks,
                view_idx,
                view,
                fps,
                scenario.video.width,
                scenario.video.height,
            ),
        }
    }

    tasks
}

/// `build_frame_tasks`, restricted to the inclusive index range `[start,
/// end]` — the same index space `render --frame N` already addresses via
/// `build_frame_tasks(scenario).get(N)`. Returns the sliced tasks alongside
/// the *full* scenario's total frame count, since callers (the encoders'
/// mux step in particular) need it to compute the segment's time offset
/// into the scenario, not just the segment's own length.
///
/// Errors with `FrameRangeOutOfRange` when `start > end` or `end` falls
/// outside `0..total`, naming both the requested range and the actual
/// total — the same contract `render_single_frame`'s `--frame` error gives
/// for a single out-of-range index.
pub fn build_frame_tasks_range(
    scenario: &Scenario,
    start: u32,
    end: u32,
) -> Result<(Vec<FrameTask>, u32)> {
    let tasks = build_frame_tasks(scenario);
    let total = tasks.len() as u32;
    if start > end || end >= total {
        return Err(RustmotionError::FrameRangeOutOfRange { start, end, total });
    }
    Ok((tasks[start as usize..=end as usize].to_vec(), total))
}

/// Frames actually spent on the transition from `scenes[i]` into
/// `scenes[i + 1]` — defined by `scenes[i + 1].transition` — clamped to the
/// *outgoing* scene's own frame budget. `(frames, effective_duration)`
/// where `effective_duration` is that frame count expressed back in
/// seconds, exactly the value `transition_progress` must be given so its
/// internal `(duration * fps).round()` reproduces `frames` instead of the
/// raw, unclamped declared duration.
///
/// A transition longer than the scene it leaves cannot consume more frames
/// than that scene has: `scenes[i]` only has `scene_frames` frames to spend,
/// full stop. Before this clamp existed, the *entering* scene's
/// `normal_start` (see `build_slide_view_tasks`) was computed from the raw
/// declared duration instead of from this same number — so when a
/// transition declared e.g. 1.0s but the outgoing scene was only 0.3s long,
/// only 9 frames of `SlideTransition` were ever emitted, yet the entering
/// scene still skipped its first 30 frames (`normal_start = 30`) waiting for
/// a transition that had already finished after 9 — silently dropping 21
/// frames (0.7s) of the entering scene's own animation. Passing the raw
/// duration through to `transition_progress` compounded this: progress
/// maxed out around 0.28 instead of reaching 1.0 on the last emitted frame.
fn actual_outgoing_transition(scenes: &[Scene], i: usize, fps: u32) -> (u32, f64) {
    let Some(transition) = scenes.get(i + 1).and_then(|s| s.transition.as_ref()) else {
        return (0, 0.0);
    };
    let raw_frames = (transition.duration * fps as f64).round() as u32;
    let scene_frames = (scenes[i].duration * fps as f64).round() as u32;
    let frames = raw_frames.min(scene_frames);
    (frames, frames as f64 / fps as f64)
}

fn build_slide_view_tasks(
    tasks: &mut Vec<FrameTask>,
    view_idx: usize,
    view: &ResolvedView,
    fps: u32,
) {
    let scenes = &view.scenes;

    for (i, scene) in scenes.iter().enumerate() {
        let scene_frames = (scene.duration * fps as f64).round() as u32;
        let next_transition = scenes.get(i + 1).and_then(|s| s.transition.as_ref());
        let (outgoing_transition_frames, outgoing_effective_duration) =
            actual_outgoing_transition(scenes, i, fps);

        // Symmetric with `outgoing_transition_frames` above: the frames this
        // scene skips at its own start must equal what the *previous*
        // scene's iteration actually emitted for the transition into this
        // one, not a value recomputed independently from the raw duration.
        let incoming_transition_frames = if i > 0 {
            actual_outgoing_transition(scenes, i - 1, fps).0
        } else {
            0
        };

        let normal_start = incoming_transition_frames;
        let normal_end = scene_frames.saturating_sub(outgoing_transition_frames);

        for f in normal_start..normal_end {
            tasks.push(FrameTask::Normal {
                global_frame: tasks.len() as u32,
                view_idx,
                scene_idx: i,
                frame_in_scene: f,
                scene_total_frames: scene_frames,
            });
        }

        if let Some(transition) = next_transition {
            let scene_b_frames = (scenes[i + 1].duration * fps as f64).round() as u32;
            let easing = transition.easing.clone();
            for f in 0..outgoing_transition_frames {
                tasks.push(FrameTask::SlideTransition {
                    global_frame: tasks.len() as u32,
                    view_idx,
                    scene_a_idx: i,
                    scene_b_idx: i + 1,
                    frame_in_transition: f,
                    scene_a_frame_offset: scene_frames - outgoing_transition_frames,
                    scene_a_total_frames: scene_frames,
                    scene_b_total_frames: scene_b_frames,
                    transition_type: transition.transition_type.clone(),
                    transition_duration: outgoing_effective_duration,
                    easing: easing.clone(),
                });
            }
        }
    }
}

fn build_world_view_tasks(
    tasks: &mut Vec<FrameTask>,
    view_idx: usize,
    view: &ResolvedView,
    fps: u32,
    video_width: u32,
    video_height: u32,
) {
    let timeline = crate::engine::world::WorldTimeline::build(view, fps, video_width, video_height);
    let total_frames = timeline.total_frames(fps);
    for f in 0..total_frames {
        tasks.push(FrameTask::WorldFrame {
            global_frame: tasks.len() as u32,
            view_idx,
            frame_in_view: f,
            view_total_frames: total_frames,
        });
    }
}

/// One independently re-renderable unit of an all-slide composition, in
/// exact output order: a view's optional incoming transition, then its
/// scenes. Single-view scenarios degrade to one `Scene` slot per scene,
/// keeping previous incremental caches compatible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentSlot {
    Scene {
        view_idx: usize,
        scene_idx: usize,
    },
    /// Transition from `view_idx - 1` into `view_idx`.
    ViewTransition {
        view_idx: usize,
    },
}

/// Enumerate segment slots for an all-slide composition, mirroring
/// `build_frame_tasks` output order exactly. `None` if any view is a world
/// view (their frames aren't scene-partitioned — camera pans composite
/// several scenes per frame).
pub fn segment_slots(scenario: &Scenario) -> Option<Vec<SegmentSlot>> {
    use crate::schema::ViewType;
    let mut slots = Vec::new();
    for (view_idx, view) in scenario.views.iter().enumerate() {
        if !matches!(view.view_type, ViewType::Slide) {
            return None;
        }
        if view_idx > 0 && view.transition.is_some() {
            slots.push(SegmentSlot::ViewTransition { view_idx });
        }
        for scene_idx in 0..view.scenes.len() {
            slots.push(SegmentSlot::Scene {
                view_idx,
                scene_idx,
            });
        }
    }
    Some(slots)
}

/// Content hash of a slot. A view transition hashes both boundary scenes and
/// the transition config, so it re-renders when either side changes.
pub fn slot_hash(scenario: &Scenario, slot: &SegmentSlot) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    match slot {
        SegmentSlot::Scene {
            view_idx,
            scene_idx,
        } => hash_scene(&scenario.views[*view_idx].scenes[*scene_idx]),
        SegmentSlot::ViewTransition { view_idx } => {
            let mut h = DefaultHasher::new();
            let prev_view = &scenario.views[view_idx - 1];
            if let Some(last) = prev_view.scenes.last() {
                hash_scene(last).hash(&mut h);
            }
            if let Some(first) = scenario.views[*view_idx].scenes.first() {
                hash_scene(first).hash(&mut h);
            }
            serde_json::to_string(&scenario.views[*view_idx].transition)
                .unwrap_or_default()
                .hash(&mut h);
            h.finish()
        }
    }
}

/// Decide which slots must re-render given the previous run's segments.
/// `prev` of a different length (slot layout changed) re-renders everything.
/// A scene also re-renders when the *next* scene in the same view changed and
/// has an incoming transition (the outgoing blend frames live in this slot).
pub fn plan_dirty(
    scenario: &Scenario,
    slots: &[SegmentSlot],
    hashes: &[u64],
    prev: Option<&[SceneSegment]>,
) -> Vec<bool> {
    let Some(prev) = prev.filter(|p| p.len() == slots.len()) else {
        return vec![true; slots.len()];
    };
    let changed: Vec<bool> = hashes
        .iter()
        .zip(prev)
        .map(|(h, p)| *h != p.scene_hash)
        .collect();
    slots
        .iter()
        .enumerate()
        .map(|(i, slot)| {
            if changed[i] {
                return true;
            }
            if let SegmentSlot::Scene {
                view_idx,
                scene_idx,
            } = slot
            {
                // Same-view successor with an incoming transition?
                if let Some(next_i) = slots.iter().position(|s| {
                    matches!(s, SegmentSlot::Scene { view_idx: v, scene_idx: s2 }
                        if v == view_idx && *s2 == scene_idx + 1)
                }) {
                    let next_has_transition = scenario.views[*view_idx].scenes[scene_idx + 1]
                        .transition
                        .is_some();
                    if changed[next_i] && next_has_transition {
                        return true;
                    }
                }
            }
            false
        })
        .collect()
}

/// Frame tasks for one slot, mirroring the full builder's output.
/// `start_frame` is where this slot begins in the full schedule.
///
/// The incremental encoder builds slots independently, so the position cannot
/// come from the local `Vec`'s length the way it does in `build_frame_tasks` —
/// each slot would restart at 0 and every task past the first scene would carry
/// the wrong `global_frame`. Threaded in rather than re-derived here: summing
/// the preceding slots would be a second copy of the scheduler's arithmetic,
/// free to drift from it (`slot_tasks_reproduce_the_full_builder_exactly`
/// exists precisely because that has happened before).
impl FrameTask {
    /// Overwrite this task's position in the full schedule.
    ///
    /// Used by the incremental path, which builds each slot in isolation and
    /// only then knows where it starts.
    fn set_global_frame(&mut self, frame: u32) {
        match self {
            FrameTask::Normal { global_frame, .. }
            | FrameTask::SlideTransition { global_frame, .. }
            | FrameTask::WorldFrame { global_frame, .. }
            | FrameTask::ViewTransition { global_frame, .. } => *global_frame = frame,
        }
    }
}

pub(super) fn build_slot_frame_tasks(
    scenario: &Scenario,
    slot: &SegmentSlot,
    start_frame: u32,
) -> Vec<FrameTask> {
    let mut tasks = match slot {
        SegmentSlot::Scene {
            view_idx,
            scene_idx,
        } => build_scene_frame_tasks_in_view(scenario, *view_idx, *scene_idx),
        SegmentSlot::ViewTransition { view_idx } => {
            let fps = scenario.video.fps;
            let view = &scenario.views[*view_idx];
            let mut tasks = Vec::new();
            if let Some(ref transition) = view.transition {
                let transition_frames = (transition.duration * fps as f64).round() as u32;
                for f in 0..transition_frames {
                    tasks.push(FrameTask::ViewTransition {
                        global_frame: tasks.len() as u32,
                        view_a_idx: view_idx - 1,
                        view_b_idx: *view_idx,
                        frame_in_transition: f,
                        transition_type: transition.transition_type.clone(),
                        transition_duration: transition.duration,
                        easing: transition.easing.clone(),
                    });
                }
            }
            tasks
        }
    };
    // The slot was numbered from its own zero; place it in the full schedule.
    for (i, task) in tasks.iter_mut().enumerate() {
        task.set_global_frame(start_frame + i as u32);
    }
    tasks
}

/// Build frame tasks for a single scene (by index) within a slide view.
pub(super) fn build_scene_frame_tasks_in_view(
    scenario: &Scenario,
    view_idx: usize,
    scene_idx: usize,
) -> Vec<FrameTask> {
    let fps = scenario.video.fps;
    let scenes = &scenario.views[view_idx].scenes;
    let scene = &scenes[scene_idx];
    let mut tasks = Vec::new();

    let scene_frames = (scene.duration * fps as f64).round() as u32;
    let next_transition = scenes
        .get(scene_idx + 1)
        .and_then(|s| s.transition.as_ref());
    let (outgoing_transition_frames, outgoing_effective_duration) =
        actual_outgoing_transition(scenes, scene_idx, fps);

    // Mirrors `build_slide_view_tasks`: must agree exactly with what the
    // previous scene's own slot emitted, or the two builders diverge and
    // `slot_tasks_reproduce_the_full_builder_exactly` catches it.
    let incoming_transition_frames = if scene_idx > 0 {
        actual_outgoing_transition(scenes, scene_idx - 1, fps).0
    } else {
        0
    };

    let normal_start = incoming_transition_frames;
    let normal_end = scene_frames.saturating_sub(outgoing_transition_frames);

    for f in normal_start..normal_end {
        tasks.push(FrameTask::Normal {
            global_frame: tasks.len() as u32,
            view_idx,
            scene_idx,
            frame_in_scene: f,
            scene_total_frames: scene_frames,
        });
    }

    if let Some(transition) = next_transition {
        let scene_b_frames = (scenes[scene_idx + 1].duration * fps as f64).round() as u32;
        let easing = transition.easing.clone();
        for f in 0..outgoing_transition_frames {
            tasks.push(FrameTask::SlideTransition {
                global_frame: tasks.len() as u32,
                view_idx,
                scene_a_idx: scene_idx,
                scene_b_idx: scene_idx + 1,
                frame_in_transition: f,
                scene_a_frame_offset: scene_frames - outgoing_transition_frames,
                scene_a_total_frames: scene_frames,
                scene_b_total_frames: scene_b_frames,
                transition_type: transition.transition_type.clone(),
                transition_duration: outgoing_effective_duration,
                easing: easing.clone(),
            });
        }
    }

    tasks
}

/// Cached H.264 data for a single scene segment
#[derive(Debug)]
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

#[cfg(test)]
mod transition_progress_tests {
    use super::*;

    // The audit's own reproduction case: a 0.75s transition at 30fps rounds
    // up to 23 emitted frames (0.75*30 = 22.5), so frame_in_transition runs
    // 0..=22. The old code divided by the raw 22.5 instead of by the
    // (frame_count - 1) = 22 those indices actually span.
    #[test]
    fn last_emitted_frame_reaches_exactly_one() {
        let p = transition_progress(22, 0.75, 30);
        assert_eq!(
            p, 1.0,
            "last frame of a 23-frame/22.5-raw transition must be exactly 1.0, got {p}"
        );
        // The old formula's value, for contrast: 22.0 / 22.5 ≈ 0.9778 — this
        // is what used to be discharged as a snap in the next normal frame.
        assert!((22.0 / 22.5 - p).abs() > 0.02);
    }

    #[test]
    fn first_frame_is_zero() {
        assert_eq!(transition_progress(0, 0.75, 30), 0.0);
    }

    #[test]
    fn progress_is_monotonic_and_bounded() {
        let frames = (0.75_f64 * 30.0).round() as u32; // 23
        let mut prev = -1.0;
        for f in 0..frames {
            let p = transition_progress(f, 0.75, 30);
            assert!(
                (0.0..=1.0).contains(&p),
                "progress {p} out of range at frame {f}"
            );
            assert!(
                p > prev,
                "progress must be strictly increasing: {prev} -> {p} at frame {f}"
            );
            prev = p;
        }
        assert_eq!(prev, 1.0);
    }

    // Frame counts that are already exact multiples of fps had the same bug,
    // just a smaller residual: dividing by N instead of N-1 never reaches 1.0
    // on the last frame either.
    #[test]
    fn exact_integer_duration_still_reaches_one() {
        // 0.5s @ 30fps = 15 frames exactly, indices 0..=14.
        let p = transition_progress(14, 0.5, 30);
        assert_eq!(p, 1.0);
    }

    // A single-frame transition must not divide by zero.
    #[test]
    fn single_frame_transition_does_not_panic() {
        let p = transition_progress(0, 1.0 / 60.0, 30);
        assert!(p.is_finite());
    }
}

// Constat 1 (audit lot "transitions"): a transition longer than the scene it
// leaves must not silently drop frames from the scene it enters.
#[cfg(test)]
mod outgoing_transition_clamp_tests {
    use super::*;
    use crate::schema::Scene;

    fn scene(duration: f64) -> Scene {
        serde_json::from_value(serde_json::json!({
            "duration": duration,
            "children": []
        }))
        .unwrap()
    }

    fn scene_with_transition(duration: f64, transition_duration: f64) -> Scene {
        serde_json::from_value(serde_json::json!({
            "duration": duration,
            "children": [],
            "transition": { "type": "fade", "duration": transition_duration }
        }))
        .unwrap()
    }

    // The audit's own repro: scene A is far shorter than the declared
    // transition, so the transition can only ever spend A's own 9 frames
    // (0.3s @ 30fps), not the raw 30 (1.0s @ 30fps) it asked for.
    #[test]
    fn clamps_to_the_outgoing_scenes_own_frame_budget() {
        let scenes = vec![scene(0.3), scene_with_transition(2.0, 1.0)];
        let (frames, effective_duration) = actual_outgoing_transition(&scenes, 0, 30);
        assert_eq!(
            frames, 9,
            "9 frames is all scene A (0.3s @ 30fps) has to spend"
        );
        assert!(
            (effective_duration - 0.3).abs() < 1e-9,
            "effective duration must reflect the clamped frame count, not the raw 1.0s: {effective_duration}"
        );
    }

    #[test]
    fn no_clamp_needed_when_transition_fits() {
        // 0.5s transition entering scene B, 2.0s outgoing scene A @ 30fps:
        // 15 frames <= 60 available, no clamp — effective duration is the
        // declared one, byte-for-byte.
        let pair = vec![scene(2.0), scene_with_transition(2.0, 0.5)];
        let (frames, effective_duration) = actual_outgoing_transition(&pair, 0, 30);
        assert_eq!(frames, 15);
        assert!((effective_duration - 0.5).abs() < 1e-9);
    }

    // The core regression: every one of scene B's own local-frame indices
    // must be rendered exactly once, somewhere in the output — either as
    // part of the SlideTransition (indices 0..outgoing_frames) or as a
    // Normal frame (indices normal_start..scene_b_frames). Before the fix,
    // indices `[9, 30)` were rendered nowhere: the transition only ever
    // advanced scene B through frame 8, and Normal frames for B started at
    // the unclamped 30.
    #[test]
    fn every_local_frame_of_the_entering_scene_is_rendered_exactly_once() {
        let json = r#"{
            "video": { "width": 320, "height": 180, "fps": 30 },
            "scenes": [
                { "duration": 0.3, "children": [] },
                { "duration": 2.0, "children": [],
                  "transition": { "type": "fade", "duration": 1.0 } }
            ]
        }"#;
        let scenario = crate::loader::load_scenario_from_source(None, Some(json)).unwrap();
        let tasks = build_frame_tasks(&scenario);

        let scene_b_frames = (2.0_f64 * 30.0).round() as u32; // 60
        let mut covered = vec![0u32; scene_b_frames as usize];
        for t in &tasks {
            match t {
                FrameTask::SlideTransition {
                    scene_b_idx: 1,
                    frame_in_transition,
                    ..
                } => covered[*frame_in_transition as usize] += 1,
                FrameTask::Normal {
                    scene_idx: 1,
                    frame_in_scene,
                    ..
                } => covered[*frame_in_scene as usize] += 1,
                _ => {}
            }
        }

        let missing: Vec<usize> = covered
            .iter()
            .enumerate()
            .filter(|(_, &c)| c == 0)
            .map(|(i, _)| i)
            .collect();
        assert!(
            missing.is_empty(),
            "scene B local frames never rendered: {missing:?} (covered={covered:?})"
        );
        let duplicated: Vec<usize> = covered
            .iter()
            .enumerate()
            .filter(|(_, &c)| c > 1)
            .map(|(i, _)| i)
            .collect();
        assert!(
            duplicated.is_empty(),
            "scene B local frames rendered more than once: {duplicated:?}"
        );

        // Exact frame-accounting check: 9 SlideTransition frames (scene A's
        // entire 9-frame budget) + 51 Normal frames for scene B
        // (60 - 9 = 51) + 0 Normal frames for scene A (fully absorbed by
        // the clamped transition) = 60 tasks total.
        assert_eq!(tasks.len(), 60, "tasks: {tasks:?}");
    }
}

// Constat 6 (audit lot "transitions"): a ViewTransition frame must never be
// byte-identical to the Normal frame it sits next to in the output stream.
#[cfg(test)]
mod view_transition_progress_tests {
    use super::*;

    #[test]
    fn never_reaches_either_endpoint() {
        let frames = (0.2_f64 * 30.0).round() as u32; // 6
        for f in 0..frames {
            let p = view_transition_progress(f, 0.2, 30);
            assert!(
                p > 0.0 && p < 1.0,
                "frame {f}: progress {p} must be strictly inside (0.0, 1.0)"
            );
        }
    }

    #[test]
    fn monotonic_and_symmetric_about_the_midpoint() {
        let frames = (0.2_f64 * 30.0).round() as u32; // 6
        let mut prev = 0.0;
        let mut values = Vec::new();
        for f in 0..frames {
            let p = view_transition_progress(f, 0.2, 30);
            assert!(p > prev, "must be strictly increasing: {prev} -> {p}");
            prev = p;
            values.push(p);
        }
        // (f+1)/(N+1) is symmetric: values[i] + values[N-1-i] == 1.0.
        for i in 0..values.len() {
            let j = values.len() - 1 - i;
            assert!(
                (values[i] + values[j] - 1.0).abs() < 1e-9,
                "not symmetric about the midpoint: values[{i}]={} values[{j}]={}",
                values[i],
                values[j]
            );
        }
    }

    #[test]
    fn single_frame_transition_does_not_panic_and_stays_open() {
        let p = view_transition_progress(0, 1.0 / 60.0, 30);
        assert!(p.is_finite());
        assert!(p > 0.0 && p < 1.0);
    }

    // Contrast with `transition_progress`, which the SlideTransition path
    // deliberately pins to exactly 0.0/1.0 at its ends (new content there,
    // not a repeat — see `transition_progress`'s doc). A `ViewTransition`
    // must not share that behaviour.
    #[test]
    fn differs_from_the_closed_interval_slide_transition_formula() {
        let frames = (0.2_f64 * 30.0).round() as u32;
        assert_eq!(transition_progress(0, 0.2, 30), 0.0);
        assert!(view_transition_progress(0, 0.2, 30) > 0.0);
        assert_eq!(transition_progress(frames - 1, 0.2, 30), 1.0);
        assert!(view_transition_progress(frames - 1, 0.2, 30) < 1.0);
    }
}

// Constat 6, black-box: an actual ViewTransition composite must not be
// byte-identical to the Normal frame sitting next to it in the output
// stream (the last frame of view A, or the first frame of view B).
#[cfg(test)]
mod view_transition_no_duplicate_frame_tests {
    use super::*;
    use crate::loader::load_scenario_from_source;

    fn two_view_scenario() -> String {
        r##"{
            "video": { "width": 64, "height": 64, "fps": 30, "background": "#000000" },
            "composition": [
                { "type": "slide", "scenes": [
                    { "duration": 0.5, "children": [
                        { "type": "shape", "shape": "rect", "position": "absolute",
                          "x": 0, "y": 0, "style": { "width": 64, "height": 64, "background": "#ffffff" },
                          "animation": [{ "name": "fade_in", "duration": 0.5, "easing": "linear" }] }
                    ] }
                ] },
                { "type": "slide",
                  "transition": { "type": "fade", "duration": 0.2 },
                  "scenes": [
                    { "duration": 0.5, "children": [
                        { "type": "shape", "shape": "rect", "position": "absolute",
                          "x": 0, "y": 0, "style": { "width": 64, "height": 64, "background": "#000000" } }
                    ] }
                ] }
            ]
        }"##
        .to_string()
    }

    #[test]
    fn transition_frames_are_never_byte_identical_to_their_adjacent_normal_frame() {
        let json = two_view_scenario();
        let scenario = load_scenario_from_source(None, Some(&json)).expect("load");
        let tasks = build_frame_tasks(&scenario);

        // Locate: last Normal frame of view 0, first/last ViewTransition
        // frame, first Normal frame of view 1 — in output order, exactly as
        // they sit in the encoded stream.
        let last_normal_a = tasks
            .iter()
            .rposition(|t| matches!(t, FrameTask::Normal { view_idx: 0, .. }))
            .expect("view 0 has normal frames");
        let vt_frames: Vec<usize> = tasks
            .iter()
            .enumerate()
            .filter(|(_, t)| matches!(t, FrameTask::ViewTransition { .. }))
            .map(|(i, _)| i)
            .collect();
        assert!(!vt_frames.is_empty(), "expected ViewTransition frames");
        let first_vt = vt_frames[0];
        let last_vt = *vt_frames.last().unwrap();
        let first_normal_b = tasks
            .iter()
            .position(|t| matches!(t, FrameTask::Normal { view_idx: 1, .. }))
            .expect("view 1 has normal frames");
        assert_eq!(last_normal_a + 1, first_vt, "no gap before the transition");
        assert_eq!(last_vt + 1, first_normal_b, "no gap after the transition");

        let render = |i: usize| render_frame_task(&scenario.video, &scenario, &tasks[i]).unwrap();
        let buf_last_normal_a = render(last_normal_a);
        let buf_first_vt = render(first_vt);
        let buf_last_vt = render(last_vt);
        let buf_first_normal_b = render(first_normal_b);

        assert_ne!(
            buf_last_normal_a, buf_first_vt,
            "first ViewTransition frame duplicates the last Normal frame of view 0"
        );
        assert_ne!(
            buf_last_vt, buf_first_normal_b,
            "last ViewTransition frame duplicates the first Normal frame of view 1"
        );
    }
}

#[cfg(test)]
mod hit_tests {
    use super::*;

    const SCENARIO: &str = r##"{
        "video": { "width": 800, "height": 600, "background": "#101418" },
        "scenes": [ { "duration": 1.0, "children": [
            { "type": "text", "content": "Hello", "style": { "font-size": 48 } }
        ] } ]
    }"##;

    #[test]
    fn normal_frame_returns_text_hit() {
        let scenario = crate::loader::load_scenario_from_source(None, Some(SCENARIO)).unwrap();
        let tasks = crate::encode::build_frame_tasks(&scenario);
        let hits = render_frame_task_hits(&scenario, &tasks[0]);
        assert!(
            hits.iter().any(|h| h.kind == "text"),
            "expected a text hit, got {hits:?}"
        );
    }
}

#[cfg(test)]
mod segment_tests {
    use super::*;
    use crate::loader::load_scenario_from_source;
    use crate::schema::ResolvedScenario;

    fn scenario(json: &str) -> ResolvedScenario {
        load_scenario_from_source(None, Some(json)).expect("load")
    }

    fn two_view_json(second_text: &str, with_view_transition: bool) -> String {
        let vt = if with_view_transition {
            r#""transition": {"type": "fade", "duration": 0.2},"#
        } else {
            ""
        };
        format!(
            r##"{{
            "video": {{"width": 32, "height": 32, "fps": 10}},
            "composition": [
                {{"type": "slide", "scenes": [
                    {{"duration": 0.2, "children": [{{"type": "text", "content": "one"}}]}},
                    {{"duration": 0.2, "children": [{{"type": "text", "content": "{second_text}"}}]}}
                ]}},
                {{"type": "slide", {vt} "scenes": [
                    {{"duration": 0.2, "children": [{{"type": "text", "content": "three"}}]}}
                ]}}
            ]
        }}"##
        )
    }

    #[test]
    fn slots_enumerate_scenes_and_view_transitions_in_output_order() {
        let s = scenario(&two_view_json("two", true));
        let slots = segment_slots(&s).expect("all-slide");
        assert_eq!(
            slots,
            vec![
                SegmentSlot::Scene {
                    view_idx: 0,
                    scene_idx: 0
                },
                SegmentSlot::Scene {
                    view_idx: 0,
                    scene_idx: 1
                },
                SegmentSlot::ViewTransition { view_idx: 1 },
                SegmentSlot::Scene {
                    view_idx: 1,
                    scene_idx: 0
                },
            ]
        );
        let no_vt = scenario(&two_view_json("two", false));
        assert_eq!(segment_slots(&no_vt).unwrap().len(), 3);
    }

    #[test]
    fn slot_tasks_reproduce_the_full_builder_exactly() {
        // Concatenated per-slot tasks must equal build_frame_tasks: the
        // incremental output stream may not differ from a full encode.
        for with_vt in [false, true] {
            let s = scenario(&two_view_json("two", with_vt));
            let slots = segment_slots(&s).unwrap();
            let mut next_frame = 0u32;
            let concatenated: Vec<String> = slots
                .iter()
                .flat_map(|slot| {
                    let tasks = build_slot_frame_tasks(&s, slot, next_frame);
                    next_frame += tasks.len() as u32;
                    tasks
                })
                .map(|t| format!("{t:?}"))
                .collect();
            let full: Vec<String> = build_frame_tasks(&s)
                .iter()
                .map(|t| format!("{t:?}"))
                .collect();
            assert_eq!(concatenated, full, "with_vt={with_vt}");
        }
    }

    #[test]
    fn plan_dirty_marks_changed_scene_and_dependent_view_transition() {
        let base = scenario(&two_view_json("two", true));
        let slots = segment_slots(&base).unwrap();
        let base_hashes: Vec<u64> = slots.iter().map(|s| slot_hash(&base, s)).collect();
        let prev: Vec<SceneSegment> = base_hashes
            .iter()
            .map(|h| SceneSegment {
                h264_data: Vec::new(),
                scene_hash: *h,
            })
            .collect();

        // Unchanged: nothing re-renders.
        let clean = plan_dirty(&base, &slots, &base_hashes, Some(&prev));
        assert!(clean.iter().all(|d| !d), "clean plan: {clean:?}");

        // Change scene (0,1): it is the last scene of view 0, so the view
        // transition into view 1 depends on it and must re-render too.
        let changed = scenario(&two_view_json("TWO CHANGED", true));
        let new_hashes: Vec<u64> = slots.iter().map(|s| slot_hash(&changed, s)).collect();
        let dirty = plan_dirty(&changed, &slots, &new_hashes, Some(&prev));
        assert_eq!(
            dirty,
            vec![false, true, true, false],
            "scene(0,1) and VT(1) must re-render: {dirty:?}"
        );

        // Layout change (slot count mismatch) → everything re-renders.
        let no_vt = scenario(&two_view_json("two", false));
        let nv_slots = segment_slots(&no_vt).unwrap();
        let nv_hashes: Vec<u64> = nv_slots.iter().map(|s| slot_hash(&no_vt, s)).collect();
        let all = plan_dirty(&no_vt, &nv_slots, &nv_hashes, Some(&prev));
        assert!(all.iter().all(|d| *d));
    }
}
