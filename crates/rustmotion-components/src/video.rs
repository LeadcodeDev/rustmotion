use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, ColorType, ImageInfo, Paint, Rect};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{extract_video_frame, find_closest_frame, video_frame_cache};
use rustmotion_core::schema::{ImageFit, TimelineStep};
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

fn default_volume() -> f32 { 1.0 }

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

        let Ok(frame_data) = extract_video_frame(&self.src, source_time, width, height) else {
            return;
        };
        let skia_data = skia_safe::Data::new_copy(&frame_data);
        if let Some(img) = skia_safe::Image::from_encoded(skia_data) {
            let dst = Rect::from_xywh(0.0, 0.0, layout.width, layout.height);
            let paint = Paint::default();
            canvas.draw_image_rect(img, None, dst, &paint);
        }
    }
}
