use std::sync::Arc;

use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, ColorType, ImageInfo, Paint, Rect};

use crate::engine::renderer::gif_cache;
use crate::layout::{Constraints, LayoutNode};
use crate::schema::{ImageFit, Size};
use crate::traits::{AnimationConfig, RenderContext, StyleConfig, TimingConfig, Widget};

fn default_loop_true() -> bool { true }

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Gif {
    pub src: String,
    #[serde(default)]
    pub size: Option<Size>,
    #[serde(default)]
    pub fit: ImageFit,
    #[serde(default = "default_loop_true")]
    pub loop_gif: bool,
    #[serde(flatten)]
    pub animation: AnimationConfig,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(flatten)]
    pub style: StyleConfig,
}

crate::impl_traits!(Gif {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Widget for Gif {
    fn render(&self, canvas: &Canvas, layout: &LayoutNode, ctx: &RenderContext) -> Result<()> {
        let gcache = gif_cache();

        let cached = if let Some(cached) = gcache.get(&self.src) {
            cached.clone()
        } else {
            let file = std::fs::File::open(&self.src)
                .map_err(|e| anyhow::anyhow!("Failed to open GIF '{}': {}", self.src, e))?;

            let mut decoder = gif::DecodeOptions::new();
            decoder.set_color_output(gif::ColorOutput::RGBA);
            let mut decoder = decoder.read_info(file)
                .map_err(|e| anyhow::anyhow!("Failed to decode GIF '{}': {}", self.src, e))?;

            let gif_width = decoder.width() as u32;
            let gif_height = decoder.height() as u32;

            let mut frames: Vec<(Vec<u8>, u32, u32)> = Vec::new();
            let mut cumulative_times: Vec<f64> = Vec::new();
            let mut accumulated = 0.0;

            while let Some(frame) = decoder.read_next_frame()
                .map_err(|e| anyhow::anyhow!("Failed to read GIF frame: {}", e))? {
                let delay = frame.delay as f64 / 100.0;
                let delay = if delay < 0.01 { 0.1 } else { delay };
                accumulated += delay;
                frames.push((frame.buffer.to_vec(), gif_width, gif_height));
                cumulative_times.push(accumulated);
            }

            let total_duration = accumulated;
            let cached = Arc::new((frames, cumulative_times, total_duration));
            gcache.insert(self.src.clone(), cached.clone());
            cached
        };

        let (ref frames, ref cumulative_times, total_duration) = *cached;

        if frames.is_empty() {
            return Ok(());
        }

        let effective_time = if self.loop_gif {
            ctx.time % total_duration
        } else {
            ctx.time.min(total_duration)
        };

        let frame_idx = cumulative_times.partition_point(|&t| t <= effective_time).min(frames.len() - 1);
        let (ref frame_data, gif_width, gif_height) = frames[frame_idx];

        let img_info = ImageInfo::new(
            (gif_width as i32, gif_height as i32),
            ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let row_bytes = gif_width as usize * 4;
        let data = skia_safe::Data::new_copy(frame_data);
        if let Some(img) = skia_safe::images::raster_from_data(&img_info, data, row_bytes) {
            let dst = Rect::from_xywh(0.0, 0.0, layout.width, layout.height);
            let paint = Paint::default();
            canvas.draw_image_rect(img, None, dst, &paint);
        }

        Ok(())
    }

    fn measure(&self, _constraints: &Constraints) -> (f32, f32) {
        match &self.size {
            Some(s) => (s.width, s.height),
            None => (100.0, 100.0),
        }
    }
}
