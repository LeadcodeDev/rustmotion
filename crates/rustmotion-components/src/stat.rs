use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Color, ColorType, ImageInfo, Paint, PaintStyle, Path, Point, Rect};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    asset_cache, draw_text_with_fallback, emoji_typeface, fetch_icon_svg, font_mgr,
    measure_text_with_fallback, paint_from_hex, parse_hex_color,
};
use rustmotion_core::schema::{AnimationEffect, Size, TimelineStep};
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

fn default_value_font_size() -> f32 {
    48.0
}

fn default_label_font_size() -> f32 {
    14.0
}

fn default_value_color() -> String {
    "#FFFFFF".to_string()
}

fn default_label_color() -> String {
    "#94A3B8".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TrendDirection {
    Up,
    Down,
    Neutral,
}

impl Default for TrendDirection {
    fn default() -> Self {
        Self::Neutral
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StatTrend {
    pub value: String,
    #[serde(default)]
    pub direction: TrendDirection,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Stat {
    pub value: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub trend: Option<StatTrend>,
    #[serde(default)]
    pub sparkline_data: Vec<f64>,
    #[serde(default)]
    pub sparkline_color: Option<String>,
    #[serde(default)]
    pub size: Option<Size>,
    #[serde(default = "default_value_font_size")]
    pub value_font_size: f32,
    #[serde(default = "default_label_font_size")]
    pub label_font_size: f32,
    #[serde(default = "default_value_color")]
    pub value_color: String,
    #[serde(default = "default_label_color")]
    pub label_color: String,
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

rustmotion_core::impl_traits!(Stat {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Stat {
    fn paint(&self, canvas: &Canvas, layout_w: f32, layout_h: f32) {
        let w = layout_w;
        let h = layout_h;
        let fm = font_mgr();

        // Background if set
        if let Some(bg) = self.style.background_color_str() {
            let mut bg_paint = paint_from_hex(bg);
            bg_paint.set_style(PaintStyle::Fill);
            bg_paint.set_anti_alias(true);
            let radius = self.style.border_radius_px_or(12.0);
            let rect = Rect::from_xywh(0.0, 0.0, w, h);
            let rrect = skia_safe::RRect::new_rect_xy(rect, radius, radius);
            canvas.draw_rrect(rrect, &bg_paint);
        }

        let pad = 20.0;
        let mut y_cursor = pad;

        // Label (top)
        if let Some(label) = &self.label {
            let font_style = skia_safe::FontStyle::normal();
            let typeface = fm
                .match_family_style("Inter", font_style)
                .or_else(|| fm.match_family_style("Helvetica", font_style))
                .unwrap_or_else(|| fm.legacy_make_typeface(None, font_style).unwrap());
            let font = skia_safe::Font::from_typeface(typeface, self.label_font_size);
            let emoji_font =
                emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, self.label_font_size));
            let (_, metrics) = font.metrics();

            let mut label_paint = paint_from_hex(&self.label_color);
            label_paint.set_anti_alias(true);

            let ly = y_cursor + (-metrics.ascent);
            draw_text_with_fallback(
                canvas, label, &font, &emoji_font, 0.0, pad, ly, &label_paint,
            );
            y_cursor += self.label_font_size * 1.5;
        }

        // Value (large)
        {
            let font_style = skia_safe::FontStyle::bold();
            let typeface = fm
                .match_family_style("Inter", font_style)
                .or_else(|| fm.match_family_style("Helvetica", font_style))
                .unwrap_or_else(|| fm.legacy_make_typeface(None, font_style).unwrap());
            let font = skia_safe::Font::from_typeface(typeface, self.value_font_size);
            let emoji_font = emoji_typeface()
                .map(|tf| skia_safe::Font::from_typeface(tf, self.value_font_size));
            let (_, metrics) = font.metrics();

            let mut val_paint = paint_from_hex(&self.value_color);
            val_paint.set_anti_alias(true);

            let vy = y_cursor + (-metrics.ascent);
            draw_text_with_fallback(
                canvas,
                &self.value,
                &font,
                &emoji_font,
                0.0,
                pad,
                vy,
                &val_paint,
            );

            // Trend inline after value
            if let Some(trend) = &self.trend {
                let val_w =
                    measure_text_with_fallback(&self.value, &font, &emoji_font, 0.0);
                let trend_fs = self.value_font_size * 0.4;
                let bold_style = skia_safe::FontStyle::bold();
                let trend_typeface = fm
                    .match_family_style("Inter", bold_style)
                    .or_else(|| fm.match_family_style("Helvetica", bold_style))
                    .or_else(|| fm.match_family_style("Arial", bold_style))
                    .unwrap_or_else(|| fm.legacy_make_typeface(None, bold_style).expect("no font available"));
                let trend_font = skia_safe::Font::from_typeface(trend_typeface, trend_fs);
                let trend_emoji =
                    emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, trend_fs));

                let trend_color = trend.color.as_deref().unwrap_or_else(|| {
                    match trend.direction {
                        TrendDirection::Up => "#22C55E",
                        TrendDirection::Down => "#EF4444",
                        TrendDirection::Neutral => "#94A3B8",
                    }
                });

                let mut trend_paint = paint_from_hex(trend_color);
                trend_paint.set_anti_alias(true);

                let (_, trend_metrics) = trend_font.metrics();
                let mut tx = pad + val_w + 12.0;
                let ty = vy - self.value_font_size * 0.15 + trend_metrics.ascent * 0.2;

                // Draw trend arrow icon
                let icon_id = match trend.direction {
                    TrendDirection::Up => Some("lucide:trending-up"),
                    TrendDirection::Down => Some("lucide:trending-down"),
                    TrendDirection::Neutral => None,
                };

                if let Some(icon_id) = icon_id {
                    let icon_sz = (trend_fs * 1.0).round() as u32;
                    let cache_key = format!("stat_icon:{}:{}:{}x{}", icon_id, trend_color, icon_sz, icon_sz);
                    let cache = asset_cache();

                    let icon_img = if let Some(cached) = cache.get(&cache_key) {
                        Some(cached.clone())
                    } else if let Ok(svg_data) = fetch_icon_svg(icon_id, trend_color, icon_sz, icon_sz) {
                        let opt = usvg::Options::default();
                        if let Ok(tree) = usvg::Tree::from_data(&svg_data, &opt) {
                            let svg_size = tree.size();
                            if let Some(mut pixmap) = tiny_skia::Pixmap::new(icon_sz, icon_sz) {
                                let sx = icon_sz as f32 / svg_size.width();
                                let sy = icon_sz as f32 / svg_size.height();
                                resvg::render(&tree, tiny_skia::Transform::from_scale(sx, sy), &mut pixmap.as_mut());
                                let img_data = skia_safe::Data::new_copy(pixmap.data());
                                let info = ImageInfo::new(
                                    (icon_sz as i32, icon_sz as i32),
                                    ColorType::RGBA8888,
                                    skia_safe::AlphaType::Premul,
                                    None,
                                );
                                if let Some(decoded) = skia_safe::images::raster_from_data(&info, img_data, icon_sz as usize * 4) {
                                    cache.insert(cache_key, decoded.clone());
                                    Some(decoded)
                                } else { None }
                            } else { None }
                        } else { None }
                    } else { None };

                    if let Some(img) = icon_img {
                        let icon_y = ty - trend_fs * 0.8;
                        let dst = Rect::from_xywh(tx, icon_y, icon_sz as f32, icon_sz as f32);
                        canvas.draw_image_rect(img, None, dst, &Paint::default());
                        tx += icon_sz as f32 + 4.0;
                    }
                }

                draw_text_with_fallback(
                    canvas,
                    &trend.value,
                    &trend_font,
                    &trend_emoji,
                    0.0,
                    tx,
                    ty,
                    &trend_paint,
                );
            }

            y_cursor += self.value_font_size * 1.2;
        }

        // Sparkline (bottom)
        if self.sparkline_data.len() >= 2 {
            let spark_h = (h - y_cursor - pad).max(20.0);
            let spark_w = w - pad * 2.0;
            let spark_y = y_cursor + 4.0;

            let max_v = self.sparkline_data.iter().fold(f64::MIN, |a, &b| a.max(b));
            let min_v = self.sparkline_data.iter().fold(f64::MAX, |a, &b| a.min(b));
            let range = (max_v - min_v).max(0.001);
            let n = self.sparkline_data.len();

            let spark_color = self
                .sparkline_color
                .as_deref()
                .unwrap_or("#3B82F6");

            let mut line_path = Path::new();
            let mut fill_path = Path::new();

            for (i, &val) in self.sparkline_data.iter().enumerate() {
                let x = pad + (i as f32 / (n - 1) as f32) * spark_w;
                let y = spark_y + spark_h - ((val - min_v) / range) as f32 * spark_h;

                if i == 0 {
                    line_path.move_to((x, y));
                    fill_path.move_to((x, spark_y + spark_h));
                    fill_path.line_to((x, y));
                } else {
                    line_path.line_to((x, y));
                    fill_path.line_to((x, y));
                }
            }
            fill_path.line_to((pad + spark_w, spark_y + spark_h));
            fill_path.close();

            // Gradient fill
            let (r, g, b, _) = parse_hex_color(spark_color);
            let top_color = Color::from_argb(50, r, g, b);
            let bottom_color = Color::from_argb(0, r, g, b);
            let shader = skia_safe::shader::Shader::linear_gradient(
                (
                    Point::new(0.0, spark_y),
                    Point::new(0.0, spark_y + spark_h),
                ),
                skia_safe::gradient_shader::GradientShaderColors::Colors(&[
                    top_color,
                    bottom_color,
                ]),
                None,
                skia_safe::TileMode::Clamp,
                None,
                None,
            );
            if let Some(shader) = shader {
                let mut fp = skia_safe::Paint::default();
                fp.set_style(PaintStyle::Fill);
                fp.set_anti_alias(true);
                fp.set_shader(shader);
                canvas.draw_path(&fill_path, &fp);
            }

            let mut line_paint = paint_from_hex(spark_color);
            line_paint.set_style(PaintStyle::Stroke);
            line_paint.set_stroke_width(2.0);
            line_paint.set_anti_alias(true);
            line_paint.set_stroke_cap(skia_safe::paint::Cap::Round);
            line_paint.set_stroke_join(skia_safe::paint::Join::Round);
            canvas.draw_path(&line_path, &line_paint);
        }
    }
}

impl Painter for Stat {
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
