use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, ColorType, ImageInfo, Paint, Rect};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    extract_video_frame, find_closest_frame, video_frame_cache,
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

impl Painter for Video {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        let rate = self.playback_rate.unwrap_or(1.0);
        let trim_start = self.trim_start.unwrap_or(0.0);
        let source_time = trim_start + ctx.time * rate;
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
                    let dst = Rect::from_xywh(0.0, 0.0, layout.width, layout.height);
                    let paint = Paint::default();
                    canvas.draw_image_rect(img, None, dst, &paint);
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
            let dst = Rect::from_xywh(0.0, 0.0, layout.width, layout.height);
            let paint = Paint::default();
            canvas.draw_image_rect(img, None, dst, &paint);
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
