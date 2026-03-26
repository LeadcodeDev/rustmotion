use rustmotion_core::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, Path, Rect, RRect};

use rustmotion_core::engine::renderer::{font_mgr, paint_from_hex, wrap_text};
use rustmotion_core::error::RustmotionError;
use rustmotion_core::layout::{Constraints, LayoutNode};
use rustmotion_core::schema::{LayerStyle, Size};
use rustmotion_core::traits::{RenderContext, TimingConfig, Widget};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ArrowDirection {
    Top,
    Bottom,
    Left,
    Right,
}

impl Default for ArrowDirection {
    fn default() -> Self {
        Self::Bottom
    }
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
    #[serde(default)]
    pub size: Option<Size>,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

fn default_arrow_size() -> f32 {
    12.0
}

rustmotion_core::impl_traits!(Callout {
    Animatable => style,
    Timed => timing,
    Styled => style,
});

impl Callout {
    fn bg_color(&self) -> &str {
        self.style.background.as_deref().unwrap_or("#333333")
    }

    fn text_color(&self) -> &str {
        self.style.color.as_deref().unwrap_or("#FFFFFF")
    }

    fn radius(&self) -> f32 {
        self.style.border_radius.unwrap_or(8.0)
    }

    fn font_size(&self) -> f32 {
        self.style.font_size.unwrap_or(16.0)
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
        let mut path = Path::new();
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

        path
    }
}

impl Widget for Callout {
    fn render(
        &self,
        canvas: &Canvas,
        layout: &LayoutNode,
        _ctx: &RenderContext,
        _props: &rustmotion_core::engine::animator::AnimatedProperties,
        _pipeline: &dyn rustmotion_core::traits::RenderPipeline,
    ) -> Result<()> {
        let w = layout.width;
        let h = layout.height;
        let radius = self.radius();
        let font_size = self.font_size();

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
        let fm = font_mgr();
        let font_style = skia_safe::FontStyle::normal();
        let family = self.style.font_family.as_deref().unwrap_or("Inter");
        let typeface = fm
            .match_family_style(family, font_style)
            .or_else(|| fm.match_family_style("Helvetica", font_style))
            .or_else(|| fm.match_family_style("Arial", font_style))
            .ok_or(RustmotionError::FontNotFound)?;

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

    fn measure(&self, _constraints: &Constraints) -> (f32, f32) {
        if let Some(size) = &self.size {
            return (size.width, size.height);
        }

        let font_size = self.font_size();
        let fm = font_mgr();
        let typeface = fm
            .match_family_style("Inter", skia_safe::FontStyle::normal())
            .or_else(|| fm.match_family_style("Helvetica", skia_safe::FontStyle::normal()))
            .expect(&RustmotionError::FontNotFound.to_string());
        let font = skia_safe::Font::from_typeface(typeface, font_size);
        let text_w = font.measure_str(&self.text, None).0;

        let padding = 12.0;
        let w = text_w + padding * 2.0 + 16.0;
        let h = font_size * 1.4 + padding * 2.0;

        match self.arrow_direction {
            ArrowDirection::Top | ArrowDirection::Bottom => (w, h + self.arrow_size),
            ArrowDirection::Left | ArrowDirection::Right => (w + self.arrow_size, h),
        }
    }
}
