use rustmotion_components::box_builder::{build_scene_with_anim, BuildAnimationCtx};
use rustmotion_components::legacy_dispatch::LegacyPaintDispatcher;
use rustmotion_components::{ChildComponent, Component, PositionMode};
use rustmotion_core::css::taffy_bridge::ConversionContext;
use rustmotion_core::engine::layout_pass::run_layout;
use rustmotion_core::engine::paint_pass::{paint_tree, PaintFrame};

const W: u32 = 400;
const H: u32 = 300;

fn render_caption(json: serde_json::Value, time: f64) -> Vec<u8> {
    render_caption_at(json, time, 150.0)
}

fn render_caption_at(json: serde_json::Value, time: f64, y: f32) -> Vec<u8> {
    let component: Component = serde_json::from_value(json).expect("deserialize caption");
    let child = ChildComponent {
        id: None,
        component,
        position: Some(PositionMode::Absolute { x: 0.0, y }),
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
        light: Default::default(),
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

fn count_pixels(buf: &[u8], pred: impl Fn(&[u8]) -> bool) -> usize {
    buf.as_chunks::<4>()
        .0
        .iter()
        .filter(|p| pred(p.as_slice()))
        .count()
}

fn lit_pixels(buf: &[u8]) -> usize {
    count_pixels(buf, |p| p[3] > 0)
}

fn magenta_pixels(buf: &[u8]) -> usize {
    count_pixels(buf, |p| {
        p[0] > 180 && p[2] > 180 && p[1] < 100 && p[3] > 200
    })
}

fn yellow_pixels(buf: &[u8]) -> usize {
    count_pixels(buf, |p| p[0] > 180 && p[1] > 180 && p[2] < 100)
}

fn white_pixels(buf: &[u8]) -> usize {
    count_pixels(buf, |p| p[0] > 200 && p[1] > 200 && p[2] > 200)
}

fn word_pop_caption() -> serde_json::Value {
    serde_json::json!({
        "type": "caption",
        "mode": "word_pop",
        "active_color": "#FFFF00",
        "pill_color": "#FF00FF",
        "words": [
            { "text": "hello", "start": 0.0, "end": 0.5 },
            { "text": "world", "start": 0.5, "end": 1.0 }
        ],
        "style": { "width": "400px", "font-size": 48 }
    })
}

#[test]
fn word_pop_shows_active_word_with_pill() {
    let buf = render_caption(word_pop_caption(), 0.45);
    let pill = magenta_pixels(&buf);
    let word = yellow_pixels(&buf);
    assert!(pill > 500, "pill not visible: {pill} magenta pixels");
    assert!(word > 50, "active word not visible: {word} yellow pixels");
}

#[test]
fn word_pop_hides_inactive_words() {
    let buf = render_caption(word_pop_caption(), 1.5);
    let lit = lit_pixels(&buf);
    assert!(lit < 50, "expected empty canvas, got {lit} lit pixels");
}

#[test]
fn word_pop_scales_in() {
    let early = lit_pixels(&render_caption(word_pop_caption(), 0.02));
    let settled = lit_pixels(&render_caption(word_pop_caption(), 0.45));
    assert!(
        settled > early * 2,
        "scale-in did not grow the word (early={early}, settled={settled})"
    );
}

#[test]
fn word_pop_default_pill_is_translucent_black() {
    let mut json = word_pop_caption();
    json.as_object_mut().unwrap().remove("pill_color");
    let buf = render_caption(json, 0.45);
    let dark_translucent = count_pixels(&buf, |p| {
        p[3] > 120 && p[3] < 220 && p[0] < 40 && p[1] < 40 && p[2] < 40
    });
    assert!(
        dark_translucent > 300,
        "default pill not visible: {dark_translucent} dark translucent pixels"
    );
}

#[test]
fn karaoke_pop_highlights_active_word_with_pill() {
    let json = serde_json::json!({
        "type": "caption",
        "mode": "karaoke_pop",
        "active_color": "#FFFF00",
        "pill_color": "#FF00FF",
        "max_width": 380.0,
        "words": [
            { "text": "hello", "start": 0.0, "end": 0.5 },
            { "text": "brave", "start": 0.5, "end": 1.0 },
            { "text": "world", "start": 1.0, "end": 1.5 }
        ],
        "style": { "width": "400px", "font-size": 40, "color": "#FFFFFF" }
    });
    let buf = render_caption(json, 0.75);
    let pill = magenta_pixels(&buf);
    let active = yellow_pixels(&buf);
    let inactive = white_pixels(&buf);
    assert!(pill > 300, "pill not visible: {pill} magenta pixels");
    assert!(
        active > 50,
        "active word not visible: {active} yellow pixels"
    );
    assert!(
        inactive > 100,
        "inactive words not visible: {inactive} white pixels"
    );
}

#[test]
fn wraps_within_its_layout_box_when_max_width_is_unset() {
    let json = serde_json::json!({
        "type": "caption",
        "mode": "highlight",
        "words": [
            { "text": "the", "start": 0.0, "end": 100.0 },
            { "text": "quick", "start": 0.0, "end": 100.0 },
            { "text": "brown", "start": 0.0, "end": 100.0 },
            { "text": "fox", "start": 0.0, "end": 100.0 },
            { "text": "jumps", "start": 0.0, "end": 100.0 },
            { "text": "over", "start": 0.0, "end": 100.0 },
            { "text": "the", "start": 0.0, "end": 100.0 },
            { "text": "lazy", "start": 0.0, "end": 100.0 },
            { "text": "dog", "start": 0.0, "end": 100.0 }
        ],
        "style": { "width": "150px", "font-size": 24, "color": "#FFFFFF" }
    });
    let buf = render_caption_at(json, 0.5, 20.0);
    let (minx, maxx, miny, maxy) = ink_bounds(&buf).expect("caption must paint something");

    assert!(
        maxx - minx < 200,
        "must wrap to roughly the 150px box width, got ink width {}",
        maxx - minx
    );
    assert!(
        maxy - miny > 60,
        "must spread across multiple lines (9 words don't fit 150px on one line at 30px \
         font-size), got ink height {}",
        maxy - miny
    );
}

fn ink_bounds(buf: &[u8]) -> Option<(i32, i32, i32, i32)> {
    let (mut minx, mut maxx, mut miny, mut maxy) = (i32::MAX, i32::MIN, i32::MAX, i32::MIN);
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            let idx = ((y * W as i32 + x) * 4 + 3) as usize;
            if buf[idx] > 0 {
                minx = minx.min(x);
                maxx = maxx.max(x);
                miny = miny.min(y);
                maxy = maxy.max(y);
            }
        }
    }
    (minx <= maxx).then_some((minx, maxx, miny, maxy))
}
