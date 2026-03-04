use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Font, FontStyle, PaintStyle, Rect, TextBlob};

use crate::engine::renderer::{font_mgr, make_text_blob_with_spacing, paint_from_hex, wrap_text};
use crate::layout::{Constraints, LayoutNode};
use crate::schema::{FontStyleType, FontWeight, Stroke, TextAlign, TextBackground, TextShadow};
use crate::traits::{AnimationConfig, RenderContext, StyleConfig, TimingConfig, Widget};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Text {
    pub content: String,
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
    pub max_width: Option<f32>,
    #[serde(default)]
    pub line_height: Option<f32>,
    #[serde(default)]
    pub letter_spacing: Option<f32>,
    #[serde(default)]
    pub shadow: Option<TextShadow>,
    #[serde(default)]
    pub stroke: Option<Stroke>,
    #[serde(default)]
    pub background: Option<TextBackground>,
    // Composed behaviors
    #[serde(flatten)]
    pub animation: AnimationConfig,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(flatten)]
    pub style: StyleConfig,
}

crate::impl_traits!(Text {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Widget for Text {
    fn render(&self, canvas: &Canvas, layout: &LayoutNode, _ctx: &RenderContext) -> Result<()> {
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
            .or_else(|| {
                if fm.count_families() > 0 {
                    fm.match_family_style(&fm.family_name(0), font_style)
                } else {
                    None
                }
            })
            .expect("No fonts available on this system");

        let font = Font::from_typeface(typeface, self.font_size);
        let mut paint = paint_from_hex(&self.color);
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
        let line_height = self.line_height.unwrap_or(self.font_size * 1.3);
        let letter_spacing = self.letter_spacing.unwrap_or(0.0);
        // Center text visually within each line_height box.
        // draw_text_blob uses y as baseline. We compute the baseline offset that
        // centers the glyph bounding box (ascent+descent) within line_height.
        let (_, metrics) = font.metrics();
        let ascent = -metrics.ascent; // positive
        let descent = metrics.descent; // positive
        let baseline_offset = (line_height + ascent - descent) / 2.0;

        // Prepare optional shadow and stroke paints
        let shadow_paint = self.shadow.as_ref().map(|shadow| {
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

        let stroke_paint = self.stroke.as_ref().map(|stroke| {
            let mut p = paint_from_hex(&stroke.color);
            p.set_style(PaintStyle::Stroke);
            p.set_stroke_width(stroke.width);
            p
        });

        // Compute alignment width: use layout width if bounded, else use max line width
        let align_width = if layout.width.is_finite() && layout.width > 0.0 {
            layout.width
        } else {
            // Compute max line width for self-centering
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

                let x = match self.align {
                    TextAlign::Left => 0.0,
                    TextAlign::Center => (align_width - advance_width) / 2.0,
                    TextAlign::Right => align_width - advance_width,
                };
                let y = i as f32 * line_height + baseline_offset;

                // Draw background highlight behind text
                if let Some(ref bg) = self.background {
                    let bg_paint = paint_from_hex(&bg.color);
                    let bg_rect = Rect::from_xywh(
                        x - bg.padding + blob_bounds.left,
                        y - self.font_size + blob_bounds.top.min(0.0) - bg.padding / 2.0,
                        line_width + bg.padding * 2.0,
                        line_height + bg.padding,
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
            .unwrap_or_else(|| fm.match_family_style("sans-serif", font_style).unwrap());
        let font = Font::from_typeface(typeface, self.font_size);
        let lines = wrap_text(&self.content, &font, self.max_width);
        let line_height = self.line_height.unwrap_or(self.font_size * 1.3);
        let max_w = lines.iter().map(|l| {
            let (w, _) = font.measure_str(l, None);
            w
        }).fold(0.0f32, f32::max);
        let h = lines.len() as f32 * line_height;
        (max_w, h)
    }
}

fn default_font_size() -> f32 { 48.0 }
fn default_color() -> String { "#FFFFFF".to_string() }
fn default_font_family() -> String { "Inter".to_string() }
