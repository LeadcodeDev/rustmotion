//! Regression tests for the workstream A (animation & paint) audit findings
//! tracked in issue #220: RM-01, RM-09, RM-10, RM-37.

use rustmotion_core::css::style::{
    Background, BackgroundLayer, BoxShadow, Color as CssColor, CssStyle, Display, FlexDirection,
    GradientStop, Position, Size as CSize,
};
use rustmotion_core::css::taffy_bridge::ConversionContext;
use rustmotion_core::css::units::{Length, LengthPercentage as CLP};
use rustmotion_core::engine::animator::spring_value;
use rustmotion_core::engine::box_tree::{BoxKind, BoxNode};
use rustmotion_core::engine::layout_pass::run_layout;
use rustmotion_core::engine::paint_pass::{paint_tree, NoopDispatcher, PaintFrame};
use rustmotion_core::schema::SpringConfig;

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

// ---- RM-01: opacity layer must not clip the node's own outset box-shadow ----

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

// ---- RM-09: underdamped spring step response must start from rest ----

fn spring_config(damping: f64, stiffness: f64, mass: f64) -> SpringConfig {
    SpringConfig {
        damping,
        stiffness,
        mass,
        duration: None,
        rest_threshold: None,
    }
}

#[test]
fn underdamped_spring_step_response_starts_at_rest() {
    // Shipped defaults named in the audit finding: damping=15, stiffness=100,
    // mass=1 -> zeta=0.75 (underdamped). A step response that starts from
    // rest has ~0 velocity at t=0; a wrong sine-argument formula produces a
    // jolt of about 6.21/s instead.
    let config = spring_config(15.0, 100.0, 1.0);
    let h = 1e-5;
    let v0 = spring_value(0.0, &config);
    assert!(v0.abs() < 1e-9, "sanity: spring must start at 0, got {v0}");

    let vh = spring_value(h, &config);
    let slope = (vh - v0) / h;
    assert!(
        slope.abs() < 0.05,
        "underdamped step response must start at rest (~0 initial velocity), got slope {slope}"
    );
}

#[test]
fn underdamped_spring_matches_the_analytic_closed_form() {
    // Reference computed independently of the engine's implementation from
    // the textbook closed form for an underdamped step response:
    //   1 - e^{-zeta*omega*t} * [cos(omega_d*t) + (zeta*omega/omega_d)*sin(omega_d*t)]
    let damping = 15.0_f64;
    let stiffness = 100.0_f64;
    let mass = 1.0_f64;
    let omega = (stiffness / mass).sqrt();
    let zeta = damping / (2.0 * (stiffness * mass).sqrt());
    let omega_d = omega * (1.0 - zeta * zeta).sqrt();
    let reference = |t: f64| -> f64 {
        let decay = (-zeta * omega * t).exp();
        1.0 - decay * ((omega_d * t).cos() + (zeta * omega / omega_d) * (omega_d * t).sin())
    };

    let config = spring_config(damping, stiffness, mass);
    for t in [0.0, 1.0 / 60.0, 0.1, 0.3, 0.6, 1.0] {
        let expected = reference(t);
        let actual = spring_value(t, &config);
        assert!(
            (actual - expected).abs() < 1e-6,
            "t={t}: expected {expected} (analytic reference), got {actual}"
        );
    }
}

// ---- RM-10: linear-gradient(180deg, ...) must put the first stop at the top ----

fn gradient_card(w: f32, h: f32, angle: f32) -> BoxNode {
    let css = CssStyle {
        width: Some(CSize::Length(CLP::Px(w))),
        height: Some(CSize::Length(CLP::Px(h))),
        background: Some(Background::Single(BackgroundLayer::LinearGradient {
            angle: Some(angle),
            stops: vec![
                GradientStop {
                    color: CssColor::String("#ffffff".into()),
                    offset: Some(0.0),
                },
                GradientStop {
                    color: CssColor::String("#000000".into()),
                    offset: Some(1.0),
                },
            ],
        })),
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
fn linear_gradient_180deg_puts_the_first_stop_at_the_top() {
    // CSS: `angle: 180` ("to bottom") points the gradient line downward, so
    // the first stop lands at the top and the last stop at the bottom.
    let mut root = gradient_card(100.0, 100.0, 180.0);
    let buf = render_pixels(&mut root, 100, 100);

    let top = probe(&buf, 100, 50, 2);
    let bottom = probe(&buf, 100, 50, 97);
    assert!(
        top.0 > 200 && top.1 > 200 && top.2 > 200,
        "angle: 180 must put the white first stop at the top, got {top:?}"
    );
    assert!(
        bottom.0 < 50 && bottom.1 < 50 && bottom.2 < 50,
        "angle: 180 must put the black last stop at the bottom, got {bottom:?}"
    );
}

// ---- RM-37: `spring_settle_time` must not be rescanned on every `spring_value` call ----

#[test]
fn spring_settle_time_is_memoized_not_rescanned_every_call() {
    // `SpringConfig::duration` routes every `spring_value` sample through
    // `spring_settle_time`'s 2k-20k-step coarse-then-bisect scan (see
    // animator.rs). The scan result depends only on the spring's own
    // (damping, stiffness, mass, threshold) — invariant across every frame
    // an animation is sampled at — so repeating it per call is pure waste.
    // Measured uncached on this parameter set: 2000 calls take ~530ms in a
    // debug build; memoized, the same 2000 calls (one real scan, the rest
    // cache hits) complete in well under a tenth of that.
    let config = SpringConfig {
        damping: 37.0,
        stiffness: 733.0,
        mass: 1.0,
        duration: Some(0.42),
        rest_threshold: None,
    };
    let start = std::time::Instant::now();
    let mut acc = 0.0;
    for i in 0..2000 {
        let t = (i as f64) * 1e-4;
        acc += spring_value(t, &config);
    }
    let elapsed = start.elapsed();
    assert!(
        acc.is_finite(),
        "sanity: accumulated spring values must be finite"
    );
    assert!(
        elapsed.as_millis() < 150,
        "2000 spring_value calls with the same spring parameters took {elapsed:?}; \
         spring_settle_time must be memoized on (damping, stiffness, mass, threshold) rather \
         than re-scanned on every call"
    );
}
