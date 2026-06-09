use crate::engine::transition::{apply_transition, camera_pan_transition};
use crate::error::Result;
use crate::schema::{EasingType, ResolvedView, Scene, ResolvedScenario as Scenario, TransitionType, VideoConfig, ViewType};

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

pub fn render_frame_task(config: &VideoConfig, scenario: &Scenario, task: &FrameTask) -> Result<Vec<u8>> {
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
        FrameTask::Normal { view_idx, scene_idx, frame_in_scene, .. } => {
            let scene = &scenario.views[*view_idx].scenes[*scene_idx];
            render_scene_hits(&scenario.video, scene, *frame_in_scene)
        }
        _ => Vec::new(),
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

pub fn render_frame_task_scaled(
    config: &VideoConfig,
    scenario: &Scenario,
    task: &FrameTask,
    scale_factor: f32,
) -> Result<Vec<u8>> {
    use crate::engine::render::{render_scene_frame, render_scene_frame_scaled, render_scene_frame_scaled_with_prev_bg, render_scene_bg_scaled, render_scene_fg_scaled};

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
            crate::engine::render::render_world_frame_scaled(
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

fn render_last_frame_of_view(
    config: &VideoConfig,
    view: &ResolvedView,
    fps: u32,
    scale_factor: f32,
) -> Result<Vec<u8>> {
    use crate::engine::render::render_scene_frame_scaled;
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
            crate::engine::render::render_world_frame_scaled(
                config, view, &timeline, total_frames.saturating_sub(1), scale_factor,
            )
        }
    }
}

fn render_first_frame_of_view(
    config: &VideoConfig,
    view: &ResolvedView,
    fps: u32,
    scale_factor: f32,
) -> Result<Vec<u8>> {
    use crate::engine::render::render_scene_frame_scaled;
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
            crate::engine::render::render_world_frame_scaled(
                config, view, &timeline, 0, scale_factor,
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

/// Build frame tasks for a single scene (by index) within a slide view.
/// Used by incremental encoding (operates on view 0 only for backward compat).
pub(super) fn build_scene_frame_tasks(scenario: &Scenario, scene_idx: usize) -> Vec<FrameTask> {
    let fps = scenario.video.fps;
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
