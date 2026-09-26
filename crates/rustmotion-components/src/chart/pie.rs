use rustmotion_core::error::Result;
use skia_safe::{Canvas, PaintStyle, PathBuilder, Rect};

use rustmotion_core::engine::renderer::paint_from_hex;

use super::Chart;

const FULL_SWEEP: f32 = 359.99;

impl Chart {
    pub(super) fn render_pie(&self, canvas: &Canvas, w: f32, h: f32, progress: f32) -> Result<()> {
        self.render_pie_ring(canvas, w, h, progress, 0.0)
    }

    pub(super) fn render_donut(
        &self,
        canvas: &Canvas,
        w: f32,
        h: f32,
        progress: f32,
    ) -> Result<()> {
        let inner = self.inner_radius.clamp(0.1, 0.95) as f32;
        self.render_pie_ring(canvas, w, h, progress, inner)
    }

    fn render_pie_ring(
        &self,
        canvas: &Canvas,
        w: f32,
        h: f32,
        progress: f32,
        inner_ratio: f32,
    ) -> Result<()> {
        let values: Vec<f64> = self.data.iter().map(|d| d.value.max(0.0)).collect();
        let total: f64 = values.iter().sum();
        if total <= 0.0 {
            return Ok(());
        }

        let cx = w / 2.0;
        let cy = h / 2.0;
        let outer_r = cx.min(cy) - 8.0;
        if outer_r <= 0.0 {
            return Ok(());
        }
        let inner_r = outer_r * inner_ratio;

        let outer = Rect::from_xywh(cx - outer_r, cy - outer_r, outer_r * 2.0, outer_r * 2.0);
        let inner = Rect::from_xywh(cx - inner_r, cy - inner_r, inner_r * 2.0, inner_r * 2.0);

        let total_sweep = 360.0 * progress;
        let mut start_angle = -90.0_f32;

        for (i, &value) in values.iter().enumerate() {
            let sweep = (value / total) as f32 * total_sweep;
            if sweep <= 0.0 {
                continue;
            }
            let color = self.data[i]
                .color
                .as_deref()
                .unwrap_or_else(|| self.get_color(i));

            let mut paint = paint_from_hex(color);
            paint.set_style(PaintStyle::Fill);
            paint.set_anti_alias(true);

            if sweep >= FULL_SWEEP {
                if inner_r > 0.0 {
                    let mut ring = paint;
                    ring.set_style(PaintStyle::Stroke);
                    ring.set_stroke_width(outer_r - inner_r);
                    canvas.draw_circle((cx, cy), (outer_r + inner_r) / 2.0, &ring);
                } else {
                    canvas.draw_circle((cx, cy), outer_r, &paint);
                }
            } else {
                let mut path = PathBuilder::new();
                if inner_r > 0.0 {
                    path.arc_to(outer, start_angle, sweep, false);
                    path.arc_to(inner, start_angle + sweep, -sweep, false);
                } else {
                    path.move_to((cx, cy));
                    path.arc_to(outer, start_angle, sweep, false);
                }
                path.close();
                canvas.draw_path(&path.detach(), &paint);
            }

            start_angle += sweep;
        }

        Ok(())
    }
}
