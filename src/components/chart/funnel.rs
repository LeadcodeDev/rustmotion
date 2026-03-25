use anyhow::Result;
use skia_safe::{Canvas, PaintStyle, Path};

use crate::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, measure_text_with_fallback, paint_from_hex,
};

use super::axes::contrast_text_color;
use super::Chart;

impl Chart {
    pub(super) fn render_funnel(&self, canvas: &Canvas, w: f32, h: f32, progress: f32) -> Result<()> {
        let horizontal = self
            .direction
            .as_deref()
            .map(|d| d == "horizontal")
            .unwrap_or(false);

        if horizontal {
            self.render_funnel_horizontal(canvas, w, h, progress)
        } else {
            self.render_funnel_vertical(canvas, w, h, progress)
        }
    }

    fn render_funnel_vertical(&self, canvas: &Canvas, w: f32, h: f32, progress: f32) -> Result<()> {
        let n = self.data.len();
        let max_val = self
            .data
            .iter()
            .map(|d| d.value)
            .fold(0.0_f64, f64::max)
            .max(0.001);

        let gap = 4.0;
        let total_gap = gap * (n.saturating_sub(1)) as f32;
        let seg_h = (h - total_gap) / n as f32;

        let font = self.make_label_font();
        let emoji_font =
            emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, self.label_font_size));
        let (_, metrics) = font.metrics();
        let ascent = -metrics.ascent;

        for (i, dp) in self.data.iter().enumerate() {
            let ratio = (dp.value / max_val) as f32 * progress;
            let next_ratio = if i + 1 < n {
                (self.data[i + 1].value / max_val) as f32 * progress
            } else {
                ratio * 0.6
            };

            let top_w = w * ratio;
            let bot_w = w * next_ratio;
            let top_x = (w - top_w) / 2.0;
            let bot_x = (w - bot_w) / 2.0;
            let y_top = i as f32 * (seg_h + gap);
            let y_bot = y_top + seg_h;

            let color = dp.color.as_deref().unwrap_or_else(|| self.get_color(i));
            let mut paint = paint_from_hex(color);
            paint.set_style(PaintStyle::Fill);
            paint.set_anti_alias(true);

            let mut path = Path::new();
            path.move_to((top_x, y_top));
            path.line_to((top_x + top_w, y_top));
            path.line_to((bot_x + bot_w, y_bot));
            path.line_to((bot_x, y_bot));
            path.close();
            canvas.draw_path(&path, &paint);

            if self.show_labels {
                if let Some(label) = &dp.label {
                    let contrast = contrast_text_color(color);
                    let mut label_paint = paint_from_hex(&contrast);
                    label_paint.set_anti_alias(true);
                    let label_w = measure_text_with_fallback(label, &font, &emoji_font, 0.0);
                    let lx = (w - label_w) / 2.0;
                    let ly = y_top + seg_h / 2.0 + ascent / 2.0;
                    draw_text_with_fallback(
                        canvas, label, &font, &emoji_font, 0.0, lx, ly, &label_paint,
                    );
                }
            }
        }

        Ok(())
    }

    fn render_funnel_horizontal(&self, canvas: &Canvas, w: f32, h: f32, progress: f32) -> Result<()> {
        let n = self.data.len();
        let max_val = self
            .data
            .iter()
            .map(|d| d.value)
            .fold(0.0_f64, f64::max)
            .max(0.001);

        let gap = 4.0;
        let total_gap = gap * (n.saturating_sub(1)) as f32;
        let seg_w = (w - total_gap) / n as f32;

        let font = self.make_label_font();
        let emoji_font =
            emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, self.label_font_size));
        let (_, metrics) = font.metrics();
        let ascent = -metrics.ascent;

        for (i, dp) in self.data.iter().enumerate() {
            let ratio = (dp.value / max_val) as f32 * progress;
            let next_ratio = if i + 1 < n {
                (self.data[i + 1].value / max_val) as f32 * progress
            } else {
                ratio * 0.6
            };

            let left_h = h * ratio;
            let right_h = h * next_ratio;
            let left_y = (h - left_h) / 2.0;
            let right_y = (h - right_h) / 2.0;
            let x_left = i as f32 * (seg_w + gap);
            let x_right = x_left + seg_w;

            let color = dp.color.as_deref().unwrap_or_else(|| self.get_color(i));
            let mut paint = paint_from_hex(color);
            paint.set_style(PaintStyle::Fill);
            paint.set_anti_alias(true);

            let mut path = Path::new();
            path.move_to((x_left, left_y));
            path.line_to((x_right, right_y));
            path.line_to((x_right, right_y + right_h));
            path.line_to((x_left, left_y + left_h));
            path.close();
            canvas.draw_path(&path, &paint);

            if self.show_labels {
                if let Some(label) = &dp.label {
                    let contrast = contrast_text_color(color);
                    let mut label_paint = paint_from_hex(&contrast);
                    label_paint.set_anti_alias(true);
                    let label_w = measure_text_with_fallback(label, &font, &emoji_font, 0.0);
                    let lx = x_left + (seg_w - label_w) / 2.0;
                    let ly = h / 2.0 + ascent / 2.0;
                    draw_text_with_fallback(
                        canvas, label, &font, &emoji_font, 0.0, lx, ly, &label_paint,
                    );
                }
            }
        }

        Ok(())
    }
}
