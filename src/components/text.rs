use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Font, FontStyle, PaintStyle, Rect, TextBlob};

use crate::engine::renderer::{font_mgr, make_text_blob_with_spacing, paint_from_hex, wrap_text};
use crate::layout::{Constraints, LayoutNode};
use crate::schema::{FontStyleType, FontWeight, LayerStyle, TextAlign};
use crate::traits::{AnimationConfig, RenderContext, TimingConfig, Widget};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Text {
    pub content: String,
    #[serde(default)]
    pub max_width: Option<f32>,
    // Composed behaviors
    #[serde(flatten)]
    pub animation: AnimationConfig,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

crate::impl_traits!(Text {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Widget for Text {
    fn render(&self, canvas: &Canvas, layout: &LayoutNode, _ctx: &RenderContext) -> Result<()> {
        let font_size = self.style.font_size_or(48.0);
        let color = self.style.color_or("#FFFFFF");
        let font_family = self.style.font_family_or("Inter");
        let font_weight = self.style.font_weight_or(FontWeight::Normal);
        let font_style_type = self.style.font_style_or(FontStyleType::Normal);
        let align = self.style.text_align_or(TextAlign::Left);
        let line_height_val = self.style.line_height.unwrap_or(font_size * 1.3);
        let letter_spacing = self.style.letter_spacing.unwrap_or(0.0);

        let fm = font_mgr();
        let slant = match font_style_type {
            FontStyleType::Normal => skia_safe::font_style::Slant::Upright,
            FontStyleType::Italic => skia_safe::font_style::Slant::Italic,
            FontStyleType::Oblique => skia_safe::font_style::Slant::Oblique,
        };
        let weight = match font_weight {
            FontWeight::Bold => skia_safe::font_style::Weight::BOLD,
            FontWeight::Normal => skia_safe::font_style::Weight::NORMAL,
        };
        let skia_font_style = FontStyle::new(weight, skia_safe::font_style::Width::NORMAL, slant);

        let typeface = fm
            .match_family_style(font_family, skia_font_style)
            .or_else(|| fm.match_family_style("Helvetica", skia_font_style))
            .or_else(|| fm.match_family_style("Arial", skia_font_style))
            .or_else(|| fm.match_family_style("sans-serif", skia_font_style))
            .or_else(|| {
                if fm.count_families() > 0 {
                    fm.match_family_style(&fm.family_name(0), skia_font_style)
                } else {
                    None
                }
            })
            .expect("No fonts available on this system");

        let font = Font::from_typeface(typeface, font_size);
        let mut paint = paint_from_hex(color);
        paint.set_alpha_f(1.0);

        // Use layout width as wrapping constraint, combined with max_width
        let wrap_width = if layout.width.is_finite() && layout.width > 0.0 {
            match self.max_width {
                Some(mw) => Some(mw.min(layout.width)),
                None => Some(layout.width),
            }
        } else {
            self.max_width
        };

        let lines = wrap_text(&self.content, &font, wrap_width);
        let (_, metrics) = font.metrics();
        let ascent = -metrics.ascent;
        let descent = metrics.descent;
        let baseline_offset = (line_height_val + ascent - descent) / 2.0;

        // Prepare optional shadow and stroke paints
        let shadow_paint = self.style.text_shadow.as_ref().map(|shadow| {
            let mut p = paint_from_hex(&shadow.color);
            if shadow.blur > 0.01 {
                if let Some(filter) = skia_safe::image_filters::blur(
                    (shadow.blur, shadow.blur),
                    skia_safe::TileMode::Clamp,
                    None,
                    None,
                ) {
                    p.set_image_filter(filter);
                }
            }
            (p, shadow.offset_x, shadow.offset_y)
        });

        let stroke_paint = self.style.stroke.as_ref().map(|stroke| {
            let mut p = paint_from_hex(&stroke.color);
            p.set_style(PaintStyle::Stroke);
            p.set_stroke_width(stroke.width);
            p
        });

        // Compute alignment width
        let align_width = if layout.width.is_finite() && layout.width > 0.0 {
            layout.width
        } else {
            let mut max_w = 0.0f32;
            for line in &lines {
                let (w, _) = font.measure_str(line, None);
                max_w = max_w.max(w);
            }
            max_w
        };

        for (i, line) in lines.iter().enumerate() {
            if line.is_empty() {
                continue;
            }

            let blob = if letter_spacing.abs() > 0.01 {
                make_text_blob_with_spacing(line, &font, letter_spacing)
            } else {
                TextBlob::new(line, &font)
            };

            if let Some(blob) = blob {
                let (advance_width, _) = font.measure_str(line, None);
                let advance_width = advance_width + letter_spacing * (line.chars().count() as f32 - 1.0).max(0.0);
                let blob_bounds = blob.bounds();
                let line_width = blob_bounds.width();

                let x = match align {
                    TextAlign::Left => 0.0,
                    TextAlign::Center => (align_width - advance_width) / 2.0,
                    TextAlign::Right => align_width - advance_width,
                };
                let y = i as f32 * line_height_val + baseline_offset;

                // Draw background highlight behind text
                if let Some(ref bg) = self.style.text_background {
                    let bg_paint = paint_from_hex(&bg.color);
                    let bg_rect = Rect::from_xywh(
                        x - bg.padding + blob_bounds.left,
                        y - font_size + blob_bounds.top.min(0.0) - bg.padding / 2.0,
                        line_width + bg.padding * 2.0,
                        line_height_val + bg.padding,
                    );
                    if bg.corner_radius > 0.0 {
                        let rrect = skia_safe::RRect::new_rect_xy(bg_rect, bg.corner_radius, bg.corner_radius);
                        canvas.draw_rrect(rrect, &bg_paint);
                    } else {
                        canvas.draw_rect(bg_rect, &bg_paint);
                    }
                }

                // Draw shadow
                if let Some((ref sp, ox, oy)) = shadow_paint {
                    canvas.draw_text_blob(&blob, (x + ox, y + oy), sp);
                }

                // Draw stroke (outline)
                if let Some(ref sp) = stroke_paint {
                    canvas.draw_text_blob(&blob, (x, y), sp);
                }

                // Draw fill
                canvas.draw_text_blob(&blob, (x, y), &paint);
            }
        }

        Ok(())
    }

    fn measure(&self, _constraints: &Constraints) -> (f32, f32) {
        let font_size = self.style.font_size_or(48.0);
        let font_family = self.style.font_family_or("Inter");
        let font_weight = self.style.font_weight_or(FontWeight::Normal);
        let font_style_type = self.style.font_style_or(FontStyleType::Normal);

        let fm = font_mgr();
        let slant = match font_style_type {
            FontStyleType::Normal => skia_safe::font_style::Slant::Upright,
            FontStyleType::Italic => skia_safe::font_style::Slant::Italic,
            FontStyleType::Oblique => skia_safe::font_style::Slant::Oblique,
        };
        let weight = match font_weight {
            FontWeight::Bold => skia_safe::font_style::Weight::BOLD,
            FontWeight::Normal => skia_safe::font_style::Weight::NORMAL,
        };
        let skia_font_style = FontStyle::new(weight, skia_safe::font_style::Width::NORMAL, slant);
        let typeface = fm
            .match_family_style(font_family, skia_font_style)
            .or_else(|| fm.match_family_style("Helvetica", skia_font_style))
            .or_else(|| fm.match_family_style("Arial", skia_font_style))
            .unwrap_or_else(|| fm.match_family_style("sans-serif", skia_font_style).unwrap());
        let font = Font::from_typeface(typeface, font_size);
        let lines = wrap_text(&self.content, &font, self.max_width);
        let line_height_val = self.style.line_height.unwrap_or(font_size * 1.3);
        let max_w = lines.iter().map(|l| {
            let (w, _) = font.measure_str(l, None);
            w
        }).fold(0.0f32, f32::max);
        let h = lines.len() as f32 * line_height_val;
        (max_w, h)
    }
}
