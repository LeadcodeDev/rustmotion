use rustmotion_core::error::Result;
use skia_safe::{Canvas, PaintStyle, Rect};

use rustmotion_core::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, measure_text_with_fallback, paint_from_hex,
};

use super::axes::{contrast_text_color, format_number};
use super::Chart;

impl Chart {
    pub(super) fn render_bar(&self, canvas: &Canvas, w: f32, h: f32, progress: f32) -> Result<()> {
        let (mt, mr, mb, ml) = self.chart_margins();
        let chart_w = w - ml - mr;
        let chart_h = h - mt - mb;

        // A bar chart's baseline is zero, not the smallest datum, so the scale
        // has to span [min(0, min), max(0, max)]. Scaling by `max` alone drew
        // negative bars downward from the bottom edge, straight out of the
        // component box (measured: values [10, -6, 4] in a 800x500 box put
        // 58072 ink pixels outside it, reaching 243px past the bottom).
        // All-positive data is unaffected: min_val stays 0 and the arithmetic
        // reduces to the previous `value / max_val`.
        let (min_val, max_val, range) = self.value_extent();

        let x_labels: Vec<String> = self.data.iter().filter_map(|d| d.label.clone()).collect();

        self.draw_axes(
            canvas, ml, mt, chart_w, chart_h, min_val, max_val, &x_labels, true,
        );

        let n = self.data.len();
        let gap = 8.0;
        let bar_w = (chart_w - gap * (n + 1) as f32) / n as f32;
        let zero_y = mt + chart_h - ((0.0 - min_val) / range) as f32 * chart_h;

        for (i, dp) in self.data.iter().enumerate() {
            let color = dp.color.as_deref().unwrap_or_else(|| self.get_color(i));
            let bar_h = (dp.value.abs() / range) as f32 * chart_h * progress;
            let x = ml + gap + i as f32 * (bar_w + gap);
            let negative = dp.value < 0.0;
            let y = if negative { zero_y } else { zero_y - bar_h };

            let mut paint = paint_from_hex(color);
            paint.set_style(PaintStyle::Fill);
            paint.set_anti_alias(true);

            let rect = Rect::from_xywh(x, y, bar_w, bar_h);
            let radius = (bar_w * 0.15).min(8.0);
            // Round the end away from the baseline.
            let round = (radius, radius).into();
            let square = (0.0, 0.0).into();
            let radii = if negative {
                [square, square, round, round]
            } else {
                [round, round, square, square]
            };
            let rrect = skia_safe::RRect::new_rect_radii(rect, &radii);
            canvas.draw_rrect(rrect, &paint);
        }

        Ok(())
    }

    /// `(min, max, range)` for a zero-anchored value axis over `self.data`.
    /// The range is never zero, so callers can divide by it unconditionally.
    pub(super) fn value_extent(&self) -> (f64, f64, f64) {
        let min_val = self
            .data
            .iter()
            .map(|d| d.value)
            .fold(0.0_f64, f64::min)
            .min(0.0);
        let max_val = self
            .data
            .iter()
            .map(|d| d.value)
            .fold(0.0_f64, f64::max)
            .max(0.0);
        (min_val, max_val, (max_val - min_val).max(0.001))
    }

    pub(super) fn render_horizontal_bar(
        &self,
        canvas: &Canvas,
        w: f32,
        h: f32,
        progress: f32,
    ) -> Result<()> {
        let Some(font) = self.make_label_font() else {
            return Ok(());
        };
        let emoji_font =
            emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, self.label_font_size));
        let (_, metrics) = font.metrics();
        let ascent = -metrics.ascent;
        let measure = |s: &str| measure_text_with_fallback(s, &font, &emoji_font, 0.0);

        // `chart_margins()` sizes the left gutter for *numeric* tick labels
        // (3.5em), which is the wrong axis here: on a horizontal bar chart the
        // vertical axis is categorical. Size the gutter to the widest category
        // name instead, and cap it so the bars keep the majority of the width.
        let mt = 8.0;
        let mr = 8.0;
        let mb = if self.show_x_labels {
            self.label_font_size * 2.0 + 8.0
        } else {
            8.0
        };
        let gutter = if self.show_y_labels {
            let widest = self
                .data
                .iter()
                .filter_map(|d| d.label.as_deref())
                .map(measure)
                .fold(0.0_f32, f32::max);
            (widest + 14.0).min(w * 0.45)
        } else {
            0.0
        };
        let ml = gutter + 8.0;
        let chart_w = (w - ml - mr).max(1.0);
        let chart_h = (h - mt - mb).max(1.0);

        let (min_val, max_val, range) = self.value_extent();
        self.draw_value_axis_x(canvas, ml, mt, chart_w, chart_h, min_val, max_val);

        let n = self.data.len();
        let gap = 8.0;
        let row_h = (chart_h - gap * (n + 1) as f32) / n as f32;
        if row_h <= 0.0 {
            return Ok(());
        }
        let zero_x = ml + ((0.0 - min_val) / range) as f32 * chart_w;

        for (i, dp) in self.data.iter().enumerate() {
            let color = dp.color.as_deref().unwrap_or_else(|| self.get_color(i));
            let bar_len = (dp.value.abs() / range) as f32 * chart_w * progress;
            let negative = dp.value < 0.0;
            let x = if negative { zero_x - bar_len } else { zero_x };
            let y = mt + gap + i as f32 * (row_h + gap);
            let radius = (row_h * 0.15).min(8.0);

            // Track behind every row. Without it a zero-valued row is simply
            // absent — the defect that made a "6 vs 0" comparison unreadable —
            // and short bars have nothing to be read against. Matches the
            // track `radial_bar` already draws for the same reason.
            let mut track_paint = paint_from_hex("#FFFFFF");
            track_paint.set_style(PaintStyle::Fill);
            track_paint.set_anti_alias(true);
            track_paint.set_alpha_f(0.06);
            canvas.draw_rrect(
                skia_safe::RRect::new_rect_xy(
                    Rect::from_xywh(ml, y, chart_w, row_h),
                    radius,
                    radius,
                ),
                &track_paint,
            );

            let mut paint = paint_from_hex(color);
            paint.set_style(PaintStyle::Fill);
            paint.set_anti_alias(true);

            let rect = Rect::from_xywh(x, y, bar_len, row_h);
            // Round the end away from the baseline.
            let round = (radius, radius).into();
            let square = (0.0, 0.0).into();
            let radii = if negative {
                [round, square, square, round]
            } else {
                [square, round, round, square]
            };
            canvas.draw_rrect(skia_safe::RRect::new_rect_radii(rect, &radii), &paint);

            let baseline_y = y + row_h / 2.0 + ascent / 2.0;

            // Value annotation, right-aligned in the track so it never depends
            // on how long the bar happens to be.
            let mut value_left = ml + chart_w;
            if self.show_labels {
                let text = format_number(dp.value);
                let text_w = measure(&text);
                value_left = ml + chart_w - text_w - 10.0;
                let on_bar = !negative && x + bar_len >= value_left;
                let color_hex = if on_bar {
                    contrast_text_color(color)
                } else {
                    self.label_color.clone()
                };
                let mut value_paint = paint_from_hex(&color_hex);
                value_paint.set_anti_alias(true);
                draw_text_with_fallback(
                    canvas,
                    &text,
                    &font,
                    &emoji_font,
                    0.0,
                    value_left,
                    baseline_y,
                    &value_paint,
                );
            }

            let Some(label) = &dp.label else { continue };

            if gutter > 0.0 {
                // Category names live in the left gutter, right-aligned against
                // the bars — the classic ranking layout.
                let mut label_paint = paint_from_hex(&self.label_color);
                label_paint.set_anti_alias(true);
                let lx = (gutter - measure(label)).max(0.0);
                draw_text_with_fallback(
                    canvas,
                    label,
                    &font,
                    &emoji_font,
                    0.0,
                    lx,
                    baseline_y,
                    &label_paint,
                );
            } else if self.show_labels || self.show_x_labels {
                // Inside the bar when it fits, otherwise just past its end in
                // the label colour. Drawing it inside unconditionally left the
                // text of a zero-length bar floating on the background in a
                // colour picked for contrast against a bar that isn't there.
                let label_w = measure(label);
                let fits = bar_len >= label_w + 20.0;
                let (lx, color_hex) = if fits {
                    (x + 10.0, contrast_text_color(color))
                } else {
                    (x + bar_len + 10.0, self.label_color.clone())
                };
                // Never run into the value annotation.
                if lx + label_w <= value_left - 6.0 || fits {
                    let mut label_paint = paint_from_hex(&color_hex);
                    label_paint.set_anti_alias(true);
                    draw_text_with_fallback(
                        canvas,
                        label,
                        &font,
                        &emoji_font,
                        0.0,
                        lx,
                        baseline_y,
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
        self.draw_axes(
            canvas, ml, mt, chart_w, chart_h, 0.0, max_val, &x_labels, true,
        );

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
