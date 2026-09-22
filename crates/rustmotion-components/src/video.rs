use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, ColorType, ImageInfo, Paint, Rect};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    extract_video_frame, find_closest_frame, probe_video_metadata, video_frame_cache,
};
use rustmotion_core::schema::{ImageFit, TimelineStep};
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

fn default_volume() -> f32 {
    1.0
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Video {
    pub src: String,
    #[serde(default)]
    pub trim_start: Option<f64>,
    #[serde(default)]
    pub trim_end: Option<f64>,
    #[serde(default)]
    pub playback_rate: Option<f64>,
    #[serde(default)]
    pub fit: ImageFit,
    #[serde(default = "default_volume")]
    pub volume: f32,
    #[serde(default)]
    pub loop_video: Option<bool>,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(Video {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

/// The rectangle an `img_w`×`img_h` source draws into to honour `fit` inside
/// a `target_w`×`target_h` box — the same three CSS `object-fit` semantics
/// `image.rs`'s painter already implements for the `image` component.
fn fit_rect(fit: &ImageFit, img_w: f32, img_h: f32, target_w: f32, target_h: f32) -> Rect {
    match fit {
        ImageFit::Fill => Rect::from_xywh(0.0, 0.0, target_w, target_h),
        ImageFit::Contain => {
            let scale = (target_w / img_w).min(target_h / img_h);
            let w = img_w * scale;
            let h = img_h * scale;
            Rect::from_xywh((target_w - w) / 2.0, (target_h - h) / 2.0, w, h)
        }
        ImageFit::Cover => {
            let scale = (target_w / img_w).max(target_h / img_h);
            let w = img_w * scale;
            let h = img_h * scale;
            Rect::from_xywh((target_w - w) / 2.0, (target_h - h) / 2.0, w, h)
        }
    }
}

/// Draws `img` into `layout`'s box according to `fit`, clipping to the box
/// for `Cover` (the only mode whose fitted rectangle can extend past it).
fn draw_fitted(canvas: &Canvas, img: skia_safe::Image, fit: &ImageFit, layout: &BoxLayout) {
    let dst = fit_rect(
        fit,
        img.width() as f32,
        img.height() as f32,
        layout.width,
        layout.height,
    );
    let paint = Paint::default();
    if matches!(fit, ImageFit::Cover) {
        canvas.save();
        canvas.clip_rect(
            Rect::from_xywh(0.0, 0.0, layout.width, layout.height),
            skia_safe::ClipOp::Intersect,
            true,
        );
        canvas.draw_image_rect(img, None, dst, &paint);
        canvas.restore();
    } else {
        canvas.draw_image_rect(img, None, dst, &paint);
    }
}

/// The source clip's own duration, probed via `ffprobe` and memoized per
/// `src` for the life of the process — `effective_source_time` below is
/// called once per painted frame, and re-probing on every one of them would
/// mean one subprocess spawn per frame for any looping video. `None` on a
/// probe failure (no ffprobe on `PATH`, or the source can't be read) is
/// memoized too, so a broken source fails fast on every subsequent frame
/// instead of retrying the same failing probe.
fn video_duration_secs(src: &str) -> Option<f64> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, Option<f64>>>,
    > = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));

    if let Some(hit) = cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(src)
    {
        return *hit;
    }
    let probed = probe_video_metadata(src).ok().map(|p| p.duration_secs);
    cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(src.to_string(), probed);
    probed
}

impl Video {
    /// The timestamp to sample from the source clip for a given scene time.
    /// When `loop_video` is set, playback wraps within the source's own
    /// probed duration instead of running past it and holding on
    /// whatever the last extractable frame happens to be.
    fn effective_source_time(&self, ctx_time: f64) -> f64 {
        let rate = self.playback_rate.unwrap_or(1.0);
        let trim_start = self.trim_start.unwrap_or(0.0);
        let raw = trim_start + ctx_time * rate;

        if self.loop_video == Some(true) {
            if let Some(duration) = video_duration_secs(&self.src) {
                if duration > trim_start {
                    return trim_start + (raw - trim_start).rem_euclid(duration - trim_start);
                }
            }
        }

        raw
    }
}

impl Painter for Video {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        let source_time = self.effective_source_time(ctx.time);
        let width = layout.width as u32;
        let height = layout.height as u32;

        let cache_key = format!("{}:{}x{}", self.src, width, height);
        let cache = video_frame_cache();

        if let Some(cached_frames) = cache.get(&cache_key) {
            if let Some((rgba, fw, fh)) = find_closest_frame(&cached_frames, source_time) {
                let img_info = ImageInfo::new(
                    (fw as i32, fh as i32),
                    ColorType::RGBA8888,
                    skia_safe::AlphaType::Premul,
                    None,
                );
                let row_bytes = fw as usize * 4;
                let data = skia_safe::Data::new_copy(rgba);
                if let Some(img) = skia_safe::images::raster_from_data(&img_info, data, row_bytes) {
                    draw_fitted(canvas, img, &self.fit, layout);
                }
                return;
            }
        }

        let frame_data = match extract_video_frame(&self.src, source_time, width, height) {
            Ok(data) => data,
            Err(e) => {
                // Item 3 (issue #167): decoding failures (ffmpeg missing, or
                // this specific frame failing) used to be a silent `return`
                // — a video component would render entirely blank with no
                // trace anywhere. `paint_content` runs once per frame, so
                // the warning is deduplicated per `src` via `warn_once_for`
                // (the same guard `lib.rs` already uses for exactly this
                // per-frame-call-site problem) instead of printing the same
                // line a thousand times over a render.
                if crate::warn_once_for(&format!("video-frame:{}", self.src)) {
                    eprintln!(
                        "Warning: video '{}' could not be decoded: {e}. This component will \
                         render nothing for the remainder of the video.",
                        self.src
                    );
                }
                return;
            }
        };
        let skia_data = skia_safe::Data::new_copy(&frame_data);
        if let Some(img) = skia_safe::Image::from_encoded(skia_data) {
            draw_fitted(canvas, img, &self.fit, layout);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmotion_core::engine::animator::AnimatedProperties;
    use rustmotion_core::engine::layout_pass::BoxLayout;
    use rustmotion_core::traits::PaintCtx;

    fn base_ctx() -> PaintCtx {
        PaintCtx {
            time: 0.0,
            scenario_time: 0.0,
            scene_duration: 1.0,
            frame_index: 0,
            fps: 30,
            video_width: 100,
            video_height: 100,
            stagger_offset: 0.0,
        }
    }

    /// A failed frame extraction (bad src, or no ffmpeg) must be reported,
    /// not swallowed. Pre-fix, `paint_content` never calls `warn_once_for`
    /// on this path at all, so the slot for this exact src stays unclaimed
    /// ("first sighting" == true) forever — this is the observable half of
    /// total silence we can assert on without capturing stderr.
    #[test]
    fn a_failed_frame_extraction_must_claim_its_warn_once_slot() {
        let missing_src = std::env::temp_dir().join(format!(
            "rustmotion-video-test-missing-{}-{}.mp4",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let _ = std::fs::remove_file(&missing_src);
        let src_str = missing_src.to_str().unwrap().to_string();

        let video = Video {
            src: src_str.clone(),
            trim_start: None,
            trim_end: None,
            playback_rate: None,
            fit: Default::default(),
            volume: 1.0,
            loop_video: None,
            timing: Default::default(),
            style: CssStyle::default(),
            timeline: Vec::new(),
            stagger: None,
        };
        let layout = BoxLayout {
            width: 40.0,
            height: 40.0,
            ..Default::default()
        };
        let ctx = base_ctx();
        let props = AnimatedProperties::default();
        let mut surface = skia_safe::surfaces::raster_n32_premul((40, 40)).unwrap();
        {
            let canvas = surface.canvas();
            video.paint_content(canvas, &layout, &props, &ctx);
        }

        let key = format!("video-frame:{}", src_str);
        assert!(
            !crate::warn_once_for(&key),
            "paint_content must have claimed this warning slot on the failed extraction \
             path — it is still unclaimed (first sighting), meaning nothing warned about \
             the failure"
        );
    }
}
