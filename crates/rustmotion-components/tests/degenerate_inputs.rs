//! A painter must never panic — nor hang — on input the schema accepts.
//!
//! `rustmotion validate` is the documented gate before delivery, and it answers
//! "Valid scenario" for every component below. Each one was observed aborting or
//! wedging the renderer mid-frame, which kills the whole encode: the scenarios
//! here are the reproductions, verbatim, kept as a regression floor.
//!
//! Every case routes through the real pipeline — serde, box_builder, run_layout,
//! paint_tree — so a fix that only guards the painter while leaving the component
//! undeserialisable would still fail.

use std::sync::mpsc;
use std::time::Duration;

use rustmotion_components::box_builder::{build_scene_with_anim, BuildAnimationCtx};
use rustmotion_components::legacy_dispatch::LegacyPaintDispatcher;
use rustmotion_components::{ChildComponent, Component, PositionMode};
use rustmotion_core::css::taffy_bridge::ConversionContext;
use rustmotion_core::engine::layout_pass::run_layout;
use rustmotion_core::engine::paint_pass::{paint_tree, PaintFrame};

const W: u32 = 400;
const H: u32 = 300;

/// Deserialize one component and paint it at `time`. Panics propagate — that is
/// the point of the test.
fn paint(json: serde_json::Value, time: f64) {
    let component: Component = serde_json::from_value(json).expect("component is schema-valid");
    let children = vec![ChildComponent {
        component,
        position: Some(PositionMode::Absolute { x: 0.0, y: 0.0 }),
        x: None,
        y: None,
        z_index: None,
        bleed: false,
    }];

    let mut surface =
        skia_safe::surfaces::raster_n32_premul((W as i32, H as i32)).expect("raster surface");
    let canvas = surface.canvas();
    canvas.clear(skia_safe::Color4f::new(0.0, 0.0, 0.0, 0.0));

    let built = build_scene_with_anim(
        &children,
        (W as f32, H as f32),
        BuildAnimationCtx {
            time,
            scenario_time: time,
            scene_duration: 2.0,
            fps: 30,
        },
    );
    let layout = run_layout(
        &built.root,
        (W as f32, H as f32),
        &ConversionContext::default(),
    );
    let dispatcher = LegacyPaintDispatcher::for_scene(&built);
    let frame = PaintFrame {
        time,
        scenario_time: time,
        frame_index: (time * 30.0) as u32,
        fps: 30,
        video_width: W,
        video_height: H,
        scene_duration: 2.0,
        camera: None,
    };
    paint_tree(canvas, &built.root, &layout, &frame, &dispatcher);
}

/// Paint on a worker so a runaway loop fails the test instead of wedging the
/// suite. `dot_spacing: 0` used to spin for billions of iterations; a plain
/// `paint()` call would hang CI rather than report.
fn paint_within(json: serde_json::Value, time: f64, budget: Duration, what: &str) {
    let (tx, rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        paint(json, time);
        let _ = tx.send(());
    });
    match rx.recv_timeout(budget) {
        Ok(()) => worker.join().expect(what),
        Err(_) => panic!("{what}: still painting after {budget:?} — runaway loop"),
    }
}

#[test]
fn shape_survives_a_gradient_whose_stops_do_not_match_its_colors() {
    // skia asserts pos.len() == colors.len() inside the gradient shader, so a
    // mismatched `stops` aborted the process rather than returning an error.
    for gradient_type in ["linear", "radial"] {
        paint(
            serde_json::json!({
                "type": "shape",
                "shape": "rect",
                "style": { "width": 200, "height": 100 },
                "fill": {
                    "type": gradient_type,
                    "colors": ["#FF0000", "#0000FF"],
                    "stops": [0.0, 0.5, 1.0]
                }
            }),
            0.5,
        );
    }
}

#[test]
fn table_survives_an_empty_row_colors_list() {
    // `row_colors: []` deserializes to Some(vec![]), so the default palette was
    // never substituted and the modulo guard still indexed an empty slice.
    paint(
        serde_json::json!({
            "type": "table",
            "headers": ["A", "B"],
            "rows": [["1", "2"], ["3", "4"]],
            "row_colors": [],
            "style": { "width": 300, "height": 200 }
        }),
        0.5,
    );
}

#[test]
fn tag_cloud_survives_an_empty_colors_list() {
    // palette() returned the caller's empty vec, and the painter took
    // `index % palette.len()` on it.
    paint(
        serde_json::json!({
            "type": "tag_cloud",
            "tags": [{ "text": "rust", "weight": 3 }, { "text": "skia", "weight": 1 }],
            "colors": [],
            "style": { "width": 300, "height": 200 }
        }),
        0.5,
    );
}

#[test]
fn dot_map_terminates_on_a_zero_dot_spacing() {
    // (w - 0) / 0 is +inf, and `inf as u32` saturates to u32::MAX in Rust, so the
    // nested loop was scheduled for ~1.8e19 iterations. The geometry pass catches
    // dot_spacing: 0.01 but not 0.
    paint_within(
        serde_json::json!({
            "type": "dot_map",
            "points": [{ "lat": 48.8, "lng": 2.3 }],
            "dot_spacing": 0,
            "style": { "width": 300, "height": 150 }
        }),
        0.5,
        Duration::from_secs(10),
        "dot_map with dot_spacing: 0",
    );
}

#[test]
fn codeblock_diff_survives_multibyte_text() {
    // The edit script counts bytes while the reveal interpolates a fraction of
    // that count, so mid-animation offsets landed inside a multi-byte character
    // and `replace_range` aborted with "not a char boundary".
    for (from, to) in [
        ("let a = 1;", "let café = «héllo→»;"),
        ("let x = 1;", "let y = \"éàü\";"),
        ("a", "日本語のテキスト"),
        ("ok", "🎬 clap"),
    ] {
        // Sweep the reveal: the panic only fires on the frames where progress
        // lands part-way through a glyph.
        for step in 0..=20 {
            paint(
                serde_json::json!({
                    "type": "codeblock",
                    "code": from,
                    "language": "rust",
                    "diff": true,
                    "states": [
                        { "code": from, "at": 0.0 },
                        { "code": to, "at": 0.4, "cursor": { "enabled": true } }
                    ],
                    "style": { "width": 380, "height": 120 }
                }),
                f64::from(step) * 0.1,
            );
        }
    }
}
