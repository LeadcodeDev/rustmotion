use rustmotion_core::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Paint, PaintStyle, Point};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    build_shape_path, color4f_from_hex, draw_shape_path, draw_text_with_fallback, emoji_typeface,
    font_mgr, measure_text_with_fallback, paint_from_hex, wrap_text_with_fallback,
};
use rustmotion_core::error::RustmotionError;
use rustmotion_core::schema::{
    Fill, FontWeight, GradientType, ShapeText, ShapeType, Stroke, TextAlign, TimelineStep,
};
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Shape {
    pub shape: ShapeType,
    #[serde(default)]
    pub text: Option<ShapeText>,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
    #[serde(default)]
    pub fill: Option<Fill>,
    #[serde(default)]
    pub stroke: Option<Stroke>,
}

rustmotion_core::impl_traits!(Shape {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Painter for Shape {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        props: &AnimatedProperties,
        _ctx: &PaintCtx,
    ) {
        let w = layout.width;
        let h = layout.height;
        let corner_radius = self.style.border_radius_px();

        if let Some(fill) = &self.fill {
            let mut paint = match fill {
                Fill::Solid(color) => paint_from_hex(color),
                Fill::Gradient(gradient) => {
                    let colors: Vec<skia_safe::Color4f> = gradient
                        .colors
                        .iter()
                        .map(|c| color4f_from_hex(c))
                        .collect();
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
                        paint.set_dither(true);
                    }
                    paint
                }
            };
            paint.set_style(PaintStyle::Fill);
            draw_shape_path(canvas, &self.shape, 0.0, 0.0, w, h, corner_radius, &paint);
        }

        if let Some(stroke) = &self.stroke {
            let mut paint = paint_from_hex(&stroke.color);
            paint.set_style(PaintStyle::Stroke);
            let stroke_w = if props.stroke_width >= 0.0 {
                props.stroke_width
            } else {
                stroke.width
            };
            paint.set_stroke_width(stroke_w);

            if props.draw_progress >= 0.0 && props.draw_progress < 1.0 {
                if let Some(path) = build_shape_path(&self.shape, 0.0, 0.0, w, h, corner_radius) {
                    let mut measure = skia_safe::PathMeasure::new(&path, false, None);
                    let path_len = measure.length();
                    if path_len > 0.0 {
                        let draw_len = path_len * props.draw_progress.clamp(0.0, 1.0);
                        let intervals = [draw_len, path_len - draw_len + 0.01];
                        if let Some(dash) = skia_safe::PathEffect::dash(&intervals, 0.0) {
                            paint.set_path_effect(dash);
                        }
                    }
                }
            }

            draw_shape_path(canvas, &self.shape, 0.0, 0.0, w, h, corner_radius, &paint);
        }

        if let Some(text) = &self.text {
            let _ = render_shape_text(canvas, text, 0.0, 0.0, w, h);
        }
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
    use rustmotion_core::schema::VerticalAlign;

    let pad = text.padding.unwrap_or(0.0);
    let area_x = shape_x + pad;
    let area_y = shape_y + pad;
    let area_w = shape_w - 2.0 * pad;
    let area_h = shape_h - 2.0 * pad;

    let fm = font_mgr();
    let font_style = match text.font_weight {
        FontWeight::Bold => skia_safe::FontStyle::bold(),
        FontWeight::Normal => skia_safe::FontStyle::normal(),
        FontWeight::Weight(w) => skia_safe::FontStyle::new(
            skia_safe::font_style::Weight::from(w as i32),
            skia_safe::font_style::Width::NORMAL,
            skia_safe::font_style::Slant::Upright,
        ),
    };

    let typeface = fm
        .match_family_style(&text.font_family, font_style)
        .or_else(|| fm.match_family_style("Helvetica", font_style))
        .or_else(|| fm.match_family_style("Arial", font_style))
        .or_else(|| fm.match_family_style("sans-serif", font_style))
        .or_else(|| {
            if fm.count_families() > 0 {
                fm.match_family_style(fm.family_name(0), font_style)
            } else {
                None
            }
        })
        .ok_or(RustmotionError::FontNotFound)?;

    let font = skia_safe::Font::from_typeface(typeface, text.font_size);
    let emoji_font = emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, text.font_size));
    let (_strike_width, metrics) = font.metrics();
    let ascent = -metrics.ascent;
    let line_height = match text.line_height {
        Some(v) if v <= 10.0 => text.font_size * v,
        Some(v) => v,
        None => text.font_size * 1.3,
    };
    let letter_spacing = text.letter_spacing.unwrap_or(0.0);

    let lines = wrap_text_with_fallback(&text.content, &font, &emoji_font, Some(area_w));
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

        let line_width = measure_text_with_fallback(line, &font, &emoji_font, letter_spacing);

        let x = match text.align {
            TextAlign::Left => area_x,
            TextAlign::Center => area_x + (area_w - line_width) / 2.0,
            TextAlign::Right => area_x + area_w - line_width,
        };
        let y = y_start + i as f32 * line_height;
        draw_text_with_fallback(
            canvas,
            line,
            &font,
            &emoji_font,
            letter_spacing,
            x,
            y,
            &paint,
        );
    }

    Ok(())
}
