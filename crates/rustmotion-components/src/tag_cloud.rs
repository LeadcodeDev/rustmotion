use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::Canvas;

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, measure_text_with_fallback, paint_from_hex,
    parse_hex_color, typeface_with_fallback,
};
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

const DEFAULT_PALETTE: &[&str] = &[
    "#3B82F6", "#EF4444", "#22C55E", "#F59E0B", "#8B5CF6", "#EC4899", "#06B6D4", "#F97316",
];

fn default_min_font_size() -> f32 {
    14.0
}

fn default_max_font_size() -> f32 {
    64.0
}

fn default_animated() -> bool {
    true
}

fn default_animation_duration() -> f64 {
    1.5
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct TagItem {
    pub text: String,
    #[serde(default)]
    pub weight: f64,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct TagCloud {
    pub tags: Vec<TagItem>,
    #[serde(default = "default_min_font_size")]
    pub min_font_size: f32,
    #[serde(default = "default_max_font_size")]
    pub max_font_size: f32,
    #[serde(default)]
    pub colors: Option<Vec<String>>,
    #[serde(default = "default_animated")]
    pub animated: bool,
    #[serde(default = "default_animation_duration")]
    pub animation_duration: f64,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(TagCloud {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl TagCloud {
    fn progress_at(&self, time: f64) -> f32 {
        if !self.animated {
            return 1.0;
        }
        let p = (time / self.animation_duration).clamp(0.0, 1.0) as f32;
        1.0 - (1.0 - p).powi(3)
    }

    /// Never empty: callers index into it with `%`, and `"colors": []` — which a
    /// generator emits to mean "no custom palette" — otherwise divides by zero.
    fn palette(&self) -> Vec<&str> {
        match &self.colors {
            Some(colors) if !colors.is_empty() => colors.iter().map(|s| s.as_str()).collect(),
            _ => DEFAULT_PALETTE.to_vec(),
        }
    }

    fn font_size_for_weight(&self, weight: f64, min_weight: f64, max_weight: f64) -> f32 {
        let range = (max_weight - min_weight).max(0.001);
        let normalized = ((weight - min_weight) / range) as f32;
        self.min_font_size + normalized * (self.max_font_size - self.min_font_size)
    }
}

impl TagCloud {
    fn paint(&self, canvas: &Canvas, layout_w: f32, layout_h: f32, time: f64) {
        let w = layout_w;
        let h = layout_h;

        if self.tags.is_empty() {
            return;
        }

        let progress = self.progress_at(time);
        let palette = self.palette();

        // Collect weights
        let min_weight = self.tags.iter().map(|t| t.weight).fold(f64::MAX, f64::min);
        let max_weight = self.tags.iter().map(|t| t.weight).fold(f64::MIN, f64::max);

        // Sort tags by weight descending (indices for stagger)
        let mut sorted_indices: Vec<usize> = (0..self.tags.len()).collect();
        sorted_indices.sort_by(|&a, &b| {
            self.tags[b]
                .weight
                .partial_cmp(&self.tags[a].weight)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let tag_count = self.tags.len();
        let h_gap = 12.0_f32;
        let v_gap = 8.0_f32;

        // Pre-compute tag metrics for flow layout
        struct TagMetrics {
            index: usize,
            font_size: f32,
            text_width: f32,
            ascent: f32,
            height: f32,
        }

        let mut metrics_list: Vec<TagMetrics> = Vec::with_capacity(tag_count);
        for &idx in &sorted_indices {
            let tag = &self.tags[idx];
            let font_size = self.font_size_for_weight(tag.weight, min_weight, max_weight);
            let font_style = skia_safe::FontStyle::bold();
            let Ok(typeface) = typeface_with_fallback("Inter", font_style) else {
                continue;
            };
            let font = skia_safe::Font::from_typeface(typeface, font_size);
            let emoji_font =
                emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, font_size));

            let text_width = measure_text_with_fallback(&tag.text, &font, &emoji_font, 0.0);
            let (_, fm_metrics) = font.metrics();
            let ascent = -fm_metrics.ascent;
            let height = ascent + fm_metrics.descent;

            metrics_list.push(TagMetrics {
                index: idx,
                font_size,
                text_width,
                ascent,
                height,
            });
        }

        // Flow layout: place tags left-to-right, wrap when exceeding width
        struct PlacedTag {
            index: usize,
            x: f32,
            y: f32,
            font_size: f32,
            ascent: f32,
            line_height: f32,
        }

        let mut placed: Vec<PlacedTag> = Vec::with_capacity(tag_count);
        let mut cursor_x = 0.0_f32;
        let mut cursor_y = 0.0_f32;
        let mut line_max_height = 0.0_f32;

        for m in &metrics_list {
            let item_width = m.text_width + h_gap;

            if cursor_x + m.text_width > w && cursor_x > 0.0 {
                // Wrap to next line
                cursor_y += line_max_height + v_gap;
                cursor_x = 0.0;
                line_max_height = 0.0;
            }

            placed.push(PlacedTag {
                index: m.index,
                x: cursor_x,
                y: cursor_y,
                font_size: m.font_size,
                ascent: m.ascent,
                line_height: m.height,
            });

            if m.height > line_max_height {
                line_max_height = m.height;
            }

            cursor_x += item_width;
        }

        // Compute total content height for vertical centering
        let total_height = if let Some(last) = placed.last() {
            last.y
                + placed.iter().fold(0.0_f32, |max_h, p| {
                    if (p.y - last.y).abs() < 0.01 {
                        max_h.max(p.line_height)
                    } else {
                        max_h
                    }
                })
        } else {
            0.0
        };
        let y_offset = ((h - total_height) / 2.0).max(0.0);

        // Render each tag
        for (draw_order, pt) in placed.iter().enumerate() {
            let tag = &self.tags[pt.index];

            // Staggered opacity animation
            let tag_alpha = if self.animated {
                let stagger_delay = (draw_order as f64 / tag_count as f64) * 0.6;
                let tag_progress = ((time - stagger_delay) / (self.animation_duration * 0.4))
                    .clamp(0.0, 1.0) as f32;
                tag_progress * progress
            } else {
                1.0
            };

            if tag_alpha <= 0.0 {
                continue;
            }

            // Pick color
            let color_str = tag
                .color
                .as_deref()
                .unwrap_or(palette[pt.index % palette.len()]);

            let (_r, _g, _b, _) = parse_hex_color(color_str);
            let mut text_paint = paint_from_hex(color_str);
            text_paint.set_anti_alias(true);
            text_paint.set_alpha((tag_alpha * 255.0) as u8);

            let font_style = skia_safe::FontStyle::bold();
            let Ok(typeface) = typeface_with_fallback("Inter", font_style) else {
                continue;
            };
            let font = skia_safe::Font::from_typeface(typeface, pt.font_size);
            let emoji_font =
                emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, pt.font_size));

            let text_x = pt.x;
            let text_y = y_offset + pt.y + pt.ascent;

            draw_text_with_fallback(
                canvas,
                &tag.text,
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

impl Painter for TagCloud {
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
