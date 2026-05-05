use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, Rect};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, font_mgr, measure_text_with_fallback, paint_from_hex,
};
use rustmotion_core::schema::{AnimationEffect, Size, TimelineStep};
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

fn default_max() -> f64 {
    100.0
}

fn default_track_color() -> String {
    "#333333".to_string()
}

fn default_fill_color() -> String {
    "#3B82F6".to_string()
}

fn default_track_width() -> f32 {
    16.0
}

fn default_start_angle() -> f32 {
    135.0
}

fn default_end_angle() -> f32 {
    405.0
}

fn default_animated() -> bool {
    true
}

fn default_animation_duration() -> f64 {
    1.5
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Gauge {
    #[serde(default)]
    pub value: f64,
    #[serde(default)]
    pub min: f64,
    #[serde(default = "default_max")]
    pub max: f64,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub size: Option<Size>,
    #[serde(default = "default_track_color")]
    pub track_color: String,
    #[serde(default = "default_fill_color")]
    pub fill_color: String,
    #[serde(default = "default_track_width")]
    pub track_width: f32,
    #[serde(default = "default_start_angle")]
    pub start_angle: f32,
    #[serde(default = "default_end_angle")]
    pub end_angle: f32,
    #[serde(default = "default_show_value")]
    pub show_value: bool,
    #[serde(default = "default_animated")]
    pub animated: bool,
    #[serde(default = "default_animation_duration")]
    pub animation_duration: f64,
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

fn default_show_value() -> bool {
    true
}

rustmotion_core::impl_traits!(Gauge {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Gauge {
    fn progress_at(&self, time: f64) -> f32 {
        if !self.animated {
            return 1.0;
        }
        let p = (time / self.animation_duration).clamp(0.0, 1.0) as f32;
        1.0 - (1.0 - p).powi(3)
    }

    fn paint(&self, canvas: &Canvas, layout_w: f32, layout_h: f32, time: f64) {
        let w = layout_w;
        let h = layout_h;
        let progress = self.progress_at(time);

        let cx = w / 2.0;
        let cy = h / 2.0;
        let radius = cx.min(cy) - self.track_width / 2.0 - 4.0;

        let oval = Rect::from_xywh(cx - radius, cy - radius, radius * 2.0, radius * 2.0);
        let total_sweep = self.end_angle - self.start_angle;

        // Track arc
        let mut track_paint = paint_from_hex(&self.track_color);
        track_paint.set_style(PaintStyle::Stroke);
        track_paint.set_stroke_width(self.track_width);
        track_paint.set_stroke_cap(skia_safe::paint::Cap::Round);
        track_paint.set_anti_alias(true);
        canvas.draw_arc(oval, self.start_angle, total_sweep, false, &track_paint);

        // Fill arc
        let range = (self.max - self.min).max(0.001);
        let value_ratio = ((self.value - self.min) / range).clamp(0.0, 1.0) as f32;
        let fill_sweep = total_sweep * value_ratio * progress;

        let mut fill_paint = paint_from_hex(&self.fill_color);
        fill_paint.set_style(PaintStyle::Stroke);
        fill_paint.set_stroke_width(self.track_width);
        fill_paint.set_stroke_cap(skia_safe::paint::Cap::Round);
        fill_paint.set_anti_alias(true);
        canvas.draw_arc(oval, self.start_angle, fill_sweep, false, &fill_paint);

        // Value text
        if self.show_value {
            let display_val = self.min + (self.value - self.min) * progress as f64;
            let text = format!("{}", display_val.round() as i64);

            let font_size = (radius * 0.45).max(16.0);
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

        // Label text
        if let Some(label) = &self.label {
            let font_size = (radius * 0.18).max(10.0);
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

            let mut label_paint = paint_from_hex("#888888");
            label_paint.set_anti_alias(true);

            let label_w = measure_text_with_fallback(label, &font, &emoji_font, 0.0);
            let label_x = cx - label_w / 2.0;
            let label_y = cy + radius * 0.35;

            draw_text_with_fallback(
                canvas, label, &font, &emoji_font, 0.0, label_x, label_y, &label_paint,
            );
        }
    }
}

impl Painter for Gauge {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        self.paint(canvas, layout.width, layout.height, ctx.time);
    }
}
