use rustmotion_core::css::CssStyle;
use rustmotion_core::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Paint, PaintStyle, RRect, Rect};

use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    asset_cache, draw_text_with_fallback, emoji_typeface, measure_text_with_fallback,
    paint_from_hex, typeface_with_fallback,
};
use rustmotion_core::error::RustmotionError;
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

fn default_avatar_size() -> f32 {
    48.0
}

fn default_overlap() -> f32 {
    16.0
}

fn default_border_width() -> f32 {
    3.0
}

fn default_border_color() -> String {
    "#0f172a".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AvatarGroupItem {
    pub src: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct AvatarGroup {
    pub avatars: Vec<AvatarGroupItem>,
    #[serde(default)]
    pub max_display: Option<usize>,
    #[serde(default = "default_avatar_size")]
    pub size: f32,
    #[serde(default = "default_overlap")]
    pub overlap: f32,
    #[serde(default = "default_border_width")]
    pub border_width: f32,
    #[serde(default = "default_border_color")]
    pub border_color: String,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(AvatarGroup {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl AvatarGroup {
    pub fn visible_count(&self) -> usize {
        match self.max_display {
            Some(max) => max.min(self.avatars.len()),
            None => self.avatars.len(),
        }
    }

    pub fn overflow_count(&self) -> usize {
        self.avatars.len().saturating_sub(self.visible_count())
    }
}

impl AvatarGroup {
    fn paint(&self, canvas: &Canvas) -> Result<()> {
        let s = self.size;
        let step = s - self.overlap;
        let visible = self.visible_count();
        let overflow = self.overflow_count();
        let cache = asset_cache();

        // Draw avatars in reverse order so first avatar is on top
        for rev_i in (0..visible).rev() {
            let avatar = &self.avatars[rev_i];
            let x = rev_i as f32 * step;

            // Border circle (background ring)
            let mut border_paint = paint_from_hex(&self.border_color);
            border_paint.set_style(PaintStyle::Fill);
            border_paint.set_anti_alias(true);
            canvas.draw_circle((x + s / 2.0, s / 2.0), s / 2.0, &border_paint);

            // Load image
            let img = if let Some(cached) = cache.get(&avatar.src) {
                cached.clone()
            } else {
                let data = std::fs::read(&avatar.src).map_err(|e| RustmotionError::ImageLoad {
                    path: avatar.src.clone(),
                    reason: e.to_string(),
                })?;
                let skia_data = skia_safe::Data::new_copy(&data);
                let decoded = skia_safe::Image::from_encoded(skia_data).ok_or_else(|| {
                    RustmotionError::ImageDecode {
                        path: avatar.src.clone(),
                    }
                })?;
                cache.insert(avatar.src.clone(), decoded.clone());
                decoded
            };

            // Clip to circle inset by border_width
            let inset = self.border_width;
            let inner_r = s / 2.0 - inset;
            let cx = x + s / 2.0;
            let cy = s / 2.0;

            let clip_rect =
                Rect::from_xywh(cx - inner_r, cy - inner_r, inner_r * 2.0, inner_r * 2.0);
            let clip_rrect = RRect::new_oval(clip_rect);

            canvas.save();
            canvas.clip_rrect(clip_rrect, skia_safe::ClipOp::Intersect, true);

            // Draw image with cover fit
            let img_w = img.width() as f32;
            let img_h = img.height() as f32;
            let d = inner_r * 2.0;
            let scale = (d / img_w).max(d / img_h);
            let draw_w = img_w * scale;
            let draw_h = img_h * scale;
            let offset_x = cx - inner_r + (d - draw_w) / 2.0;
            let offset_y = cy - inner_r + (d - draw_h) / 2.0;

            let dst = Rect::from_xywh(offset_x, offset_y, draw_w, draw_h);
            canvas.draw_image_rect(img, None, dst, &Paint::default());
            canvas.restore();
        }

        // "+N" overflow badge
        if overflow > 0 {
            let x = visible as f32 * step;
            let cx = x + s / 2.0;
            let cy = s / 2.0;

            // Background circle
            let mut bg_paint = paint_from_hex("#374151");
            bg_paint.set_style(PaintStyle::Fill);
            bg_paint.set_anti_alias(true);
            canvas.draw_circle((cx, cy), s / 2.0, &bg_paint);

            // Border
            let mut border_paint = paint_from_hex(&self.border_color);
            border_paint.set_style(PaintStyle::Stroke);
            border_paint.set_stroke_width(self.border_width);
            border_paint.set_anti_alias(true);
            canvas.draw_circle((cx, cy), s / 2.0 - self.border_width / 2.0, &border_paint);

            // Text
            let text = format!("+{}", overflow);
            let font_size = s * 0.35;
            let font_style = skia_safe::FontStyle::bold();
            let Ok(typeface) = typeface_with_fallback("Inter", font_style) else {
                return Ok(());
            };
            let font = skia_safe::Font::from_typeface(typeface, font_size);
            let emoji_font =
                emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, font_size));

            let mut text_paint = paint_from_hex("#D1D5DB");
            text_paint.set_anti_alias(true);

            let text_w = measure_text_with_fallback(&text, &font, &emoji_font, 0.0);
            let (_, metrics) = font.metrics();
            let text_x = cx - text_w / 2.0;
            let text_y = cy + (-metrics.ascent) / 2.0;

            draw_text_with_fallback(
                canvas,
                &text,
                &font,
                &emoji_font,
                0.0,
                text_x,
                text_y,
                &text_paint,
            );
        }

        Ok(())
    }
}

impl Painter for AvatarGroup {
    fn paint_content(
        &self,
        canvas: &Canvas,
        _layout: &BoxLayout,
        _props: &AnimatedProperties,
        _ctx: &PaintCtx,
    ) {
        let _ = self.paint(canvas);
    }
}
