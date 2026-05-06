use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, ColorType, ImageInfo, Paint, PaintStyle, RRect, Rect};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{asset_cache, fetch_icon_svg, font_mgr, paint_from_hex, emoji_typeface, draw_text_with_fallback, measure_text_with_fallback};
use rustmotion_core::error::RustmotionError;
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BadgeVariant {
    Solid,
    Outline,
}

impl Default for BadgeVariant {
    fn default() -> Self {
        Self::Solid
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BadgeSize {
    Sm,
    Md,
    Lg,
}

impl Default for BadgeSize {
    fn default() -> Self {
        Self::Md
    }
}

impl BadgeSize {
    fn params(&self) -> (f32, f32, f32, f32) {
        // (font_size, h_padding, v_padding, icon_size)
        match self {
            BadgeSize::Sm => (12.0, 8.0, 4.0, 14.0),
            BadgeSize::Md => (14.0, 12.0, 6.0, 18.0),
            BadgeSize::Lg => (18.0, 16.0, 8.0, 22.0),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Badge {
    pub text: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub variant: BadgeVariant,
    #[serde(default)]
    pub badge_size: BadgeSize,
    /// Show a small colored dot indicator.
    #[serde(default)]
    pub dot: bool,
    /// Dot color (defaults to the badge background color).
    #[serde(default)]
    pub dot_color: Option<String>,
    /// Animate the dot with a pulse effect.
    #[serde(default)]
    pub pulse: bool,
    /// Show a numeric count badge (e.g. notification count).
    #[serde(default)]
    pub count: Option<u32>,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(Badge {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Badge {
    fn resolved_font_size(&self) -> f32 {
        self.style
            .font_size_px_or(self.badge_size.params().0)
    }

    /// Returns (h_padding, v_padding, icon_size) scaled proportionally
    /// to the resolved font size. If style.font_size overrides the default,
    /// padding and icon scale with it.
    fn resolved_params(&self) -> (f32, f32, f32) {
        let (default_fs, h_pad, v_pad, icon_size) = self.badge_size.params();
        let actual_fs = self.resolved_font_size();
        let ratio = actual_fs / default_fs;
        (h_pad * ratio, v_pad * ratio, icon_size * ratio)
    }

    fn make_font(&self) -> skia_safe::Font {
        let fm = font_mgr();
        let font_style = skia_safe::FontStyle::normal();
        let family = self.style.font_family.as_deref().unwrap_or("Inter");

        let typeface = fm
            .match_family_style(family, font_style)
            .or_else(|| fm.match_family_style("Helvetica", font_style))
            .or_else(|| fm.match_family_style("Arial", font_style))
            .or_else(|| fm.match_family_style("sans-serif", font_style))
            .or_else(|| fm.legacy_make_typeface(None, font_style))
            .expect(&RustmotionError::FontNotFound.to_string());

        skia_safe::Font::from_typeface(typeface, self.resolved_font_size())
    }

}

impl Badge {
    fn paint(&self, canvas: &Canvas, layout_w: f32, layout_h: f32, time: f64) {
        let color = self.style.background_color_str().unwrap_or("#3B82F6");
        let (h_pad, _v_pad, icon_size) = self.resolved_params();

        let w = layout_w;
        let h = layout_h;
        let radius = h / 2.0;

        // Background / outline
        let rect = Rect::from_xywh(0.0, 0.0, w, h);
        let rrect = RRect::new_rect_xy(rect, radius, radius);

        let mut bg_paint = paint_from_hex(color);
        bg_paint.set_anti_alias(true);

        match self.variant {
            BadgeVariant::Solid => {
                bg_paint.set_style(PaintStyle::Fill);
                canvas.draw_rrect(rrect, &bg_paint);
            }
            BadgeVariant::Outline => {
                bg_paint.set_style(PaintStyle::Stroke);
                bg_paint.set_stroke_width(1.5);
                canvas.draw_rrect(rrect, &bg_paint);
            }
        }

        // Icon
        let mut x_offset = h_pad;
        if let Some(icon_id) = &self.icon {
            let icon_color = if matches!(self.variant, BadgeVariant::Solid) {
                "#FFFFFF"
            } else {
                color
            };

            let icon_w = icon_size.round() as u32;
            let icon_h = icon_size.round() as u32;
            let cache_key = format!("icon:{}:{}:{}x{}", icon_id, icon_color, icon_w, icon_h);

            let cache = asset_cache();
            let img = if let Some(cached) = cache.get(&cache_key) {
                cached.clone()
            } else if let Ok(svg_data) = fetch_icon_svg(icon_id, icon_color, icon_w, icon_h) {
                let opt = usvg::Options::default();
                if let Ok(tree) = usvg::Tree::from_data(&svg_data, &opt) {
                    let svg_size = tree.size();
                    if let Some(mut pixmap) = tiny_skia::Pixmap::new(icon_w, icon_h) {
                        let sx = icon_w as f32 / svg_size.width();
                        let sy = icon_h as f32 / svg_size.height();
                        resvg::render(
                            &tree,
                            tiny_skia::Transform::from_scale(sx, sy),
                            &mut pixmap.as_mut(),
                        );
                        let img_data = skia_safe::Data::new_copy(pixmap.data());
                        let info = ImageInfo::new(
                            (icon_w as i32, icon_h as i32),
                            ColorType::RGBA8888,
                            skia_safe::AlphaType::Premul,
                            None,
                        );
                        if let Some(decoded) = skia_safe::images::raster_from_data(
                            &info,
                            img_data,
                            icon_w as usize * 4,
                        ) {
                            cache.insert(cache_key, decoded.clone());
                            decoded
                        } else {
                            return;
                        }
                    } else {
                        return;
                    }
                } else {
                    return;
                }
            } else {
                return;
            };

            let icon_y = (h - icon_size) / 2.0;
            let dst = Rect::from_xywh(x_offset, icon_y, icon_size, icon_size);
            canvas.draw_image_rect(img, None, dst, &Paint::default());

            let ratio = self.resolved_font_size() / self.badge_size.params().0;
            x_offset += icon_size + 6.0 * ratio;
        }

        // Text
        let text_color = if matches!(self.variant, BadgeVariant::Solid) {
            "#FFFFFF"
        } else {
            color
        };
        let font = self.make_font();
        let font_size = self.resolved_font_size();
        let emoji_font = emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, font_size));
        let mut text_paint = paint_from_hex(text_color);
        text_paint.set_anti_alias(true);

        let (_, metrics) = font.metrics();
        let ascent = -metrics.ascent;
        // Use cap height for visual centering (excludes descenders like g, p, y)
        let cap_h = if metrics.cap_height > 0.0 {
            metrics.cap_height
        } else {
            ascent * 0.7
        };
        let text_y = (h - cap_h) / 2.0 + cap_h;

        draw_text_with_fallback(canvas, &self.text, &font, &emoji_font, 0.0, x_offset, text_y, &text_paint);

        // Dot indicator (top-right)
        if self.dot {
            let dot_r = font_size * 0.3;
            let dot_cx = w - dot_r * 0.5;
            let dot_cy = dot_r * 0.5;
            let dot_color = self.dot_color.as_deref().unwrap_or(color);

            // Pulse ring animation
            if self.pulse {
                let phase = (time * 2.0).fract() as f32;
                let pulse_r = dot_r * (1.0 + phase * 1.5);
                let pulse_alpha = (1.0 - phase).max(0.0) * 0.5;
                let mut pulse_paint = paint_from_hex(dot_color);
                pulse_paint.set_style(PaintStyle::Fill);
                pulse_paint.set_anti_alias(true);
                pulse_paint.set_alpha_f(pulse_alpha);
                canvas.draw_circle((dot_cx, dot_cy), pulse_r, &pulse_paint);
            }

            let mut dot_paint = paint_from_hex(dot_color);
            dot_paint.set_style(PaintStyle::Fill);
            dot_paint.set_anti_alias(true);
            canvas.draw_circle((dot_cx, dot_cy), dot_r, &dot_paint);
        }

        // Count badge (top-right, outside bounds)
        if let Some(count) = self.count {
            let count_text = if count > 99 {
                "99+".to_string()
            } else {
                count.to_string()
            };

            let count_fs = font_size * 0.65;
            let fm = font_mgr();
            let count_typeface = fm
                .match_family_style("Inter", skia_safe::FontStyle::bold())
                .or_else(|| fm.match_family_style("Helvetica", skia_safe::FontStyle::bold()))
                .or_else(|| fm.match_family_style("Arial", skia_safe::FontStyle::bold()))
                .unwrap_or_else(|| fm.legacy_make_typeface(None, skia_safe::FontStyle::bold()).unwrap());
            let count_font = skia_safe::Font::from_typeface(count_typeface, count_fs);
            let count_emoji = emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, count_fs));

            let count_w = measure_text_with_fallback(&count_text, &count_font, &count_emoji, 0.0);
            let badge_pad = count_fs * 0.4;
            let badge_w = (count_w + badge_pad * 2.0).max(count_fs * 1.3);
            let badge_h = count_fs * 1.4;
            let badge_x = w - badge_w * 0.5;
            let badge_y = -badge_h * 0.3;

            // Red background pill
            let badge_rect = Rect::from_xywh(badge_x, badge_y, badge_w, badge_h);
            let badge_rrect = RRect::new_rect_xy(badge_rect, badge_h / 2.0, badge_h / 2.0);
            let mut count_bg = paint_from_hex("#EF4444");
            count_bg.set_style(PaintStyle::Fill);
            count_bg.set_anti_alias(true);
            canvas.draw_rrect(badge_rrect, &count_bg);

            // Count text
            let mut count_paint = paint_from_hex("#FFFFFF");
            count_paint.set_anti_alias(true);
            let (_, count_metrics) = count_font.metrics();
            let cx = badge_x + (badge_w - count_w) / 2.0;
            let cy = badge_y + (badge_h + (-count_metrics.ascent)) / 2.0;
            draw_text_with_fallback(canvas, &count_text, &count_font, &count_emoji, 0.0, cx, cy, &count_paint);
        }
    }
}

impl Painter for Badge {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        self.paint(canvas, layout.width, layout.height, ctx.time);
    }
}
