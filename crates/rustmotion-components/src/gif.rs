use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, ColorType, ImageInfo, Paint, Rect};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::gif_cache;
use rustmotion_core::schema::{ImageFit, TimelineStep};
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

fn default_loop_true() -> bool {
    true
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Gif {
    pub src: String,
    #[serde(default)]
    pub fit: ImageFit,
    #[serde(default = "default_loop_true")]
    pub loop_gif: bool,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(Gif {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

fn frame_rect(canvas_w: u32, canvas_h: u32, frame: &gif::Frame<'_>) -> (u32, u32, u32, u32) {
    let left = frame.left.min(canvas_w as u16) as u32;
    let top = frame.top.min(canvas_h as u16) as u32;
    let w = (frame.width as u32).min(canvas_w.saturating_sub(left));
    let h = (frame.height as u32).min(canvas_h.saturating_sub(top));
    (left, top, w, h)
}

fn blit_frame(composed: &mut [u8], canvas_w: u32, canvas_h: u32, frame: &gif::Frame<'_>) {
    let (left, top, w, h) = frame_rect(canvas_w, canvas_h, frame);
    for y in 0..h {
        for x in 0..w {
            let src = ((y * frame.width as u32 + x) * 4) as usize;
            let Some(px) = frame.buffer.get(src..src + 4) else {
                return;
            };
            if px[3] == 0 {
                continue;
            }
            let dst = (((top + y) * canvas_w + (left + x)) * 4) as usize;
            composed[dst..dst + 4].copy_from_slice(px);
        }
    }
}

fn clear_rect(composed: &mut [u8], canvas_w: u32, canvas_h: u32, frame: &gif::Frame<'_>) {
    let (left, top, w, h) = frame_rect(canvas_w, canvas_h, frame);
    for y in 0..h {
        let row = (((top + y) * canvas_w + left) * 4) as usize;
        composed[row..row + (w as usize * 4)].fill(0);
    }
}

type DecodedGif = (Vec<(Vec<u8>, u32, u32)>, Vec<f64>, f64);

#[cfg(test)]
static DECODE_STALL_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[cfg(test)]
fn stall_decode_for_tests() {
    let stall_ms = DECODE_STALL_MS.load(std::sync::atomic::Ordering::SeqCst);
    if stall_ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(stall_ms));
    }
}

const MAX_GIF_CANVAS_BYTES: u64 = 128 * 1024 * 1024;

const MAX_GIF_FRAMES: usize = 600;

fn decode_composed_frames(src: &str, max_canvas_w: u32, max_canvas_h: u32) -> Option<DecodedGif> {
    let file = match std::fs::File::open(src) {
        Ok(f) => f,
        Err(e) => {
            if crate::warn_once_for(&format!("gif-open:{src}")) {
                eprintln!("rustmotion: gif '{src}' could not be opened: {e}");
            }
            return None;
        }
    };

    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut decoder = match options.read_info(file) {
        Ok(d) => d,
        Err(e) => {
            if crate::warn_once_for(&format!("gif-decode:{src}")) {
                eprintln!("rustmotion: gif '{src}' could not be decoded: {e}");
            }
            return None;
        }
    };

    let canvas_w = decoder.width() as u32;
    let canvas_h = decoder.height() as u32;

    let canvas_bytes = canvas_w as u64 * canvas_h as u64 * 4;
    if canvas_bytes > MAX_GIF_CANVAS_BYTES || canvas_w > max_canvas_w || canvas_h > max_canvas_h {
        if crate::warn_once_for(&format!("gif-oversized:{src}")) {
            eprintln!(
                "rustmotion: gif '{src}' declares a {canvas_w}x{canvas_h} canvas ({canvas_bytes} \
                 bytes/frame), over the {MAX_GIF_CANVAS_BYTES}-byte budget or larger than this \
                 render's own {max_canvas_w}x{max_canvas_h} video — refusing to decode it."
            );
        }
        return None;
    }

    #[cfg(test)]
    stall_decode_for_tests();

    let mut frames: Vec<(Vec<u8>, u32, u32)> = Vec::new();
    let mut cumulative_times: Vec<f64> = Vec::new();
    let mut accumulated = 0.0;
    let mut composed = vec![0u8; canvas_w as usize * canvas_h as usize * 4];

    while let Ok(Some(frame)) = decoder.read_next_frame() {
        if frames.len() >= MAX_GIF_FRAMES {
            if crate::warn_once_for(&format!("gif-frame-cap:{src}")) {
                eprintln!(
                    "rustmotion: gif '{src}' has more than {MAX_GIF_FRAMES} frames — truncating \
                     the decoded animation at the cap."
                );
            }
            break;
        }

        let restore = (frame.dispose == gif::DisposalMethod::Previous).then(|| composed.clone());

        blit_frame(&mut composed, canvas_w, canvas_h, frame);

        let delay = frame.delay as f64 / 100.0;
        let delay = if delay < 0.01 { 0.1 } else { delay };
        accumulated += delay;
        frames.push((composed.clone(), canvas_w, canvas_h));
        cumulative_times.push(accumulated);

        match frame.dispose {
            gif::DisposalMethod::Background => clear_rect(&mut composed, canvas_w, canvas_h, frame),
            gif::DisposalMethod::Previous => {
                if let Some(prev) = restore {
                    composed = prev;
                }
            }
            _ => {}
        }
    }

    if frames.is_empty() {
        if crate::warn_once_for(&format!("gif-empty:{src}")) {
            eprintln!("rustmotion: gif '{src}' decoded to zero frames");
        }
        return None;
    }

    Some((frames, cumulative_times, accumulated))
}

fn cached_decode(src: &str, max_canvas_w: u32, max_canvas_h: u32) -> Option<Arc<DecodedGif>> {
    gif_cache()
        .entry(src.to_string())
        .or_try_insert_with(|| {
            decode_composed_frames(src, max_canvas_w, max_canvas_h)
                .map(Arc::new)
                .ok_or(())
        })
        .ok()
        .map(|entry| entry.clone())
}

impl Painter for Gif {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        let Some(cached) = cached_decode(&self.src, ctx.video_width, ctx.video_height) else {
            return;
        };

        let (ref frames, ref cumulative_times, total_duration) = *cached;

        if frames.is_empty() {
            return;
        }

        let effective_time = if self.loop_gif {
            ctx.time % total_duration
        } else {
            ctx.time.min(total_duration)
        };

        let frame_idx = cumulative_times
            .partition_point(|&t| t <= effective_time)
            .min(frames.len() - 1);
        let (ref frame_data, gif_width, gif_height) = frames[frame_idx];

        let img_info = ImageInfo::new(
            (gif_width as i32, gif_height as i32),
            ColorType::RGBA8888,
            skia_safe::AlphaType::Unpremul,
            None,
        );
        let row_bytes = gif_width as usize * 4;
        let data = skia_safe::Data::new_copy(frame_data);
        if let Some(img) = skia_safe::images::raster_from_data(&img_info, data, row_bytes) {
            let dst = Rect::from_xywh(0.0, 0.0, layout.width, layout.height);
            let paint = Paint::default();
            canvas.draw_image_rect(img, None, dst, &paint);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_two_frame_gif(path: &std::path::Path) {
        let palette: &[u8] = &[0xFF, 0x00, 0x00, 0x00, 0x00, 0xFF];
        let mut file = std::fs::File::create(path).expect("create gif fixture");
        let mut encoder = gif::Encoder::new(&mut file, 4, 2, palette).expect("gif encoder");

        let mut full = gif::Frame::from_indexed_pixels(4, 2, vec![0; 8], None);
        full.delay = 10;
        encoder.write_frame(&full).expect("write frame 0");

        let mut patch = gif::Frame::from_indexed_pixels(2, 2, vec![1; 4], None);
        patch.left = 2;
        patch.top = 0;
        patch.delay = 10;
        patch.dispose = gif::DisposalMethod::Keep;
        encoder.write_frame(&patch).expect("write frame 1");
    }

    fn px(frame: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * w + x) * 4) as usize;
        frame[i..i + 4].try_into().expect("pixel")
    }

    #[test]
    fn a_subrectangle_frame_is_composed_onto_the_previous_one() {
        let path = std::env::temp_dir().join(format!(
            "rustmotion_gif_{}.gif",
            std::process::id() as u64 * 31 + 7
        ));
        write_two_frame_gif(&path);

        let (frames, times, total) =
            decode_composed_frames(path.to_str().expect("utf-8 path"), 1920, 1080)
                .expect("gif must decode");
        std::fs::remove_file(&path).ok();

        assert_eq!(frames.len(), 2, "both frames must be drawable");
        for (buf, w, h) in &frames {
            assert_eq!(
                buf.len(),
                (*w as usize) * (*h as usize) * 4,
                "every stored frame must be full-canvas, or raster_from_data \
                 silently returns None and nothing is painted"
            );
        }
        assert_ne!(frames[0].0, frames[1].0, "the two frames must differ");

        assert_eq!(px(&frames[1].0, 4, 0, 0), [0xFF, 0x00, 0x00, 0xFF], "kept");
        assert_eq!(
            px(&frames[1].0, 4, 3, 0),
            [0x00, 0x00, 0xFF, 0xFF],
            "patched"
        );

        assert_eq!(times.len(), 2);
        assert!((total - 0.2).abs() < 1e-9, "0.1s per frame, got {total}");
    }

    #[test]
    fn a_missing_file_reports_instead_of_returning_nothing() {
        let missing = std::env::temp_dir().join("rustmotion_gif_absent_xyz.gif");
        assert!(decode_composed_frames(missing.to_str().expect("utf-8"), 1920, 1080).is_none());
        assert!(
            !crate::warn_once_for(&format!("gif-open:{}", missing.to_str().expect("utf-8"))),
            "the open failure must have reported once"
        );
    }

    #[test]
    fn a_frame_rect_running_past_the_canvas_is_clamped() {
        let mut frame = gif::Frame::from_indexed_pixels(4, 4, vec![0; 16], None);
        frame.left = 3;
        frame.top = 3;
        assert_eq!(frame_rect(4, 4, &frame), (3, 3, 1, 1));
    }

    struct StallGuard;

    impl Drop for StallGuard {
        fn drop(&mut self) {
            DECODE_STALL_MS.store(0, std::sync::atomic::Ordering::SeqCst);
        }
    }

    #[test]
    fn concurrent_paints_of_the_same_uncached_gif_decode_exactly_once() {
        let _guard = StallGuard;
        DECODE_STALL_MS.store(120, std::sync::atomic::Ordering::SeqCst);

        let path = std::env::temp_dir().join(format!(
            "rustmotion_gif_stampede_{}_{}.gif",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        write_two_frame_gif(&path);
        let src = path.to_str().expect("utf-8 path").to_string();

        const THREADS: usize = 8;
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(THREADS));
        let handles: Vec<_> = (0..THREADS)
            .map(|_| {
                let barrier = barrier.clone();
                let src = src.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    cached_decode(&src, 4, 2)
                })
            })
            .collect();

        let results: Vec<Option<Arc<DecodedGif>>> =
            handles.into_iter().map(|h| h.join().unwrap()).collect();
        std::fs::remove_file(&path).ok();

        let first = results[0].as_ref().expect("gif must decode");
        for (i, result) in results.iter().enumerate() {
            let result = result.as_ref().unwrap_or_else(|| {
                panic!("thread {i} did not get a decoded result");
            });
            assert!(
                Arc::ptr_eq(first, result),
                "thread {i} observed a different Arc than thread 0 — the GIF was decoded \
                 more than once for the same cache-cold source"
            );
        }
    }

    fn write_oversized_header_gif(path: &std::path::Path, w: u16, h: u16) {
        let palette: &[u8] = &[0, 0, 0, 255, 255, 255];
        let mut file = std::fs::File::create(path).expect("create gif fixture");
        let mut encoder = gif::Encoder::new(&mut file, w, h, palette).expect("gif encoder");
        let frame = gif::Frame::from_indexed_pixels(1, 1, vec![0], None);
        encoder.write_frame(&frame).expect("write frame");
    }

    #[test]
    fn a_canvas_over_the_byte_budget_is_rejected_without_allocating_it() {
        let path = std::env::temp_dir().join(format!(
            "rustmotion_gif_bomb_budget_{}_{}.gif",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        write_oversized_header_gif(&path, 6000, 6000);

        let result = decode_composed_frames(path.to_str().expect("utf-8"), 8192, 8192);
        std::fs::remove_file(&path).ok();

        assert!(
            result.is_none(),
            "a 144 MiB canvas must be refused, not decoded"
        );
        assert!(
            !crate::warn_once_for(&format!("gif-oversized:{}", path.to_str().unwrap())),
            "the rejection must have reported once"
        );
    }

    #[test]
    fn a_canvas_larger_than_the_video_is_rejected_even_under_the_byte_budget() {
        let path = std::env::temp_dir().join(format!(
            "rustmotion_gif_bomb_dims_{}_{}.gif",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        write_oversized_header_gif(&path, 3000, 2000);

        let result = decode_composed_frames(path.to_str().expect("utf-8"), 1920, 1080);
        std::fs::remove_file(&path).ok();

        assert!(
            result.is_none(),
            "a canvas bigger than the video's own dimensions must be refused"
        );
    }

    fn write_many_frame_gif(path: &std::path::Path, count: u32) {
        let palette: &[u8] = &[0, 0, 0, 255, 255, 255];
        let mut file = std::fs::File::create(path).expect("create gif fixture");
        let mut encoder = gif::Encoder::new(&mut file, 2, 2, palette).expect("gif encoder");
        for _ in 0..count {
            let mut frame = gif::Frame::from_indexed_pixels(2, 2, vec![0, 1, 1, 0], None);
            frame.delay = 1;
            encoder.write_frame(&frame).expect("write frame");
        }
    }

    #[test]
    fn frame_count_beyond_the_cap_is_truncated_not_unbounded() {
        let path = std::env::temp_dir().join(format!(
            "rustmotion_gif_many_frames_{}_{}.gif",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        write_many_frame_gif(&path, MAX_GIF_FRAMES as u32 + 50);

        let (frames, times, _total) = decode_composed_frames(path.to_str().expect("utf-8"), 10, 10)
            .expect("a small, merely-long gif must still decode");
        std::fs::remove_file(&path).ok();

        assert_eq!(
            frames.len(),
            MAX_GIF_FRAMES,
            "frame count must be truncated to the cap, not grow past it"
        );
        assert_eq!(times.len(), MAX_GIF_FRAMES);
    }
}
