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

/// The frame's rectangle, clamped to the logical canvas.
///
/// A malformed GIF can declare a sub-rectangle that runs past the canvas;
/// clamping keeps the blit in bounds instead of panicking on the slice.
fn frame_rect(canvas_w: u32, canvas_h: u32, frame: &gif::Frame<'_>) -> (u32, u32, u32, u32) {
    let left = frame.left.min(canvas_w as u16) as u32;
    let top = frame.top.min(canvas_h as u16) as u32;
    let w = (frame.width as u32).min(canvas_w.saturating_sub(left));
    let h = (frame.height as u32).min(canvas_h.saturating_sub(top));
    (left, top, w, h)
}

/// Composite one decoded frame onto the persistent canvas.
///
/// GIF transparency is an index, not a channel: the decoder emits alpha 0 for
/// transparent pixels, and those must leave what is underneath alone — that is
/// the whole point of the sub-rectangle encoding.
fn blit_frame(composed: &mut [u8], canvas_w: u32, canvas_h: u32, frame: &gif::Frame<'_>) {
    let (left, top, w, h) = frame_rect(canvas_w, canvas_h, frame);
    for y in 0..h {
        for x in 0..w {
            let src = ((y * frame.width as u32 + x) * 4) as usize;
            let Some(px) = frame.buffer.get(src..src + 4) else {
                return; // truncated buffer: keep what we have rather than panic
            };
            if px[3] == 0 {
                continue;
            }
            let dst = (((top + y) * canvas_w + (left + x)) * 4) as usize;
            composed[dst..dst + 4].copy_from_slice(px);
        }
    }
}

/// `DisposalMethod::Background`: clear this frame's rectangle before the next.
fn clear_rect(composed: &mut [u8], canvas_w: u32, canvas_h: u32, frame: &gif::Frame<'_>) {
    let (left, top, w, h) = frame_rect(canvas_w, canvas_h, frame);
    for y in 0..h {
        let row = (((top + y) * canvas_w + left) * 4) as usize;
        composed[row..row + (w as usize * 4)].fill(0);
    }
}

/// One decoded GIF: full-canvas RGBA frames with their dimensions, the
/// cumulative end time of each, and the total duration. Mirrors what
/// `gif_cache` stores.
type DecodedGif = (Vec<(Vec<u8>, u32, u32)>, Vec<f64>, f64);

/// Decode a GIF into full-canvas RGBA frames, their cumulative end times, and
/// the total duration.
///
/// Every frame after the first is usually a *sub-rectangle* holding only the
/// pixels that changed, so frames must be composed onto a persistent canvas
/// rather than used as images in their own right. Storing `frame.buffer` with
/// the canvas dimensions produced a buffer shorter than `width * height * 4`;
/// `raster_from_data` then returned `None` and the paint was skipped — which is
/// why only the first frame ever appeared (issue #185).
///
/// `None` means nothing can be drawn, and the reason has already been reported.
fn decode_composed_frames(src: &str) -> Option<DecodedGif> {
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

    let mut frames: Vec<(Vec<u8>, u32, u32)> = Vec::new();
    let mut cumulative_times: Vec<f64> = Vec::new();
    let mut accumulated = 0.0;
    let mut composed = vec![0u8; canvas_w as usize * canvas_h as usize * 4];

    while let Ok(Some(frame)) = decoder.read_next_frame() {
        // `Previous` disposal restores what was there before this frame, so it
        // has to be captured before compositing.
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
            // `Any` and `Keep` both leave the canvas as it stands.
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

impl Painter for Gif {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        let gcache = gif_cache();

        let cached = if let Some(cached) = gcache.get(&self.src) {
            cached.clone()
        } else {
            let Some(decoded) = decode_composed_frames(&self.src) else {
                return;
            };
            let cached = Arc::new(decoded);
            gcache.insert(self.src.clone(), cached.clone());
            cached
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

    /// Write a 4x2 GIF: frame 0 fills the canvas red, frame 1 paints a 2x2 blue
    /// sub-rectangle at (2,0) and keeps what is under it — the shape every
    /// optimising encoder produces, and the one that used to render nothing.
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

    /// The regression: an optimised GIF must yield as many drawable frames as
    /// it has, all full-canvas, and they must differ.
    #[test]
    fn a_subrectangle_frame_is_composed_onto_the_previous_one() {
        let path = std::env::temp_dir().join(format!(
            "rustmotion_gif_{}.gif",
            std::process::id() as u64 * 31 + 7
        ));
        write_two_frame_gif(&path);

        let (frames, times, total) =
            decode_composed_frames(path.to_str().expect("utf-8 path")).expect("gif must decode");
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

        // Frame 1 patches the right half and keeps the left: the whole point of
        // composing rather than drawing the sub-rectangle alone.
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
        assert!(decode_composed_frames(missing.to_str().expect("utf-8")).is_none());
        // The warn-once slot must have been claimed — silence is the bug.
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
}
