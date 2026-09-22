//! Regression tests for the workstream A (animation & paint) audit findings
//! tracked in issue #220.

use rustmotion_core::css::style::{
    Background, BoxShadow, Color as CssColor, CssStyle, Display, FlexDirection, Position,
    Size as CSize,
};
use rustmotion_core::css::taffy_bridge::ConversionContext;
use rustmotion_core::css::units::{Length, LengthPercentage as CLP};
use rustmotion_core::engine::box_tree::{BoxKind, BoxNode};
use rustmotion_core::engine::layout_pass::run_layout;
use rustmotion_core::engine::paint_pass::{paint_tree, NoopDispatcher, PaintFrame};

fn test_frame(w: u32, h: u32) -> PaintFrame {
    PaintFrame {
        time: 0.0,
        scenario_time: 0.0,
        frame_index: 0,
        fps: 30,
        video_width: w,
        video_height: h,
        scene_duration: 1.0,
        camera: None,
    }
}

fn render_pixels(root: &mut BoxNode, w: u32, h: u32) -> Vec<u8> {
    root.assign_ids(0);
    let layout = run_layout(root, (w as f32, h as f32), &ConversionContext::default());
    let mut surface = skia_safe::surfaces::raster_n32_premul((w as i32, h as i32)).unwrap();
    paint_tree(
        surface.canvas(),
        root,
        &layout,
        &test_frame(w, h),
        &NoopDispatcher,
    );
    let info = skia_safe::ImageInfo::new(
        (w as i32, h as i32),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Unpremul,
        None,
    );
    let mut buf = vec![0u8; (w * h * 4) as usize];
    surface.read_pixels(&info, &mut buf, (w * 4) as usize, (0, 0));
    buf
}

fn root_node(w: f32, h: f32, background: &str, children: Vec<BoxNode>) -> BoxNode {
    BoxNode {
        id: 0,
        kind: BoxKind::Container,
        css: CssStyle {
            display: Some(Display::Flex),
            flex_direction: Some(FlexDirection::Column),
            width: Some(CSize::Length(CLP::Px(w))),
            height: Some(CSize::Length(CLP::Px(h))),
            background: Some(Background::Color(CssColor::String(background.to_string()))),
            ..Default::default()
        },
        children,
        intrinsic: None,
        source_path: None,
        window: None,
    }
}

fn probe(buf: &[u8], w: u32, x: usize, y: usize) -> (u8, u8, u8) {
    let i = (y * w as usize + x) * 4;
    (buf[i], buf[i + 1], buf[i + 2])
}

// ---- opacity layer must not clip the node's own outset box-shadow ----

fn card_with_shadow(opacity: Option<f32>) -> BoxNode {
    let css = CssStyle {
        position: Some(Position::Absolute),
        left: Some(CLP::Px(50.0)),
        top: Some(CLP::Px(50.0)),
        width: Some(CSize::Length(CLP::Px(100.0))),
        height: Some(CSize::Length(CLP::Px(100.0))),
        background: Some(Background::Color(CssColor::String("#ffffff".into()))),
        box_shadow: Some(vec![BoxShadow {
            offset_x: Length::Px(0.0),
            offset_y: Length::Px(0.0),
            blur: None,
            spread: Some(Length::Px(20.0)),
            color: Some(CssColor::String("#ff0000".into())),
            inset: None,
        }]),
        opacity,
        ..Default::default()
    };
    BoxNode {
        id: 0,
        kind: BoxKind::Container,
        css,
        children: vec![],
        intrinsic: None,
        source_path: None,
        window: None,
    }
}

#[test]
fn opacity_layer_does_not_clip_own_outset_box_shadow() {
    // 100x100 white card at (50,50) on a 200x200 black canvas, outset
    // box-shadow (red, spread 20, blur 0 -> hard-edged halo rect from
    // (30,30) to (170,170)). Probe point (100,45) sits in the halo band
    // above the card, outside its own border-box. `opacity: 0.999` forces
    // the opacity/filter SaveLayerRec open without visibly dimming the
    // probed color.
    let opaque = {
        let mut root = root_node(200.0, 200.0, "#000000", vec![card_with_shadow(None)]);
        render_pixels(&mut root, 200, 200)
    };
    let faded = {
        let mut root = root_node(200.0, 200.0, "#000000", vec![card_with_shadow(Some(0.999))]);
        render_pixels(&mut root, 200, 200)
    };

    let above_opaque = probe(&opaque, 200, 100, 45);
    assert!(
        above_opaque.0 > 200 && above_opaque.1 < 50,
        "sanity: shadow halo must be visible without an opacity layer, got {above_opaque:?}"
    );

    let above_faded = probe(&faded, 200, 100, 45);
    assert!(
        above_faded.0 > 200 && above_faded.1 < 50,
        "an opacity<1 layer must not clip the node's own outset box-shadow, got {above_faded:?}"
    );
}
