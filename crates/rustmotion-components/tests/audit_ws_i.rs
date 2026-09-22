//! Regression tests for workstream I of the 2026-09 audit (components and
//! the CSS cascade).
//!
//! - `apply_intrinsic_overrides`'s default-size branches resolved
//!   `font-size` through the context-free `font_size_px_or`, which returns
//!   `0.0` for a relative unit (`rem`/`vw`/`vh`) instead of resolving it —
//!   collapsing `marquee`/`list`/`callout`/`tooltip`/`pill_nav` to a 0px box.
//! - `cascade::inherit_from` computes inherited `color`/`font-*` but
//!   nothing on the render path ever reads the result — every painter reads
//!   its own component's un-cascaded `style` field instead.
//! - `TableIntrinsic` measures content-fitted per-column widths, but
//!   the painter splits the box evenly across columns regardless.

use rustmotion_components::box_builder::{build_scene_with_anim, BuildAnimationCtx};
use rustmotion_components::{ChildComponent, Component, PositionMode};
use rustmotion_core::css::taffy_bridge::ConversionContext;
use rustmotion_core::engine::layout_pass::run_layout;

fn single_child_scene(json: serde_json::Value) -> ChildComponent {
    let component: Component = serde_json::from_value(json).expect("deserialize component");
    ChildComponent {
        component,
        position: Some(PositionMode::Absolute { x: 0.0, y: 0.0 }),
        x: None,
        y: None,
        z_index: None,
        bleed: false,
    }
}

// ─── default sizes must resolve against the node's font-size ───────────────

#[test]
fn marquee_with_relative_font_size_gets_a_positive_height() {
    // Reproduction: `font_size_px_or` returns 0.0 for any relative unit
    // (`.px()` can't resolve `%`/`em`/`rem`/`vw`/`vh`). Five sites in
    // `apply_intrinsic_overrides` still called it — Marquee among them —
    // so a marquee declaring `"font-size": "2rem"` and no explicit height
    // used to collapse `apply_default_size(css, 800.0, 0.0)` to a 0px-tall
    // box, and `paint_pass`'s `height <= 0.0` guard then skipped it
    // entirely: the marquee never appeared on screen.
    let child = single_child_scene(serde_json::json!({
        "type": "marquee",
        "content": "BREAKING NEWS",
        "style": { "font-size": "2rem" }
    }));
    let children = vec![child];

    let built = build_scene_with_anim(
        &children,
        (1920.0, 1080.0),
        BuildAnimationCtx {
            time: 0.0,
            scenario_time: 0.0,
            scene_duration: 1.0,
            fps: 30,
        },
    );
    let layout = run_layout(&built.root, (1920.0, 1080.0), &ConversionContext::default());
    let marquee_id = built.root.children[0].id;
    let l = layout.get(marquee_id).expect("marquee laid out");
    assert!(
        l.height > 0.0,
        "marquee at font-size: 2rem must lay out with a positive height, got {}",
        l.height
    );
}
