use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Paint, PaintStyle, Point};

use crate::engine::renderer::{color4f_from_hex, draw_shape_path, font_mgr, make_text_blob_with_spacing, paint_from_hex, wrap_text};
use crate::layout::{Constraints, LayoutNode};
use crate::schema::{Fill, GradientType, LayerStyle, ShapeText, ShapeType, Size, TextAlign, FontWeight};
use crate::traits::{AnimationConfig, RenderContext, TimingConfig, Widget};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Shape {
    pub shape: ShapeType,
    #[serde(default)]
    pub size: Size,
    #[serde(default)]
    pub text: Option<ShapeText>,
    #[serde(flatten)]
    pub animation: AnimationConfig,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

crate::impl_traits!(Shape {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Widget for Shape {
    fn render(&self, canvas: &Canvas, layout: &LayoutNode, _ctx: &RenderContext) -> Result<()> {
        let w = layout.width;
        let h = layout.height;
        let corner_radius = self.style.border_radius;

        // Fill
        if let Some(fill) = &self.style.fill {
            let mut paint = match fill {
                Fill::Solid(color) => paint_from_hex(color),
                Fill::Gradient(gradient) => {
                    let colors: Vec<skia_safe::Color4f> = gradient.colors.iter().map(|c| color4f_from_hex(c)).collect();
                    let stops: Option<Vec<f32>> = gradient.stops.clone();
                    let mut paint = Paint::default();
                    paint.set_anti_alias(true);

                    let shader = match gradient.gradient_type {
                        GradientType::Linear => {
                            let angle = gradient.angle.unwrap_or(0.0);
                            let rad = angle.to_radians();
                            let cx = w / 2.0;
                            let cy = h / 2.0;
                            let dx = (w / 2.0) * rad.cos();
                            let dy = (h / 2.0) * rad.sin();
                            let start = Point::new(cx - dx, cy - dy);
                            let end = Point::new(cx + dx, cy + dy);
                            skia_safe::shader::Shader::linear_gradient(
                                (start, end),
                                skia_safe::gradient_shader::GradientShaderColors::ColorsInSpace(
                                    &colors,
                                    Some(skia_safe::ColorSpace::new_srgb()),
                                ),
                                stops.as_deref(),
                                skia_safe::TileMode::Clamp,
                                None,
                                None,
                            )
                        }
                        GradientType::Radial => {
                            let center = Point::new(w / 2.0, h / 2.0);
                            let radius = w.max(h) / 2.0;
                            skia_safe::shader::Shader::radial_gradient(
                                center,
                                radius,
                                skia_safe::gradient_shader::GradientShaderColors::ColorsInSpace(
                                    &colors,
                                    Some(skia_safe::ColorSpace::new_srgb()),
                                ),
                                stops.as_deref(),
                                skia_safe::TileMode::Clamp,
                                None,
                                None,
                            )
                        }
                    };
                    if let Some(shader) = shader {
                        paint.set_shader(shader);
                    }
                    paint
                }
            };
            paint.set_style(PaintStyle::Fill);
            draw_shape_path(canvas, &self.shape, 0.0, 0.0, w, h, corner_radius, &paint);
        }

        // Stroke
        if let Some(stroke) = &self.style.stroke {
            let mut paint = paint_from_hex(&stroke.color);
            paint.set_style(PaintStyle::Stroke);
            paint.set_stroke_width(stroke.width);
            draw_shape_path(canvas, &self.shape, 0.0, 0.0, w, h, corner_radius, &paint);
        }

        // Text inside shape
        if let Some(text) = &self.text {
            render_shape_text(canvas, text, 0.0, 0.0, w, h)?;
        }

        Ok(())
    }

    fn measure(&self, _constraints: &Constraints) -> (f32, f32) {
        (self.size.width, self.size.height)
    }
}

fn render_shape_text(
    canvas: &Canvas,
    text: &ShapeText,
    shape_x: f32,
    shape_y: f32,
    shape_w: f32,
    shape_h: f32,
) -> Result<()> {
    use crate::schema::VerticalAlign;

    let pad = text.padding.unwrap_or(0.0);
    let area_x = shape_x + pad;
    let area_y = shape_y + pad;
    let area_w = shape_w - 2.0 * pad;
    let area_h = shape_h - 2.0 * pad;

    let fm = font_mgr();
    let font_style = match text.font_weight {
        FontWeight::Bold => skia_safe::FontStyle::bold(),
        FontWeight::Normal => skia_safe::FontStyle::normal(),
        FontWeight::Weight(w) => skia_safe::FontStyle::new(skia_safe::font_style::Weight::from(w as i32), skia_safe::font_style::Width::NORMAL, skia_safe::font_style::Slant::Upright),
    };

    let typeface = fm
        .match_family_style(&text.font_family, font_style)
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

    let font = skia_safe::Font::from_typeface(typeface, text.font_size);
    let (_strike_width, metrics) = font.metrics();
    let ascent = -metrics.ascent;
    let line_height = text.line_height.unwrap_or(text.font_size * 1.3);
    let letter_spacing = text.letter_spacing.unwrap_or(0.0);

    let lines = wrap_text(&text.content, &font, Some(area_w));
    let descent = metrics.descent;
    let total_h = if lines.len() > 1 {
        (lines.len() - 1) as f32 * line_height + ascent + descent
    } else {
        ascent + descent
    };

    let y_start = match text.vertical_align {
        VerticalAlign::Top => area_y + ascent,
        VerticalAlign::Middle => area_y + (area_h - total_h) / 2.0 + ascent,
        VerticalAlign::Bottom => area_y + area_h - total_h + ascent,
    };

    let mut paint = paint_from_hex(&text.color);
    paint.set_alpha_f(1.0);

    for (i, line) in lines.iter().enumerate() {
        if line.is_empty() {
            continue;
        }

        let blob = if letter_spacing.abs() > 0.01 {
            make_text_blob_with_spacing(line, &font, letter_spacing)
        } else {
            skia_safe::TextBlob::new(line, &font)
        };

        if let Some(blob) = blob {
            let blob_bounds = blob.bounds();
            let line_width = blob_bounds.width();

            let x = match text.align {
                TextAlign::Left => area_x - blob_bounds.left,
                TextAlign::Center => area_x + (area_w - line_width) / 2.0 - blob_bounds.left,
                TextAlign::Right => area_x + area_w - line_width - blob_bounds.left,
            };
            let y = y_start + i as f32 * line_height;
            canvas.draw_text_blob(&blob, (x, y), &paint);
        }
    }

    Ok(())
}
