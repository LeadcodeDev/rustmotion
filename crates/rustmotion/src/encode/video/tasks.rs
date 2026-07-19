use crate::engine::transition::{apply_transition, camera_pan_transition};
use crate::error::Result;
use crate::schema::{
    EasingType, ResolvedScenario as Scenario, ResolvedView, Scene, TransitionType, VideoConfig,
    ViewType,
};

/// Description of what to render for a specific frame
#[derive(Clone, Debug)]
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
            let mut pixels = render_scene_frame_scaled_with_prev_bg(
                config,
                scene,
                *frame_in_scene,
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
                let bg = render_scene_bg_scaled(
                    config,
                    &scenes[*scene_a_idx],
                    frame_a_idx,
                    scale_factor,
                )?;
                let fg_a = render_scene_fg_scaled(
                    config,
                    &scenes[*scene_a_idx],
                    frame_a_idx,
                    *scene_a_total_frames,
                    scale_factor,
                )?;
                let fg_b = render_scene_fg_scaled(
                    config,
                    &scenes[*scene_b_idx],
                    *frame_in_transition,
                    *scene_b_total_frames,
                    scale_factor,
                )?;
                // CameraPan composites two scenes; apply scene_b effects to the composited result.
                let mut composited = camera_pan_transition(
                    &bg,
                    &fg_a,
                    &fg_b,
                    scaled_w,
                    scaled_h,
                    progress,
                    dx * scale_factor,
                    dy * scale_factor,
                    easing,
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
                    *scene_a_total_frames,
                )?;
                let b = render_scene_frame(
                    config,
                    &scenes[*scene_b_idx],
                    *frame_in_transition,
                    *scene_b_total_frames,
                )?;
                (a, b)
            } else {
                let a = render_scene_frame_scaled(
                    config,
                    &scenes[*scene_a_idx],
                    frame_a_idx,
                    *scene_a_total_frames,
                    scale_factor,
                )?;
                let b = render_scene_frame_scaled(
                    config,
                    &scenes[*scene_b_idx],
                    *frame_in_transition,
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
            view_idx,
            frame_in_view,
            view_total_frames: _,
        } => {
            use crate::engine::world::WorldTimeline;
            let view = &scenario.views[*view_idx];
            let timeline = WorldTimeline::build(view, config.fps, config.width, config.height);
            crate::engine::render::render_world_frame_scaled(
                config,
                view,
                &timeline,
                *frame_in_view,
                scale_factor,
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
                render_scene_frame_scaled(
                    config,
                    last_scene,
                    scene_frames.saturating_sub(1),
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
                scale_factor,
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
pub(super) fn build_slot_frame_tasks(scenario: &Scenario, slot: &SegmentSlot) -> Vec<FrameTask> {
    match slot {
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
    }
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
            let concatenated: Vec<String> = slots
                .iter()
                .flat_map(|slot| build_slot_frame_tasks(&s, slot))
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
