use rustmotion_components::box_builder::{build_scene_with_anim, BuildAnimationCtx};
use rustmotion_components::legacy_dispatch::LegacyPaintDispatcher;
use rustmotion_components::{ChildComponent, Component, PositionMode};
use rustmotion_core::css::taffy_bridge::ConversionContext;
use rustmotion_core::engine::layout_pass::run_layout;
use rustmotion_core::engine::paint_pass::{paint_tree, PaintFrame};

fn single_child_scene(json: serde_json::Value) -> ChildComponent {
    let component: Component = serde_json::from_value(json).expect("deserialize component");
    ChildComponent {
        id: None,
        component,
        position: Some(PositionMode::Absolute { x: 0.0, y: 0.0 }),
        x: None,
        y: None,
        z_index: None,
        bleed: false,
    }
}

#[test]
fn marquee_with_relative_font_size_gets_a_positive_height() {
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

const CASCADE_SCENE_W: i32 = 600;
const CASCADE_SCENE_H: i32 = 500;

struct PaintedScene {
    pixels: Vec<u8>,
    child_layout_height: f32,
}

fn paint_card_with_child(card_json: serde_json::Value) -> PaintedScene {
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
    let child_id = built.root.children[0].children[0].id;
    let child_layout_height = layout.get(child_id).expect("child laid out").height;

    let mut surface = skia_safe::surfaces::raster_n32_premul((CASCADE_SCENE_W, CASCADE_SCENE_H))
        .expect("raster surface");
    let canvas = surface.canvas();
    canvas.clear(skia_safe::Color::BLACK);
    let dispatcher = LegacyPaintDispatcher::for_scene(&built);
    let frame = PaintFrame {
        light: Default::default(),
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
        child_layout_height,
    }
}

fn count_dominant(pixels: &[u8], dominant: usize, muted: &[usize]) -> usize {
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[dominant] > 180 && muted.iter().all(|&m| p[m] < 80))
        .count()
}

#[test]
fn card_color_and_font_size_cascade_to_painted_text_child() {
    let scene = paint_card_with_child(serde_json::json!({
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
        scene.child_layout_height > 150.0,
        "text child with no font-size of its own must be measured at the \
         card's cascaded 200px (~240px line height), not the 48px default \
         (~57px line height) — got layout height {}",
        scene.child_layout_height
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
    let scene = paint_card_with_child(serde_json::json!({
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

fn any_ink_above_black_below_y(pixels: &[u8], width: i32, y_threshold: i32) -> bool {
    pixels.as_chunks::<4>().0.iter().enumerate().any(|(i, p)| {
        let y = i as i32 / width;
        y > y_threshold && (p[0] > 20 || p[1] > 20 || p[2] > 20)
    })
}

#[test]
fn card_font_size_cascades_to_measured_and_painted_gradient_text_child() {
    let scene = paint_card_with_child(serde_json::json!({
        "type": "card",
        "style": {
            "font-size": 200,
            "width": 500,
            "flex-direction": "column"
        },
        "children": [
            { "type": "gradient_text", "content": "W" }
        ]
    }));

    assert!(
        scene.child_layout_height > 150.0,
        "gradient_text child with no font-size of its own must be measured \
         at the card's cascaded 200px, not the 48px default — got layout \
         height {}",
        scene.child_layout_height
    );
    assert!(
        any_ink_above_black_below_y(&scene.pixels, CASCADE_SCENE_W, 100),
        "gradient_text painted at the card's cascaded 200px must have ink \
         reaching well past y=100 — a 48px default line would not"
    );
}

#[test]
fn card_color_cascades_to_painted_caption_child() {
    let scene = paint_card_with_child(serde_json::json!({
        "type": "card",
        "style": {
            "color": "#ff0000",
            "width": 500,
            "flex-direction": "column"
        },
        "children": [
            {
                "type": "caption",
                "words": [{ "text": "HELLO", "start": 10.0, "end": 20.0 }]
            }
        ]
    }));

    let red_pixels = count_dominant(&scene.pixels, 0, &[1, 2]);
    assert!(
        red_pixels > 20,
        "caption's inactive word (painted at time=0, outside its \
         [10,20) window) must use the card's cascaded red, not the \
         painter's white fallback — found {red_pixels} red-dominant pixels"
    );
}

#[test]
fn card_color_cascades_to_painted_rich_text_child() {
    let scene = paint_card_with_child(serde_json::json!({
        "type": "card",
        "style": {
            "color": "#ff0000",
            "width": 500,
            "flex-direction": "column"
        },
        "children": [
            { "type": "rich_text", "spans": [{ "text": "WWWW" }] }
        ]
    }));

    let red_pixels = count_dominant(&scene.pixels, 0, &[1, 2]);
    assert!(
        red_pixels > 20,
        "rich_text span with no color of its own must paint in the card's \
         cascaded red, not the painter's white fallback — found {red_pixels} \
         red-dominant pixels"
    );
}

#[test]
fn cascaded_components_match_the_verified_set() {
    use rustmotion_components::Component;
    use rustmotion_core::css::style::{Color, CssStyle as CoreCssStyle};
    use rustmotion_core::css::Length;

    let resolved = CoreCssStyle {
        color: Some(Color::String("#ff0000".into())),
        font_size: Some(Length::Px(40.0)),
        ..Default::default()
    };

    let cases: &[(&str, bool, serde_json::Value)] = &[
        (
            "text",
            true,
            serde_json::json!({"type":"text","content":"x"}),
        ),
        (
            "gradient_text",
            true,
            serde_json::json!({"type":"gradient_text","content":"x"}),
        ),
        (
            "caption",
            true,
            serde_json::json!({"type":"caption","words":[{"text":"x","start":0.0,"end":0.0}]}),
        ),
        (
            "rich_text",
            true,
            serde_json::json!({"type":"rich_text","spans":[{"text":"x"}]}),
        ),
        (
            "badge",
            true,
            serde_json::json!({"type":"badge","text":"x"}),
        ),
        (
            "callout",
            true,
            serde_json::json!({"type":"callout","text":"x"}),
        ),
        (
            "counter",
            true,
            serde_json::json!({"type":"counter","from":0.0,"to":1.0}),
        ),
        ("divider", true, serde_json::json!({"type":"divider"})),
        (
            "icon",
            true,
            serde_json::json!({"type":"icon","icon":"lucide:home"}),
        ),
        ("kbd", true, serde_json::json!({"type":"kbd","key":"A"})),
        (
            "list",
            true,
            serde_json::json!({"type":"list","items":[{"text":"x"}]}),
        ),
        (
            "marquee",
            true,
            serde_json::json!({"type":"marquee","content":"x"}),
        ),
        (
            "number_wheel",
            true,
            serde_json::json!({"type":"number_wheel","value":"1"}),
        ),
        (
            "pill_nav",
            true,
            serde_json::json!({"type":"pill_nav","items":["a"]}),
        ),
        (
            "table",
            true,
            serde_json::json!({"type":"table","headers":["A"],"rows":[]}),
        ),
        (
            "tooltip",
            true,
            serde_json::json!({"type":"tooltip","text":"x"}),
        ),
        (
            "audio_spectrum",
            false,
            serde_json::json!({"type":"audio_spectrum"}),
        ),
        (
            "shape",
            false,
            serde_json::json!({"type":"shape","shape":"rect"}),
        ),
        (
            "image",
            false,
            serde_json::json!({"type":"image","src":"x.png"}),
        ),
        ("svg", false, serde_json::json!({"type":"svg"})),
        (
            "video",
            false,
            serde_json::json!({"type":"video","src":"x.mp4"}),
        ),
        (
            "gif",
            false,
            serde_json::json!({"type":"gif","src":"x.gif"}),
        ),
        ("cursor", false, serde_json::json!({"type":"cursor"})),
        (
            "connector",
            false,
            serde_json::json!({"type":"connector","from":{"x":0.0,"y":0.0},"to":{"x":1.0,"y":1.0}}),
        ),
        (
            "avatar",
            false,
            serde_json::json!({"type":"avatar","src":"x.png"}),
        ),
        (
            "avatar_group",
            false,
            serde_json::json!({"type":"avatar_group","avatars":[]}),
        ),
        (
            "arrow",
            false,
            serde_json::json!({"type":"arrow","x2":10.0,"y2":10.0}),
        ),
        (
            "chart",
            false,
            serde_json::json!({"type":"chart","chart_type":"bar"}),
        ),
        (
            "comparison",
            false,
            serde_json::json!({"type":"comparison"}),
        ),
        ("countdown", false, serde_json::json!({"type":"countdown"})),
        (
            "dot_map",
            false,
            serde_json::json!({"type":"dot_map","points":[]}),
        ),
        ("gauge", false, serde_json::json!({"type":"gauge"})),
        (
            "heatmap",
            false,
            serde_json::json!({"type":"heatmap","data":[]}),
        ),
        (
            "line",
            false,
            serde_json::json!({"type":"line","x2":10.0,"y2":10.0}),
        ),
        ("lottie", false, serde_json::json!({"type":"lottie"})),
        (
            "mockup",
            false,
            serde_json::json!({"type":"mockup","device":"iphone","src":"x.png"}),
        ),
        (
            "particle",
            false,
            serde_json::json!({"type":"particle","particle_type":"confetti"}),
        ),
        ("progress", false, serde_json::json!({"type":"progress"})),
        (
            "qr_code",
            false,
            serde_json::json!({"type":"qr_code","content":"x"}),
        ),
        (
            "success_check",
            false,
            serde_json::json!({"type":"success_check"}),
        ),
        ("pointer", false, serde_json::json!({"type":"pointer"})),
        ("rating", false, serde_json::json!({"type":"rating"})),
        ("skeleton", false, serde_json::json!({"type":"skeleton"})),
        ("slider", false, serde_json::json!({"type":"slider"})),
        (
            "sparkline",
            false,
            serde_json::json!({"type":"sparkline","data":[]}),
        ),
        (
            "stat",
            false,
            serde_json::json!({"type":"stat","value":"1"}),
        ),
        (
            "stepper",
            false,
            serde_json::json!({"type":"stepper","steps":[]}),
        ),
        ("switch", false, serde_json::json!({"type":"switch"})),
        (
            "tag_cloud",
            false,
            serde_json::json!({"type":"tag_cloud","tags":[]}),
        ),
        (
            "timeline",
            false,
            serde_json::json!({"type":"timeline","steps":[]}),
        ),
        (
            "treemap",
            false,
            serde_json::json!({"type":"treemap","data":[]}),
        ),
        (
            "positioned",
            false,
            serde_json::json!({"type":"positioned"}),
        ),
        ("flex", false, serde_json::json!({"type":"flex"})),
        ("grid", false, serde_json::json!({"type":"grid"})),
        ("card", false, serde_json::json!({"type":"card"})),
        ("div", false, serde_json::json!({"type":"div"})),
        ("waveform", false, serde_json::json!({"type":"waveform"})),
    ];

    assert_eq!(
        cases.len(),
        57,
        "this table must cover every Component variant (currently 57) — the \
         compiler enforces that with_cascaded_style itself classifies every \
         variant, but only this list enforces that the classification stays \
         the one this workstream verified"
    );

    for (name, expected_typographic, json) in cases {
        let component: Component =
            serde_json::from_value(json.clone()).unwrap_or_else(|e| panic!("{name}: {e}"));
        let got = component.with_cascaded_style(&resolved).is_some();
        assert_eq!(
            got, *expected_typographic,
            "{name}: with_cascaded_style returned is_some()={got}, expected \
             {expected_typographic}"
        );
    }
}

#[test]
fn table_columns_are_sized_by_content_not_split_evenly() {
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

#[test]
fn caption_own_color_wins_over_cascaded_card_color_at_paint_time() {
    let scene = paint_card_with_child(serde_json::json!({
        "type": "card",
        "style": { "color": "#ff0000", "width": 500, "flex-direction": "column" },
        "children": [
            {
                "type": "caption",
                "words": [{ "text": "WWWW", "start": 0.0, "end": 0.0 }],
                "style": { "color": "#00ff00" }
            }
        ]
    }));

    let green_pixels = count_dominant(&scene.pixels, 1, &[0, 2]);
    let red_pixels = count_dominant(&scene.pixels, 0, &[1, 2]);
    assert!(
        green_pixels > 20,
        "caption's own explicit color must still be painted — found \
         {green_pixels} green-dominant pixels"
    );
    assert_eq!(
        red_pixels, 0,
        "caption's own explicit color must win over the card's cascaded red \
         — found {red_pixels} red-dominant pixels"
    );
}

#[test]
fn newly_cascaded_components_inherit_unset_typography_from_the_parent() {
    use rustmotion_components::Component;
    use rustmotion_core::css::style::{Color, CssStyle as CoreCssStyle};
    use rustmotion_core::css::Length;

    let parent = CoreCssStyle {
        color: Some(Color::String("#ff0000".into())),
        font_size: Some(Length::Px(200.0)),
        ..Default::default()
    };

    let cases: &[(&str, serde_json::Value)] = &[
        ("badge", serde_json::json!({"type":"badge","text":"x"})),
        ("callout", serde_json::json!({"type":"callout","text":"x"})),
        (
            "counter",
            serde_json::json!({"type":"counter","from":0.0,"to":1.0}),
        ),
        ("divider", serde_json::json!({"type":"divider"})),
        (
            "icon",
            serde_json::json!({"type":"icon","icon":"lucide:home"}),
        ),
        ("kbd", serde_json::json!({"type":"kbd","key":"A"})),
        (
            "list",
            serde_json::json!({"type":"list","items":[{"text":"x"}]}),
        ),
        (
            "marquee",
            serde_json::json!({"type":"marquee","content":"x"}),
        ),
        (
            "number_wheel",
            serde_json::json!({"type":"number_wheel","value":"1"}),
        ),
        (
            "pill_nav",
            serde_json::json!({"type":"pill_nav","items":["a"]}),
        ),
        (
            "table",
            serde_json::json!({"type":"table","headers":["A"],"rows":[]}),
        ),
        ("tooltip", serde_json::json!({"type":"tooltip","text":"x"})),
    ];

    for (name, json) in cases {
        let component: Component =
            serde_json::from_value(json.clone()).unwrap_or_else(|e| panic!("{name}: {e}"));
        let mut resolved = component.as_styled().style_config().clone();
        rustmotion_core::css::cascade::inherit_from(&parent, &mut resolved);
        let cascaded = component
            .with_cascaded_style(&resolved)
            .unwrap_or_else(|| panic!("{name} must be classified as typographic"));
        let style = cascaded.as_styled().style_config();
        assert_eq!(
            style.color, parent.color,
            "{name} with no color of its own must inherit the parent's"
        );
        assert_eq!(
            style.font_size, parent.font_size,
            "{name} with no font-size of its own must inherit the parent's"
        );
    }
}

#[test]
fn table_own_color_wins_over_cascaded_card_color() {
    use rustmotion_components::Component;
    use rustmotion_core::css::style::{Color, CssStyle as CoreCssStyle};

    let parent = CoreCssStyle {
        color: Some(Color::String("#ff0000".into())),
        ..Default::default()
    };
    let component: Component = serde_json::from_value(serde_json::json!({
        "type": "table",
        "headers": ["A"],
        "rows": [],
        "style": { "color": "#00ff00" }
    }))
    .expect("deserialize table");
    let mut resolved = component.as_styled().style_config().clone();
    rustmotion_core::css::cascade::inherit_from(&parent, &mut resolved);

    let cascaded = component
        .with_cascaded_style(&resolved)
        .expect("table is typographic");
    assert_eq!(
        cascaded.as_styled().style_config().color,
        Some(Color::String("#00ff00".into())),
        "table's own explicit color must win over the cascaded parent's"
    );
}

#[test]
fn divider_color_cascades_from_card_to_painted_line() {
    let scene = paint_card_with_child(serde_json::json!({
        "type": "card",
        "style": {
            "color": "#ff0000",
            "width": 500,
            "height": 200,
            "flex-direction": "column"
        },
        "children": [ { "type": "divider" } ]
    }));

    let red_pixels = count_dominant(&scene.pixels, 0, &[1, 2]);
    assert!(
        red_pixels > 0,
        "divider with no color of its own must paint its line in the card's \
         cascaded red, not the painter's white fallback — found \
         {red_pixels} red-dominant pixels"
    );
}

#[test]
fn table_color_and_font_size_cascade_to_painted_cells() {
    let scene = paint_card_with_child(serde_json::json!({
        "type": "card",
        "style": {
            "color": "#ff0000",
            "font-size": 40,
            "width": 500,
            "flex-direction": "column"
        },
        "children": [
            { "type": "table", "headers": ["ID"], "rows": [["1"]] }
        ]
    }));

    let red_pixels = count_dominant(&scene.pixels, 0, &[1, 2]);
    assert!(
        red_pixels > 20,
        "a table body cell with no color of its own must paint in the \
         card's cascaded red — found {red_pixels} red-dominant pixels"
    );
}
