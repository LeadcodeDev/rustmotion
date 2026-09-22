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

// ─── `trim_end` was honoured only on the extracted audio ───────────────────

/// Frames beyond `trim_end` sit in the cache (simulating a preextraction
/// window, or a direct extraction, wider than the intended trim), so the
/// picture path must never pick one of them once `trim_end` is set: past
/// `trim_end`, playback holds on the last in-window frame. Before the fix,
/// `source_time` had no upper bound at all — querying past `trim_end` on a
/// cache/source that extends further would draw whatever sits further
/// along the source, not the frame at the trim boundary.
#[test]
fn trim_end_clamps_playback_instead_of_running_past_it() {
    let src = unique_src("trimend");
    let cache_key = format!("{src}:20x20");
    let frames = vec![
        (0.0, solid_rgba([255, 0, 0, 255], 2, 2), 2, 2),
        (0.5, solid_rgba([0, 0, 255, 255], 2, 2), 2, 2),
        (1.0, solid_rgba([0, 255, 0, 255], 2, 2), 2, 2),
        (1.5, solid_rgba([128, 0, 128, 255], 2, 2), 2, 2),
        (2.0, solid_rgba([255, 165, 0, 255], 2, 2), 2, 2),
    ];
    video_frame_cache().insert(cache_key, Arc::new(frames));

    let v = video(&src, ImageFit::Fill, Some(0.0), Some(1.0), None);
    let ctx = ctx_at(1.8);
    let pixels = paint_and_read(&v, &ctx, 20, 20, false);

    assert_eq!(
        px(&pixels, 20, 10, 10),
        [0, 255, 0, 255],
        "past trim_end, playback must clamp to the frame at trim_end (green), not the frame \
         nearest the unclamped query time (orange)"
    );
}

// ─── `loop_video` made neither the picture nor the audio loop ─────────────

/// Cache frames only cover `[0.0, 1.0)`; `trim_end: Some(1.0)` gives
/// `loop_video` a window to wrap within without needing a real source file
/// to probe. Querying at `ctx.time = 2.1` (raw source time 2.1s, i.e. "2
/// full loops plus 0.1s") must land near 0.1s once wrapped — nearest to
/// that among `{0.0, 0.25, 0.5, 0.75}` is red. Before this fix, the same
/// query — with the trim-end clamp from the previous test already in place
/// but no loop branch yet — clamped to `min(2.1, 1.0) = 1.0`, whose nearest
/// cached frame is yellow: a clearly different pixel, which is what proves
/// this test is exercising the loop path and not being masked by the clamp.
#[test]
fn loop_video_wraps_playback_within_the_trim_window() {
    let src = unique_src("loop");
    let cache_key = format!("{src}:20x20");
    let frames = vec![
        (0.0, solid_rgba([255, 0, 0, 255], 2, 2), 2, 2),
        (0.25, solid_rgba([0, 255, 0, 255], 2, 2), 2, 2),
        (0.5, solid_rgba([0, 0, 255, 255], 2, 2), 2, 2),
        (0.75, solid_rgba([255, 255, 0, 255], 2, 2), 2, 2),
    ];
    video_frame_cache().insert(cache_key, Arc::new(frames));

    let v = video(&src, ImageFit::Fill, Some(0.0), Some(1.0), Some(true));
    let ctx = ctx_at(2.1);
    let pixels = paint_and_read(&v, &ctx, 20, 20, false);

    assert_eq!(
        px(&pixels, 20, 10, 10),
        [255, 0, 0, 255],
        "looping must wrap the query time back into the window (nearest: red), not clamp to \
         the window's own end (nearest: yellow)"
    );
}

/// Without `loop_video`, a `trim_end`-bounded video must still clamp
/// (unaffected by the loop branch existing) rather than wrap — the same
/// scenario as the wrap test above, minus the flag.
#[test]
fn without_loop_video_playback_still_clamps_not_wraps() {
    let src = unique_src("no-loop");
    let cache_key = format!("{src}:20x20");
    let frames = vec![
        (0.0, solid_rgba([255, 0, 0, 255], 2, 2), 2, 2),
        (0.25, solid_rgba([0, 255, 0, 255], 2, 2), 2, 2),
        (0.5, solid_rgba([0, 0, 255, 255], 2, 2), 2, 2),
        (0.75, solid_rgba([255, 255, 0, 255], 2, 2), 2, 2),
    ];
    video_frame_cache().insert(cache_key, Arc::new(frames));

    let v = video(&src, ImageFit::Fill, Some(0.0), Some(1.0), None);
    let ctx = ctx_at(2.1);
    let pixels = paint_and_read(&v, &ctx, 20, 20, false);

    assert_eq!(
        px(&pixels, 20, 10, 10),
        [255, 255, 0, 255],
        "no loop_video: must clamp to the window's end (nearest: yellow), not wrap"
    );
}

// ─── cached-frame draw path mistagged straight alpha as premultiplied ──────

/// ffmpeg's `-pix_fmt rgba` output — what fills the video-frame cache — is
/// straight (unpremultiplied) alpha. Tagging that buffer `AlphaType::Premul`
/// makes Skia treat the RGB channels as already scaled by alpha instead of
/// scaling them itself, which brightens (here: doubles) every
/// semi-transparent pixel's channels once composited.
///
/// A straight-alpha (200, 100, 50, 128) pixel, composited over black:
/// correctly tagged `Unpremul`, Skia premultiplies it to
/// (200×128/255, 100×128/255, 50×128/255) ≈ (100, 50, 25) before compositing
/// over black, landing there almost exactly (the `(1 - alpha) * 0` background
/// term vanishes either way). Mistagged `Premul`, Skia uses the raw channel
/// values directly as if already scaled — (200, 100, 50) — composited over
/// black with no further scaling, landing at roughly double the correct
/// result.
#[test]
fn cached_frame_straight_alpha_composites_correctly_not_doubled() {
    let src = unique_src("alpha");
    let cache_key = format!("{src}:10x10");
    let straight = [200u8, 100, 50, 128];
    video_frame_cache().insert(
        cache_key,
        Arc::new(vec![(0.0, solid_rgba(straight, 2, 2), 2, 2)]),
    );

    let v = video(&src, ImageFit::Fill, None, None, None);
    let ctx = ctx_at(0.0);
    let pixels = paint_and_read(&v, &ctx, 10, 10, false);
    let composited = px(&pixels, 10, 5, 5);

    let close = |actual: u8, expected: u8| (actual as i16 - expected as i16).abs() <= 4;
    assert!(
        close(composited[0], 100) && close(composited[1], 50) && close(composited[2], 25),
        "straight-alpha (200,100,50,128) over black must composite to roughly (100,50,25), \
         got {composited:?} — a value near (200,100,50) means the buffer is still mistagged \
         as premultiplied and its channels are being used unscaled"
    );
}
