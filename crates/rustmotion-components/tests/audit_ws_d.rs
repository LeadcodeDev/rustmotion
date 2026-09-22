//! Regression tests for the `video` component's dead-field fixes: `fit`,
//! `trim_end`, `loop_video`, and the straight-vs-premultiplied alpha bug on
//! its cached-frame draw path.
//!
//! Every case populates `video_frame_cache()` directly with hand-built RGBA
//! frames rather than shelling out to a real ffmpeg decode: the field this
//! module exercises (`Video::paint_content`) is one call away from the
//! cache, and driving it that way keeps these tests hermetic and fast while
//! still going through the real, public `Painter` implementation — no
//! private items from `rustmotion-components` are touched.

use std::sync::Arc;

use rustmotion_components::Video;
use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::video_frame_cache;
use rustmotion_core::schema::ImageFit;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

fn unique_src(label: &str) -> String {
    format!(
        "audit-ws-d-video-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before UNIX epoch")
            .as_nanos()
    )
}

#[allow(clippy::too_many_arguments)]
fn video(
    src: &str,
    fit: ImageFit,
    trim_start: Option<f64>,
    trim_end: Option<f64>,
    loop_video: Option<bool>,
) -> Video {
    Video {
        src: src.to_string(),
        trim_start,
        trim_end,
        playback_rate: None,
        fit,
        volume: 1.0,
        loop_video,
        timing: TimingConfig::default(),
        style: CssStyle::default(),
        timeline: Vec::new(),
        stagger: None,
    }
}

fn ctx_at(time: f64) -> PaintCtx {
    PaintCtx {
        time,
        scenario_time: time,
        scene_duration: 10.0,
        frame_index: 0,
        fps: 30,
        video_width: 1920,
        video_height: 1080,
        stagger_offset: 0.0,
    }
}

fn solid_rgba(color: [u8; 4], w: u32, h: u32) -> Vec<u8> {
    let mut buf = Vec::with_capacity((w * h * 4) as usize);
    for _ in 0..(w * h) {
        buf.extend_from_slice(&color);
    }
    buf
}

/// Paints `video` into a fresh `w`×`h` surface (background transparent if
/// `transparent_bg`, opaque black otherwise) and reads the composited pixels
/// back as straight (unpremultiplied) RGBA.
fn paint_and_read(video: &Video, ctx: &PaintCtx, w: i32, h: i32, transparent_bg: bool) -> Vec<u8> {
    let mut surface = skia_safe::surfaces::raster_n32_premul((w, h)).expect("raster surface");
    let bg = if transparent_bg {
        skia_safe::Color4f::new(0.0, 0.0, 0.0, 0.0)
    } else {
        skia_safe::Color4f::new(0.0, 0.0, 0.0, 1.0)
    };
    surface.canvas().clear(bg);

    let layout = BoxLayout {
        width: w as f32,
        height: h as f32,
        ..Default::default()
    };
    let props = AnimatedProperties::default();
    video.paint_content(surface.canvas(), &layout, &props, ctx);

    let mut pixels = vec![0u8; (w * h * 4) as usize];
    let info = skia_safe::ImageInfo::new(
        (w, h),
        skia_safe::ColorType::RGBA8888,
        skia_safe::AlphaType::Unpremul,
        None,
    );
    surface.read_pixels(&info, &mut pixels, (w * 4) as usize, (0, 0));
    pixels
}

fn px(buf: &[u8], w: i32, x: i32, y: i32) -> [u8; 4] {
    let i = ((y * w + x) * 4) as usize;
    buf[i..i + 4].try_into().expect("pixel in bounds")
}

// ─── `fit` was declared, documented, and never read ────────────────────────

/// A 10×20 source into a 40×40 box under `contain` must letterbox — scale
/// is `min(40/10, 40/40) = 1`, so the drawn region is 10 wide, centred with
/// a 15px empty margin on each side. Before the fix, the painter always
/// stretched to the full box regardless of `fit`, so every pixel — margins
/// included — came out opaque.
#[test]
fn contain_fit_letterboxes_instead_of_stretching() {
    let src = unique_src("fit-contain");
    let (fw, fh) = (10u32, 20u32);
    let cache_key = format!("{src}:40x40");
    video_frame_cache().insert(
        cache_key,
        Arc::new(vec![(
            0.0,
            solid_rgba([255, 255, 255, 255], fw, fh),
            fw,
            fh,
        )]),
    );

    let v = video(&src, ImageFit::Contain, None, None, None);
    let ctx = ctx_at(0.0);
    let pixels = paint_and_read(&v, &ctx, 40, 40, true);

    assert_eq!(
        px(&pixels, 40, 0, 20)[3],
        0,
        "letterboxed left margin must stay empty, not be stretched into"
    );
    assert_eq!(
        px(&pixels, 40, 39, 20)[3],
        0,
        "letterboxed right margin must stay empty, not be stretched into"
    );
    assert_eq!(
        px(&pixels, 40, 20, 20),
        [255, 255, 255, 255],
        "the drawn column itself must still be opaque"
    );
}

/// `fill` (the CSS default `object-fit: fill` behaviour) must still stretch
/// to cover the whole box exactly as before — the fix must not regress the
/// one mode that already matched the pre-fix behaviour.
#[test]
fn fill_fit_still_stretches_to_the_whole_box() {
    let src = unique_src("fit-fill");
    let (fw, fh) = (10u32, 20u32);
    let cache_key = format!("{src}:40x40");
    video_frame_cache().insert(
        cache_key,
        Arc::new(vec![(
            0.0,
            solid_rgba([255, 255, 255, 255], fw, fh),
            fw,
            fh,
        )]),
    );

    let v = video(&src, ImageFit::Fill, None, None, None);
    let ctx = ctx_at(0.0);
    let pixels = paint_and_read(&v, &ctx, 40, 40, true);

    assert_eq!(
        px(&pixels, 40, 0, 0)[3],
        255,
        "fill must cover every corner"
    );
    assert_eq!(
        px(&pixels, 40, 39, 39)[3],
        255,
        "fill must cover every corner"
    );
}
