use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Paint, Rect};

use crate::engine::renderer::asset_cache;
use crate::layout::{Constraints, LayoutNode};
use crate::schema::{ImageFit, Size};
use crate::traits::{AnimationConfig, RenderContext, StyleConfig, TimingConfig, Widget};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Image {
    pub src: String,
    #[serde(default)]
    pub size: Option<Size>,
    #[serde(default)]
    pub fit: ImageFit,
    #[serde(flatten)]
    pub animation: AnimationConfig,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(flatten)]
    pub style: StyleConfig,
}

crate::impl_traits!(Image {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Widget for Image {
    fn render(&self, canvas: &Canvas, layout: &LayoutNode, _ctx: &RenderContext) -> Result<()> {
        let cache = asset_cache();
        let img = if let Some(cached) = cache.get(&self.src) {
            cached.clone()
        } else {
            let data = std::fs::read(&self.src)
                .map_err(|e| anyhow::anyhow!("Failed to load image '{}': {}", self.src, e))?;
            let skia_data = skia_safe::Data::new_copy(&data);
            let decoded = skia_safe::Image::from_encoded(skia_data)
                .ok_or_else(|| anyhow::anyhow!("Failed to decode image '{}'", self.src))?;
            cache.insert(self.src.clone(), decoded.clone());
            decoded
        };

        let img_w = img.width() as f32;
        let img_h = img.height() as f32;
        let target_w = layout.width;
        let target_h = layout.height;

        let (draw_w, draw_h, offset_x, offset_y) = match self.fit {
            ImageFit::Fill => (target_w, target_h, 0.0, 0.0),
            ImageFit::Contain => {
                let scale = (target_w / img_w).min(target_h / img_h);
                let w = img_w * scale;
                let h = img_h * scale;
                (w, h, (target_w - w) / 2.0, (target_h - h) / 2.0)
            }
            ImageFit::Cover => {
                let scale = (target_w / img_w).max(target_h / img_h);
                let w = img_w * scale;
                let h = img_h * scale;
                (w, h, (target_w - w) / 2.0, (target_h - h) / 2.0)
            }
        };

        let dst = Rect::from_xywh(offset_x, offset_y, draw_w, draw_h);
        let paint = Paint::default();

        if matches!(self.fit, ImageFit::Cover) {
            canvas.save();
            canvas.clip_rect(
                Rect::from_xywh(0.0, 0.0, target_w, target_h),
                skia_safe::ClipOp::Intersect,
                true,
            );
            canvas.draw_image_rect(img, None, dst, &paint);
            canvas.restore();
        } else {
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
