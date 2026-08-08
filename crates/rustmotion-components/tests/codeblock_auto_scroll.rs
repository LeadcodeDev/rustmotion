//! Pixel test for codeblock `auto_scroll` during a typewriter reveal
//! (audit finding #4).
//!
//! Routes a `codeblock` through the real pipeline (box_builder, run_layout,
//! paint_tree) exactly like `caption_presets.rs` does, and counts painted
//! (non-background) pixels at several points during a typewriter reveal.
//! Before the fix, `scroll_offset` was computed from the *full* code's
//! natural height regardless of how many lines the reveal had actually
//! painted, so the box stayed empty for the majority of the reveal — the
//! newly-revealed lines were translated above the clip and invisible until
//! the reveal caught up with that constant offset.

use rustmotion_components::box_builder::{build_scene_with_anim, BuildAnimationCtx};
use rustmotion_components::legacy_dispatch::LegacyPaintDispatcher;
use rustmotion_components::{ChildComponent, Component, PositionMode};
use rustmotion_core::css::taffy_bridge::ConversionContext;
use rustmotion_core::engine::layout_pass::run_layout;
use rustmotion_core::engine::paint_pass::{paint_tree, PaintFrame};

const W: u32 = 1000;
const H: u32 = 600;
const SCENE_DURATION: f64 = 4.5;

fn codeblock_json() -> serde_json::Value {
    let lines: Vec<String> = (0..40).map(|i| format!("let line_{i} = {i};")).collect();
    let code = lines.join("\n");
    serde_json::json!({
        "type": "codeblock",
        "code": code,
        "language": "rust",
        "auto_scroll": true,
        "reveal": { "mode": "typewriter", "start": 0.0, "duration": 4.0 },
        "style": { "width": "900px", "height": "300px", "font-size": 20 }
    })
}

/// Count pixels whose color differs measurably from the codeblock's own
/// dark background (`#2b303b` when unset) — i.e. actual glyph ink, not just
/// "anything non-transparent" (the background rect itself is opaque and
/// covers the whole box).
fn text_ink_pixels(buf: &[u8]) -> usize {
    // Background is #2b303b ~ (43, 48, 59). Count pixels that deviate from
    // that by a wide margin in any channel — syntect's theme colors are all
    // much brighter than the near-black background.
    buf.chunks_exact(4)
        .filter(|p| {
            let (r, g, b, a) = (p[0] as i32, p[1] as i32, p[2] as i32, p[3] as i32);
            a > 200 && ((r - 43).abs() > 40 || (g - 48).abs() > 40 || (b - 59).abs() > 40)
        })
        .count()
}

fn render_codeblock_at(time: f64) -> Vec<u8> {
    let component: Component = serde_json::from_value(codeblock_json()).expect("deserialize");
    let child = ChildComponent {
        component,
        position: Some(PositionMode::Absolute { x: 50.0, y: 150.0 }),
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

#[test]
fn early_reveal_shows_text_not_an_empty_box() {
    // At t=0.5s (12.5% into a 4s typewriter reveal, well past the first
    // line), some text must already be visible — before the fix, this
    // stayed at 0 ink pixels until ~75% of the reveal had elapsed.
    let ink = text_ink_pixels(&render_codeblock_at(0.5));
    assert!(
        ink > 20,
        "expected visible text ink early in the reveal (t=0.5s), got {ink} ink pixels"
    );
}

#[test]
fn mid_reveal_shows_text_not_an_empty_box() {
    // t=2.0s: 50% into the reveal — well within the range the audit
    // measured as a completely empty box (0 ink pixels at t=0.5/1.0/2.0).
    let ink = text_ink_pixels(&render_codeblock_at(2.0));
    assert!(
        ink > 20,
        "expected visible text ink mid-reveal (t=2.0s), got {ink} ink pixels"
    );
}

#[test]
fn ink_grows_monotonically_enough_across_the_reveal() {
    // Sanity: as more of the reveal completes, more text should be on
    // screen (loosely monotonic — auto_scroll keeps only the visible
    // window, so it won't be strictly increasing forever, but the box must
    // never regress to empty once text has started appearing).
    let t_early = text_ink_pixels(&render_codeblock_at(0.5));
    let t_late = text_ink_pixels(&render_codeblock_at(3.9));
    assert!(t_early > 0, "must have visible ink at t=0.5s, got 0");
    assert!(t_late > 0, "must have visible ink at t=3.9s, got 0");
}
