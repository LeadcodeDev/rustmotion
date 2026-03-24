use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, Rect, RRect};

use crate::engine::renderer::{
    color4f_from_hex, draw_text_with_fallback, emoji_typeface, font_mgr, measure_text_with_fallback,
    paint_from_hex,
};
use crate::layout::{Constraints, LayoutNode};
use crate::schema::LayerStyle;
use crate::traits::{RenderContext, TimingConfig, Widget};

fn default_progress_width() -> f32 {
    300.0
}
fn default_progress_height() -> f32 {
    20.0
}
fn default_progress_bg() -> String {
    "#333333".to_string()
}
fn default_progress_fill() -> String {
    "#4CAF50".to_string()
}
fn default_track_width() -> f32 {
    8.0
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProgressVariant {
    Linear,
    Circular,
}

impl Default for ProgressVariant {
    fn default() -> Self {
        Self::Linear
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Progress {
    #[serde(default)]
    pub progress: f64,
    #[serde(default)]
    pub variant: ProgressVariant,
    #[serde(default = "default_progress_width")]
    pub width: f32,
    #[serde(default = "default_progress_height")]
    pub height: f32,
    #[serde(default = "default_progress_bg")]
    pub background_color: String,
    #[serde(default = "default_progress_fill")]
    pub fill_color: String,
    #[serde(default)]
    pub border_radius: f32,
    #[serde(default = "default_track_width")]
    pub track_width: f32,
    #[serde(default)]
    pub show_value: bool,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

crate::impl_traits!(Progress {
    Animatable => style,
    Timed => timing,
    Styled => style,
});

impl Widget for Progress {
    fn render(
        &self,
        canvas: &Canvas,
        _layout: &LayoutNode,
        _ctx: &RenderContext,
        _props: &crate::engine::animator::AnimatedProperties,
    ) -> Result<()> {
        match self.variant {
            ProgressVariant::Linear => self.render_linear(canvas),
            ProgressVariant::Circular => self.render_circular(canvas),
        }
    }

    fn measure(&self, _constraints: &Constraints) -> (f32, f32) {
        (self.width, self.height)
    }
}

impl Progress {
    fn render_linear(&self, canvas: &Canvas) -> Result<()> {
        let w = self.width;
        let h = self.height;
        let radius = self.border_radius;
        let progress = self.progress.clamp(0.0, 1.0) as f32;

        // Background
        let mut bg_paint = skia_safe::Paint::new(color4f_from_hex(&self.background_color), None);
        bg_paint.set_style(PaintStyle::Fill);
        bg_paint.set_anti_alias(true);

        let bg_rect = Rect::from_xywh(0.0, 0.0, w, h);
        let bg_rrect = RRect::new_rect_xy(bg_rect, radius, radius);
        canvas.draw_rrect(bg_rrect, &bg_paint);

        // Fill (progress)
        if progress > 0.001 {
            let mut fill_paint =
                skia_safe::Paint::new(color4f_from_hex(&self.fill_color), None);
            fill_paint.set_style(PaintStyle::Fill);
            fill_paint.set_anti_alias(true);

            let fill_w = w * progress;
            let fill_rect = Rect::from_xywh(0.0, 0.0, fill_w, h);

            canvas.save();
            canvas.clip_rrect(bg_rrect, skia_safe::ClipOp::Intersect, true);
            canvas.draw_rect(fill_rect, &fill_paint);
            canvas.restore();
        }

        Ok(())
    }

    fn render_circular(&self, canvas: &Canvas) -> Result<()> {
        let w = self.width;
        let h = self.height;
        let progress = self.progress.clamp(0.0, 1.0) as f32;

        let cx = w / 2.0;
        let cy = h / 2.0;
        let radius = cx.min(cy) - self.track_width / 2.0 - 2.0;
        let oval = Rect::from_xywh(cx - radius, cy - radius, radius * 2.0, radius * 2.0);

        // Track
        let mut track_paint = paint_from_hex(&self.background_color);
        track_paint.set_style(PaintStyle::Stroke);
        track_paint.set_stroke_width(self.track_width);
        track_paint.set_stroke_cap(skia_safe::paint::Cap::Round);
        track_paint.set_anti_alias(true);
        canvas.draw_arc(oval, 0.0, 360.0, false, &track_paint);

        // Fill arc
        if progress > 0.001 {
            let sweep = 360.0 * progress;
            let mut fill_paint = paint_from_hex(&self.fill_color);
            fill_paint.set_style(PaintStyle::Stroke);
            fill_paint.set_stroke_width(self.track_width);
            fill_paint.set_stroke_cap(skia_safe::paint::Cap::Round);
            fill_paint.set_anti_alias(true);
            canvas.draw_arc(oval, -90.0, sweep, false, &fill_paint);
        }

        // Value text
        if self.show_value {
            let text = format!("{}%", (progress * 100.0).round() as i32);
            let font_size = (radius * 0.5).max(10.0);
            let fm = font_mgr();
            let font_style = skia_safe::FontStyle::bold();
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
            let text_x = cx - text_w / 2.0;
            let text_y = cy + (-metrics.ascent) / 2.0;
            draw_text_with_fallback(
                canvas, &text, &font, &emoji_font, 0.0, text_x, text_y, &text_paint,
            );
        }

        Ok(())
    }
}
