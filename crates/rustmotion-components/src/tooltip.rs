use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, PathBuilder, Rect};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, measure_text_with_fallback, paint_from_hex,
    typeface_with_fallback,
};
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

fn default_font_size() -> f32 {
    13.0
}

fn default_bg_color() -> String {
    "#1E293B".to_string()
}

fn default_text_color() -> String {
    "#E2E8F0".to_string()
}

fn default_arrow_size() -> f32 {
    8.0
}

/// Arrow direction — where the arrow points (toward the target element).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum TooltipArrow {
    Top,
    #[default]
    Bottom,
    Left,
    Right,
    None,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Tooltip {
    pub text: String,
    #[serde(default)]
    pub arrow: TooltipArrow,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default = "default_bg_color")]
    pub background_color: String,
    #[serde(default = "default_text_color")]
    pub text_color: String,
    #[serde(default = "default_arrow_size")]
    pub arrow_size: f32,
    #[serde(default)]
    pub border_color: Option<String>,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(Tooltip {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Tooltip {
    /// Resolves `font-size` against a real per-frame viewport (`rem`/`vw`/
    /// `vh` now resolve instead of silently dropping to 0px — lot B, wave
    /// S). `em`/`%` on `font-size` itself remain approximate — see
    /// `crate::intrinsic::font_size_ctx`'s doc comment.
    fn resolved_font_size(&self, ctx: &PaintCtx) -> f32 {
        self.style.font_size_px_ctx(
            &crate::intrinsic::font_size_ctx(ctx.video_width as f32, ctx.video_height as f32, 0.0),
            self.font_size,
        )
    }

    fn make_font(&self, ctx: &PaintCtx) -> Option<skia_safe::Font> {
        let fs = self.resolved_font_size(ctx);
        let font_style = skia_safe::FontStyle::normal();
        let family = self.style.font_family_or("Inter");
        let typeface = typeface_with_fallback(family, font_style).ok()?;
        Some(skia_safe::Font::from_typeface(typeface, fs))
    }
}

impl Tooltip {
    fn paint(&self, canvas: &Canvas, layout_w: f32, layout_h: f32, ctx: &PaintCtx) {
        let w = layout_w;
        let h = layout_h;
        let bg_color = self
            .style
            .background_color_str()
            .unwrap_or(&self.background_color);
        let radius = self.style.border_radius_px_or(8.0);
        let arrow_sz = self.arrow_size;

        // Compute body rect (excluding arrow area)
        let (body_x, body_y, body_w, body_h) = match self.arrow {
            TooltipArrow::Bottom => (0.0, 0.0, w, h - arrow_sz),
            TooltipArrow::Top => (0.0, arrow_sz, w, h - arrow_sz),
            TooltipArrow::Right => (0.0, 0.0, w - arrow_sz, h),
            TooltipArrow::Left => (arrow_sz, 0.0, w - arrow_sz, h),
            TooltipArrow::None => (0.0, 0.0, w, h),
        };

        // Body rounded rect
        let body_rect = Rect::from_xywh(body_x, body_y, body_w, body_h);
        let body_rrect = skia_safe::RRect::new_rect_xy(body_rect, radius, radius);

        let mut bg_paint = paint_from_hex(bg_color);
        bg_paint.set_style(PaintStyle::Fill);
        bg_paint.set_anti_alias(true);
        canvas.draw_rrect(body_rrect, &bg_paint);

        // Border
        if let Some(bc) = &self.border_color {
            let mut border_paint = paint_from_hex(bc);
            border_paint.set_style(PaintStyle::Stroke);
            border_paint.set_stroke_width(1.0);
            border_paint.set_anti_alias(true);
            canvas.draw_rrect(body_rrect, &border_paint);
        }

        // Arrow triangle
        if !matches!(self.arrow, TooltipArrow::None) {
            let mut arrow_path = PathBuilder::new();
            match self.arrow {
                TooltipArrow::Bottom => {
                    let cx = body_x + body_w / 2.0;
                    let ay = body_y + body_h;
                    arrow_path.move_to((cx - arrow_sz, ay));
                    arrow_path.line_to((cx, ay + arrow_sz));
                    arrow_path.line_to((cx + arrow_sz, ay));
                    arrow_path.close();
                }
                TooltipArrow::Top => {
                    let cx = body_x + body_w / 2.0;
                    let ay = body_y;
                    arrow_path.move_to((cx - arrow_sz, ay));
                    arrow_path.line_to((cx, ay - arrow_sz));
                    arrow_path.line_to((cx + arrow_sz, ay));
                    arrow_path.close();
                }
                TooltipArrow::Right => {
                    let cy = body_y + body_h / 2.0;
                    let ax = body_x + body_w;
                    arrow_path.move_to((ax, cy - arrow_sz));
                    arrow_path.line_to((ax + arrow_sz, cy));
                    arrow_path.line_to((ax, cy + arrow_sz));
                    arrow_path.close();
                }
                TooltipArrow::Left => {
                    let cy = body_y + body_h / 2.0;
                    let ax = body_x;
                    arrow_path.move_to((ax, cy - arrow_sz));
                    arrow_path.line_to((ax - arrow_sz, cy));
                    arrow_path.line_to((ax, cy + arrow_sz));
                    arrow_path.close();
                }
                TooltipArrow::None => {}
            }
            canvas.draw_path(&arrow_path.detach(), &bg_paint);
        }

        // Text centered in body
        let Some(font) = self.make_font(ctx) else {
            return;
        };
        let fs = self.resolved_font_size(ctx);
        let emoji_font = emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, fs));

        let text_color = self.style.color_str().unwrap_or(&self.text_color);
        let mut text_paint = paint_from_hex(text_color);
        text_paint.set_anti_alias(true);

        let text_w = measure_text_with_fallback(&self.text, &font, &emoji_font, 0.0);
        let (_, metrics) = font.metrics();
        let text_x = body_x + (body_w - text_w) / 2.0;
        let text_y = body_y + (body_h + (-metrics.ascent)) / 2.0;

        draw_text_with_fallback(
            canvas,
            &self.text,
            &font,
            &emoji_font,
            0.0,
            text_x,
            text_y,
            &text_paint,
        );
    }
}

impl Painter for Tooltip {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        self.paint(canvas, layout.width, layout.height, ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmotion_core::css::CssStyle;
    use rustmotion_core::css::Length;

    fn test_ctx() -> PaintCtx {
        PaintCtx {
            time: 0.0,
            scenario_time: 0.0,
            scene_duration: 1.0,
            frame_index: 0,
            fps: 30,
            video_width: 400,
            video_height: 200,
            stagger_offset: 0.0,
        }
    }

    // ─── Lot B, wave S: relative `font-size` units ─────────────────────────

    #[test]
    fn rem_font_size_paints_visible_ink() {
        // Reproduction: `font-size: "2rem"` used to resolve to 0px via the
        // context-free `font_size_px_or`.
        let tooltip = Tooltip {
            text: "hello".to_string(),
            arrow: TooltipArrow::None,
            font_size: default_font_size(),
            background_color: default_bg_color(),
            text_color: default_text_color(),
            arrow_size: default_arrow_size(),
            border_color: None,
            timing: Default::default(),
            style: CssStyle {
                font_size: Some(Length::String("2rem".into())),
                ..Default::default()
            },
            timeline: Vec::new(),
            stagger: None,
        };
        const W: i32 = 200;
        const H: i32 = 100;
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        {
            let canvas = surface.canvas();
            tooltip.paint(canvas, 150.0, 60.0, &test_ctx());
        }
        let snapshot = surface.image_snapshot();
        let info = skia_safe::ImageInfo::new(
            (W, H),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut buf = vec![0u8; (W * H * 4) as usize];
        let ok = snapshot.read_pixels(
            &info,
            &mut buf,
            (W * 4) as usize,
            skia_safe::IPoint::new(0, 0),
            skia_safe::image::CachingHint::Disallow,
        );
        assert!(ok, "pixel read should succeed");
        // Text is near-white (#E2E8F0 default) on a dark #1E293B body —
        // probe for near-white ink specifically.
        let text_ink = buf
            .chunks_exact(4)
            .filter(|p| p[3] > 0 && p[0] > 180 && p[1] > 180 && p[2] > 180)
            .count();
        assert!(
            text_ink > 5,
            "tooltip at font-size: 2rem must paint visible text, got {text_ink} pixels"
        );
    }
}
