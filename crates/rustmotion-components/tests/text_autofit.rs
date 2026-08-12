//! End-to-end pixel tests for `text-autofit` through the *real* render
//! pipeline (box_builder → run_layout → paint_tree), not just the
//! `TextIntrinsic`/`Text::paint` unit-level tests in `intrinsic.rs`/
//! `text.rs`. Routes a `text` component through the same
//! `build_scene_with_anim` → `run_layout` → `paint_tree` sequence
//! `codeblock_auto_scroll.rs` uses — the same sequence
//! `render_with_new_pipeline_iter` runs once per rendered frame in the real
//! encoder. This is the strongest available proof that `TextIntrinsic::
//! measure` (which determines the box `run_layout` reserves) and
//! `Text::paint` (which draws into whatever box that pass assigned) agree
//! on the real render path, and that nothing about the resolved size drifts
//! frame to frame.

use rustmotion_components::box_builder::{build_scene_with_anim, BuildAnimationCtx};
use rustmotion_components::legacy_dispatch::LegacyPaintDispatcher;
use rustmotion_components::{ChildComponent, Component, PositionMode};
use rustmotion_core::css::taffy_bridge::ConversionContext;
use rustmotion_core::engine::layout_pass::run_layout;
use rustmotion_core::engine::paint_pass::{paint_tree, PaintFrame};

const W: u32 = 800;
const H: u32 = 300;
const SCENE_DURATION: f64 = 3.0;
const BOX_X: f32 = 50.0;
const BOX_Y: f32 = 50.0;
const BOX_W: f32 = 300.0;
const BOX_H: f32 = 60.0;

fn text_json(content: &str, autofit: bool) -> serde_json::Value {
    serde_json::json!({
        "type": "text",
        "content": content,
        "style": {
            "width": BOX_W,
            "height": BOX_H,
            "font-size": 90,
            "color": "#ffffff",
            "white-space": "nowrap",
            "text-autofit": autofit,
        }
    })
}

fn render_at(content: &str, autofit: bool, time: f64) -> Vec<u8> {
    let component: Component =
        serde_json::from_value(text_json(content, autofit)).expect("deserialize");
    let child = ChildComponent {
        component,
        position: Some(PositionMode::Absolute { x: BOX_X, y: BOX_Y }),
        x: None,
        y: None,
        z_index: None,
        bleed: false,
    };
    let children = vec![child];

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
            scene_duration: SCENE_DURATION,
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
        scene_duration: SCENE_DURATION,
        camera: None,
    };
    paint_tree(canvas, &built.root, &layout, &frame, &dispatcher);

    let row_bytes = W as usize * 4;
    let mut pixels = vec![0u8; row_bytes * H as usize];
    let info = skia_safe::ImageInfo::new(
        (W as i32, H as i32),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    surface.read_pixels(&info, &mut pixels, row_bytes, (0, 0));
    pixels
}

/// Rightmost column (absolute, full-surface coordinates) with any painted
/// (non-zero-alpha) ink.
fn max_ink_x(pixels: &[u8]) -> Option<i32> {
    let mut max_x: Option<i32> = None;
    for y in 0..H as i32 {
        for x in (0..W as i32).rev() {
            let idx = ((y * W as i32 + x) * 4 + 3) as usize;
            if pixels[idx] > 0 {
                max_x = Some(max_x.map_or(x, |m| m.max(x)));
                break;
            }
        }
    }
    max_x
}

#[test]
fn without_autofit_the_nowrap_line_bleeds_past_the_box_through_the_real_pipeline() {
    // Control: a 90px `white-space: nowrap` line in a 300px-wide box, no
    // `text-autofit` — must bleed well past the box's right edge, exactly
    // as it always has (nowrap's existing, unmodified contract).
    let pixels = render_at("the quick brown fox jumps over the lazy dog", false, 0.0);
    let ink_right = max_ink_x(&pixels).expect("text must paint some ink");
    let box_right = (BOX_X + BOX_W) as i32;
    assert!(
        ink_right > box_right + 20,
        "control: without text-autofit, a 90px nowrap line must bleed well past its 300px box \
         (right edge {box_right}), got ink at x={ink_right}"
    );
}

#[test]
fn with_autofit_the_same_line_stays_inside_the_box_through_the_real_pipeline() {
    // Same fixture as the control above, `text-autofit: true` added. The
    // box `run_layout` reserves (via `TextIntrinsic::measure`, invoked by
    // taffy inside `run_layout`) and the pixels `paint_tree` actually draws
    // (via `Text::paint`, invoked by `LegacyPaintDispatcher`) must agree —
    // if they disagreed, the box would still be 300px wide but the ink
    // would still bleed past it exactly like the control test above.
    let pixels = render_at("the quick brown fox jumps over the lazy dog", true, 0.0);
    let ink_right = max_ink_x(&pixels).expect("text must paint some ink");
    let box_right = (BOX_X + BOX_W) as i32;
    assert!(
        ink_right <= box_right + 3,
        "text-autofit: true must keep the painted line inside its 300px box (right edge \
         {box_right}), got ink at x={ink_right}"
    );
}

#[test]
fn autofit_end_to_end_is_stable_across_frames_for_fixed_content() {
    // Temporal stability through the real per-frame pipeline (the box tree
    // and layout are rebuilt fresh every frame in the real encoder, exactly
    // like this test does via `build_scene_with_anim` at two different
    // `time`s): static content, static box → must be pixel-identical.
    let frame_a = render_at("the quick brown fox jumps over the lazy dog", true, 0.0);
    let frame_b = render_at("the quick brown fox jumps over the lazy dog", true, 2.0);
    assert_eq!(
        frame_a, frame_b,
        "fixed content in a fixed box must render byte-identically across frames/time — a \
         per-frame drift here is exactly what the temporal-stability requirement forbids"
    );
}
