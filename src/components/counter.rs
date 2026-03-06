use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Font, FontStyle, PaintStyle, TextBlob};

use crate::engine::renderer::{font_mgr, format_counter_value, make_text_blob_with_spacing, paint_from_hex};
use crate::layout::{Constraints, LayoutNode};
use crate::schema::{EasingType, FontStyleType, FontWeight, LayerStyle, TextAlign};
use crate::traits::{AnimationConfig, RenderContext, TimingConfig, Widget};

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
    // Composed behaviors
    #[serde(flatten)]
    pub animation: AnimationConfig,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

crate::impl_traits!(Counter {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Widget for Counter {
    fn render(&self, canvas: &Canvas, layout: &LayoutNode, ctx: &RenderContext) -> Result<()> {
        use crate::engine::animator::ease;

        let font_size = self.style.font_size_or(48.0);
        let color = self.style.color_or("#FFFFFF");
        let font_family = self.style.font_family_or("Inter");
        let font_weight = self.style.font_weight_or(FontWeight::Normal);
        let font_style_type = self.style.font_style_or(FontStyleType::Normal);
        let align = self.style.text_align_or(TextAlign::Left);

        let duration = ctx.scene_duration;
        let t = if duration > 0.0 {
            (ctx.time / duration).clamp(0.0, 1.0)
        } else {
            1.0
        };

        let progress = ease(t, &self.easing);
        let value = self.from + (self.to - self.from) * progress;
        let content = format_counter_value(value, self.decimals, &self.separator, &self.prefix, &self.suffix);

        let fm = font_mgr();
        let slant = match font_style_type {
            FontStyleType::Normal => skia_safe::font_style::Slant::Upright,
            FontStyleType::Italic => skia_safe::font_style::Slant::Italic,
            FontStyleType::Oblique => skia_safe::font_style::Slant::Oblique,
        };
        let weight = match font_weight {
            FontWeight::Bold => skia_safe::font_style::Weight::BOLD,
            FontWeight::Normal => skia_safe::font_style::Weight::NORMAL,
            FontWeight::Weight(w) => skia_safe::font_style::Weight::from(w as i32),
        };
        let skia_font_style = FontStyle::new(weight, skia_safe::font_style::Width::NORMAL, slant);

        let typeface = fm
            .match_family_style(font_family, skia_font_style)
            .or_else(|| fm.match_family_style("Helvetica", skia_font_style))
            .or_else(|| fm.match_family_style("Arial", skia_font_style))
            .or_else(|| fm.match_family_style("sans-serif", skia_font_style))
            .expect("No fonts available on this system");

        let font = Font::from_typeface(typeface, font_size);
        let mut paint = paint_from_hex(color);
        paint.set_alpha_f(1.0);

        let letter_spacing = self.style.letter_spacing.unwrap_or(0.0);

        let blob = if letter_spacing.abs() > 0.01 {
            make_text_blob_with_spacing(&content, &font, letter_spacing)
        } else {
            TextBlob::new(&content, &font)
        };

        if let Some(blob) = blob {
            let (advance_width, _) = font.measure_str(&content, None);

            let x = match align {
                TextAlign::Left => 0.0,
                TextAlign::Center => (layout.width - advance_width) / 2.0,
                TextAlign::Right => layout.width - advance_width,
            };
            let (_, metrics) = font.metrics();
            let line_height = font_size * 1.3;
            let ascent = -metrics.ascent;
            let descent = metrics.descent;
            let y = (line_height + ascent - descent) / 2.0;

            // Draw shadow
            if let Some(ref shadow) = self.style.text_shadow {
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
            if let Some(ref stroke) = self.style.stroke {
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
        let font_size = self.style.font_size_or(48.0);
        let font_family = self.style.font_family_or("Inter");
        let font_weight = self.style.font_weight_or(FontWeight::Normal);

        let fm = font_mgr();
        let skia_font_style = match font_weight {
            FontWeight::Bold => FontStyle::bold(),
            FontWeight::Normal => FontStyle::normal(),
            FontWeight::Weight(w) => FontStyle::new(skia_safe::font_style::Weight::from(w as i32), skia_safe::font_style::Width::NORMAL, skia_safe::font_style::Slant::Upright),
        };
        let typeface = fm
            .match_family_style(font_family, skia_font_style)
            .or_else(|| fm.match_family_style("Helvetica", skia_font_style))
            .or_else(|| fm.match_family_style("Arial", skia_font_style))
            .unwrap_or_else(|| fm.match_family_style("sans-serif", skia_font_style).unwrap());
        let font = Font::from_typeface(typeface, font_size);
        let display = format_counter_value(self.to, self.decimals, &self.separator, &self.prefix, &self.suffix);
        let (text_width, _) = font.measure_str(&display, None);
        let line_height = font_size * 1.3;
        (text_width, line_height)
    }
}
