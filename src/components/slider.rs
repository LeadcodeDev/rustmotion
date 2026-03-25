use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, RRect, Rect};

use crate::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, font_mgr, measure_text_with_fallback, paint_from_hex,
};
use crate::layout::{Constraints, LayoutNode};
use crate::schema::LayerStyle;
use crate::traits::{RenderContext, TimingConfig, Widget};

fn default_slider_value() -> f64 {
    0.5
}
fn default_animation_duration() -> f64 {
    1.0
}
fn default_slider_width() -> f32 {
    300.0
}
fn default_slider_height() -> f32 {
    8.0
}
fn default_track_color() -> String {
    "#333333".to_string()
}
fn default_fill_color() -> String {
    "#3B82F6".to_string()
}
fn default_thumb_size() -> f32 {
    20.0
}
fn default_thumb_color() -> String {
    "#FFFFFF".to_string()
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Slider {
    #[serde(default = "default_slider_value")]
    pub value: f64,
    #[serde(default)]
    pub animate_to: Option<f64>,
    #[serde(default)]
    pub animate_at: Option<f64>,
    #[serde(default = "default_animation_duration")]
    pub animation_duration: f64,
    #[serde(default = "default_slider_width")]
    pub width: f32,
    #[serde(default = "default_slider_height")]
    pub height: f32,
    #[serde(default = "default_track_color")]
    pub track_color: String,
    #[serde(default = "default_fill_color")]
    pub fill_color: String,
    #[serde(default = "default_thumb_size")]
    pub thumb_size: f32,
    #[serde(default = "default_thumb_color")]
    pub thumb_color: String,
    #[serde(default)]
    pub show_value: bool,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

crate::impl_traits!(Slider {
    Animatable => style,
    Timed => timing,
    Styled => style,
});

impl Slider {
    fn ease_out_cubic(t: f64) -> f64 {
        1.0 - (1.0 - t).powi(3)
    }

    fn current_value(&self, ctx: &RenderContext) -> f64 {
        match (self.animate_to, self.animate_at) {
            (Some(target), Some(start_time)) if ctx.time >= start_time => {
                let elapsed = ctx.time - start_time;
                let progress = (elapsed / self.animation_duration).clamp(0.0, 1.0);
                let eased = Self::ease_out_cubic(progress);
                self.value + (target - self.value) * eased
            }
            _ => self.value,
        }
    }
}

impl Widget for Slider {
    fn render(
        &self,
        canvas: &Canvas,
        _layout: &LayoutNode,
        ctx: &RenderContext,
        _props: &crate::engine::animator::AnimatedProperties,
    ) -> Result<()> {
        let w = self.width;
        let h = self.height;
        let thumb_r = self.thumb_size / 2.0;
        let track_y = thumb_r - h / 2.0;
        let radius = h / 2.0;
        let current = self.current_value(ctx).clamp(0.0, 1.0) as f32;

        // Track background
        let mut track_paint = paint_from_hex(&self.track_color);
        track_paint.set_style(PaintStyle::Fill);
        track_paint.set_anti_alias(true);

        let track_rect = Rect::from_xywh(0.0, track_y, w, h);
        let track_rrect = RRect::new_rect_xy(track_rect, radius, radius);
        canvas.draw_rrect(track_rrect, &track_paint);

        // Fill portion
        if current > 0.001 {
            let mut fill_paint = paint_from_hex(&self.fill_color);
            fill_paint.set_style(PaintStyle::Fill);
            fill_paint.set_anti_alias(true);

            let fill_w = w * current;
            let fill_rect = Rect::from_xywh(0.0, track_y, fill_w, h);

            canvas.save();
            canvas.clip_rrect(track_rrect, skia_safe::ClipOp::Intersect, true);
            canvas.draw_rect(fill_rect, &fill_paint);
            canvas.restore();
        }

        // Thumb
        let thumb_cx = w * current;
        let thumb_cy = thumb_r;

        let mut thumb_paint = paint_from_hex(&self.thumb_color);
        thumb_paint.set_style(PaintStyle::Fill);
        thumb_paint.set_anti_alias(true);
        canvas.draw_circle((thumb_cx, thumb_cy), thumb_r, &thumb_paint);

        // Value text
        if self.show_value {
            let text = format!("{}%", (current * 100.0).round() as i32);
            let font_size = (self.thumb_size * 0.7).max(12.0);
            let fm = font_mgr();
            let font_style = skia_safe::FontStyle::normal();
            let typeface = fm
                .match_family_style("Inter", font_style)
                .or_else(|| fm.match_family_style("Helvetica", font_style))
                .or_else(|| fm.match_family_style("Arial", font_style))
                .unwrap_or_else(|| fm.legacy_make_typeface(None, font_style).unwrap());
            let font = skia_safe::Font::from_typeface(typeface, font_size);
            let emoji_font =
                emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, font_size));

            let mut text_paint = paint_from_hex(&self.fill_color);
            text_paint.set_anti_alias(true);

            let text_w = measure_text_with_fallback(&text, &font, &emoji_font, 0.0);
            let (_, metrics) = font.metrics();
            let text_x = thumb_cx - text_w / 2.0;
            let text_y = thumb_cy - thumb_r - 4.0 - (-metrics.descent);

            draw_text_with_fallback(
                canvas, &text, &font, &emoji_font, 0.0, text_x, text_y, &text_paint,
            );
        }

        Ok(())
    }

    fn measure(&self, _constraints: &Constraints) -> (f32, f32) {
        let total_height = self.thumb_size;
        let value_extra = if self.show_value {
            let font_size = (self.thumb_size * 0.7).max(12.0);
            font_size + 4.0
        } else {
            0.0
        };
        (self.width, total_height + value_extra)
    }
}
