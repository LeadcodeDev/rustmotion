use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, RRect, Rect};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, measure_text_with_fallback, paint_from_hex,
    typeface_with_fallback,
};
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

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
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(Treemap {
    Animatable => animation,
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
        // Ramp measured from `start_at`, not from scene time zero — matches
        // `Counter::ramp_progress`. A treemap delayed with `start_at` used
        // to read raw scene time, so it was already fully scaled in on the
        // very first frame it became visible.
        let start = self.timing.start_at.unwrap_or(0.0);
        let elapsed = (time - start).max(0.0);
        let p = (elapsed / self.animation_duration).clamp(0.0, 1.0) as f32;
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

        let font_style = skia_safe::FontStyle::normal();
        let Ok(typeface) = typeface_with_fallback("Inter", font_style) else {
            return;
        };

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
            let scaled_rect =
                Rect::from_xywh(cx - scaled_w / 2.0, cy - scaled_h / 2.0, scaled_w, scaled_h);

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
                let mut text_parts: Vec<String> = vec![];
                if self.show_labels {
                    if let Some(label) = &item.label {
                        text_parts.push(label.clone());
                    }
                }
                if self.show_values {
                    text_parts.push(format!("{}", item.value));
                }

                if text_parts.is_empty() {
                    continue;
                }

                let font_size = (scaled_rect.width() * 0.12).clamp(10.0, 24.0);
                // `draw_text_with_fallback` builds a single-line `TextBlob`
                // (renderer/text.rs) — joining label and value with "\n"
                // never produced a line break, it fed the blob a literal
                // control glyph. Each part now gets its own baseline, and
                // the space each line needs (`20.0` per line, same floor
                // the old single-line check used) is checked before
                // drawing instead of after.
                let line_height = font_size * 1.2;
                if scaled_rect.width() < 30.0
                    || scaled_rect.height() < 20.0 * text_parts.len() as f32
                {
                    continue;
                }

                let font = skia_safe::Font::from_typeface(&typeface, font_size);
                let emoji_font =
                    emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, font_size));

                let mut text_paint = paint_from_hex("#FFFFFF");
                text_paint.set_anti_alias(true);

                let (_, metrics) = font.metrics();
                let ascent = -metrics.ascent;
                let descent = metrics.descent;
                let block_h = line_height * text_parts.len() as f32;
                let block_top = scaled_rect.top + scaled_rect.height() / 2.0 - block_h / 2.0;

                for (li, part) in text_parts.iter().enumerate() {
                    let part_w = measure_text_with_fallback(part, &font, &emoji_font, 0.0);
                    let part_x = scaled_rect.left + (scaled_rect.width() - part_w) / 2.0;
                    let line_center_y = block_top + (li as f32 + 0.5) * line_height;
                    let part_y = line_center_y + (ascent - descent) / 2.0;

                    draw_text_with_fallback(
                        canvas,
                        part,
                        &font,
                        &emoji_font,
                        0.0,
                        part_x,
                        part_y,
                        &text_paint,
                    );
                }
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use rustmotion_core::traits::TimingConfig;

    fn base_treemap(data: Vec<TreemapItem>) -> Treemap {
        Treemap {
            data,
            gap: default_gap(),
            border_radius: default_border_radius(),
            show_labels: default_show_labels(),
            show_values: false,
            animated: true,
            animation_duration: 1.0,
            timing: TimingConfig::default(),
            style: CssStyle::default(),
            timeline: Vec::new(),
            stagger: None,
        }
    }

    fn read_rgba(surface: &mut skia_safe::Surface, w: i32, h: i32) -> Vec<u8> {
        let snapshot = surface.image_snapshot();
        let info = skia_safe::ImageInfo::new(
            (w, h),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut buf = vec![0u8; (w * h * 4) as usize];
        snapshot.read_pixels(
            &info,
            &mut buf,
            (w * 4) as usize,
            skia_safe::IPoint::new(0, 0),
            skia_safe::image::CachingHint::Disallow,
        );
        buf
    }

    /// A pixel is "text ink" if it's opaque-ish and near-white — the fixed
    /// `#FFFFFF` label/value color, distinct from the cell's own colored
    /// (never white) `DEFAULT_PALETTE` background fill that would otherwise
    /// dominate a naive alpha-only scan.
    fn is_text_ink(buf: &[u8], w: i32, x: i32, y: i32) -> bool {
        let idx = ((y * w + x) * 4) as usize;
        let (r, g, b, a) = (buf[idx], buf[idx + 1], buf[idx + 2], buf[idx + 3]);
        a > 40 && r > 200 && g > 200 && b > 200
    }

    /// Contiguous vertical bands (start_y, end_y) of text ink, merging rows
    /// separated by a 1px anti-aliasing gap but splitting on anything
    /// wider — used to tell "two stacked text lines" apart from "one line
    /// of text".
    fn row_bands(buf: &[u8], w: i32, h: i32) -> Vec<(i32, i32)> {
        let mut bands: Vec<(i32, i32)> = vec![];
        for y in 0..h {
            let has_ink = (0..w).any(|x| is_text_ink(buf, w, x, y));
            if has_ink {
                match bands.last_mut() {
                    Some((_, end)) if y <= *end + 1 => *end = y,
                    _ => bands.push((y, y)),
                }
            }
        }
        bands
    }

    fn row_ink_x_range(buf: &[u8], w: i32, y0: i32, y1: i32) -> (i32, i32) {
        let (mut minx, mut maxx) = (i32::MAX, i32::MIN);
        for y in y0..=y1 {
            for x in 0..w {
                if is_text_ink(buf, w, x, y) {
                    minx = minx.min(x);
                    maxx = maxx.max(x);
                }
            }
        }
        (minx, maxx)
    }

    #[test]
    fn label_and_value_render_on_two_separate_centered_lines() {
        // #8's exact repro: `text_parts.join("\n")` fed a single-line
        // `TextBlob` a literal "\n" glyph — the label and value landed side
        // by side on the same baseline instead of stacked, and the whole
        // (wrongly wide) string was centered as one block, decentering the
        // label itself.
        const W: i32 = 300;
        const H: i32 = 200;
        let mut treemap = base_treemap(vec![TreemapItem {
            label: Some("Alpha".to_string()),
            value: 50.0,
            color: None,
        }]);
        treemap.show_labels = true;
        treemap.show_values = true;
        treemap.animated = false;

        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        {
            let canvas = surface.canvas();
            treemap.paint(canvas, W as f32, H as f32, 0.0);
        }
        let buf = read_rgba(&mut surface, W, H);
        let bands = row_bands(&buf, W, H);
        assert_eq!(
            bands.len(),
            2,
            "label and value must render as two stacked lines, got bands={bands:?}"
        );
        for (y0, y1) in bands {
            let (minx, maxx) = row_ink_x_range(&buf, W, y0, y1);
            let center = (minx + maxx) as f32 / 2.0;
            assert!(
                (center - W as f32 / 2.0).abs() < 12.0,
                "line y=[{y0}..{y1}] is not centered on the box: ink x center = {center}"
            );
        }
    }

    #[test]
    fn a_single_label_still_renders_as_one_centered_line() {
        const W: i32 = 300;
        const H: i32 = 200;
        let mut treemap = base_treemap(vec![TreemapItem {
            label: Some("Alpha".to_string()),
            value: 50.0,
            color: None,
        }]);
        treemap.show_labels = true;
        treemap.show_values = false;
        treemap.animated = false;

        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        {
            let canvas = surface.canvas();
            treemap.paint(canvas, W as f32, H as f32, 0.0);
        }
        let buf = read_rgba(&mut surface, W, H);
        let bands = row_bands(&buf, W, H);
        assert_eq!(
            bands.len(),
            1,
            "a single label is one line, got bands={bands:?}"
        );
    }

    #[test]
    fn progress_ramp_starts_at_start_at_not_at_scene_time_zero() {
        let mut treemap = base_treemap(vec![TreemapItem {
            label: None,
            value: 1.0,
            color: None,
        }]);
        treemap.animation_duration = 1.5;
        treemap.timing = TimingConfig {
            start_at: Some(2.0),
            end_at: None,
        };
        assert_eq!(treemap.progress_at(2.0), 0.0);
        assert!(treemap.progress_at(2.75) < 1.0);
        assert_eq!(treemap.progress_at(3.5), 1.0);
    }
}
