use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, ColorType, ImageInfo, Paint, PaintStyle, RRect, Rect};

use crate::engine::renderer::{asset_cache, fetch_icon_svg, font_mgr, paint_from_hex, emoji_typeface, draw_text_with_fallback, measure_text_with_fallback};
use crate::error::RustmotionError;
use crate::layout::{Constraints, LayoutNode};
use crate::schema::LayerStyle;
use crate::traits::{RenderContext, TimingConfig, Widget};

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
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

crate::impl_traits!(Badge {
    Animatable => style,
    Timed => timing,
    Styled => style,
});

impl Badge {
    fn resolved_font_size(&self) -> f32 {
        self.style
            .font_size
            .unwrap_or_else(|| self.badge_size.params().0)
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
            .expect(&RustmotionError::FontNotFound.to_string());

        skia_safe::Font::from_typeface(typeface, self.resolved_font_size())
    }

    fn measure_content(&self) -> (f32, f32) {
        let (h_pad, v_pad, icon_size) = self.resolved_params();
        let font = self.make_font();
        let font_size = self.resolved_font_size();

        let emoji_font = emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, font_size));
        let text_width = measure_text_with_fallback(&self.text, &font, &emoji_font, 0.0);
        let ratio = self.resolved_font_size() / self.badge_size.params().0;
        let icon_gap = if self.icon.is_some() { 6.0 * ratio } else { 0.0 };
        let icon_w = if self.icon.is_some() { icon_size } else { 0.0 };

        let w = h_pad * 2.0 + text_width + icon_w + icon_gap;
        let h = v_pad * 2.0 + font_size * 1.3;

        (w, h)
    }
}

impl Widget for Badge {
    fn render(
        &self,
        canvas: &Canvas,
        layout: &LayoutNode,
        _ctx: &RenderContext,
        _props: &crate::engine::animator::AnimatedProperties,
    ) -> Result<()> {
        let color = self.style.background.as_deref().unwrap_or("#3B82F6");
        let (h_pad, _v_pad, icon_size) = self.resolved_params();

        let w = layout.width;
        let h = layout.height;
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
                            return Ok(());
                        }
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                }
            } else {
                return Ok(());
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

        Ok(())
    }

    fn measure(&self, _constraints: &Constraints) -> (f32, f32) {
        self.measure_content()
    }
}
