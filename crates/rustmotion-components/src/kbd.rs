use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, Rect};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, measure_text_with_fallback, paint_from_hex,
    typeface_with_fallback,
};
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

fn default_font_size() -> f32 {
    14.0
}

fn default_bg_color() -> String {
    "#1E293B".to_string()
}

fn default_border_color() -> String {
    "#475569".to_string()
}

fn default_text_color() -> String {
    "#E2E8F0".to_string()
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Kbd {
    pub key: String,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default = "default_bg_color")]
    pub background_color: String,
    #[serde(default = "default_border_color")]
    pub border_color: String,
    #[serde(default = "default_text_color")]
    pub text_color: String,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(Kbd {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Kbd {
    fn make_font(&self) -> Option<skia_safe::Font> {
        let fs = self.style.font_size_px_or(self.font_size);
        let font_style = skia_safe::FontStyle::normal();
        let family = self.style.font_family.as_deref().unwrap_or("SF Mono");
        let typeface = typeface_with_fallback(family, font_style).ok()?;
        Some(skia_safe::Font::from_typeface(typeface, fs))
    }
}

impl Kbd {
    fn paint(&self, canvas: &Canvas, layout_w: f32, layout_h: f32) {
        let w = layout_w;
        let h = layout_h;
        let radius = 6.0;

        let bg_color = self
            .style
            .background_color_str()
            .unwrap_or(&self.background_color);

        // Shadow (bottom edge to simulate physical key depth)
        let shadow_h = 3.0;
        let shadow_rect = Rect::from_xywh(0.0, shadow_h, w, h);
        let shadow_rrect = skia_safe::RRect::new_rect_xy(shadow_rect, radius, radius);
        let mut shadow_paint = paint_from_hex(&self.border_color);
        shadow_paint.set_style(PaintStyle::Fill);
        shadow_paint.set_anti_alias(true);
        canvas.draw_rrect(shadow_rrect, &shadow_paint);

        // Key face background
        let face_rect = Rect::from_xywh(0.0, 0.0, w, h);
        let face_rrect = skia_safe::RRect::new_rect_xy(face_rect, radius, radius);
        let mut face_paint = paint_from_hex(bg_color);
        face_paint.set_style(PaintStyle::Fill);
        face_paint.set_anti_alias(true);
        canvas.draw_rrect(face_rrect, &face_paint);

        // Border
        let mut border_paint = paint_from_hex(&self.border_color);
        border_paint.set_style(PaintStyle::Stroke);
        border_paint.set_stroke_width(1.0);
        border_paint.set_anti_alias(true);
        canvas.draw_rrect(face_rrect, &border_paint);

        // Text centered
        let Some(font) = self.make_font() else {
            return;
        };
        let fs = self.style.font_size_px_or(self.font_size);
        let emoji_font = emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, fs));

        let text_color = self.style.color_str().unwrap_or(&self.text_color);
        let mut text_paint = paint_from_hex(text_color);
        text_paint.set_anti_alias(true);

        let text_w = measure_text_with_fallback(&self.key, &font, &emoji_font, 0.0);
        let (_, metrics) = font.metrics();
        let text_x = (w - text_w) / 2.0;
        let text_y = (h + (-metrics.ascent)) / 2.0;

        draw_text_with_fallback(
            canvas,
            &self.key,
            &font,
            &emoji_font,
            0.0,
            text_x,
            text_y,
            &text_paint,
        );
    }
}

impl Painter for Kbd {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &AnimatedProperties,
        _ctx: &PaintCtx,
    ) {
        self.paint(canvas, layout.width, layout.height);
    }
}
