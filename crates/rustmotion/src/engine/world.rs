use crate::engine::animator::{ease, safe_div};
use crate::schema::{EasingType, ResolvedView, Scene};

/// Timeline for a world view: scene time windows and camera waypoints.
#[derive(Debug)]
pub struct WorldTimeline {
    /// (start, end) time in seconds for each scene
    pub scene_windows: Vec<(f64, f64)>,
    /// Camera position waypoints at scene boundaries
    pub camera_waypoints: Vec<CameraWaypoint>,
    /// Total duration of the world view in seconds
    pub total_duration: f64,
    /// Declared view-level camera pan duration (seconds), before the
    /// per-boundary clamp below. Kept for callers that have no boundary
    /// index at hand; prefer `boundary_pan_duration` wherever one is known.
    pub camera_pan_duration: f64,
    /// Actual pan duration (seconds) used at each scene boundary —
    /// `boundary_pan_duration[i]` governs the pan between `scenes[i]` and
    /// `scenes[i + 1]`. `len() == scene_windows.len().saturating_sub(1)`.
    ///
    /// Clamped to `min(scenes[i].duration, scenes[i + 1].duration)`: a pan
    /// window is centered on the boundary and reaches `pan_half` into each
    /// side, so capping `pan_half` at half of *both* neighbouring scenes'
    /// durations guarantees two consecutive pan windows never overlap.
    /// Before this clamp existed, a `camera_pan_duration` longer than a
    /// scene's own duration made boundary `i`'s window reach past boundary
    /// `i + 1`'s start; `camera_at` returns the first matching window it
    /// finds, so time entering that overlap jumped from boundary `i`'s
    /// (already near-complete) interpolation straight to boundary `i +
    /// 1`'s — a same-frame camera teleport measured at 245px (77% of the
    /// frame width) in the audit's repro.
    pub boundary_pan_duration: Vec<f64>,
}

#[derive(Debug, Clone)]
pub struct CameraWaypoint {
    pub time: f64,
    pub x: f32,
    pub y: f32,
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct VisibleScene {
    pub scene_idx: usize,
    /// Time relative to when the scene's animations should start
    /// (after the camera pan finishes arriving at this scene).
    /// Can be negative during the pan-in phase (animations haven't started yet).
    pub local_time: f64,
    pub local_frame: u32,
    pub scene_total_frames: u32,
    pub is_persisted: bool,
    /// Opacity for crossfade during camera pans (1.0 = fully visible, 0.0 = invisible).
    /// The outgoing scene fades out and the incoming scene fades in during the pan.
    pub opacity: f32,
}

impl WorldTimeline {
    /// Build a timeline from a world view's scenes.
    ///
    /// Scenes are sequential: scene 0 starts at t=0, scene 1 starts when scene 0 ends, etc.
    /// Camera pans are centered on scene boundaries, taking `camera_pan_duration` seconds.
    /// During a pan, both scenes are visible.
    pub fn build(view: &ResolvedView, _fps: u32, video_width: u32, video_height: u32) -> Self {
        // Clamp the pan duration to a non-negative value. A negative value
        // would invert pan_start/pan_end and silently scramble the camera
        // interpolation; a zero is fine (handled downstream by safe_div).
        let pan_dur = view.camera_pan_duration.max(0.0);
        let scenes = &view.scenes;

        if scenes.is_empty() {
            return WorldTimeline {
                scene_windows: Vec::new(),
                camera_waypoints: Vec::new(),
                total_duration: 0.0,
                camera_pan_duration: pan_dur,
                boundary_pan_duration: Vec::new(),
            };
        }

        let mut windows = Vec::with_capacity(scenes.len());
        let mut waypoints = Vec::with_capacity(scenes.len());
        let mut t = 0.0;

        let vw = video_width as f32;
        let vh = video_height as f32;

        for (i, scene) in scenes.iter().enumerate() {
            let start = t;
            let end = t + scene.duration;
            windows.push((start, end));

            // Use world-position if specified, otherwise fall back to horizontal grid
            let (wx, wy) = scene
                .world_position
                .as_ref()
                .map(|p| (p.x, p.y))
                .unwrap_or((vw / 2.0 + i as f32 * vw, vh / 2.0));

            // Camera arrives at this scene's position at the start of the scene
            waypoints.push(CameraWaypoint {
                time: start,
                x: wx,
                y: wy,
            });

            t = end;
        }

        let total_duration = t;

        // Per-boundary clamp — see the field doc on `boundary_pan_duration`
        // for why `min` of both neighbouring scene durations is what
        // guarantees non-overlapping pan windows.
        let boundary_pan_duration: Vec<f64> = (0..scenes.len().saturating_sub(1))
            .map(|i| pan_dur.min(scenes[i].duration).min(scenes[i + 1].duration))
            .collect();

        WorldTimeline {
            scene_windows: windows,
            camera_waypoints: waypoints,
            total_duration,
            camera_pan_duration: pan_dur,
            boundary_pan_duration,
        }
    }

    /// Total number of frames for this world view.
    /// The rectangle of world space the camera ever shows, in world
    /// coordinates: `(x, y, width, height)`.
    ///
    /// Each waypoint puts that world point at the viewport's top-left, so the
    /// span is the union of one viewport-sized rect per waypoint. Backgrounds
    /// painted across the world need this rather than a fixed multiple of the
    /// viewport: a world spanning two screens and one spanning ten are not the
    /// same canvas, and a `halo` zone expressed as a fraction of the wrong one
    /// lands nowhere near where its author aimed it.
    ///
    /// Falls back to the viewport itself when there are no waypoints.
    pub fn world_extent(&self, viewport_w: f32, viewport_h: f32) -> (f32, f32, f32, f32) {
        let Some(first) = self.camera_waypoints.first() else {
            return (0.0, 0.0, viewport_w, viewport_h);
        };
        let (mut min_x, mut min_y) = (first.x, first.y);
        let (mut max_x, mut max_y) = (first.x, first.y);
        for wp in &self.camera_waypoints {
            min_x = min_x.min(wp.x);
            min_y = min_y.min(wp.y);
            max_x = max_x.max(wp.x);
            max_y = max_y.max(wp.y);
        }
        (
            min_x,
            min_y,
            (max_x - min_x) + viewport_w,
            (max_y - min_y) + viewport_h,
        )
    }

    pub fn total_frames(&self, fps: u32) -> u32 {
        (self.total_duration * fps as f64).round() as u32
    }

    /// Interpolate camera position at a given time, using the view's easing.
    pub fn camera_at(&self, time: f64, easing: &EasingType) -> (f32, f32) {
        if self.camera_waypoints.is_empty() {
            return (0.0, 0.0);
        }
        if self.camera_waypoints.len() == 1 {
            let wp = &self.camera_waypoints[0];
            return (wp.x, wp.y);
        }

        // Before the first waypoint
        if time < self.camera_waypoints[0].time {
            let wp = &self.camera_waypoints[0];
            return (wp.x, wp.y);
        }

        // Check each pair of waypoints
        for i in 0..self.camera_waypoints.len() - 1 {
            let wp_a = &self.camera_waypoints[i];
            let wp_b = &self.camera_waypoints[i + 1];

            // Each boundary uses its own clamped pan duration (see
            // `boundary_pan_duration`'s doc) so consecutive windows never
            // overlap and this loop's first match is always the right one.
            let pan_half = self
                .boundary_pan_duration
                .get(i)
                .copied()
                .unwrap_or(self.camera_pan_duration)
                / 2.0;

            // Pan starts pan_half before wp_b.time and ends pan_half after wp_b.time
            let pan_start = wp_b.time - pan_half;
            let pan_end = wp_b.time + pan_half;

            // Before this pan starts → camera is stationary at wp_a
            if time < pan_start {
                return (wp_a.x, wp_a.y);
            }

            // During this pan → interpolate between wp_a and wp_b
            if time <= pan_end {
                let raw_progress =
                    safe_div(time - pan_start, pan_end - pan_start, 1.0).clamp(0.0, 1.0);
                let t = ease(raw_progress, easing) as f32;
                let x = wp_a.x + (wp_b.x - wp_a.x) * t;
                let y = wp_a.y + (wp_b.y - wp_a.y) * t;
                return (x, y);
            }
        }

        // After the last pan — snap to last waypoint
        let last = self.camera_waypoints.last().unwrap();
        (last.x, last.y)
    }

    /// Pan duration (seconds) into `scenes[i]` (from `i - 1`) and out of it
    /// (to `i + 1`). `0.0` at the timeline's own edges, where there is no
    /// neighbour to pan from/to.
    fn boundary_pans_for(&self, i: usize) -> (f64, f64) {
        let in_pan_dur = if i > 0 {
            self.boundary_pan_duration
                .get(i - 1)
                .copied()
                .unwrap_or(0.0)
        } else {
            0.0
        };
        let out_pan_dur = self.boundary_pan_duration.get(i).copied().unwrap_or(0.0);
        (in_pan_dur, out_pan_dur)
    }

    /// Return all scenes that should be visible at the given time.
    ///
    /// A scene is visible if:
    /// - We're within its time window, OR
    /// - We're within its own boundary's pan duration / 2 of its start or
    ///   end (it's being panned to/from — see `boundary_pan_duration`), OR
    /// - It has `persist: true` and its window has ended
    pub fn visible_scenes_at(&self, time: f64, scenes: &[Scene], fps: u32) -> Vec<VisibleScene> {
        let mut result = Vec::new();

        for (i, (start, end)) in self.scene_windows.iter().enumerate() {
            let scene = &scenes[i];
            let scene_total_frames = (scene.duration * fps as f64).round() as u32;

            // Pan durations either side of this scene, each independently
            // clamped at build time — see `boundary_pan_duration`.
            let (in_pan_dur, out_pan_dur) = self.boundary_pans_for(i);
            let in_pan_half = in_pan_dur / 2.0;
            let out_pan_half = out_pan_dur / 2.0;

            // The pan to this scene starts at `start - in_pan_half` and finishes at `start + in_pan_half`
            // Animations begin after the pan finishes arriving, so anim_start = start + in_pan_half
            // (For the first scene, there's no incoming pan, so anim_start = start)
            let anim_start = if i == 0 { *start } else { start + in_pan_half };

            // Is this scene currently in its active window (including pan margins)?
            let visible_start = start - in_pan_half;
            let visible_end = *end + out_pan_half;

            let is_in_window = time >= visible_start.max(0.0) && time < visible_end;
            let is_persisted = scene.persist && time >= *end;

            if is_in_window || is_persisted {
                let local_time = time - anim_start;
                let local_frame = if local_time <= 0.0 {
                    0
                } else {
                    ((local_time * fps as f64).round() as u32)
                        .min(scene_total_frames.saturating_sub(1))
                };

                // The outgoing pan window, centred on `end`: starts at
                // `end - out_pan_half`, ends at `end + out_pan_half`. Shared
                // by the non-persisted fade-out branch and the persisted
                // recovery ramp below, so both agree on where it sits.
                let out_pan_start = *end - out_pan_half;
                let out_pan_end = *end + out_pan_half;
                let has_outgoing_pan = i < self.scene_windows.len() - 1;

                // Calculate opacity for crossfade during camera pans.
                //
                // Both branches use mirrored power-curve exponents rather
                // than a plain linear ramp — the same shape
                // `camera_pan_transition`'s `FG_DISSOLVE` uses for the
                // slide-view side of a scene-to-scene cut (see
                // `crates/rustmotion-core/src/engine/transition.rs`). A
                // linear 1-t / t ramp on two scenes that occupy roughly
                // equal, non-overlapping screen slices at the pan's
                // midpoint (the camera sits between their world-positions)
                // multiplies through to a p²+(1-p)² luminance curve that
                // dips to 50% exactly mid-pan — a wash-out the `world` view
                // exists to avoid. Pinned at both ends (`0` and `1`) so a
                // transition frame's opacity is always exactly 1.0 at its
                // own scene's t=0/t=1 junction against a non-pan frame.
                const CROSSFADE_DISSOLVE: f32 = 1.6;
                let fade_in_curve = |p: f32| 1.0 - (1.0 - p).powf(CROSSFADE_DISSOLVE);
                let fade_out_curve = |p: f32| 1.0 - p.powf(CROSSFADE_DISSOLVE);

                let opacity = if is_persisted {
                    // `persist` keeps this scene's content around after its
                    // own window ends instead of disappearing — but until
                    // `has_outgoing_pan` is checked, `is_persisted` alone
                    // says nothing about *how far* past `end` we are. If
                    // we're still inside the same outgoing pan window that
                    // a non-persisted scene would be fading out through,
                    // continue that exact curve from the value it already
                    // reached at `time == end` and ramp it back up to 1.0 by
                    // `out_pan_end`, instead of snapping straight to 1.0.
                    // The snap was the bug: the fade-out curve is still
                    // mid-descent at `end` (progress 0.5 into the outgoing
                    // window), so forcing opacity to 1.0 right there produced
                    // a same-frame pop from a partial value — on a feature
                    // whose entire point is a callback with no rupture.
                    if has_outgoing_pan && time < out_pan_end {
                        let value_at_end = fade_out_curve(0.5);
                        let recovery =
                            safe_div(time - *end, out_pan_end - *end, 1.0).clamp(0.0, 1.0) as f32;
                        value_at_end + (1.0 - value_at_end) * fade_in_curve(recovery)
                    } else {
                        1.0_f32
                    }
                } else {
                    // Check if scene is fading IN (pan arriving at this scene)
                    let in_pan_start = *start - in_pan_half;
                    let in_pan_end = *start + in_pan_half;

                    if i > 0 && time >= in_pan_start.max(0.0) && time < in_pan_end {
                        // Fading in: opacity goes 0 → 1 during incoming pan
                        let denom = in_pan_end - in_pan_start.max(0.0);
                        let progress = safe_div(time - in_pan_start.max(0.0), denom, 1.0)
                            .clamp(0.0, 1.0) as f32;
                        fade_in_curve(progress)
                    } else if has_outgoing_pan && time >= out_pan_start && time <= out_pan_end {
                        // Fading out: opacity goes 1 → 0 during outgoing pan
                        let progress =
                            safe_div(time - out_pan_start, out_pan_end - out_pan_start, 1.0)
                                .clamp(0.0, 1.0) as f32;
                        fade_out_curve(progress)
                    } else {
                        1.0
                    }
                };

                result.push(VisibleScene {
                    scene_idx: i,
                    local_time,
                    local_frame,
                    scene_total_frames,
                    is_persisted,
                    opacity,
                });
            }
        }

        result
    }

    /// The scene actively "in front" at `time` — the highest-indexed
    /// non-persisted visible scene, mirroring the selection
    /// `render_world_frame_scaled` uses to pick which scene's background
    /// dominates a frame. `None` when nothing is visible (e.g. an empty
    /// view). Shared with the frame-task post-effects pass so both agree on
    /// which scene's `effects` apply to a given `WorldFrame`.
    pub fn active_scene_idx(&self, time: f64, scenes: &[Scene], fps: u32) -> Option<usize> {
        self.visible_scenes_at(time, scenes, fps)
            .iter()
            .filter(|v| !v.is_persisted)
            .map(|v| v.scene_idx)
            .max()
    }
}

#[cfg(test)]
mod world_timeline_tests {
    use super::*;

    fn view_from_json(json: &str) -> crate::schema::ResolvedView {
        let scenario =
            crate::loader::load_scenario_from_source(None, Some(json)).expect("scenario must load");
        scenario
            .views
            .into_iter()
            .next()
            .expect("scenario must have at least one view")
    }

    // Constat 5: `camera_pan_duration` longer than a scene's own duration
    // must not let two consecutive pan windows overlap.
    mod camera_teleport {
        use super::*;

        const REPRO: &str = r#"{
            "video": { "width": 320, "height": 180, "fps": 30 },
            "composition": [
                { "type": "world", "camera_pan_duration": 2.0, "camera_easing": "linear",
                  "scenes": [
                    { "duration": 0.5, "children": [] },
                    { "duration": 0.5, "children": [] },
                    { "duration": 0.5, "children": [] },
                    { "duration": 0.5, "children": [] }
                  ] }
            ]
        }"#;

        #[test]
        fn boundary_pan_duration_is_clamped_per_junction() {
            let view = view_from_json(REPRO);
            let timeline = WorldTimeline::build(&view, 30, 320, 180);
            assert_eq!(timeline.boundary_pan_duration.len(), 3);
            for (i, d) in timeline.boundary_pan_duration.iter().enumerate() {
                assert!(
                    (*d - 0.5).abs() < 1e-9,
                    "boundary {i}: expected clamp to 0.5s (both neighbouring scenes are 0.5s), got {d}"
                );
            }
        }

        #[test]
        fn first_frame_is_not_already_decadre() {
            let view = view_from_json(REPRO);
            let timeline = WorldTimeline::build(&view, 30, 320, 180);
            let (x, y) = timeline.camera_at(0.0, &view.camera_easing);
            assert_eq!(
                (x, y),
                (160.0, 90.0),
                "camera_at(0) must sit exactly on scene 0's default waypoint, not already \
                 mid-pan toward scene 1 (the secondary effect the audit measured as x=240)"
            );
        }

        // The decisive test: sample the camera's x position at every
        // rendered frame across the whole timeline and measure the largest
        // frame-to-frame jump. Before the per-boundary clamp, two
        // overlapping pan windows produced a same-frame jump of 245px (the
        // audit's repro measured x: 480 -> 725.3 between t=1.5 and
        // t=1.5333, one frame apart). With the clamp, the theoretical worst
        // case is the full 320px waypoint spacing spread over one 15-frame
        // (0.5s) pan window: 320/15 ≈ 21.3px/frame.
        #[test]
        fn camera_x_never_jumps_more_than_one_pans_worth_of_travel_per_frame() {
            let view = view_from_json(REPRO);
            let timeline = WorldTimeline::build(&view, 30, 320, 180);
            let fps = 30u32;
            let total_frames = timeline.total_frames(fps);

            let mut max_jump = 0.0_f32;
            let mut worst_at = 0u32;
            let mut prev_x = timeline.camera_at(0.0, &view.camera_easing).0;
            for f in 1..total_frames {
                let t = f as f64 / fps as f64;
                let (x, _y) = timeline.camera_at(t, &view.camera_easing);
                let jump = (x - prev_x).abs();
                if jump > max_jump {
                    max_jump = jump;
                    worst_at = f;
                }
                prev_x = x;
            }

            // Generous headroom (40px) over the ~21.3px theoretical worst
            // case — still an order of magnitude under the 245px the bug
            // produced.
            assert!(
                max_jump < 40.0,
                "max per-frame camera jump {max_jump}px at frame {worst_at} — expected < 40px \
                 (bug produced 245px in one frame)"
            );
        }
    }

    // Constat 3: the scene-to-scene crossfade opacity must not wash the
    // whole frame out to 50% at the midpoint of a pan.
    mod crossfade_dissolve {
        use super::*;

        const REPRO: &str = r#"{
            "video": { "width": 320, "height": 180, "fps": 30 },
            "composition": [
                { "type": "world", "camera_pan_duration": 0.8, "camera_easing": "linear",
                  "scenes": [
                    { "duration": 2.0, "children": [] },
                    { "duration": 2.0, "children": [] }
                  ] }
            ]
        }"#;

        #[test]
        fn opacity_is_exactly_pinned_at_the_fade_in_windows_own_junctions() {
            let view = view_from_json(REPRO);
            let timeline = WorldTimeline::build(&view, 30, 320, 180);
            // Boundary pan: 0.8s clamped to min(2.0, 2.0) = 0.8s, half = 0.4s,
            // centred on t=2.0 (end of scene 0 / start of scene 1).
            let scene1_at = |t: f64| {
                timeline
                    .visible_scenes_at(t, &view.scenes, 30)
                    .into_iter()
                    .find(|v| v.scene_idx == 1)
                    .map(|v| v.opacity)
            };
            assert_eq!(
                scene1_at(1.6),
                Some(0.0),
                "t=0 of scene 1's fade-in window must be exactly 0.0"
            );
            assert_eq!(
                scene1_at(2.4),
                Some(1.0),
                "t=1 of scene 1's fade-in window must be exactly 1.0"
            );
        }

        #[test]
        fn mid_pan_opacity_stays_well_above_the_old_50_percent_floor() {
            let view = view_from_json(REPRO);
            let timeline = WorldTimeline::build(&view, 30, 320, 180);
            let visible = timeline.visible_scenes_at(2.0, &view.scenes, 30);

            let scene0 = visible
                .iter()
                .find(|v| v.scene_idx == 0)
                .expect("scene 0 must still be visible mid-pan");
            let scene1 = visible
                .iter()
                .find(|v| v.scene_idx == 1)
                .expect("scene 1 must be visible mid-pan");

            // Mirrored power-curve exponent (k=1.6) at progress=0.5:
            // 1 - 0.5^1.6 ≈ 0.670. The old linear ramp gave exactly 0.5.
            for (label, opacity) in [
                ("scene0 (fading out)", scene0.opacity),
                ("scene1 (fading in)", scene1.opacity),
            ] {
                assert!(
                    opacity > 0.6,
                    "{label} mid-pan opacity {opacity} must be well above the old 0.5 floor"
                );
                assert!(
                    (opacity - 0.670).abs() < 0.01,
                    "{label} mid-pan opacity {opacity} should match the mirrored-exponent curve (~0.670)"
                );
            }
        }
    }

    // Constat 4: `persist: true` must not pop back to 1.0 opacity while a
    // scene is still mid-fade-out; it must rise continuously.
    mod persist_recovery {
        use super::*;

        const REPRO: &str = r#"{
            "video": { "width": 320, "height": 180, "fps": 30 },
            "composition": [
                { "type": "world", "camera_pan_duration": 1.0, "camera_easing": "linear",
                  "scenes": [
                    { "duration": 1.0, "persist": true, "world-position": { "x": 160, "y": 90 },
                      "children": [] },
                    { "duration": 1.0, "world-position": { "x": 160, "y": 90 }, "children": [] }
                  ] }
            ]
        }"#;

        fn scene0_opacity_at(timeline: &WorldTimeline, scenes: &[Scene], t: f64) -> f32 {
            timeline
                .visible_scenes_at(t, scenes, 30)
                .into_iter()
                .find(|v| v.scene_idx == 0)
                .expect("scene 0 must be visible/persisted throughout [0.5, 1.5]")
                .opacity
        }

        // The exact junction the bug hit: `is_persisted` flips true at
        // `time >= end` (1.0), but the fade-out curve is only half-descended
        // there (progress 0.5 into the [0.5, 1.5] outgoing window) — the old
        // code forced opacity to 1.0 at that exact instant regardless.
        #[test]
        fn no_pop_at_the_instant_persist_takes_over() {
            let view = view_from_json(REPRO);
            let timeline = WorldTimeline::build(&view, 30, 320, 180);
            let just_before = scene0_opacity_at(&timeline, &view.scenes, 1.0 - 1.0 / 300.0);
            let at_end = scene0_opacity_at(&timeline, &view.scenes, 1.0);
            let delta = (at_end - just_before).abs();
            assert!(
                delta < 0.05,
                "opacity must be continuous across the is_persisted switch: \
                 just_before={just_before} at_end={at_end} delta={delta} (bug produced ~0.40-0.48)"
            );
        }

        #[test]
        fn reaches_exactly_one_by_the_end_of_the_outgoing_pan_window() {
            let view = view_from_json(REPRO);
            let timeline = WorldTimeline::build(&view, 30, 320, 180);
            assert_eq!(scene0_opacity_at(&timeline, &view.scenes, 1.5), 1.0);
            assert_eq!(scene0_opacity_at(&timeline, &view.scenes, 2.0), 1.0);
        }

        // The decisive test: sample every rendered frame across the outgoing
        // pan window and measure the largest frame-to-frame opacity jump.
        #[test]
        fn opacity_never_jumps_more_than_one_frames_worth_across_the_whole_window() {
            let view = view_from_json(REPRO);
            let timeline = WorldTimeline::build(&view, 30, 320, 180);
            let fps = 30u32;

            let start_frame = (0.5 * fps as f64).round() as u32;
            let end_frame = (1.5 * fps as f64).round() as u32;

            let mut max_jump = 0.0_f32;
            let mut worst_at = start_frame;
            let mut prev =
                scene0_opacity_at(&timeline, &view.scenes, start_frame as f64 / fps as f64);
            for f in (start_frame + 1)..=end_frame {
                let t = f as f64 / fps as f64;
                let cur = scene0_opacity_at(&timeline, &view.scenes, t);
                let jump = (cur - prev).abs();
                if jump > max_jump {
                    max_jump = jump;
                    worst_at = f;
                }
                prev = cur;
            }

            // The bug produced a single-frame jump of ~0.40-0.48 (measured
            // 409/1024 ≈ 0.40 in the audit's YAVG repro). A continuous curve
            // sampled at 30fps over a 1.0s window should never move more
            // than a small fraction per frame.
            assert!(
                max_jump < 0.15,
                "max per-frame opacity jump {max_jump} at frame {worst_at} — expected < 0.15 \
                 (bug produced ~0.40-0.48 in one frame)"
            );
        }
    }
}

#[cfg(test)]
mod world_extent_tests {
    use super::*;

    fn timeline_with(waypoints: &[(f32, f32)]) -> WorldTimeline {
        WorldTimeline {
            scene_windows: Vec::new(),
            camera_waypoints: waypoints
                .iter()
                .map(|&(x, y)| CameraWaypoint { time: 0.0, x, y })
                .collect(),
            total_duration: 0.0,
            camera_pan_duration: 0.0,
            boundary_pan_duration: Vec::new(),
        }
    }

    /// The extent is the union of one viewport per waypoint — the span the
    /// camera actually shows — not a fixed multiple of the viewport.
    #[test]
    fn extent_spans_the_waypoints_plus_one_viewport() {
        let t = timeline_with(&[(0.0, 0.0), (2016.0, 0.0), (2016.0, 1080.0)]);
        assert_eq!(t.world_extent(1920.0, 1080.0), (0.0, 0.0, 3936.0, 2160.0));
    }

    /// A single-waypoint world is exactly one screen, where `viewport * 5.0`
    /// used to claim five — and divided every halo radius by five with it.
    #[test]
    fn a_single_waypoint_world_is_one_viewport() {
        let t = timeline_with(&[(0.0, 0.0)]);
        assert_eq!(t.world_extent(1920.0, 1080.0), (0.0, 0.0, 1920.0, 1080.0));
    }

    /// Negative waypoints are inside the world, not outside it: the origin
    /// moves rather than the span being measured from zero.
    #[test]
    fn negative_waypoints_move_the_origin() {
        let t = timeline_with(&[(-1920.0, -540.0), (0.0, 0.0)]);
        assert_eq!(
            t.world_extent(1920.0, 1080.0),
            (-1920.0, -540.0, 3840.0, 1620.0)
        );
    }

    #[test]
    fn no_waypoints_falls_back_to_the_viewport() {
        let t = timeline_with(&[]);
        assert_eq!(t.world_extent(1920.0, 1080.0), (0.0, 0.0, 1920.0, 1080.0));
    }
}
