use rustmotion_core::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Font, FontStyle, PaintStyle};

use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{font_mgr, format_counter_value, paint_from_hex, emoji_typeface, draw_text_with_fallback, measure_text_with_fallback};
use rustmotion_core::error::RustmotionError;
use rustmotion_core::layout::{Constraints, LayoutNode};
use rustmotion_core::schema::{EasingType, FontStyleType, FontWeight, LayerStyle, TextAlign};
use rustmotion_core::traits::{PaintCtx, Painter, RenderContext, TimingConfig, Widget};

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
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

rustmotion_core::impl_traits!(Counter {
    Animatable => style,
    Timed => timing,
    Styled => style,
});

impl Counter {
    fn paint(&self, canvas: &Canvas, layout_width: f32, time: f64, scene_duration: f64) -> Result<()> {
        use rustmotion_core::engine::animator::ease;

        let font_size = self.style.font_size_or(48.0);
        let color = self.style.color_or("#FFFFFF");
        let font_family = self.style.font_family_or("Inter");
        let font_weight = self.style.font_weight_or(FontWeight::Normal);
        let font_style_type = self.style.font_style_or(FontStyleType::Normal);
        let align = self.style.text_align_or(TextAlign::Left);

        let start = self.timing.start_at.unwrap_or(0.0);
        let elapsed = (time - start).max(0.0);
        let remaining_duration = scene_duration - start;
        let t = if remaining_duration > 0.0 {
            (elapsed / remaining_duration).clamp(0.0, 1.0)
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
            .ok_or(RustmotionError::FontNotFound)?;

        let font = Font::from_typeface(typeface, font_size);
        let emoji_font = emoji_typeface().map(|tf| Font::from_typeface(tf, font_size));
        let mut paint = paint_from_hex(color);
        paint.set_alpha_f(1.0);

        let letter_spacing = self.style.letter_spacing.unwrap_or(0.0);

        let advance_width = measure_text_with_fallback(&content, &font, &emoji_font, letter_spacing);

        // For center/right alignment, anchor positioning on the same `absmax`
        // width that `measure()` reserved. This keeps the right edge (or
        // bounding box midpoint) of the counter stable across frames instead
        // of letting it shift sub-pixel as the digit count changes.
        let stable_width = if matches!(align, TextAlign::Center | TextAlign::Right) {
            let absmax = self.from.abs().max(self.to.abs());
            let signed = if self.from < 0.0 || self.to < 0.0 { -absmax } else { absmax };
            let display = format_counter_value(signed, self.decimals, &self.separator, &self.prefix, &self.suffix);
            measure_text_with_fallback(&display, &font, &emoji_font, letter_spacing)
        } else {
            advance_width
        };

        let raw_x = match align {
            TextAlign::Left => 0.0,
            TextAlign::Center => (layout_width - stable_width) / 2.0
                + (stable_width - advance_width) / 2.0,
            TextAlign::Right => layout_width - advance_width,
        };
        // Snap to whole pixels to eliminate the sub-pixel jitter that the
        // glyph rasterizer would otherwise introduce on a moving counter.
        let x = raw_x.round();
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
            draw_text_with_fallback(canvas, &content, &font, &emoji_font, letter_spacing, x + shadow.offset_x, y + shadow.offset_y, &sp);
        }

        // Draw stroke
        if let Some(ref stroke) = self.style.stroke {
            let mut sp = paint_from_hex(&stroke.color);
            sp.set_style(PaintStyle::Stroke);
            sp.set_stroke_width(stroke.width);
            draw_text_with_fallback(canvas, &content, &font, &emoji_font, letter_spacing, x, y, &sp);
        }

        draw_text_with_fallback(canvas, &content, &font, &emoji_font, letter_spacing, x, y, &paint);

        Ok(())
    }
}

impl Widget for Counter {
    fn render(&self, canvas: &Canvas, layout: &LayoutNode, ctx: &RenderContext, _props: &AnimatedProperties, _pipeline: &dyn rustmotion_core::traits::RenderPipeline) -> Result<()> {
        self.paint(canvas, layout.width, ctx.time, ctx.scene_duration as f64)
    }

    fn measure(&self, constraints: &Constraints) -> (f32, f32) {
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
            .or_else(|| fm.match_family_style("sans-serif", skia_font_style))
            .unwrap_or_else(|| fm.legacy_make_typeface(None, skia_font_style).expect("No fallback font"));
        let font = Font::from_typeface(typeface, font_size);
        let emoji_font = emoji_typeface().map(|tf| Font::from_typeface(tf, font_size));
        // Reserve space for the largest absolute value the counter will display
        // (between `from` and `to`) so layout never reflows during animation.
        let absmax = self.from.abs().max(self.to.abs());
        let signed = if self.from < 0.0 || self.to < 0.0 { -absmax } else { absmax };
        let display = format_counter_value(signed, self.decimals, &self.separator, &self.prefix, &self.suffix);
        let text_width = measure_text_with_fallback(&display, &font, &emoji_font, 0.0);
        let line_height = font_size * 1.3;
        // Counter is atomic: it does not wrap. We still constrain so the layout
        // engine never assigns more than the parent allows; the geometry
        // validator will detect the natural-size overflow separately.
        constraints.constrain(text_width, line_height)
    }
}

impl Painter for Counter {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        let _ = self.paint(canvas, layout.width, ctx.time, ctx.scene_duration as f64);
    }
}
