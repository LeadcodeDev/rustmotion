use crate::engine::animator::ease;
use crate::engine::transition::{apply_transition, camera_pan_transition, TransitionOptions};
use crate::error::{Result, RustmotionError};
use crate::schema::{
    EasingType, ResolvedScenario as Scenario, ResolvedView, Scene, SceneStart, SceneTail, SnapMode,
    TimingMode, TransitionType, VideoConfig, ViewType,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CompositeParticipant {
    pub scene_idx: usize,
    pub frame_in_scene: u32,
    pub scene_total_frames: u32,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum FrameTask {
    Normal {
        global_frame: u32,
        view_idx: usize,
        scene_idx: usize,
        frame_in_scene: u32,
        scene_total_frames: u32,
    },
    SlideTransition {
        global_frame: u32,
        view_idx: usize,
        scene_a_idx: usize,
        scene_b_idx: usize,
        frame_in_transition: u32,
        scene_a_frame_offset: u32,
        scene_a_frame_advance: bool,
        scene_a_total_frames: u32,
        scene_b_total_frames: u32,
        transition_type: TransitionType,
        options: TransitionOptions,
        transition_duration: f64,
        easing: EasingType,
    },
    Composite {
        global_frame: u32,
        view_idx: usize,
        participants: Vec<CompositeParticipant>,
    },
    WorldFrame {
        global_frame: u32,
        view_idx: usize,
        frame_in_view: u32,
        view_total_frames: u32,
    },
    ViewTransition {
        global_frame: u32,
        view_a_idx: usize,
        view_b_idx: usize,
        frame_in_transition: u32,
        transition_type: TransitionType,
        options: TransitionOptions,
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
                *frame_in_scene as f64 / config.fps as f64,
            );
            Ok(pixels)
        }
        FrameTask::Composite {
            global_frame,
            view_idx,
            participants,
        } => {
            use crate::engine::render::composite::composite_over;
            let scenario_time = *global_frame as f64 / config.fps as f64;
            let view = &scenario.views[*view_idx];
            let scaled_w = (config.width as f32 * scale_factor) as u32;
            let scaled_h = (config.height as f32 * scale_factor) as u32;

            let bottom = participants
                .first()
                .ok_or(RustmotionError::SurfaceCreation)?;
            let mut pixels = render_scene_frame_scaled(
                config,
                &view.scenes[bottom.scene_idx],
                bottom.frame_in_scene,
                scenario_time,
                bottom.scene_total_frames,
                scale_factor,
            )?;

            for participant in &participants[1..] {
                let overlay = render_scene_fg_scaled(
                    config,
                    &view.scenes[participant.scene_idx],
                    participant.frame_in_scene,
                    scenario_time,
                    participant.scene_total_frames,
                    scale_factor,
                )?;
                composite_over(&mut pixels, &overlay);
            }

            apply_post_effects(
                &mut pixels,
                scaled_w,
                scaled_h,
                &view.scenes[bottom.scene_idx].effects,
                bottom.frame_in_scene,
                bottom.frame_in_scene as f64 / config.fps as f64,
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
            scene_a_frame_advance,
            scene_a_total_frames,
            scene_b_total_frames,
            transition_type,
            options,
            transition_duration,
            easing,
        } => {
            let scenario_time = *global_frame as f64 / config.fps as f64;
            let scenes = &scenario.views[*view_idx].scenes;
            let scaled_w = (config.width as f32 * scale_factor) as u32;
            let scaled_h = (config.height as f32 * scale_factor) as u32;
            let fps = config.fps;
            let progress = transition_progress(*frame_in_transition, *transition_duration, fps);
            let frame_a_idx = if *scene_a_frame_advance {
                scene_a_frame_offset + frame_in_transition
            } else {
                *scene_a_frame_offset
            };

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
                let pan_bg = scenes[*scene_b_idx]
                    .transition
                    .as_ref()
                    .map(|t| t.background)
                    .unwrap_or_default();
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
                    *frame_in_transition as f64 / config.fps as f64,
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

            let progress = eased_transition_progress(progress, easing);
            let mut composited = apply_transition(
                &frame_a,
                &frame_b,
                scaled_w,
                scaled_h,
                progress,
                transition_type,
                options,
            );
            apply_post_effects(
                &mut composited,
                scaled_w,
                scaled_h,
                &scenes[*scene_b_idx].effects,
                *frame_in_transition,
                *frame_in_transition as f64 / config.fps as f64,
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
                    *frame_in_view as f64 / config.fps as f64,
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
            options,
            transition_duration,
            easing,
        } => {
            let scenario_time = *global_frame as f64 / config.fps as f64;
            let scaled_w = (config.width as f32 * scale_factor) as u32;
            let scaled_h = (config.height as f32 * scale_factor) as u32;
            let fps = config.fps;
            let progress =
                view_transition_progress(*frame_in_transition, *transition_duration, fps);
            let progress = eased_transition_progress(progress, easing);

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
                options,
            );
            if let Some(first_scene) = view_b.scenes.first() {
                apply_post_effects(
                    &mut composited,
                    scaled_w,
                    scaled_h,
                    &first_scene.effects,
                    *frame_in_transition,
                    *frame_in_transition as f64 / config.fps as f64,
                );
            }
            Ok(composited)
        }
    }
}

fn transition_progress(frame_in_transition: u32, transition_duration: f64, fps: u32) -> f64 {
    let transition_frames = (transition_duration * fps as f64).round() as u32;
    frame_in_transition as f64 / transition_frames.saturating_sub(1).max(1) as f64
}

fn eased_transition_progress(progress: f64, easing: &EasingType) -> f64 {
    ease(progress, easing)
}

fn view_transition_progress(frame_in_transition: u32, transition_duration: f64, fps: u32) -> f64 {
    let transition_frames = (transition_duration * fps as f64).round().max(1.0);
    (frame_in_transition as f64 + 1.0) / (transition_frames + 1.0)
}

fn render_last_frame_of_view(
    config: &VideoConfig,
    view: &ResolvedView,
    fps: u32,
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
                        options: transition.into(),
                        transition_duration: transition.duration,
                        easing: transition.easing.clone(),
                    });
                }
            }
        }

        match view.view_type {
            ViewType::Slide => match view_timing(view) {
                TimingMode::V1 => build_slide_view_tasks(&mut tasks, view_idx, view, fps),
                TimingMode::V2 => build_slide_view_tasks_v2(&mut tasks, view_idx, view, fps),
            },
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
                    scene_a_frame_advance: true,
                    scene_a_total_frames: scene_frames,
                    scene_b_total_frames: scene_b_frames,
                    transition_type: transition.transition_type.clone(),
                    options: transition.into(),
                    transition_duration: outgoing_effective_duration,
                    easing: easing.clone(),
                });
            }
        }
    }
}

fn view_timing(view: &ResolvedView) -> TimingMode {
    view.scenes
        .first()
        .map(|s| s.resolved_timing)
        .unwrap_or_default()
}

fn v2_incoming_transition_frames(entering: &Scene, entering_frames: u32, fps: u32) -> u32 {
    let Some(transition) = entering.transition.as_ref() else {
        return 0;
    };
    let raw = (transition.duration * fps as f64).round() as u32;
    raw.min(entering_frames)
}

fn snap_seconds_to_beat(seconds: f64, beat_offset: f64, bpm: f64) -> f64 {
    let beat_len = 60.0 / bpm;
    let n = ((seconds - beat_offset) / beat_len).round();
    beat_offset + n * beat_len
}

fn v2_resolve_at_frames(
    scene: &Scene,
    scene_idx: usize,
    fps: u32,
    fallback: u32,
    snap: SnapDuringPlacement,
) -> u32 {
    let SceneStart::At(ref tp) = scene.at else {
        return fallback;
    };
    let seconds = match tp.resolve_absolute(&scene.resolved_time_ctx) {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "warning: scene {scene_idx}'s `at` could not be resolved ({e}); falling back \
                 to automatic placement"
            );
            return fallback;
        }
    };
    let seconds = match (snap, scene.resolved_snap) {
        (SnapDuringPlacement::Apply, Some(SnapMode::Beat)) => match scene.resolved_time_ctx.bpm {
            Some(bpm) => snap_seconds_to_beat(seconds, scene.resolved_time_ctx.beat_offset, bpm),
            None => seconds,
        },
        _ => seconds,
    };
    (seconds * fps as f64).round().max(0.0) as u32
}

fn hold_scene_frame(
    tasks: &mut Vec<FrameTask>,
    view_idx: usize,
    scene_idx: usize,
    frame_in_scene: u32,
    scene_total_frames: u32,
    count: u32,
) {
    for _ in 0..count {
        tasks.push(FrameTask::Normal {
            global_frame: tasks.len() as u32,
            view_idx,
            scene_idx,
            frame_in_scene,
            scene_total_frames,
        });
    }
}

fn build_slide_view_tasks_v2(
    tasks: &mut Vec<FrameTask>,
    view_idx: usize,
    view: &ResolvedView,
    fps: u32,
) {
    let scenes = &view.scenes;
    if scenes.is_empty() {
        return;
    }

    let duration_frames: Vec<u32> = scenes
        .iter()
        .map(|s| (s.duration * fps as f64).round() as u32)
        .collect();

    let transition_frames: Vec<u32> = scenes
        .iter()
        .enumerate()
        .map(|(k, s)| {
            if k == 0 {
                0
            } else {
                v2_incoming_transition_frames(s, duration_frames[k], fps)
            }
        })
        .collect();

    let as_written = v2_scene_starts(scenes, &duration_frames, fps, SnapDuringPlacement::Ignore);
    let author_overlaps = v2_has_overlap(&as_written, &duration_frames, &transition_frames);

    let starts = v2_scene_starts(scenes, &duration_frames, fps, SnapDuringPlacement::Apply);

    if author_overlaps {
        v2_build_composited(tasks, view_idx, scenes, &duration_frames, &starts);
        return;
    }

    let starts = v2_clamp_forward(&starts, &duration_frames);
    v2_build_sequential(
        tasks,
        view_idx,
        scenes,
        &duration_frames,
        &transition_frames,
        &starts,
        fps,
    );
}

#[derive(Clone, Copy, PartialEq)]
enum SnapDuringPlacement {
    Apply,
    Ignore,
}

fn v2_scene_starts(
    scenes: &[Scene],
    duration_frames: &[u32],
    fps: u32,
    snap: SnapDuringPlacement,
) -> Vec<u32> {
    let mut starts = Vec::with_capacity(scenes.len());
    let mut cursor: u32 = 0;
    for (i, scene) in scenes.iter().enumerate() {
        let start = match scene.at {
            SceneStart::Auto(_) => cursor,
            SceneStart::At(_) => v2_resolve_at_frames(scene, i, fps, cursor, snap),
        };
        starts.push(start);
        cursor = start + duration_frames[i];
    }
    starts
}

fn v2_has_overlap(starts: &[u32], duration_frames: &[u32], transition_frames: &[u32]) -> bool {
    (1..starts.len()).any(|i| {
        let previous_end = starts[i - 1] + duration_frames[i - 1];
        starts[i] + transition_frames[i] < previous_end
    })
}

fn v2_clamp_forward(starts: &[u32], duration_frames: &[u32]) -> Vec<u32> {
    let mut out = Vec::with_capacity(starts.len());
    let mut cursor: u32 = 0;
    for (i, requested) in starts.iter().enumerate() {
        let start = if *requested < cursor {
            eprintln!(
                "warning: scene {i}'s `at` lands before the previous scene's own window ends \
                 ({:.3}s of frames) once snapped to the beat grid, and has been pushed forward. \
                 Snapping quantises a cut, it does not ask two scenes to play at once — write \
                 the overlap into `at` itself if that is what you want.",
                cursor as f64
            );
            cursor
        } else {
            *requested
        };
        out.push(start);
        cursor = start + duration_frames[i];
    }
    out
}

fn v2_build_sequential(
    tasks: &mut Vec<FrameTask>,
    view_idx: usize,
    scenes: &[Scene],
    duration_frames: &[u32],
    transition_frames: &[u32],
    starts: &[u32],
    fps: u32,
) {
    let mut cursor: u32 = 0;
    for (i, scene) in scenes.iter().enumerate() {
        let start = starts[i];
        if start > cursor {
            let gap = start - cursor;
            if i == 0 {
                hold_scene_frame(tasks, view_idx, 0, 0, duration_frames[0], gap);
            } else {
                let prev = i - 1;
                hold_scene_frame(
                    tasks,
                    view_idx,
                    prev,
                    duration_frames[prev].saturating_sub(1),
                    duration_frames[prev],
                    gap,
                );
            }
        }

        for f in transition_frames[i]..duration_frames[i] {
            tasks.push(FrameTask::Normal {
                global_frame: tasks.len() as u32,
                view_idx,
                scene_idx: i,
                frame_in_scene: f,
                scene_total_frames: duration_frames[i],
            });
        }
        cursor = start + duration_frames[i];

        if let Some(next_scene) = scenes.get(i + 1) {
            let d = transition_frames[i + 1];
            if d > 0 {
                let transition = next_scene
                    .transition
                    .as_ref()
                    .expect("transition_frames[i+1] > 0 implies scenes[i+1].transition.is_some()");
                let advance = matches!(scene.tail, SceneTail::Continue);
                let offset = if advance {
                    duration_frames[i]
                } else {
                    duration_frames[i].saturating_sub(1)
                };
                let scene_b_frames = duration_frames[i + 1];
                let easing = transition.easing.clone();
                for f in 0..d {
                    tasks.push(FrameTask::SlideTransition {
                        global_frame: tasks.len() as u32,
                        view_idx,
                        scene_a_idx: i,
                        scene_b_idx: i + 1,
                        frame_in_transition: f,
                        scene_a_frame_offset: offset,
                        scene_a_frame_advance: advance,
                        scene_a_total_frames: duration_frames[i],
                        scene_b_total_frames: scene_b_frames,
                        transition_type: transition.transition_type.clone(),
                        options: transition.into(),
                        transition_duration: d as f64 / fps as f64,
                        easing: easing.clone(),
                    });
                }
            }
        }
    }
}

fn v2_build_composited(
    tasks: &mut Vec<FrameTask>,
    view_idx: usize,
    scenes: &[Scene],
    duration_frames: &[u32],
    starts: &[u32],
) {
    for (i, scene) in scenes.iter().enumerate() {
        if i > 0 && scene.transition.is_some() {
            let previous_end = starts[i - 1] + duration_frames[i - 1];
            if starts[i] < previous_end {
                eprintln!(
                    "warning: scene {i} both overlaps scene {} on the absolute timeline and \
                     declares a `transition`. A transition composites two finished frame \
                     buffers and an overlap composites live scenes; the two cannot both \
                     describe the same frames. The transition is ignored here — remove it, or \
                     move `at` so the scenes no longer overlap.",
                    i - 1
                );
            }
        }
    }

    let total_frames = starts
        .iter()
        .zip(duration_frames)
        .map(|(start, duration)| start + duration)
        .max()
        .unwrap_or(0);

    for frame in 0..total_frames {
        let participants: Vec<CompositeParticipant> = starts
            .iter()
            .zip(duration_frames)
            .enumerate()
            .filter(|(_, (start, duration))| frame >= **start && frame < **start + **duration)
            .map(|(scene_idx, (start, duration))| CompositeParticipant {
                scene_idx,
                frame_in_scene: frame - start,
                scene_total_frames: *duration,
            })
            .collect();

        match participants.len() {
            0 => {
                let last_live = starts
                    .iter()
                    .zip(duration_frames)
                    .enumerate()
                    .filter(|(_, (start, duration))| **start + **duration <= frame)
                    .max_by_key(|(_, (start, duration))| **start + **duration);
                match last_live {
                    Some((scene_idx, (_, duration))) => tasks.push(FrameTask::Normal {
                        global_frame: tasks.len() as u32,
                        view_idx,
                        scene_idx,
                        frame_in_scene: duration.saturating_sub(1),
                        scene_total_frames: *duration,
                    }),
                    None => tasks.push(FrameTask::Normal {
                        global_frame: tasks.len() as u32,
                        view_idx,
                        scene_idx: 0,
                        frame_in_scene: 0,
                        scene_total_frames: duration_frames[0],
                    }),
                }
            }
            1 => tasks.push(FrameTask::Normal {
                global_frame: tasks.len() as u32,
                view_idx,
                scene_idx: participants[0].scene_idx,
                frame_in_scene: participants[0].frame_in_scene,
                scene_total_frames: participants[0].scene_total_frames,
            }),
            _ => tasks.push(FrameTask::Composite {
                global_frame: tasks.len() as u32,
                view_idx,
                participants,
            }),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentSlot {
    Scene { view_idx: usize, scene_idx: usize },
    ViewTransition { view_idx: usize },
}

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

impl FrameTask {
    fn set_global_frame(&mut self, frame: u32) {
        match self {
            FrameTask::Normal { global_frame, .. }
            | FrameTask::SlideTransition { global_frame, .. }
            | FrameTask::Composite { global_frame, .. }
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
                        options: transition.into(),
                        transition_duration: transition.duration,
                        easing: transition.easing.clone(),
                    });
                }
            }
            tasks
        }
    };
    for (i, task) in tasks.iter_mut().enumerate() {
        task.set_global_frame(start_frame + i as u32);
    }
    tasks
}

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
                scene_a_frame_advance: true,
                scene_a_total_frames: scene_frames,
                scene_b_total_frames: scene_b_frames,
                transition_type: transition.transition_type.clone(),
                options: transition.into(),
                transition_duration: outgoing_effective_duration,
                easing: easing.clone(),
            });
        }
    }

    tasks
}

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

    #[test]
    fn last_emitted_frame_reaches_exactly_one() {
        let p = transition_progress(22, 0.75, 30);
        assert_eq!(
            p, 1.0,
            "last frame of a 23-frame/22.5-raw transition must be exactly 1.0, got {p}"
        );
        assert!((22.0 / 22.5 - p).abs() > 0.02);
    }

    #[test]
    fn first_frame_is_zero() {
        assert_eq!(transition_progress(0, 0.75, 30), 0.0);
    }

    #[test]
    fn progress_is_monotonic_and_bounded() {
        let frames = (0.75_f64 * 30.0).round() as u32;
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

    #[test]
    fn exact_integer_duration_still_reaches_one() {
        let p = transition_progress(14, 0.5, 30);
        assert_eq!(p, 1.0);
    }

    #[test]
    fn single_frame_transition_does_not_panic() {
        let p = transition_progress(0, 1.0 / 60.0, 30);
        assert!(p.is_finite());
    }
}

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
        let pair = vec![scene(2.0), scene_with_transition(2.0, 0.5)];
        let (frames, effective_duration) = actual_outgoing_transition(&pair, 0, 30);
        assert_eq!(frames, 15);
        assert!((effective_duration - 0.5).abs() < 1e-9);
    }

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

        let scene_b_frames = (2.0_f64 * 30.0).round() as u32;
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

        assert_eq!(tasks.len(), 60, "tasks: {tasks:?}");
    }
}

#[cfg(test)]
mod view_transition_progress_tests {
    use super::*;

    #[test]
    fn never_reaches_either_endpoint() {
        let frames = (0.2_f64 * 30.0).round() as u32;
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
        let frames = (0.2_f64 * 30.0).round() as u32;
        let mut prev = 0.0;
        let mut values = Vec::new();
        for f in 0..frames {
            let p = view_transition_progress(f, 0.2, 30);
            assert!(p > prev, "must be strictly increasing: {prev} -> {p}");
            prev = p;
            values.push(p);
        }
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

    #[test]
    fn differs_from_the_closed_interval_slide_transition_formula() {
        let frames = (0.2_f64 * 30.0).round() as u32;
        assert_eq!(transition_progress(0, 0.2, 30), 0.0);
        assert!(view_transition_progress(0, 0.2, 30) > 0.0);
        assert_eq!(transition_progress(frames - 1, 0.2, 30), 1.0);
        assert!(view_transition_progress(frames - 1, 0.2, 30) < 1.0);
    }
}

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

        let clean = plan_dirty(&base, &slots, &base_hashes, Some(&prev));
        assert!(clean.iter().all(|d| !d), "clean plan: {clean:?}");

        let changed = scenario(&two_view_json("TWO CHANGED", true));
        let new_hashes: Vec<u64> = slots.iter().map(|s| slot_hash(&changed, s)).collect();
        let dirty = plan_dirty(&changed, &slots, &new_hashes, Some(&prev));
        assert_eq!(
            dirty,
            vec![false, true, true, false],
            "scene(0,1) and VT(1) must re-render: {dirty:?}"
        );

        let no_vt = scenario(&two_view_json("two", false));
        let nv_slots = segment_slots(&no_vt).unwrap();
        let nv_hashes: Vec<u64> = nv_slots.iter().map(|s| slot_hash(&no_vt, s)).collect();
        let all = plan_dirty(&no_vt, &nv_slots, &nv_hashes, Some(&prev));
        assert!(all.iter().all(|d| *d));
    }
}

#[cfg(test)]
mod timing_v2_tests {
    use super::*;
    use crate::loader::load_scenario_from_source;
    use crate::schema::ResolvedScenario;

    fn six_scene_json(timing: Option<&str>) -> String {
        let timing_field = timing
            .map(|t| format!(r#""timing": "{t}","#))
            .unwrap_or_default();
        format!(
            r##"{{
            "video": {{"width": 64, "height": 64, "fps": 20}},
            {timing_field}
            "scenes": [
                {{"duration": 2.2, "children": []}},
                {{"duration": 2.6, "children": [],
                  "transition": {{"type": "fade", "duration": 0.3}}}},
                {{"duration": 2.6, "children": [],
                  "transition": {{"type": "fade", "duration": 0.3}}}},
                {{"duration": 3.12, "children": [],
                  "transition": {{"type": "fade", "duration": 0.3}}}},
                {{"duration": 2.08, "children": [],
                  "transition": {{"type": "fade", "duration": 0.25}}}},
                {{"duration": 2.4, "children": [],
                  "transition": {{"type": "fade", "duration": 0.35}}}}
            ]
        }}"##
        )
    }

    fn load(json: &str) -> ResolvedScenario {
        load_scenario_from_source(None, Some(json)).expect("load")
    }

    fn overlapping_json(second_at: &str) -> String {
        format!(
            r##"{{
            "video": {{"width": 64, "height": 64, "fps": 10}},
            "timing": "v2",
            "composition": [{{"type": "slide", "scenes": [
                {{"duration": 3.0, "children": []}},
                {{"duration": 1.0, "at": "{second_at}", "children": []}}
            ]}}]
        }}"##
        )
    }

    fn composite_participants(tasks: &[FrameTask]) -> Vec<Vec<usize>> {
        tasks
            .iter()
            .filter_map(|t| match t {
                FrameTask::Composite { participants, .. } => {
                    Some(participants.iter().map(|p| p.scene_idx).collect())
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn an_explicit_at_that_overlaps_composites_instead_of_being_clamped() {
        let scenario = load(&overlapping_json("@1.0s"));
        let tasks = build_frame_tasks(&scenario);

        let composites = composite_participants(&tasks);
        assert_eq!(
            composites.len(),
            10,
            "scene 1 runs 1.0s at 10fps entirely inside scene 0, so every one of its frames \
             composites: got {} composite frames",
            composites.len()
        );
        assert!(
            composites.iter().all(|p| p == &vec![0, 1]),
            "each composite frame carries both scenes, bottom first: got {composites:?}"
        );
        assert_eq!(
            tasks.len(),
            30,
            "the view lasts max(at + duration) = 3.0s, not the 4.0s a clamped timeline gave"
        );
    }

    #[test]
    fn a_composited_scene_advances_its_own_clock_from_its_own_at() {
        let scenario = load(&overlapping_json("@1.5s"));
        let tasks = build_frame_tasks(&scenario);

        let frames_of_scene_1: Vec<u32> = tasks
            .iter()
            .filter_map(|t| match t {
                FrameTask::Composite { participants, .. } => participants
                    .iter()
                    .find(|p| p.scene_idx == 1)
                    .map(|p| p.frame_in_scene),
                _ => None,
            })
            .collect();

        assert_eq!(
            frames_of_scene_1,
            (0..10).collect::<Vec<u32>>(),
            "the overlapping scene starts its own clock at 0 when its window opens, and \
             advances one frame per output frame"
        );
    }

    #[test]
    fn a_scene_spanning_several_others_stays_in_every_one_of_their_frames() {
        let scenario = load(
            r##"{
            "video": {"width": 64, "height": 64, "fps": 10},
            "timing": "v2",
            "composition": [{"type": "slide", "scenes": [
                {"duration": 3.0, "children": []},
                {"duration": 1.0, "at": "@0.0s", "children": []},
                {"duration": 1.0, "at": "@1.0s", "children": []},
                {"duration": 1.0, "at": "@2.0s", "children": []}
            ]}]
        }"##,
        );
        let tasks = build_frame_tasks(&scenario);

        assert_eq!(tasks.len(), 30, "the spanning scene sets the view's length");
        let composites = composite_participants(&tasks);
        assert_eq!(
            composites.len(),
            30,
            "scene 0 spans the whole view, so every frame has two live scenes"
        );
        assert!(
            composites.iter().all(|p| p[0] == 0),
            "the spanning scene is always the bottom participant, so it supplies the \
             background every frame: got {composites:?}"
        );
        let second: Vec<usize> = composites.iter().map(|p| p[1]).collect();
        assert_eq!(second[0], 1, "beat 1 on top at frame 0");
        assert_eq!(second[10], 2, "beat 2 on top at frame 10");
        assert_eq!(second[20], 3, "beat 3 on top at frame 20");
    }

    #[test]
    fn a_gap_between_overlapping_scenes_holds_the_last_live_frame() {
        let scenario = load(
            r##"{
            "video": {"width": 64, "height": 64, "fps": 10},
            "timing": "v2",
            "composition": [{"type": "slide", "scenes": [
                {"duration": 1.0, "children": []},
                {"duration": 1.0, "at": "@0.5s", "children": []},
                {"duration": 1.0, "at": "@3.0s", "children": []}
            ]}]
        }"##,
        );
        let tasks = build_frame_tasks(&scenario);
        assert_eq!(tasks.len(), 40, "the view runs to 3.0s + 1.0s");

        let held: Vec<(usize, u32)> = tasks[15..30]
            .iter()
            .filter_map(|t| match t {
                FrameTask::Normal {
                    scene_idx,
                    frame_in_scene,
                    ..
                } => Some((*scene_idx, *frame_in_scene)),
                _ => None,
            })
            .collect();
        assert!(
            held.iter().all(|(idx, frame)| *idx == 1 && *frame == 9),
            "the gap holds the last frame of the scene that ended most recently: got {held:?}"
        );
    }

    #[test]
    fn snapping_a_cut_earlier_never_creates_an_overlap() {
        let scenario = load(
            r##"{
            "video": {"width": 64, "height": 64, "fps": 10},
            "timing": "v2",
            "bpm": 24.0,
            "snap": "beat",
            "composition": [{"type": "slide", "scenes": [
                {"duration": 3.0, "children": []},
                {"duration": 3.0, "at": "@3.0s", "children": []}
            ]}]
        }"##,
        );
        let tasks = build_frame_tasks(&scenario);

        assert!(
            composite_participants(&tasks).is_empty(),
            "snapping quantises a cut; it must never be read as asking two scenes to play at \
             once, or `migrate --snap` would silently shorten every file it touches"
        );
        assert_eq!(
            tasks.len(),
            60,
            "both scenes keep their full duration once the snapped start is pushed forward"
        );
    }

    #[test]
    fn v2_timing_renders_the_full_declared_duration_with_no_subtraction() {
        let scenario = load(&six_scene_json(Some("v2")));
        let tasks = build_frame_tasks(&scenario);
        let seconds = tasks.len() as f64 / scenario.video.fps as f64;
        assert_eq!(
            seconds,
            15.0,
            "expected exactly 15.0s under timing: v2 (at_last + duration_last, no \
             subtraction), got {seconds}s ({} frames)",
            tasks.len()
        );
    }

    #[test]
    fn absent_timing_still_subtracts_transition_durations_like_today() {
        let scenario = load(&six_scene_json(None));
        let tasks = build_frame_tasks(&scenario);
        let seconds = tasks.len() as f64 / scenario.video.fps as f64;
        assert_eq!(
            seconds, 13.5,
            "expected exactly 13.5s under the default (v1) subtracting semantics, got {seconds}s"
        );
    }

    #[test]
    fn explicit_timing_v1_matches_the_default() {
        let default_tasks = build_frame_tasks(&load(&six_scene_json(None)));
        let explicit_tasks = build_frame_tasks(&load(&six_scene_json(Some("v1"))));
        assert_eq!(default_tasks.len(), explicit_tasks.len());
    }

    #[test]
    fn frame_range_still_addresses_the_same_dense_index_space_under_v2() {
        let scenario = load(&six_scene_json(Some("v2")));
        let full = build_frame_tasks(&scenario);
        let total = full.len() as u32;
        assert_eq!(total, 300, "15.0s @ 20fps must be exactly 300 frames");

        let (range_tasks, reported_total) =
            build_frame_tasks_range(&scenario, 100, 199).expect("range must be in bounds");
        assert_eq!(reported_total, total);
        assert_eq!(range_tasks.len(), 100);
        for (offset, task) in range_tasks.iter().enumerate() {
            let expected_global = 100 + offset as u32;
            let actual_global = match task {
                FrameTask::Normal { global_frame, .. } => *global_frame,
                FrameTask::SlideTransition { global_frame, .. } => *global_frame,
                FrameTask::Composite { global_frame, .. } => *global_frame,
                FrameTask::WorldFrame { global_frame, .. } => *global_frame,
                FrameTask::ViewTransition { global_frame, .. } => *global_frame,
            };
            assert_eq!(actual_global, expected_global);
        }

        assert!(build_frame_tasks_range(&scenario, 0, total).is_err());
    }

    fn two_scene_json(tail: Option<&str>, transition_duration: f64) -> String {
        let tail_field = tail
            .map(|t| format!(r#""tail": "{t}", "#))
            .unwrap_or_default();
        format!(
            r##"{{
            "video": {{"width": 64, "height": 64, "fps": 30}},
            "timing": "v2",
            "scenes": [
                {{{tail_field}"duration": 1.0, "children": []}},
                {{"duration": 1.0, "children": [],
                  "transition": {{"type": "fade", "duration": {transition_duration}}}}}
            ]
        }}"##
        )
    }

    #[test]
    fn v2_default_tail_freezes_scene_a_through_the_overlap() {
        let scenario = load(&two_scene_json(None, 0.2));
        let tasks = build_frame_tasks(&scenario);
        let transitions: Vec<_> = tasks
            .iter()
            .filter_map(|t| match t {
                FrameTask::SlideTransition {
                    scene_a_frame_offset,
                    scene_a_frame_advance,
                    scene_a_total_frames,
                    ..
                } => Some((
                    *scene_a_frame_offset,
                    *scene_a_frame_advance,
                    *scene_a_total_frames,
                )),
                _ => None,
            })
            .collect();
        assert!(!transitions.is_empty(), "expected transition frames");
        for (offset, advance, total_frames) in transitions {
            assert!(!advance, "default tail must not advance scene A's clock");
            assert_eq!(
                offset,
                total_frames - 1,
                "frozen scene A must always render its own last real frame"
            );
        }
    }

    #[test]
    fn v2_continue_tail_advances_scene_a_past_its_own_duration() {
        let scenario = load(&two_scene_json(Some("continue"), 0.2));
        let tasks = build_frame_tasks(&scenario);
        let transitions: Vec<_> = tasks
            .iter()
            .filter_map(|t| match t {
                FrameTask::SlideTransition {
                    scene_a_frame_offset,
                    scene_a_frame_advance,
                    scene_a_total_frames,
                    ..
                } => Some((
                    *scene_a_frame_offset,
                    *scene_a_frame_advance,
                    *scene_a_total_frames,
                )),
                _ => None,
            })
            .collect();
        assert!(!transitions.is_empty(), "expected transition frames");
        for (offset, advance, total_frames) in transitions {
            assert!(
                advance,
                "\"continue\" tail must keep scene A's clock advancing"
            );
            assert_eq!(
                offset, total_frames,
                "the overlap must pick up exactly where scene A's own Normal frames left off"
            );
        }
    }

    #[test]
    fn v2_scene_a_own_normal_frames_are_never_clipped() {
        let scenario = load(&two_scene_json(None, 0.5));
        let tasks = build_frame_tasks(&scenario);
        let scene_a_normal_frames: Vec<u32> = tasks
            .iter()
            .filter_map(|t| match t {
                FrameTask::Normal {
                    scene_idx: 0,
                    frame_in_scene,
                    ..
                } => Some(*frame_in_scene),
                _ => None,
            })
            .collect();
        let fps = scenario.video.fps;
        let expected_frames = (1.0 * fps as f64).round() as u32;
        assert_eq!(
            scene_a_normal_frames.len() as u32,
            expected_frames,
            "scene A's own 1.0s must render in full as Normal frames, unclipped: got {scene_a_normal_frames:?}"
        );
        assert_eq!(*scene_a_normal_frames.last().unwrap(), expected_frames - 1);
    }

    #[test]
    fn snap_beat_rounds_an_explicit_at_onto_the_grid() {
        let json = r##"{
            "video": {"width": 64, "height": 64, "fps": 20},
            "timing": "v2",
            "bpm": 120,
            "snap": "beat",
            "scenes": [
                {"duration": 1.0, "children": []},
                {"duration": 1.0, "children": [], "at": "1.8s"}
            ]
        }"##;
        let scenario = load(json);
        let tasks = build_frame_tasks(&scenario);
        let scene_1_start = tasks
            .iter()
            .find_map(|t| match t {
                FrameTask::Normal {
                    scene_idx: 1,
                    global_frame,
                    frame_in_scene: 0,
                    ..
                } => Some(*global_frame),
                _ => None,
            })
            .expect("scene 1 must have a frame_in_scene == 0 Normal task");
        let fps = scenario.video.fps as f64;
        assert_eq!(
            scene_1_start as f64 / fps,
            2.0,
            "snap: beat must round the off-grid 1.8s onto the 2.0s beat"
        );
    }

    #[test]
    fn at_beat_unit_resolves_against_the_scenarios_bpm_and_beat_offset() {
        let json = r##"{
            "video": {"width": 64, "height": 64, "fps": 20},
            "timing": "v2",
            "bpm": 120,
            "beat_offset": 2.2,
            "scenes": [
                {"duration": 2.0, "children": []},
                {"duration": 1.0, "children": [], "at": "1b"}
            ]
        }"##;
        let scenario = load(json);
        let tasks = build_frame_tasks(&scenario);
        let scene_1_start = tasks
            .iter()
            .find_map(|t| match t {
                FrameTask::Normal {
                    scene_idx: 1,
                    global_frame,
                    frame_in_scene: 0,
                    ..
                } => Some(*global_frame),
                _ => None,
            })
            .expect("scene 1 must have a frame_in_scene == 0 Normal task");
        let fps = scenario.video.fps as f64;
        assert!(
            (scene_1_start as f64 / fps - 2.7).abs() < 1e-9,
            "expected scene 1 to start at beat 1 = 2.7s, got {}s",
            scene_1_start as f64 / fps
        );
    }

    #[test]
    fn v2_gap_from_an_explicit_at_holds_the_previous_scene() {
        let json = r##"{
            "video": {"width": 64, "height": 64, "fps": 10},
            "timing": "v2",
            "scenes": [
                {"duration": 1.0, "children": []},
                {"duration": 1.0, "children": [], "at": "2.0s"}
            ]
        }"##;
        let scenario = load(json);
        let tasks = build_frame_tasks(&scenario);
        assert_eq!(
            tasks.len(),
            30,
            "1.0s scene0 + 1.0s gap + 1.0s scene1 @ 10fps"
        );
        let held: Vec<u32> = tasks[10..20]
            .iter()
            .filter_map(|t| match t {
                FrameTask::Normal {
                    scene_idx: 0,
                    frame_in_scene,
                    ..
                } => Some(*frame_in_scene),
                _ => None,
            })
            .collect();
        assert_eq!(
            held,
            vec![9; 10],
            "the 1.0s gap must hold scene 0's own last frame (index 9), got {held:?}"
        );
    }
}
