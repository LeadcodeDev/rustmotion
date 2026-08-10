//! Reproduction + regression test for relative `font-size` units (`rem`/
//! `vw`/`vh`) resolving to 0px silently (Lot B, wave S).
//!
//! Routes a `text` component through the real pipeline (box_builder →
//! run_layout → paint_tree) so both the intrinsic measurement (the box taffy
//! reserves) and the painter (what actually gets drawn) are exercised
//! together — the geometry validator's overflow checks depend on the two
//! agreeing, so a fix that only touches one side is not a real fix (see
//! `crates/rustmotion-components/tests/caption_presets.rs` for the same
//! pipeline pattern).

use rustmotion_components::box_builder::{build_scene_with_anim, BuildAnimationCtx};
use rustmotion_components::legacy_dispatch::LegacyPaintDispatcher;
use rustmotion_components::{ChildComponent, Component, PositionMode};
use rustmotion_core::css::taffy_bridge::ConversionContext;
use rustmotion_core::engine::layout_pass::run_layout;
use rustmotion_core::engine::paint_pass::{paint_tree, PaintFrame};

const W: u32 = 400;
const H: u32 = 300;

fn render(json: serde_json::Value) -> Vec<u8> {
    let component: Component = serde_json::from_value(json).expect("deserialize component");
    let child = ChildComponent {
        component,
        position: Some(PositionMode::Absolute { x: 20.0, y: 20.0 }),
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
            time: 0.5,
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
        time: 0.5,
        frame_index: 15,
        fps: 30,
        video_width: W,
        video_height: H,
        scene_duration: 2.0,
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

fn lit_pixels(buf: &[u8]) -> usize {
    buf.chunks_exact(4).filter(|p| p[3] > 0).count()
}

#[test]
fn text_with_rem_font_size_paints_visible_pixels() {
    // Reproduction: `rustmotion validate` on this exact style passes (exit
    // 0, "Valid scenario") with only warnings — `2rem` silently resolves to
    // 0px, so `TextIntrinsic` measures a 0-height box, `paint_pass`'s
    // `height <= 0.0` guard skips painting the node entirely, and the
    // rendered frame has no text at all.
    let json = serde_json::json!({
        "type": "text",
        "content": "HELLO",
        "style": { "font-size": "2rem", "color": "#FFFFFF" }
    });
    let buf = render(json);
    let lit = lit_pixels(&buf);
    assert!(
        lit > 100,
        "text at font-size: 2rem must paint visible pixels (2rem = 32px against the 16px CSS \
         root default), got {lit} lit pixels — 0 before the fix, since the relative unit \
         silently resolved to 0px"
    );
}

#[test]
fn text_with_vh_font_size_paints_visible_pixels() {
    // `vh` needs the real viewport (video_height), not just the root
    // font-size — a separate resolution path from `rem` inside
    // `LengthContext::resolve`.
    let json = serde_json::json!({
        "type": "text",
        "content": "HELLO",
        // 10vh of a 300px-tall frame = 30px.
        "style": { "font-size": "10vh", "color": "#FFFFFF" }
    });
    let buf = render(json);
    let lit = lit_pixels(&buf);
    assert!(
        lit > 100,
        "text at font-size: 10vh must paint visible pixels, got {lit} lit pixels"
    );
}
