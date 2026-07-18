use rustmotion_core::error::Result;
use skia_safe::{Canvas, PaintStyle, Rect};

use rustmotion_core::engine::renderer::paint_from_hex;

use super::Chart;

impl Chart {
    pub(super) fn render_waterfall(
        &self,
        canvas: &Canvas,
        w: f32,
        h: f32,
        progress: f32,
    ) -> Result<()> {
        let (mt, mr, mb, ml) = self.chart_margins();
        let chart_w = w - ml - mr;
        let chart_h = h - mt - mb;

        // Calculate cumulative values and find range
        let mut cumulative = Vec::with_capacity(self.data.len());
        let mut running = 0.0_f64;
        for dp in &self.data {
            let prev = running;
            running += dp.value;
            cumulative.push((prev, running));
        }

        let all_vals: Vec<f64> = cumulative
            .iter()
            .flat_map(|(a, b)| vec![*a, *b])
            .chain(std::iter::once(0.0))
            .collect();
        let min_val = all_vals.iter().fold(f64::MAX, |a, &b| a.min(b));
        let max_val = all_vals.iter().fold(f64::MIN, |a, &b| a.max(b));
        let range = (max_val - min_val).max(0.001);

        let x_labels: Vec<String> = self.data.iter().filter_map(|d| d.label.clone()).collect();
        self.draw_axes(
            canvas, ml, mt, chart_w, chart_h, min_val, max_val, &x_labels, true,
        );

        let n = self.data.len();
        let gap = 6.0;
        let bar_w = (chart_w - gap * (n + 1) as f32) / n as f32;

        // Zero baseline
        let _zero_y = mt + chart_h - ((0.0 - min_val) / range) as f32 * chart_h;

        for (i, dp) in self.data.iter().enumerate() {
            let (start_val, end_val) = cumulative[i];
            let y_start = mt + chart_h - ((start_val - min_val) / range) as f32 * chart_h;
            let y_end = mt + chart_h - ((end_val - min_val) / range) as f32 * chart_h;

            let bar_top = y_start.min(y_end);
            let bar_bottom = y_start.max(y_end);
            let bar_h = (bar_bottom - bar_top) * progress;
            let x = ml + gap + i as f32 * (bar_w + gap);

            // Green for positive, red for negative
            let color = dp.color.as_deref().unwrap_or({
                if dp.value >= 0.0 {
                    "#22C55E"
                } else {
                    "#EF4444"
                }
            });

            let mut paint = paint_from_hex(color);
            paint.set_style(PaintStyle::Fill);
            paint.set_anti_alias(true);

            // Animate from the start position
            let animated_top = if dp.value >= 0.0 {
                y_start - bar_h
            } else {
                y_start
            };

            let rect = Rect::from_xywh(x, animated_top, bar_w, bar_h);
            let radius = (bar_w * 0.1).min(4.0);
            let rrect = skia_safe::RRect::new_rect_xy(rect, radius, radius);
            canvas.draw_rrect(rrect, &paint);

            // Connector line to next bar
            if i + 1 < n {
                let next_x = ml + gap + (i + 1) as f32 * (bar_w + gap);
                let connector_y = if dp.value >= 0.0 {
                    animated_top
                } else {
                    animated_top + bar_h
                };
                let mut connector_paint = paint_from_hex("#FFFFFF30");
                connector_paint.set_style(PaintStyle::Stroke);
                connector_paint.set_stroke_width(1.0);
                connector_paint.set_anti_alias(true);
                // Dashed
                let intervals = [4.0_f32, 4.0];
                if let Some(effect) = skia_safe::PathEffect::dash(&intervals, 0.0) {
                    connector_paint.set_path_effect(effect);
                }
                canvas.draw_line(
                    (x + bar_w, connector_y),
                    (next_x, connector_y),
                    &connector_paint,
                );
            }
        }

        Ok(())
    }
}
