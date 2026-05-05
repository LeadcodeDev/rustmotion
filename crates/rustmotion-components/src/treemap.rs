use rustmotion_core::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, RRect, Rect};

use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, font_mgr, measure_text_with_fallback, paint_from_hex,
};
use rustmotion_core::layout::{Constraints, LayoutNode};
use rustmotion_core::schema::{LayerStyle, Size};
use rustmotion_core::traits::{PaintCtx, Painter, RenderContext, TimingConfig, Widget};

const DEFAULT_PALETTE: &[&str] = &[
    "#3B82F6", "#EF4444", "#22C55E", "#F59E0B", "#8B5CF6", "#EC4899", "#06B6D4", "#F97316",
];

fn default_gap() -> f32 {
    3.0
}

fn default_border_radius() -> f32 {
    6.0
}

fn default_show_labels() -> bool {
    true
}

fn default_animated() -> bool {
    true
}

fn default_animation_duration() -> f64 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TreemapItem {
    #[serde(default)]
    pub label: Option<String>,
    pub value: f64,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Treemap {
    /// Data items to display in the treemap.
    pub data: Vec<TreemapItem>,
    #[serde(default)]
    pub size: Option<Size>,
    /// Gap between rectangles in pixels.
    #[serde(default = "default_gap")]
    pub gap: f32,
    /// Corner radius of rectangles.
    #[serde(default = "default_border_radius")]
    pub border_radius: f32,
    /// Whether to show labels inside rectangles.
    #[serde(default = "default_show_labels")]
    pub show_labels: bool,
    /// Whether to show values inside rectangles.
    #[serde(default)]
    pub show_values: bool,
    /// Whether the treemap animates in.
    #[serde(default = "default_animated")]
    pub animated: bool,
    /// Duration of the scale animation in seconds.
    #[serde(default = "default_animation_duration")]
    pub animation_duration: f64,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

rustmotion_core::impl_traits!(Treemap {
    Animatable => style,
    Timed => timing,
    Styled => style,
});

fn layout_treemap(items: &[(f64, usize)], rect: Rect, vertical: bool) -> Vec<(usize, Rect)> {
    let total: f64 = items.iter().map(|i| i.0).sum();
    if total <= 0.0 || items.is_empty() {
        return vec![];
    }
    let mut results = vec![];
    let mut offset = if vertical { rect.top } else { rect.left };
    for &(value, idx) in items {
        let fraction = (value / total) as f32;
        let r = if vertical {
            let h = rect.height() * fraction;
            let r = Rect::from_xywh(rect.left, offset, rect.width(), h);
            offset += h;
            r
        } else {
            let w = rect.width() * fraction;
            let r = Rect::from_xywh(offset, rect.top, w, rect.height());
            offset += w;
            r
        };
        results.push((idx, r));
    }
    results
}

impl Treemap {
    fn progress_at(&self, time: f64) -> f32 {
        if !self.animated {
            return 1.0;
        }
        let p = (time / self.animation_duration).clamp(0.0, 1.0) as f32;
        1.0 - (1.0 - p).powi(3)
    }

    fn paint(&self, canvas: &Canvas, layout_w: f32, layout_h: f32, time: f64) {
        let w = layout_w;
        let h = layout_h;

        if self.data.is_empty() {
            return;
        }

        let progress = self.progress_at(time);

        // Sort data by value descending, keeping original indices
        let mut sorted: Vec<(f64, usize)> = self
            .data
            .iter()
            .enumerate()
            .map(|(i, item)| (item.value, i))
            .collect();
        sorted.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        let full_rect = Rect::from_xywh(0.0, 0.0, w, h);
        let rects = layout_treemap(&sorted, full_rect, false);

        let fm = font_mgr();
        let font_style = skia_safe::FontStyle::normal();
        let typeface = fm
            .match_family_style("Inter", font_style)
            .or_else(|| fm.match_family_style("Helvetica", font_style))
            .unwrap_or_else(|| fm.legacy_make_typeface(None, font_style).unwrap());

        for (idx, rect) in &rects {
            let item = &self.data[*idx];

            // Apply gap by insetting the rect
            let inset = self.gap / 2.0;
            let inset_rect = Rect::from_xywh(
                rect.left + inset,
                rect.top + inset,
                (rect.width() - self.gap).max(0.0),
                (rect.height() - self.gap).max(0.0),
            );

            if inset_rect.width() <= 0.0 || inset_rect.height() <= 0.0 {
                continue;
            }

            // Animation: scale each rect from center
            let cx = inset_rect.left + inset_rect.width() / 2.0;
            let cy = inset_rect.top + inset_rect.height() / 2.0;
            let scaled_w = inset_rect.width() * progress;
            let scaled_h = inset_rect.height() * progress;
            let scaled_rect = Rect::from_xywh(
                cx - scaled_w / 2.0,
                cy - scaled_h / 2.0,
                scaled_w,
                scaled_h,
            );

            // Color
            let color_str = item
                .color
                .as_deref()
                .unwrap_or(DEFAULT_PALETTE[*idx % DEFAULT_PALETTE.len()]);
            let mut paint = paint_from_hex(color_str);
            paint.set_style(PaintStyle::Fill);
            paint.set_anti_alias(true);

            let rrect = RRect::new_rect_xy(scaled_rect, self.border_radius, self.border_radius);
            canvas.draw_rrect(rrect, &paint);

            // Labels
            if self.show_labels || self.show_values {
                if scaled_rect.width() < 30.0 || scaled_rect.height() < 20.0 {
                    continue;
                }

                let font_size = (scaled_rect.width() * 0.12).clamp(10.0, 24.0);
                let font = skia_safe::Font::from_typeface(&typeface, font_size);
                let emoji_font =
                    emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, font_size));

                let mut text_paint = paint_from_hex("#FFFFFF");
                text_paint.set_anti_alias(true);

                let mut text_parts: Vec<String> = vec![];
                if self.show_labels {
                    if let Some(label) = &item.label {
                        text_parts.push(label.clone());
                    }
                }
                if self.show_values {
                    text_parts.push(format!("{}", item.value));
                }

                let text = text_parts.join("\n");
                let text_w = measure_text_with_fallback(&text, &font, &emoji_font, 0.0);
                let (_, metrics) = font.metrics();

                let text_x = scaled_rect.left + (scaled_rect.width() - text_w) / 2.0;
                let text_y = scaled_rect.top + scaled_rect.height() / 2.0
                    + (-metrics.ascent - metrics.descent) / 2.0;

                draw_text_with_fallback(
                    canvas,
                    &text,
                    &font,
                    &emoji_font,
                    0.0,
                    text_x,
                    text_y,
                    &text_paint,
                );
            }
        }
    }
}

impl Widget for Treemap {
    fn render(
        &self,
        canvas: &Canvas,
        layout: &LayoutNode,
        ctx: &RenderContext,
        _props: &AnimatedProperties,
        _pipeline: &dyn rustmotion_core::traits::RenderPipeline,
    ) -> Result<()> {
        self.paint(canvas, layout.width, layout.height, ctx.time);
        Ok(())
    }

    fn measure(&self, _constraints: &Constraints) -> (f32, f32) {
        if let Some(size) = &self.size {
            return (size.width, size.height);
        }
        (400.0, 300.0)
    }
}

impl Painter for Treemap {
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
