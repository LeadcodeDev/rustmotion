use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Font, FontStyle, Point, TextBlob};

use rustmotion_core::css::style::{
    FontStyle as CssFontStyle, FontWeight as CssFontWeight, FontWeightKw,
};
use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    emoji_typeface, font_mgr, paint_from_hex, parse_hex_color,
};
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

fn default_colors() -> Vec<String> {
    vec!["#3B82F6".to_string(), "#8B5CF6".to_string()]
}

fn default_angle() -> f32 {
    90.0
}

fn default_speed() -> f32 {
    0.5
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GradientText {
    pub content: String,
    #[serde(default = "default_colors")]
    pub colors: Vec<String>,
    #[serde(default = "default_angle")]
    pub angle: f32,
    #[serde(default)]
    pub animate_angle: bool,
    #[serde(default = "default_speed")]
    pub speed: f32,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(GradientText {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl GradientText {
    fn resolve_font(&self) -> (Font, Option<Font>) {
        let font_size = self.style.font_size_px_or(48.0);
        let font_family = self.style.font_family_or("Inter");

        let fm = font_mgr();
        let slant = match self.style.font_style {
            Some(CssFontStyle::Italic) => skia_safe::font_style::Slant::Italic,
            Some(CssFontStyle::Oblique) => skia_safe::font_style::Slant::Oblique,
            _ => skia_safe::font_style::Slant::Upright,
        };
        let weight = match &self.style.font_weight {
            Some(CssFontWeight::Keyword(FontWeightKw::Bold | FontWeightKw::Bolder)) => {
                skia_safe::font_style::Weight::BOLD
            }
            Some(CssFontWeight::Number(n)) => skia_safe::font_style::Weight::from(*n as i32),
            _ => skia_safe::font_style::Weight::NORMAL,
        };
        let skia_style = FontStyle::new(weight, skia_safe::font_style::Width::NORMAL, slant);

        let typeface = fm
            .match_family_style(font_family, skia_style)
            .or_else(|| fm.match_family_style("Helvetica", skia_style))
            .unwrap_or_else(|| fm.legacy_make_typeface(None, skia_style).unwrap());

        let font = Font::from_typeface(typeface, font_size);
        let emoji_font = emoji_typeface().map(|tf| Font::from_typeface(tf, font_size));
        (font, emoji_font)
    }
}

impl GradientText {
    fn paint(&self, canvas: &Canvas, time: f64) {
        if self.content.is_empty() || self.colors.is_empty() {
            return;
        }

        let (font, _emoji_font) = self.resolve_font();
        let _font_size = self.style.font_size_px_or(48.0);

        // Measure text
        let (_, metrics) = font.metrics();
        let text_w = font.measure_str(&self.content, None).0;
        let text_h = -metrics.ascent + metrics.descent;

        // Compute angle (possibly animated)
        let angle = if self.animate_angle {
            self.angle + time as f32 * self.speed * 360.0
        } else {
            self.angle
        };

        // Compute gradient endpoints from angle
        let angle_rad = angle * std::f32::consts::PI / 180.0;
        let cx = text_w / 2.0;
        let cy = text_h / 2.0;
        let half_diag = (text_w.powi(2) + text_h.powi(2)).sqrt() / 2.0;
        let start = Point::new(
            cx - angle_rad.cos() * half_diag,
            cy - angle_rad.sin() * half_diag,
        );
        let end = Point::new(
            cx + angle_rad.cos() * half_diag,
            cy + angle_rad.sin() * half_diag,
        );

        // Parse gradient colors
        let skia_colors: Vec<skia_safe::Color> = self
            .colors
            .iter()
            .map(|hex| {
                let (r, g, b, a) = parse_hex_color(hex);
                skia_safe::Color::from_argb(a, r, g, b)
            })
            .collect();

        // Build linear gradient shader
        let positions: Option<&[f32]> = None;
        let shader = skia_safe::shader::Shader::linear_gradient(
            (start, end),
            skia_safe::gradient_shader::GradientShaderColors::Colors(&skia_colors),
            positions,
            skia_safe::TileMode::Clamp,
            None,
            None,
        );

        if let Some(shader) = shader {
            // Create paint with gradient shader
            let mut gradient_paint = skia_safe::Paint::default();
            gradient_paint.set_anti_alias(true);
            gradient_paint.set_shader(shader);

            // Create text blob and draw
            if let Some(blob) = TextBlob::new(&self.content, &font) {
                let y = -metrics.ascent;
                canvas.draw_text_blob(&blob, Point::new(0.0, y), &gradient_paint);
            }
        } else {
            // Fallback: draw with first color
            let mut paint = paint_from_hex(&self.colors[0]);
            paint.set_anti_alias(true);

            if let Some(blob) = TextBlob::new(&self.content, &font) {
                let y = -metrics.ascent;
                canvas.draw_text_blob(&blob, Point::new(0.0, y), &paint);
            }
        }
    }
}

impl Painter for GradientText {
    fn paint_content(
        &self,
        canvas: &Canvas,
        _layout: &BoxLayout,
        _props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        self.paint(canvas, ctx.time);
    }
}
