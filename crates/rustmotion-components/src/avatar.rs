use rustmotion_core::css::CssStyle;
use rustmotion_core::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Paint, PaintStyle, Rect, RRect};

use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{asset_cache, paint_from_hex};
use rustmotion_core::error::RustmotionError;
use rustmotion_core::schema::{AnimationEffect, TimelineStep};
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AvatarStatus {
    Online,
    Offline,
    Away,
    #[serde(rename = "none")]
    NoStatus,
}

impl Default for AvatarStatus {
    fn default() -> Self {
        Self::NoStatus
    }
}

impl AvatarStatus {
    fn default_color(&self) -> &str {
        match self {
            AvatarStatus::Online => "#22C55E",
            AvatarStatus::Offline => "#9CA3AF",
            AvatarStatus::Away => "#F59E0B",
            AvatarStatus::NoStatus => "",
        }
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Avatar {
    pub src: String,
    #[serde(default = "default_avatar_size")]
    pub size: f32,
    #[serde(default)]
    pub border_color: Option<String>,
    #[serde(default)]
    pub border_width: Option<f32>,
    #[serde(default)]
    pub status: AvatarStatus,
    #[serde(default)]
    pub status_color: Option<String>,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default, deserialize_with = "rustmotion_core::schema::deserialize_animation_effects")]
    pub animation: Vec<AnimationEffect>,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

fn default_avatar_size() -> f32 {
    64.0
}

rustmotion_core::impl_traits!(Avatar {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Avatar {
    fn paint(&self, canvas: &Canvas, layout_w: f32, layout_h: f32) -> Result<()> {
        let w = layout_w;
        let h = layout_h;

        // Load image
        let cache = asset_cache();
        let img = if let Some(cached) = cache.get(&self.src) {
            cached.clone()
        } else {
            let data = std::fs::read(&self.src)
                .map_err(|e| RustmotionError::ImageLoad { path: self.src.clone(), reason: e.to_string() })?;
            let skia_data = skia_safe::Data::new_copy(&data);
            let decoded = skia_safe::Image::from_encoded(skia_data)
                .ok_or_else(|| RustmotionError::ImageDecode { path: self.src.clone() })?;
            cache.insert(self.src.clone(), decoded.clone());
            decoded
        };

        // Circular clip + draw image with cover fit
        let oval_rect = Rect::from_xywh(0.0, 0.0, w, h);
        let oval_rrect = RRect::new_oval(oval_rect);

        canvas.save();
        canvas.clip_rrect(oval_rrect, skia_safe::ClipOp::Intersect, true);

        let img_w = img.width() as f32;
        let img_h = img.height() as f32;
        let scale = (w / img_w).max(h / img_h);
        let draw_w = img_w * scale;
        let draw_h = img_h * scale;
        let offset_x = (w - draw_w) / 2.0;
        let offset_y = (h - draw_h) / 2.0;

        let dst = Rect::from_xywh(offset_x, offset_y, draw_w, draw_h);
        canvas.draw_image_rect(img, None, dst, &Paint::default());

        canvas.restore();

        // Border
        let border_width = self.border_width.unwrap_or(0.0);
        if border_width > 0.0 {
            if let Some(border_color) = &self.border_color {
                let mut border_paint = paint_from_hex(border_color);
                border_paint.set_style(PaintStyle::Stroke);
                border_paint.set_stroke_width(border_width);
                border_paint.set_anti_alias(true);

                let inset = border_width / 2.0;
                let border_rect = Rect::from_xywh(inset, inset, w - border_width, h - border_width);
                canvas.draw_oval(border_rect, &border_paint);
            }
        }

        // Status indicator
        if !matches!(self.status, AvatarStatus::NoStatus) {
            let dot_radius = w * 0.15;
            let dot_color = self
                .status_color
                .as_deref()
                .unwrap_or_else(|| self.status.default_color());

            let mut dot_paint = paint_from_hex(dot_color);
            dot_paint.set_style(PaintStyle::Fill);
            dot_paint.set_anti_alias(true);

            let cx = w - dot_radius;
            let cy = h - dot_radius;
            canvas.draw_circle((cx, cy), dot_radius, &dot_paint);

            // White border around dot
            let mut border_paint = paint_from_hex("#FFFFFF");
            border_paint.set_style(PaintStyle::Stroke);
            border_paint.set_stroke_width(2.0);
            border_paint.set_anti_alias(true);
            canvas.draw_circle((cx, cy), dot_radius, &border_paint);
        }

        Ok(())
    }
}

impl Painter for Avatar {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &AnimatedProperties,
        _ctx: &PaintCtx,
    ) {
        let _ = self.paint(canvas, layout.width, layout.height);
    }
}
