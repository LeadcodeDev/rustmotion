use rustmotion_core::css::CssStyle;
use rustmotion_core::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, Path, PathBuilder, RRect, Rect};

use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{paint_from_hex, typeface_with_fallback, wrap_text};
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum ArrowDirection {
    Top,
    #[default]
    Bottom,
    Left,
    Right,
}

/// Speech bubble with a directional arrow.
///
/// CSS-like properties go in `style`:
/// - `style.background` — bubble background color (default: `"#333333"`)
/// - `style.color` — text color (default: `"#FFFFFF"`)
/// - `style.border-radius` — corner radius (default: `8`)
/// - `style.font-size` — text size (default: `16`)
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Callout {
    pub text: String,
    #[serde(default)]
    pub arrow_direction: ArrowDirection,
    #[serde(default = "default_arrow_size")]
    pub arrow_size: f32,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

fn default_arrow_size() -> f32 {
    12.0
}

rustmotion_core::impl_traits!(Callout {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Callout {
    fn bg_color(&self) -> &str {
        self.style.background_color_str().unwrap_or("#333333")
    }

    fn text_color(&self) -> &str {
        self.style.color_str_or("#FFFFFF")
    }

    fn radius(&self) -> f32 {
        self.style.border_radius_px_or(8.0)
    }

    /// Resolves `font-size` against a real per-frame viewport (`rem`/`vw`/
    /// `vh` now resolve instead of silently dropping to 0px — lot B, wave
    /// S). `em`/`%` on `font-size` itself remain approximate — see
    /// `crate::intrinsic::font_size_ctx`'s doc comment.
    fn font_size(&self, ctx: &PaintCtx) -> f32 {
        self.style.font_size_px_ctx(
            &crate::intrinsic::font_size_ctx(ctx.video_width as f32, ctx.video_height as f32, 0.0),
            16.0,
        )
    }

    fn bubble_rect(&self, w: f32, h: f32) -> Rect {
        match self.arrow_direction {
            ArrowDirection::Top => Rect::from_xywh(0.0, self.arrow_size, w, h - self.arrow_size),
            ArrowDirection::Bottom => Rect::from_xywh(0.0, 0.0, w, h - self.arrow_size),
            ArrowDirection::Left => Rect::from_xywh(self.arrow_size, 0.0, w - self.arrow_size, h),
            ArrowDirection::Right => Rect::from_xywh(0.0, 0.0, w - self.arrow_size, h),
        }
    }

    fn arrow_path(&self, w: f32, h: f32) -> Path {
        let mut path = PathBuilder::new();
        let a = self.arrow_size;
        // Overlap the arrow base 1px into the bubble to eliminate anti-aliasing seam
        let overlap = 1.0;

        match self.arrow_direction {
            ArrowDirection::Bottom => {
                let cx = w / 2.0;
                let top = h - a - overlap;
                path.move_to((cx - a, top));
                path.line_to((cx, h));
                path.line_to((cx + a, top));
                path.close();
            }
            ArrowDirection::Top => {
                let cx = w / 2.0;
                let bottom = a + overlap;
                path.move_to((cx - a, bottom));
                path.line_to((cx, 0.0));
                path.line_to((cx + a, bottom));
                path.close();
            }
            ArrowDirection::Left => {
                let cy = h / 2.0;
                let right = a + overlap;
                path.move_to((right, cy - a));
                path.line_to((0.0, cy));
                path.line_to((right, cy + a));
                path.close();
            }
            ArrowDirection::Right => {
                let cy = h / 2.0;
                let left = w - a - overlap;
                path.move_to((left, cy - a));
                path.line_to((w, cy));
                path.line_to((left, cy + a));
                path.close();
            }
        }

        path.detach()
    }
}

impl Callout {
    fn paint(&self, canvas: &Canvas, layout_w: f32, layout_h: f32, ctx: &PaintCtx) -> Result<()> {
        let w = layout_w;
        let h = layout_h;
        let radius = self.radius();
        let font_size = self.font_size(ctx);

        // Draw bubble body
        let bubble = self.bubble_rect(w, h);
        let rrect = RRect::new_rect_xy(bubble, radius, radius);
        let mut bg_paint = paint_from_hex(self.bg_color());
        bg_paint.set_style(PaintStyle::Fill);
        bg_paint.set_anti_alias(true);
        canvas.draw_rrect(rrect, &bg_paint);

        // Draw arrow
        let arrow = self.arrow_path(w, h);
        canvas.draw_path(&arrow, &bg_paint);

        // Draw text
        let font_style = skia_safe::FontStyle::normal();
        let family = self.style.font_family.as_deref().unwrap_or("Inter");
        let typeface = typeface_with_fallback(family, font_style)?;

        let font = skia_safe::Font::from_typeface(typeface, font_size);
        let (_, metrics) = font.metrics();
        let ascent = -metrics.ascent;
        let line_height = font_size * 1.4;

        let padding = 12.0;
        let text_area_x = bubble.left + padding;
        let text_area_w = bubble.width() - padding * 2.0;

        let lines = wrap_text(&self.text, &font, Some(text_area_w));
        let total_text_h = lines.len() as f32 * line_height;
        let text_y_start = bubble.top + (bubble.height() - total_text_h) / 2.0 + ascent;

        let mut text_paint = paint_from_hex(self.text_color());
        text_paint.set_anti_alias(true);

        for (i, line) in lines.iter().enumerate() {
            if line.is_empty() {
                continue;
            }
            if let Some(blob) = skia_safe::TextBlob::new(line, &font) {
                let blob_w = blob.bounds().width();
                let x = text_area_x + (text_area_w - blob_w) / 2.0;
                let y = text_y_start + i as f32 * line_height;
                canvas.draw_text_blob(&blob, (x, y), &text_paint);
            }
        }

        Ok(())
    }
}

impl Painter for Callout {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        let _ = self.paint(canvas, layout.width, layout.height, ctx);
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
        let callout = Callout {
            text: "hello".to_string(),
            arrow_direction: ArrowDirection::default(),
            arrow_size: default_arrow_size(),
            timing: Default::default(),
            style: CssStyle {
                font_size: Some(Length::String("2rem".into())),
                ..Default::default()
            },
            timeline: Vec::new(),
            stagger: None,
        };
        const W: i32 = 400;
        const H: i32 = 200;
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        {
            let canvas = surface.canvas();
            callout
                .paint(canvas, W as f32, H as f32, &test_ctx())
                .expect("paint succeeds");
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
        // Text is white (#FFFFFF default) on a dark #333333 bubble — probe
        // for near-white ink specifically, since the bubble background
        // paints regardless of font-size.
        let text_ink = buf
            .chunks_exact(4)
            .filter(|p| p[3] > 0 && p[0] > 200 && p[1] > 200 && p[2] > 200)
            .count();
        assert!(
            text_ink > 10,
            "callout at font-size: 2rem must paint visible text, got {text_ink} pixels"
        );
    }
}
