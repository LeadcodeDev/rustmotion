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
    /// Camera pan duration between scenes
    pub camera_pan_duration: f64,
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
            let (wx, wy) = scene.world_position.as_ref()
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

        WorldTimeline {
            scene_windows: windows,
            camera_waypoints: waypoints,
            total_duration,
            camera_pan_duration: pan_dur,
        }
    }

    /// Total number of frames for this world view.
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

        let pan_half = self.camera_pan_duration / 2.0;

        // Before the first waypoint
        if time < self.camera_waypoints[0].time {
            let wp = &self.camera_waypoints[0];
            return (wp.x, wp.y);
        }

        // Check each pair of waypoints
        for i in 0..self.camera_waypoints.len() - 1 {
            let wp_a = &self.camera_waypoints[i];
            let wp_b = &self.camera_waypoints[i + 1];

            // Pan starts pan_half before wp_b.time and ends pan_half after wp_b.time
            let pan_start = wp_b.time - pan_half;
            let pan_end = wp_b.time + pan_half;

            // Before this pan starts → camera is stationary at wp_a
            if time < pan_start {
                return (wp_a.x, wp_a.y);
            }

            // During this pan → interpolate between wp_a and wp_b
            if time <= pan_end {
                let raw_progress = safe_div(time - pan_start, pan_end - pan_start, 1.0).clamp(0.0, 1.0);
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

    /// Return all scenes that should be visible at the given time.
    ///
    /// A scene is visible if:
    /// - We're within its time window, OR
    /// - We're within camera_pan_duration/2 of its boundary (it's being panned to/from), OR
    /// - It has `persist: true` and its window has ended
    pub fn visible_scenes_at(&self, time: f64, scenes: &[Scene], fps: u32) -> Vec<VisibleScene> {
        let mut result = Vec::new();
        let pan_half = self.camera_pan_duration / 2.0;

        for (i, (start, end)) in self.scene_windows.iter().enumerate() {
            let scene = &scenes[i];
            let scene_total_frames = (scene.duration * fps as f64).round() as u32;

            // The pan to this scene starts at `start - pan_half` and finishes at `start + pan_half`
            // Animations begin after the pan finishes arriving, so anim_start = start + pan_half
            // (For the first scene, there's no incoming pan, so anim_start = start)
            let anim_start = if i == 0 {
                *start
            } else {
                start + pan_half
            };

            // Is this scene currently in its active window (including pan margins)?
            let visible_start = start - pan_half;
            let visible_end = *end + pan_half;

            let is_in_window = time >= visible_start.max(0.0) && time < visible_end;
            let is_persisted = scene.persist && time >= *end;

            if is_in_window || is_persisted {
                let local_time = time - anim_start;
                let local_frame = if local_time <= 0.0 {
                    0
                } else {
                    ((local_time * fps as f64).round() as u32).min(scene_total_frames.saturating_sub(1))
                };

                // Calculate opacity for crossfade during camera pans
                let opacity = if is_persisted {
                    1.0_f32
                } else {
                    // Check if scene is fading OUT (pan away from this scene)
                    // The outgoing pan starts at `end - pan_half` and ends at `end + pan_half`
                    let out_pan_start = *end - pan_half;
                    let out_pan_end = *end + pan_half;

                    // Check if scene is fading IN (pan arriving at this scene)
                    let in_pan_start = *start - pan_half;
                    let in_pan_end = *start + pan_half;

                    if i > 0 && time >= in_pan_start.max(0.0) && time < in_pan_end {
                        // Fading in: opacity goes 0 → 1 during incoming pan
                        let denom = in_pan_end - in_pan_start.max(0.0);
                        let progress = safe_div(time - in_pan_start.max(0.0), denom, 1.0).clamp(0.0, 1.0);
                        progress as f32
                    } else if i < self.scene_windows.len() - 1 && time >= out_pan_start && time <= out_pan_end {
                        // Fading out: opacity goes 1 → 0 during outgoing pan
                        let progress = safe_div(time - out_pan_start, out_pan_end - out_pan_start, 1.0).clamp(0.0, 1.0);
                        1.0 - progress as f32
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
}
