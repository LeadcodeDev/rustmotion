use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Font, FontStyle, PaintStyle, TextBlob};

use crate::engine::renderer::{font_mgr, format_counter_value, make_text_blob_with_spacing, paint_from_hex};
use crate::layout::{Constraints, LayoutNode};
use crate::schema::{EasingType, FontStyleType, FontWeight, Stroke, TextAlign, TextShadow};
use crate::traits::{AnimationConfig, RenderContext, StyleConfig, TimingConfig, Widget};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Counter {
    pub from: f64,
    pub to: f64,
    #[serde(default)]
    pub decimals: u8,
    #[serde(default)]
    pub separator: Option<String>,
    #[serde(default)]
    pub prefix: Option<String>,
    #[serde(default)]
    pub suffix: Option<String>,
    #[serde(default)]
    pub easing: EasingType,
    // Visual properties
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default = "default_font_family")]
    pub font_family: String,
    #[serde(default)]
    pub font_weight: FontWeight,
    #[serde(default)]
    pub font_style: FontStyleType,
    #[serde(default)]
    pub align: TextAlign,
    #[serde(default)]
    pub letter_spacing: Option<f32>,
    #[serde(default)]
    pub shadow: Option<TextShadow>,
    #[serde(default)]
    pub stroke: Option<Stroke>,
    // Composed behaviors
    #[serde(flatten)]
    pub animation: AnimationConfig,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(flatten)]
    pub style: StyleConfig,
}

crate::impl_traits!(Counter {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Widget for Counter {
    fn render(&self, canvas: &Canvas, layout: &LayoutNode, ctx: &RenderContext) -> Result<()> {
        use crate::engine::animator::ease;

        // Calculate counter progress based on scene timing
        let duration = ctx.scene_duration;
        let t = if duration > 0.0 {
            (ctx.time / duration).clamp(0.0, 1.0)
        } else {
            1.0
        };

        let progress = ease(t, &self.easing);
        let value = self.from + (self.to - self.from) * progress;
        let content = format_counter_value(value, self.decimals, &self.separator, &self.prefix, &self.suffix);

        // Build font
        let fm = font_mgr();
        let slant = match self.font_style {
            FontStyleType::Normal => skia_safe::font_style::Slant::Upright,
            FontStyleType::Italic => skia_safe::font_style::Slant::Italic,
            FontStyleType::Oblique => skia_safe::font_style::Slant::Oblique,
        };
        let weight = match self.font_weight {
            FontWeight::Bold => skia_safe::font_style::Weight::BOLD,
            FontWeight::Normal => skia_safe::font_style::Weight::NORMAL,
        };
        let font_style = FontStyle::new(weight, skia_safe::font_style::Width::NORMAL, slant);

        let typeface = fm
            .match_family_style(&self.font_family, font_style)
            .or_else(|| fm.match_family_style("Helvetica", font_style))
            .or_else(|| fm.match_family_style("Arial", font_style))
            .or_else(|| fm.match_family_style("sans-serif", font_style))
            .expect("No fonts available on this system");

        let font = Font::from_typeface(typeface, self.font_size);
        let mut paint = paint_from_hex(&self.color);
        paint.set_alpha_f(1.0);

        let letter_spacing = self.letter_spacing.unwrap_or(0.0);

        let blob = if letter_spacing.abs() > 0.01 {
            make_text_blob_with_spacing(&content, &font, letter_spacing)
        } else {
            TextBlob::new(&content, &font)
        };

        if let Some(blob) = blob {
            let (advance_width, _) = font.measure_str(&content, None);

            let x = match self.align {
                TextAlign::Left => 0.0,
                TextAlign::Center => (layout.width - advance_width) / 2.0,
                TextAlign::Right => layout.width - advance_width,
            };
            // Center text visually within its line_height box
            let (_, metrics) = font.metrics();
            let line_height = self.font_size * 1.3;
            let ascent = -metrics.ascent;
            let descent = metrics.descent;
            let y = (line_height + ascent - descent) / 2.0;

            // Draw shadow
            if let Some(ref shadow) = self.shadow {
                let mut sp = paint_from_hex(&shadow.color);
                if shadow.blur > 0.01 {
                    if let Some(filter) = skia_safe::image_filters::blur(
                        (shadow.blur, shadow.blur),
                        skia_safe::TileMode::Clamp,
                        None,
                        None,
                    ) {
                        sp.set_image_filter(filter);
                    }
                }
                canvas.draw_text_blob(&blob, (x + shadow.offset_x, y + shadow.offset_y), &sp);
            }

            // Draw stroke
            if let Some(ref stroke) = self.stroke {
                let mut sp = paint_from_hex(&stroke.color);
                sp.set_style(PaintStyle::Stroke);
                sp.set_stroke_width(stroke.width);
                canvas.draw_text_blob(&blob, (x, y), &sp);
            }

            // Draw fill
            canvas.draw_text_blob(&blob, (x, y), &paint);
        }

        Ok(())
    }

    fn measure(&self, _constraints: &Constraints) -> (f32, f32) {
        let fm = font_mgr();
        let font_style = match self.font_weight {
            FontWeight::Bold => FontStyle::bold(),
            FontWeight::Normal => FontStyle::normal(),
        };
        let typeface = fm
            .match_family_style(&self.font_family, font_style)
            .or_else(|| fm.match_family_style("Helvetica", font_style))
            .or_else(|| fm.match_family_style("Arial", font_style))
            .unwrap_or_else(|| fm.match_family_style("sans-serif", font_style).unwrap());
        let font = Font::from_typeface(typeface, self.font_size);
        let display = format_counter_value(self.to, self.decimals, &self.separator, &self.prefix, &self.suffix);
        let (text_width, _) = font.measure_str(&display, None);
        let line_height = self.font_size * 1.3;
        (text_width, line_height)
    }
}

fn default_font_size() -> f32 { 48.0 }
fn default_color() -> String { "#FFFFFF".to_string() }
fn default_font_family() -> String { "Inter".to_string() }
