//! Regression tests — workstream E (animated backgrounds & the scene render
//! path).
//!
//! Some of the covered defects are bugs in fully private rendering
//! internals (`background.rs`'s `tile_spacing`/`compute_scroll_offset`/
//! `draw_bg_heropattern`) that this crate never exposes past its `pub`
//! surface — an external integration test crate like this one cannot name
//! them. Their regression tests live as `#[cfg(test)]` modules inside
//! `crates/rustmotion/src/engine/render/background.rs` itself, following
//! that file's own pre-existing convention (`scroll_offset_wrap_tests`,
//! `pixel_grid_tests`, `grid_lines_tests`, `halo_opacity_tests`) for testing
//! renderer-private logic directly. This file carries the defects that are
//! genuinely reachable through the crate's public API.

use rustmotion::encode::video::{build_frame_tasks, render_frame_task, FrameTask};
use rustmotion::loader::load_scenario_from_source;

/// Render a single `FrameTask::WorldFrame` at `frame_in_view`, decoded from
/// `scenario_json`. Panics (with a message naming the missing frame) if no
/// such world frame exists in the built schedule — a test bug, not a
/// render-time failure, should fail loudly here.
fn render_world_frame(scenario_json: &serde_json::Value, frame_in_view: u32) -> Vec<u8> {
    let scenario = load_scenario_from_source(None, Some(&scenario_json.to_string()))
        .expect("scenario is schema-valid");
    let tasks = build_frame_tasks(&scenario);
    let task = tasks
        .iter()
        .find(
            |t| matches!(t, FrameTask::WorldFrame { frame_in_view: f, .. } if *f == frame_in_view),
        )
        .unwrap_or_else(|| panic!("no WorldFrame task at frame_in_view={frame_in_view}"));
    render_frame_task(&scenario.video, &scenario, task).expect("frame renders")
}

/// Count pixels that read as the particle's pure-green marker colour
/// (`#00FF00`) against the scenario's plain black background — a stand-in
/// for "is the decorative child visible in this frame" that doesn't depend
/// on knowing any particle's exact on-screen position.
fn green_pixel_count(buf: &[u8]) -> usize {
    buf.chunks_exact(4)
        .filter(|px| px[1] > 100 && px[0] < 80 && px[2] < 80)
        .count()
}

mod decorative_dispatch_parity {
    //! `paint_decorative_fullscreen` (the world-view-only path that paints
    //! decorative children like `particle` without going through the box
    //! tree) used to re-derive visibility and effects by hand instead of
    //! calling `PaintWindow::contains` / `box_builder::effective_effects`
    //! like every other paint path does. Two independent symptoms: an
    //! inclusive `end_at` (visible one frame too long) and dropped
    //! `timeline` animation effects.

    use super::*;

    const FPS: u32 = 10;

    fn world_scenario(particle_extra: serde_json::Value) -> serde_json::Value {
        let mut particle = serde_json::json!({
            "type": "particle",
            "particle_type": "snow",
            "count": 30,
            "colors": ["#00FF00"],
            "size_range": {"min": 6, "max": 6},
            "speed": 0.0,
        });
        particle
            .as_object_mut()
            .unwrap()
            .extend(particle_extra.as_object().unwrap().clone());

        serde_json::json!({
            "video": {"width": 64, "height": 64, "fps": FPS, "background": "#000000"},
            "composition": [
                {"type": "world", "scenes": [
                    {"duration": 2.0, "children": [particle]}
                ]}
            ]
        })
    }

    /// `end_at` is a half-open window: the child must already be gone
    /// exactly at `end_at`, not still visible for one extra frame past it.
    #[test]
    fn end_at_is_a_half_open_window_not_inclusive() {
        let end_at = 0.5;
        let scenario = world_scenario(serde_json::json!({ "end_at": end_at }));
        let frame_just_before_end_at = (end_at * FPS as f64) as u32 - 1;
        let frame_at_end_at = (end_at * FPS as f64) as u32;

        let before = render_world_frame(&scenario, frame_just_before_end_at);
        assert!(
            green_pixel_count(&before) > 0,
            "the particle must still be visible just before its end_at"
        );

        let at_boundary = render_world_frame(&scenario, frame_at_end_at);
        assert_eq!(
            green_pixel_count(&at_boundary),
            0,
            "end_at is a half-open window ([start, end)): the particle must already be gone \
             exactly at end_at, not one extra frame later"
        );
    }

    /// A `timeline` step's `animation` entries must be folded into the
    /// resolved props like every other paint path does, not silently
    /// dropped because only `style.animation` was read.
    #[test]
    fn timeline_animation_effects_are_not_silently_dropped() {
        let fade_out_at = 0.5;
        let scenario = world_scenario(serde_json::json!({
            "timeline": [{
                "at": 0.0,
                "animation": [{
                    "name": "keyframes",
                    "keyframes": [{
                        "property": "opacity",
                        "keyframes": [
                            {"time": 0.0, "value": 1.0},
                            {"time": fade_out_at, "value": 0.0}
                        ]
                    }]
                }]
            }]
        }));

        let early = render_world_frame(&scenario, 0);
        assert!(
            green_pixel_count(&early) > 0,
            "the particle should be visible at t=0, before the timeline fade-out completes"
        );

        let frame_well_past_fade_out = (fade_out_at * FPS as f64) as u32 * 3;
        let late = render_world_frame(&scenario, frame_well_past_fade_out);
        assert_eq!(
            green_pixel_count(&late),
            0,
            "a timeline step's animation effects must apply to a decorative child, not be \
             silently dropped"
        );
    }
}
