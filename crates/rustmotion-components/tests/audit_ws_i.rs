//! Regression tests for components and the CSS cascade:
//!
//! - `apply_intrinsic_overrides`'s default-size branches resolved
//!   `font-size` through the context-free `font_size_px_or`, which returns
//!   `0.0` for a relative unit (`rem`/`vw`/`vh`) instead of resolving it —
//!   collapsing `marquee`/`list`/`callout`/`tooltip`/`pill_nav` to a 0px box.
//! - `cascade::inherit_from` computes inherited `color`/`font-*` but
//!   nothing on the render path ever reads the result — every painter reads
//!   its own component's un-cascaded `style` field instead.
//! - `TableIntrinsic` measures content-fitted per-column widths, but the
//!   painter splits the box evenly across columns regardless.

use rustmotion_components::box_builder::{build_scene_with_anim, BuildAnimationCtx};
use rustmotion_components::legacy_dispatch::LegacyPaintDispatcher;
use rustmotion_components::{ChildComponent, Component, PositionMode};
use rustmotion_core::css::taffy_bridge::ConversionContext;
use rustmotion_core::engine::layout_pass::run_layout;
use rustmotion_core::engine::paint_pass::{paint_tree, PaintFrame};

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

// ─── relative font-size on box_builder's default-size branches ────────────

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

// ─── CSS cascade reaching the painter and the intrinsic measurer ──────────

const CASCADE_SCENE_W: i32 = 600;
const CASCADE_SCENE_H: i32 = 500;

struct PaintedScene {
    pixels: Vec<u8>,
    text_layout_height: f32,
}

fn paint_card_with_text_child(card_json: serde_json::Value) -> PaintedScene {
    let child = single_child_scene(card_json);
    let children = vec![child];

    let built = build_scene_with_anim(
        &children,
        (CASCADE_SCENE_W as f32, CASCADE_SCENE_H as f32),
        BuildAnimationCtx {
            time: 0.0,
            scenario_time: 0.0,
            scene_duration: 1.0,
            fps: 30,
        },
    );
    let layout = run_layout(
        &built.root,
        (CASCADE_SCENE_W as f32, CASCADE_SCENE_H as f32),
        &ConversionContext::default(),
    );
    let text_id = built.root.children[0].children[0].id;
    let text_layout_height = layout.get(text_id).expect("text laid out").height;

    let mut surface = skia_safe::surfaces::raster_n32_premul((CASCADE_SCENE_W, CASCADE_SCENE_H))
        .expect("raster surface");
    let canvas = surface.canvas();
    canvas.clear(skia_safe::Color::BLACK);
    let dispatcher = LegacyPaintDispatcher::for_scene(&built);
    let frame = PaintFrame {
        time: 0.0,
        scenario_time: 0.0,
        frame_index: 0,
        fps: 30,
        video_width: CASCADE_SCENE_W as u32,
        video_height: CASCADE_SCENE_H as u32,
        scene_duration: 1.0,
        camera: None,
    };
    paint_tree(canvas, &built.root, &layout, &frame, &dispatcher);

    let row_bytes = CASCADE_SCENE_W as usize * 4;
    let mut pixels = vec![0u8; row_bytes * CASCADE_SCENE_H as usize];
    let info = skia_safe::ImageInfo::new(
        (CASCADE_SCENE_W, CASCADE_SCENE_H),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    surface.read_pixels(&info, &mut pixels, row_bytes, (0, 0));

    PaintedScene {
        pixels,
        text_layout_height,
    }
}

fn count_dominant(pixels: &[u8], dominant: usize, muted: &[usize]) -> usize {
    pixels
        .chunks_exact(4)
        .filter(|p| p[dominant] > 180 && muted.iter().all(|&m| p[m] < 80))
        .count()
}

#[test]
fn card_color_and_font_size_cascade_to_painted_text_child() {
    // `cascade::inherit_from` computes the twelve inheritable properties but
    // `LegacyPaintDispatcher::dispatch` binds the resolved `CssStyle` to
    // `_css` and drops it, and `component_intrinsic` builds `TextIntrinsic`
    // from `&child.component` (the un-cascaded component) — so a card's
    // `color`/`font-size` never reached a child `text` with no value of its
    // own, at either paint time or measure time. The text rendered at the
    // painter's own fallback (48px, #FFFFFF) instead of the card's (200px,
    // red).
    let scene = paint_card_with_text_child(serde_json::json!({
        "type": "card",
        "style": {
            "color": "#ff0000",
            "font-size": 200,
            "width": 500,
            "flex-direction": "column"
        },
        "children": [
            { "type": "text", "content": "WWWW" }
        ]
    }));

    assert!(
        scene.text_layout_height > 150.0,
        "text child with no font-size of its own must be measured at the \
         card's cascaded 200px (~240px line height), not the 48px default \
         (~57px line height) — got layout height {}",
        scene.text_layout_height
    );

    let red_pixels = count_dominant(&scene.pixels, 0, &[1, 2]);
    assert!(
        red_pixels > 20,
        "text child with no color of its own must paint in the card's \
         cascaded red, not the painter's white fallback — found {red_pixels} \
         red-dominant pixels"
    );
}

#[test]
fn text_own_color_wins_over_cascaded_card_color_at_paint_time() {
    // The other half of the contract this fix must not break: a `text`
    // that DOES declare its own `color` must keep winning over the parent's,
    // not just at the box-tree level (already covered by
    // `box_builder.rs::text_own_color_wins_over_inherited_card_color`) but
    // in what actually gets painted.
    let scene = paint_card_with_text_child(serde_json::json!({
        "type": "card",
        "style": {
            "color": "#ff0000",
            "width": 500,
            "flex-direction": "column"
        },
        "children": [
            { "type": "text", "content": "WWWW", "style": { "color": "#00ff00" } }
        ]
    }));

    let green_pixels = count_dominant(&scene.pixels, 1, &[0, 2]);
    let red_pixels = count_dominant(&scene.pixels, 0, &[1, 2]);
    assert!(
        green_pixels > 20,
        "text's own explicit color must still be painted — found {green_pixels} \
         green-dominant pixels"
    );
    assert_eq!(
        red_pixels, 0,
        "text's own explicit color must win over the card's cascaded red — \
         found {red_pixels} red-dominant pixels"
    );
}

// ─── table column widths: painter vs. intrinsic measurer ──────────────────

#[test]
fn table_columns_are_sized_by_content_not_split_evenly() {
    // `TableIntrinsic::compute_width` measures each column's own natural
    // width (header/cell text + padding) and sums them for the box's
    // reserved size, but `Table::resolve_column_widths` (the *painter*'s own
    // distribution) just divides the laid-out width evenly across columns
    // regardless. A table whose columns have very different natural widths
    // ("ID" vs. a long description) used to get a column border sitting at
    // the arithmetic midpoint of the box, not near the natural split the
    // measurer already computed.
    use rustmotion_components::Component;
    use rustmotion_core::engine::animator::AnimatedProperties;
    use rustmotion_core::engine::layout_pass::BoxLayout;
    use rustmotion_core::traits::{PaintCtx, Painter};

    let component: Component = serde_json::from_value(serde_json::json!({
        "type": "table",
        "headers": ["ID", "Description of the incident"],
        "rows": [["1", "Something happened during the incident"]]
    }))
    .expect("deserialize table");
    let Component::Table(table) = component else {
        panic!("expected a table component");
    };

    const W: i32 = 500;
    const H: i32 = 100;
    let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
    let canvas = surface.canvas();
    canvas.clear(skia_safe::Color::BLACK);
    let layout = BoxLayout {
        x: 0.0,
        y: 0.0,
        width: W as f32,
        height: H as f32,
        ..Default::default()
    };
    let ctx = PaintCtx {
        time: 0.0,
        scenario_time: 0.0,
        scene_duration: 1.0,
        frame_index: 0,
        fps: 30,
        video_width: W as u32,
        video_height: H as u32,
        stagger_offset: 0.0,
    };
    table.paint_content(canvas, &layout, &AnimatedProperties::default(), &ctx);

    let snapshot = surface.image_snapshot();
    let info = skia_safe::ImageInfo::new(
        (W, H),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    let mut buf = vec![0u8; (W * H * 4) as usize];
    assert!(snapshot.read_pixels(
        &info,
        &mut buf,
        (W * 4) as usize,
        skia_safe::IPoint::new(0, 0),
        skia_safe::image::CachingHint::Disallow,
    ));

    let y = 15usize;
    let border_distance = |x: usize| -> i32 {
        let idx = (y * W as usize + x) * 4;
        let (r, g, b) = (buf[idx] as i32, buf[idx + 1] as i32, buf[idx + 2] as i32);
        (r - 0x4B).abs() + (g - 0x55).abs() + (b - 0x63).abs()
    };
    let boundary_x = (20usize..(W as usize - 20))
        .min_by_key(|&x| border_distance(x))
        .expect("scan range is non-empty");
    assert!(
        border_distance(boundary_x) < 90,
        "no column-boundary border line found in the interior of the table \
         (closest match at x={boundary_x}, distance={})",
        border_distance(boundary_x)
    );
    assert!(
        boundary_x < 200,
        "the ID/Description column boundary should sit near the natural \
         header-width split, not near the evenly-split midpoint (250) — \
         found it at x={boundary_x}"
    );
}
