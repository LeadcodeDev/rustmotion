use rustmotion::encode::video::{build_frame_tasks, render_frame_task, FrameTask};
use rustmotion::loader::load_scenario_from_source;

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

fn green_pixel_count(buf: &[u8]) -> usize {
    buf.as_chunks::<4>()
        .0
        .iter()
        .filter(|px| px[1] > 100 && px[0] < 80 && px[2] < 80)
        .count()
}

mod decorative_dispatch_parity {

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
