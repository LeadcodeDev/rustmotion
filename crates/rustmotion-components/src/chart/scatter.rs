use rustmotion_core::error::Result;
use skia_safe::{Canvas, PaintStyle};

use rustmotion_core::engine::renderer::paint_from_hex;

use super::Chart;

impl Chart {
    pub(super) fn render_scatter(&self, canvas: &Canvas, w: f32, h: f32, progress: f32) -> Result<()> {
        if self.points.is_empty() {
            return Ok(());
        }

        let (mt, mr, mb, ml) = self.chart_margins();
        let chart_w = w - ml - mr;
        let chart_h = h - mt - mb;

        let min_x = self
            .points
            .iter()
            .map(|p| p.x)
            .fold(f64::MAX, f64::min);
        let max_x = self
            .points
            .iter()
            .map(|p| p.x)
            .fold(f64::MIN, f64::max);
        let min_y = self
            .points
            .iter()
            .map(|p| p.y)
            .fold(f64::MAX, f64::min);
        let max_y = self
            .points
            .iter()
            .map(|p| p.y)
            .fold(f64::MIN, f64::max);

        let range_x = (max_x - min_x).max(0.001);
        let range_y = (max_y - min_y).max(0.001);

        self.draw_axes(canvas, ml, mt, chart_w, chart_h, min_y, max_y, &[]);

        for (i, pt) in self.points.iter().enumerate() {
            let px = ml + ((pt.x - min_x) / range_x) as f32 * chart_w;
            let py = mt + chart_h - ((pt.y - min_y) / range_y) as f32 * chart_h;

            let color = pt
                .color
                .as_deref()
                .unwrap_or_else(|| self.get_color(i));
            let mut paint = paint_from_hex(color);
            paint.set_style(PaintStyle::Fill);
            paint.set_anti_alias(true);
            paint.set_alpha_f(progress);

            canvas.draw_circle((px, py), pt.size * progress, &paint);
        }

        Ok(())
    }
}
