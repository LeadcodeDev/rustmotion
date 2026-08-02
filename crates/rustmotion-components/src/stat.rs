use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Color, ColorType, ImageInfo, Paint, PaintStyle, Path, Point, Rect};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    asset_cache, draw_text_with_fallback, emoji_typeface, fetch_icon_svg,
    measure_text_with_fallback, paint_from_hex, parse_hex_color, typeface_with_fallback,
};
use rustmotion_core::schema::TimelineStep;
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
#[derive(Default)]
pub enum TrendDirection {
    Up,
    Down,
    #[default]
    Neutral,
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
        if w <= 0.0 || h <= 0.0 {
            return;
        }

        // Hard containment: whatever the math below computes, nothing gets
        // to paint past the box the layout gave this component (issue
        // #127). `stat` used to walk a `y_cursor` from a hardcoded
        // `pad = 20.0` — label, then value, then a sparkline with a forced
        // `.max(20.0)` minimum height — never once consulting `h`. A short
        // box (the audit measured 512×48) still got the same ~120px stack
        // every time, overflowing 70+px onto the next flex sibling. The
        // clip below is the backstop; the scaling further down is what
        // keeps it from ever needing to bite in practice.
        canvas.save();
        canvas.clip_rect(
            Rect::from_xywh(0.0, 0.0, w, h),
            skia_safe::ClipOp::Intersect,
            true,
        );

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

        // `pad` scales down with the box instead of a fixed 20px, and the
        // label/value font sizes are scaled by `content_scale` so the
        // label+value stack actually fits in the room `h` gives them. Both
        // stay at their authored size (pad=20, scale=1) whenever the box is
        // tall enough — which is every existing example — and only shrink
        // for a box shorter than the natural stack (like #127's 512×48).
        let pad = (h * 0.15).clamp(2.0, 20.0).min(w * 0.15).max(2.0);
        let content_h = (h - pad * 2.0).max(0.0);

        let label_natural_h = if self.label.is_some() {
            self.label_font_size * 1.5
        } else {
            0.0
        };
        let value_natural_h = self.value_font_size * 1.2;
        let text_natural_h = label_natural_h + value_natural_h;
        let content_scale = if text_natural_h > 0.0 {
            (content_h / text_natural_h).clamp(0.05, 1.0)
        } else {
            1.0
        };

        let eff_label_fs = self.label_font_size * content_scale;
        let eff_value_fs = self.value_font_size * content_scale;

        let mut y_cursor = pad;

        // Label (top)
        if let Some(label) = &self.label {
            let font_style = skia_safe::FontStyle::normal();
            let Ok(typeface) = typeface_with_fallback("Inter", font_style) else {
                canvas.restore();
                return;
            };
            let font = skia_safe::Font::from_typeface(typeface, eff_label_fs);
            let emoji_font =
                emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, eff_label_fs));
            let (_, metrics) = font.metrics();

            let mut label_paint = paint_from_hex(&self.label_color);
            label_paint.set_anti_alias(true);

            let ly = y_cursor + (-metrics.ascent);
            draw_text_with_fallback(
                canvas,
                label,
                &font,
                &emoji_font,
                0.0,
                pad,
                ly,
                &label_paint,
            );
            y_cursor += eff_label_fs * 1.5;
        }

        // Value (large)
        {
            let font_style = skia_safe::FontStyle::bold();
            let Ok(typeface) = typeface_with_fallback("Inter", font_style) else {
                canvas.restore();
                return;
            };
            let font = skia_safe::Font::from_typeface(typeface, eff_value_fs);
            let emoji_font =
                emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, eff_value_fs));
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
                let val_w = measure_text_with_fallback(&self.value, &font, &emoji_font, 0.0);
                let trend_fs = eff_value_fs * 0.4;
                let bold_style = skia_safe::FontStyle::bold();
                let Ok(trend_typeface) = typeface_with_fallback("Inter", bold_style) else {
                    canvas.restore();
                    return;
                };
                let trend_font = skia_safe::Font::from_typeface(trend_typeface, trend_fs);
                let trend_emoji =
                    emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, trend_fs));

                let trend_color = trend.color.as_deref().unwrap_or(match trend.direction {
                    TrendDirection::Up => "#22C55E",
                    TrendDirection::Down => "#EF4444",
                    TrendDirection::Neutral => "#94A3B8",
                });

                let mut trend_paint = paint_from_hex(trend_color);
                trend_paint.set_anti_alias(true);

                let (_, trend_metrics) = trend_font.metrics();
                let mut tx = pad + val_w + 12.0;
                let ty = vy - eff_value_fs * 0.15 + trend_metrics.ascent * 0.2;

                // Draw trend arrow icon
                let icon_id = match trend.direction {
                    TrendDirection::Up => Some("lucide:trending-up"),
                    TrendDirection::Down => Some("lucide:trending-down"),
                    TrendDirection::Neutral => None,
                };

                if let Some(icon_id) = icon_id {
                    let icon_sz = (trend_fs * 1.0).round() as u32;
                    let cache_key = format!(
                        "stat_icon:{}:{}:{}x{}",
                        icon_id, trend_color, icon_sz, icon_sz
                    );
                    let cache = asset_cache();

                    let icon_img = if let Some(cached) = cache.get(&cache_key) {
                        Some(cached.clone())
                    } else if let Ok(svg_data) =
                        fetch_icon_svg(icon_id, trend_color, icon_sz, icon_sz)
                    {
                        let opt = usvg::Options::default();
                        if let Ok(tree) = usvg::Tree::from_data(&svg_data, &opt) {
                            let svg_size = tree.size();
                            if let Some(mut pixmap) = tiny_skia::Pixmap::new(icon_sz, icon_sz) {
                                let sx = icon_sz as f32 / svg_size.width();
                                let sy = icon_sz as f32 / svg_size.height();
                                resvg::render(
                                    &tree,
                                    tiny_skia::Transform::from_scale(sx, sy),
                                    &mut pixmap.as_mut(),
                                );
                                let img_data = skia_safe::Data::new_copy(pixmap.data());
                                let info = ImageInfo::new(
                                    (icon_sz as i32, icon_sz as i32),
                                    ColorType::RGBA8888,
                                    skia_safe::AlphaType::Premul,
                                    None,
                                );
                                if let Some(decoded) = skia_safe::images::raster_from_data(
                                    &info,
                                    img_data,
                                    icon_sz as usize * 4,
                                ) {
                                    cache.insert(cache_key, decoded.clone());
                                    Some(decoded)
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };

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

            y_cursor += eff_value_fs * 1.2;
        }

        // Sparkline (bottom) — only drawn if room is actually left. The
        // previous `.max(20.0)` forced a sparkline into existence even when
        // the label+value stack had already consumed the whole box, which
        // is exactly what pushed ink onto the next flex sibling. `spark_y`
        // itself eats 4px of gap below the value line, so that has to come
        // out of the room budget too — the original formula measured room
        // from `y_cursor`, not from `spark_y`, which let the sparkline's
        // own bottom edge land 4px past `h - pad`.
        let spark_y = y_cursor + 4.0;
        let spark_room = (h - pad) - spark_y;
        if self.sparkline_data.len() >= 2 && spark_room >= 8.0 {
            let spark_h = spark_room;
            let spark_w = w - pad * 2.0;

            let max_v = self.sparkline_data.iter().fold(f64::MIN, |a, &b| a.max(b));
            let min_v = self.sparkline_data.iter().fold(f64::MAX, |a, &b| a.min(b));
            let range = (max_v - min_v).max(0.001);
            let n = self.sparkline_data.len();

            let spark_color = self.sparkline_color.as_deref().unwrap_or("#3B82F6");

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
                (Point::new(0.0, spark_y), Point::new(0.0, spark_y + spark_h)),
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

        canvas.restore();
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

#[cfg(test)]
mod tests {
    use super::*;

    fn full_stat() -> Stat {
        Stat {
            value: "98.2%".to_string(),
            label: Some("Tests Passing".to_string()),
            trend: Some(StatTrend {
                value: "+2.1%".to_string(),
                direction: TrendDirection::Up,
                color: None,
            }),
            sparkline_data: vec![80.0, 84.0, 82.0, 88.0, 90.0, 94.0, 98.0],
            sparkline_color: None,
            value_font_size: default_value_font_size(),
            label_font_size: default_label_font_size(),
            value_color: default_value_color(),
            label_color: default_label_color(),
            timing: Default::default(),
            style: CssStyle::default(),
            timeline: Vec::new(),
            stagger: None,
        }
    }

    /// Bounding box (min_x, max_x, min_y, max_y) of every non-transparent
    /// pixel on the surface, or `None` if nothing was painted. Mirrors the
    /// helper `caption.rs` already uses for the same kind of proof.
    fn ink_bounds(
        surface: &mut skia_safe::Surface,
        w: i32,
        h: i32,
    ) -> Option<(i32, i32, i32, i32)> {
        let snapshot = surface.image_snapshot();
        let info = skia_safe::ImageInfo::new(
            (w, h),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let ok = snapshot.read_pixels(
            &info,
            &mut buf,
            (w * 4) as usize,
            skia_safe::IPoint::new(0, 0),
            skia_safe::image::CachingHint::Disallow,
        );
        assert!(ok, "pixel read should succeed");
        let (mut minx, mut maxx, mut miny, mut maxy) = (i32::MAX, i32::MIN, i32::MAX, i32::MIN);
        for y in 0..h {
            for x in 0..w {
                if buf[((y * w + x) * 4 + 3) as usize] > 0 {
                    minx = minx.min(x);
                    maxx = maxx.max(x);
                    miny = miny.min(y);
                    maxy = maxy.max(y);
                }
            }
        }
        (minx <= maxx).then_some((minx, maxx, miny, maxy))
    }

    #[test]
    fn ink_stays_within_a_tiny_assigned_box() {
        // #127's exact repro: a 512×48 box (the measured "assigned box" for
        // this component in a 560×300 card) used to take ~120px of ink —
        // label + value + a forced-minimum sparkline — 72px onto whatever
        // sits below it in the flex column. With a solid background (the
        // same way the audit made the assigned box visible), the painted
        // background rect *is* the box, so no ink should land outside it.
        let stat = full_stat();
        const W: i32 = 512;
        const H: i32 = 48;
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        {
            let canvas = surface.canvas();
            stat.paint(canvas, W as f32, H as f32);
        }
        let (minx, maxx, miny, maxy) =
            ink_bounds(&mut surface, W, H).expect("stat must paint something");
        assert!(
            minx >= 0 && miny >= 0,
            "ink starts outside the box: ({minx},{miny})"
        );
        assert!(
            maxx < W && maxy < H,
            "ink escaped the {W}x{H} box: max=({maxx},{maxy})"
        );
    }

    #[test]
    fn ink_stays_within_box_even_without_a_background() {
        // Same box, no `style.background` — the clip still has to hold
        // even when there's no filled rect to visually anchor it to.
        let mut stat = full_stat();
        stat.style = CssStyle::default();
        const W: i32 = 512;
        const H: i32 = 48;
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        {
            let canvas = surface.canvas();
            stat.paint(canvas, W as f32, H as f32);
        }
        if let Some((minx, maxx, miny, maxy)) = ink_bounds(&mut surface, W, H) {
            assert!(minx >= 0 && miny >= 0 && maxx < W && maxy < H);
        }
    }

    #[test]
    fn generous_box_keeps_the_original_full_size_layout() {
        // A box as tall as the existing examples use (e.g. mega-showcase's
        // 380×220 stat cards) must not be affected by the new scaling —
        // `content_scale` should resolve to 1.0 and the value should still
        // render at its authored `value_font_size`.
        let stat = full_stat();
        const W: i32 = 380;
        const H: i32 = 220;
        let pad = (H as f32 * 0.15)
            .clamp(2.0, 20.0)
            .min(W as f32 * 0.15)
            .max(2.0);
        let content_h = (H as f32 - pad * 2.0).max(0.0);
        let text_natural_h = stat.label_font_size * 1.5 + stat.value_font_size * 1.2;
        let scale = (content_h / text_natural_h).clamp(0.05, 1.0);
        assert_eq!(
            scale, 1.0,
            "a generously sized box should never shrink the text"
        );
    }
}
