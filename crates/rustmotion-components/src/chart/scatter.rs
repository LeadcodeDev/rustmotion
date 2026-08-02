use rustmotion_core::error::Result;
use skia_safe::{Canvas, PaintStyle};

use rustmotion_core::engine::renderer::paint_from_hex;

use super::axes::format_number;
use super::line::series_scale;
use super::Chart;

impl Chart {
    pub(super) fn render_scatter(
        &self,
        canvas: &Canvas,
        w: f32,
        h: f32,
        progress: f32,
    ) -> Result<()> {
        if self.points.is_empty() {
            return Ok(());
        }

        let (mt, mr, mb, ml) = self.chart_margins();
        let chart_w = w - ml - mr;
        let chart_h = h - mt - mb;

        let (min_x, max_x, norm_x) = series_scale(self.points.iter().map(|p| p.x));
        let (min_y, max_y, norm_y) = series_scale(self.points.iter().map(|p| p.y));

        // `ScatterPoint` carries no label, so the previous empty slice meant
        // `show_x_labels: true` reserved a bottom gutter and then drew nothing
        // into it. A scatter's x axis is numeric: label it with ticks over
        // min_x..max_x, at the same 5 fractions `draw_axes` uses for the y
        // axis. Passing `categorical: false` places tick i at i/(n-1) of the
        // width, which is exactly where those fractions fall.
        let x_labels: Vec<String> = if self.show_x_labels {
            (0..=5)
                .map(|i| format_number(min_x + (max_x - min_x) * (i as f64 / 5.0)))
                .collect()
        } else {
            Vec::new()
        };
        self.draw_axes(
            canvas, ml, mt, chart_w, chart_h, min_y, max_y, &x_labels, false,
        );

        for (i, pt) in self.points.iter().enumerate() {
            let px = ml + norm_x(pt.x) * chart_w;
            let py = mt + chart_h - norm_y(pt.y) * chart_h;

            let color = pt.color.as_deref().unwrap_or_else(|| self.get_color(i));
            let mut paint = paint_from_hex(color);
            paint.set_style(PaintStyle::Fill);
            paint.set_anti_alias(true);
            paint.set_alpha_f(progress);

            canvas.draw_circle((px, py), pt.size * progress, &paint);
        }

        Ok(())
    }
}
