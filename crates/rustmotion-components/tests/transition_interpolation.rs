use rustmotion_components::box_builder::{build_scene_at_time, BuildAnimationCtx};
use rustmotion_components::{ChildComponent, Component, PositionMode};
use rustmotion_core::css::style::{Background, BorderRadius, CssStyle};

const SCENE_DURATION: f64 = 4.0;
const STEP_AT: f64 = 1.0;
const DURATION: f64 = 1.0;
const MIDPOINT: f64 = STEP_AT + DURATION / 2.0;

fn div_with_transition(
    from_radius: f64,
    to_radius: f64,
    from_bg: &str,
    to_bg: &str,
) -> serde_json::Value {
    serde_json::json!({
        "type": "div",
        "style": {
            "background": from_bg,
            "border-radius": from_radius,
            "transition": DURATION
        },
        "timeline": [
            { "at": STEP_AT, "style": { "background": to_bg, "border-radius": to_radius } }
        ]
    })
}

fn css_at(json: serde_json::Value, time: f64) -> CssStyle {
    let component: Component = serde_json::from_value(json).expect("deserialize component");
    let child = ChildComponent {
        id: None,
        component,
        position: Some(PositionMode::Absolute { x: 0.0, y: 0.0 }),
        x: None,
        y: None,
        z_index: None,
        bleed: false,
    };
    let children = vec![child];
    let built = build_scene_at_time(
        &children,
        (800.0, 600.0),
        CssStyle::default(),
        BuildAnimationCtx {
            time,
            scenario_time: time,
            scene_duration: SCENE_DURATION,
            fps: 30,
        },
    );
    built.root.children[0].css.clone()
}

fn radius_px(css: &CssStyle) -> f32 {
    match css.border_radius.as_ref().expect("border-radius set") {
        BorderRadius::Uniform(lp) => lp.px(),
        other => panic!("expected uniform border-radius, got {other:?}"),
    }
}

fn bg_hex(css: &CssStyle) -> String {
    match css.background.as_ref().expect("background set") {
        Background::Color(c) => c.to_css_string(),
        other => panic!("expected solid background, got {other:?}"),
    }
}

#[test]
fn border_radius_interpolates_at_midpoint() {
    let json = div_with_transition(0.0, 40.0, "#000000", "#000000");

    let before = radius_px(&css_at(json.clone(), STEP_AT - 0.01));
    let mid = radius_px(&css_at(json.clone(), MIDPOINT));
    let after = radius_px(&css_at(json.clone(), STEP_AT + DURATION + 0.01));

    assert!((before - 0.0).abs() < 0.5, "pre-step radius: {before}");
    assert!(
        (after - 40.0).abs() < 0.5,
        "post-transition radius: {after}"
    );
    assert!(
        mid > 2.0 && mid < 38.0,
        "expected a genuine mid-transition value strictly between 0 and 40, got {mid} \
         (a value pinned to 0 or 40 here means the property snapped instead of interpolating)"
    );
}

#[test]
fn background_color_interpolates_at_midpoint() {
    let json = div_with_transition(0.0, 0.0, "#000000", "#ffffff");

    let mid = bg_hex(&css_at(json.clone(), MIDPOINT));
    let after = bg_hex(&css_at(json.clone(), STEP_AT + DURATION + 0.01));

    assert_eq!(after.to_lowercase(), "#ffffff");
    assert_ne!(
        mid.to_lowercase(),
        "#000000",
        "background should have started moving away from its origin by the midpoint"
    );
    assert_ne!(
        mid.to_lowercase(),
        "#ffffff",
        "background reached its destination color before the transition finished — that's a jump, \
         got {mid} at the midpoint"
    );
}

#[test]
fn width_still_snaps_because_it_is_a_layout_property() {
    let json = serde_json::json!({
        "type": "div",
        "style": {
            "width": "100px",
            "transition": DURATION
        },
        "timeline": [
            { "at": STEP_AT, "style": { "width": "300px" } }
        ]
    });

    let mid = css_at(json.clone(), MIDPOINT).width.expect("width set");
    let px = match mid {
        rustmotion_core::css::style::Size::Length(lp) => lp.px(),
        other => panic!("expected a length, got {other:?}"),
    };
    assert!(
        (px - 300.0).abs() < 0.5,
        "width is documented as still snapping (not yet layout-integrated); got {px}, expected 300 (the snapped target)"
    );
}
