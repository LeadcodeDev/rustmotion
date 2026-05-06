use rustmotion_core::css::CssStyle;
use rustmotion_core::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, ColorType, ImageInfo, Paint, PaintStyle, RRect, Rect};

use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    asset_cache, draw_text_with_fallback, emoji_typeface, fetch_icon_svg, font_mgr, paint_from_hex,
};
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

fn default_width() -> f32 {
    360.0
}
fn default_slide_in_at() -> f64 {
    0.5
}
fn default_slide_duration() -> f64 {
    0.15
}
fn default_stack_gap() -> f32 {
    12.0
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NotificationVariant {
    Info,
    Success,
    Warning,
    Error,
}

impl Default for NotificationVariant {
    fn default() -> Self {
        Self::Info
    }
}

impl NotificationVariant {
    fn default_color(&self) -> &str {
        match self {
            NotificationVariant::Info => "#3B82F6",
            NotificationVariant::Success => "#22C55E",
            NotificationVariant::Warning => "#F59E0B",
            NotificationVariant::Error => "#EF4444",
        }
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Notification {
    pub title: String,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub variant: NotificationVariant,
    #[serde(default = "default_width")]
    pub width: f32,
    #[serde(default = "default_slide_in_at")]
    pub slide_in_at: f64,
    #[serde(default)]
    pub slide_out_at: Option<f64>,
    #[serde(default = "default_slide_duration")]
    pub slide_duration: f64,
    #[serde(default)]
    pub accent_color: Option<String>,
    /// Timestamps at which this notification gets pushed down one slot
    /// (i.e. when another notification appears above it).
    #[serde(default)]
    pub push_at: Vec<f64>,
    /// Gap between stacked notifications in pixels (default 12).
    #[serde(default = "default_stack_gap")]
    pub stack_gap: f32,
    /// Delay fade-in until after other notifications finish their push animation.
    /// Set to true when this notification triggers a push_at on others.
    #[serde(default)]
    pub wait_for_push: bool,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(Notification {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Notification {
    fn resolved_accent_color(&self) -> &str {
        self.accent_color
            .as_deref()
            .unwrap_or_else(|| self.variant.default_color())
    }

    fn title_font_size(&self) -> f32 {
        self.style.font_size_px_or(16.0)
    }

    fn message_font_size(&self) -> f32 {
        self.style.font_size_px_or(16.0) * 0.85
    }

    fn make_font(&self, bold: bool, size: f32) -> skia_safe::Font {
        let fm = font_mgr();
        let font_style = if bold {
            skia_safe::FontStyle::bold()
        } else {
            skia_safe::FontStyle::normal()
        };
        let family = self.style.font_family.as_deref().unwrap_or("Inter");
        let typeface = fm
            .match_family_style(family, font_style)
            .or_else(|| fm.match_family_style("Helvetica", font_style))
            .unwrap_or_else(|| fm.legacy_make_typeface(None, font_style).unwrap());
        skia_safe::Font::from_typeface(typeface, size)
    }

    fn compute_opacity(&self, time: f64) -> f32 {
        // Effective start: if wait_for_push, delay by slide_duration so push animations finish first
        let effective_start = if self.wait_for_push {
            self.slide_in_at + self.slide_duration
        } else {
            self.slide_in_at
        };

        // Before fade in
        if time < effective_start {
            return 0.0;
        }

        // During fade in
        let fade_in_end = effective_start + self.slide_duration;
        if time < fade_in_end {
            let t = ((time - effective_start) / self.slide_duration) as f32;
            return (t * t * (3.0 - 2.0 * t)).clamp(0.0, 1.0); // smoothstep
        }

        // Check fade out
        if let Some(slide_out_at) = self.slide_out_at {
            if time >= slide_out_at {
                let fade_out_end = slide_out_at + self.slide_duration;
                if time >= fade_out_end {
                    return 0.0;
                }
                let t = ((time - slide_out_at) / self.slide_duration) as f32;
                return (1.0 - t * t * (3.0 - 2.0 * t)).clamp(0.0, 1.0);
            }
        }

        // Fully visible
        1.0
    }

    fn render_icon_svg(
        &self,
        canvas: &Canvas,
        icon_id: &str,
        color: &str,
        x: f32,
        y: f32,
        size: f32,
    ) -> Result<()> {
        let icon_w = size.round() as u32;
        let icon_h = size.round() as u32;
        let cache_key = format!("icon:{}:{}:{}x{}", icon_id, color, icon_w, icon_h);

        let cache = asset_cache();
        let img = if let Some(cached) = cache.get(&cache_key) {
            cached.clone()
        } else if let Ok(svg_data) = fetch_icon_svg(icon_id, color, icon_w, icon_h) {
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

        let dst = Rect::from_xywh(x, y, size, size);
        canvas.draw_image_rect(img, None, dst, &Paint::default());
        Ok(())
    }
}

impl Notification {
    fn paint(&self, canvas: &Canvas, layout_w: f32, layout_h: f32, time: f64) -> Result<()> {
        let w = layout_w;
        let h = layout_h;
        let opacity = self.compute_opacity(time);

        if opacity <= 0.0 {
            return Ok(());
        }

        // Stack offset: count how many push_at timestamps have passed,
        // each one shifts this notification down by one slot with animation.
        let slot_size = h + self.stack_gap;
        let mut stack_y = 0.0_f32;
        let transition_dur = self.slide_duration;
        for &push_time in &self.push_at {
            if time >= push_time {
                let t = ((time - push_time) / transition_dur).clamp(0.0, 1.0) as f32;
                let eased = t * t * (3.0 - 2.0 * t); // smoothstep
                stack_y += slot_size * eased;
            }
        }

        canvas.save();
        if stack_y > 0.0 {
            canvas.translate((0.0, stack_y));
        }
        if opacity < 1.0 {
            let mut layer_paint = Paint::default();
            layer_paint.set_alpha_f(opacity);
            canvas.save_layer(&skia_safe::canvas::SaveLayerRec::default().paint(&layer_paint));
        }

        let bg_color = self.style.background_color_str().unwrap_or("#1E293B");
        let radius = self.style.border_radius_px_or(12.0);
        let accent_color = self.resolved_accent_color();
        let accent_width = 4.0;

        // Background rounded rect
        let bg_rect = Rect::from_xywh(0.0, 0.0, w, h);
        let bg_rrect = RRect::new_rect_xy(bg_rect, radius, radius);
        let mut bg_paint = paint_from_hex(bg_color);
        bg_paint.set_style(PaintStyle::Fill);
        bg_paint.set_anti_alias(true);
        canvas.draw_rrect(bg_rrect, &bg_paint);

        // Left accent stripe
        let accent_rect = Rect::from_xywh(0.0, 0.0, accent_width, h);
        let accent_rrect = RRect::new_rect_radii(
            accent_rect,
            &[
                (radius, radius).into(),
                (0.0, 0.0).into(),
                (0.0, 0.0).into(),
                (radius, radius).into(),
            ],
        );
        let mut accent_paint = paint_from_hex(accent_color);
        accent_paint.set_style(PaintStyle::Fill);
        accent_paint.set_anti_alias(true);
        canvas.draw_rrect(accent_rrect, &accent_paint);

        // Content area
        let h_pad = 16.0;
        let v_pad = 16.0;
        let icon_size = self.title_font_size() * 1.5;
        let mut content_x = accent_width + h_pad;

        // Icon
        if let Some(icon_id) = &self.icon {
            let icon_y = (h - icon_size) / 2.0;
            self.render_icon_svg(canvas, icon_id, accent_color, content_x, icon_y, icon_size)?;
            content_x += icon_size + 12.0;
        }

        // Title
        let title_font = self.make_font(true, self.title_font_size());
        let title_fs = self.title_font_size();
        let emoji_font_title =
            emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, title_fs));
        let title_color = self.style.color_str_or("#FFFFFF");
        let mut title_paint = paint_from_hex(title_color);
        title_paint.set_anti_alias(true);

        let (_, title_metrics) = title_font.metrics();
        let title_y = v_pad + (-title_metrics.ascent);

        draw_text_with_fallback(
            canvas,
            &self.title,
            &title_font,
            &emoji_font_title,
            0.0,
            content_x,
            title_y,
            &title_paint,
        );

        // Message
        if let Some(message) = &self.message {
            let msg_fs = self.message_font_size();
            let msg_font = self.make_font(false, msg_fs);
            let emoji_font_msg =
                emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, msg_fs));
            let mut msg_paint = paint_from_hex("#9CA3AF");
            msg_paint.set_anti_alias(true);

            let (_, msg_metrics) = msg_font.metrics();
            let msg_y = title_y + 4.0 + title_fs * 0.3 + (-msg_metrics.ascent);

            draw_text_with_fallback(
                canvas,
                message,
                &msg_font,
                &emoji_font_msg,
                0.0,
                content_x,
                msg_y,
                &msg_paint,
            );
        }

        if opacity < 1.0 {
            canvas.restore();
        }
        canvas.restore();
        Ok(())
    }
}

impl Painter for Notification {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        let _ = self.paint(canvas, layout.width, layout.height, ctx.time);
    }
}
