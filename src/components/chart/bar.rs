use anyhow::Result;
use skia_safe::{Canvas, PaintStyle, Rect};

use crate::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, paint_from_hex,
};

use super::axes::contrast_text_color;
use super::Chart;

impl Chart {
    pub(super) fn render_bar(&self, canvas: &Canvas, w: f32, h: f32, progress: f32) -> Result<()> {
        let (mt, mr, mb, ml) = self.chart_margins();
        let chart_w = w - ml - mr;
        let chart_h = h - mt - mb;

        let max_val = self
            .data
            .iter()
            .map(|d| d.value)
            .fold(0.0_f64, f64::max)
            .max(0.001);

        let x_labels: Vec<String> = self
            .data
            .iter()
            .filter_map(|d| d.label.clone())
            .collect();

        self.draw_axes(canvas, ml, mt, chart_w, chart_h, 0.0, max_val, &x_labels);

        let n = self.data.len();
        let gap = 8.0;
        let bar_w = (chart_w - gap * (n + 1) as f32) / n as f32;

        for (i, dp) in self.data.iter().enumerate() {
            let color = dp.color.as_deref().unwrap_or_else(|| self.get_color(i));
            let bar_h = (dp.value / max_val) as f32 * chart_h * progress;
            let x = ml + gap + i as f32 * (bar_w + gap);
            let y = mt + chart_h - bar_h;

            let mut paint = paint_from_hex(color);
            paint.set_style(PaintStyle::Fill);
            paint.set_anti_alias(true);

            let rect = Rect::from_xywh(x, y, bar_w, bar_h);
            let radius = (bar_w * 0.15).min(8.0);
            let rrect = skia_safe::RRect::new_rect_radii(
                rect,
                &[
                    (radius, radius).into(),
                    (radius, radius).into(),
                    (0.0, 0.0).into(),
                    (0.0, 0.0).into(),
                ],
            );
            canvas.draw_rrect(rrect, &paint);
        }

        Ok(())
    }

    pub(super) fn render_horizontal_bar(
        &self,
        canvas: &Canvas,
        w: f32,
        h: f32,
        progress: f32,
    ) -> Result<()> {
        let (mt, mr, mb, ml) = self.chart_margins();
        let chart_w = w - ml - mr;
        let chart_h = h - mt - mb;

        let max_val = self
            .data
            .iter()
            .map(|d| d.value)
            .fold(0.0_f64, f64::max)
            .max(0.001);

        let n = self.data.len();
        let gap = 8.0;
        let bar_h = (chart_h - gap * (n + 1) as f32) / n as f32;

        let font = self.make_label_font();
        let emoji_font =
            emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, self.label_font_size));
        let (_, metrics) = font.metrics();
        let ascent = -metrics.ascent;

        for (i, dp) in self.data.iter().enumerate() {
            let color = dp.color.as_deref().unwrap_or_else(|| self.get_color(i));
            let bar_w = (dp.value / max_val) as f32 * chart_w * progress;
            let x = ml;
            let y = mt + gap + i as f32 * (bar_h + gap);

            let mut paint = paint_from_hex(color);
            paint.set_style(PaintStyle::Fill);
            paint.set_anti_alias(true);

            let rect = Rect::from_xywh(x, y, bar_w, bar_h);
            let radius = (bar_h * 0.15).min(8.0);
            let rrect = skia_safe::RRect::new_rect_radii(
                rect,
                &[
                    (0.0, 0.0).into(),
                    (radius, radius).into(),
                    (radius, radius).into(),
                    (0.0, 0.0).into(),
                ],
            );
            canvas.draw_rrect(rrect, &paint);

            // Draw label inside the bar
            if self.show_labels || self.show_x_labels {
                if let Some(label) = &dp.label {
                    let contrast_color = contrast_text_color(color);
                    let mut label_paint = paint_from_hex(&contrast_color);
                    label_paint.set_anti_alias(true);
                    let lx = x + 10.0;
                    let ly = y + bar_h / 2.0 + ascent / 2.0;
                    draw_text_with_fallback(
                        canvas,
                        label,
                        &font,
                        &emoji_font,
                        0.0,
                        lx,
                        ly,
                        &label_paint,
                    );
                }
            }
        }

        Ok(())
    }

    pub(super) fn render_stacked_bar(
        &self,
        canvas: &Canvas,
        w: f32,
        h: f32,
        progress: f32,
    ) -> Result<()> {
        if self.series.is_empty() || self.categories.is_empty() {
            return Ok(());
        }

        let (mt, mr, mb, ml) = self.chart_margins();
        let chart_w = w - ml - mr;
        let chart_h = h - mt - mb;

        let n_cats = self.categories.len();
        // Find max stacked total
        let max_val = (0..n_cats)
            .map(|ci| {
                self.series
                    .iter()
                    .map(|s| s.data.get(ci).copied().unwrap_or(0.0))
                    .sum::<f64>()
            })
            .fold(0.0_f64, f64::max)
            .max(0.001);

        let x_labels: Vec<String> = self.categories.clone();
        self.draw_axes(canvas, ml, mt, chart_w, chart_h, 0.0, max_val, &x_labels);

        let gap = 8.0;
        let bar_w = (chart_w - gap * (n_cats + 1) as f32) / n_cats as f32;

        for ci in 0..n_cats {
            let x = ml + gap + ci as f32 * (bar_w + gap);
            let mut cumulative_h = 0.0_f32;

            for (si, series) in self.series.iter().enumerate() {
                let val = series.data.get(ci).copied().unwrap_or(0.0);
                let seg_h = (val / max_val) as f32 * chart_h * progress;
                let y = mt + chart_h - cumulative_h - seg_h;

                let color = series
                    .color
                    .as_deref()
                    .unwrap_or_else(|| self.get_color(si));
                let mut paint = paint_from_hex(color);
                paint.set_style(PaintStyle::Fill);
                paint.set_anti_alias(true);

                let rect = Rect::from_xywh(x, y, bar_w, seg_h);
                // Rounded top on the topmost segment only
                if si == self.series.len() - 1 {
                    let radius = (bar_w * 0.15).min(8.0);
                    let rrect = skia_safe::RRect::new_rect_radii(
                        rect,
                        &[
                            (radius, radius).into(),
                            (radius, radius).into(),
                            (0.0, 0.0).into(),
                            (0.0, 0.0).into(),
                        ],
                    );
                    canvas.draw_rrect(rrect, &paint);
                } else {
                    canvas.draw_rect(rect, &paint);
                }

                cumulative_h += seg_h;
            }
        }

        Ok(())
    }
}
