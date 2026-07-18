use rustmotion_core::error::Result;
use skia_safe::{Canvas, PaintStyle, Path, Rect};

use rustmotion_core::engine::renderer::paint_from_hex;

use super::Chart;

impl Chart {
    pub(super) fn render_pie(&self, canvas: &Canvas, w: f32, h: f32, progress: f32) -> Result<()> {
        let total: f64 = self.data.iter().map(|d| d.value).sum();
        if total <= 0.0 {
            return Ok(());
        }

        let cx = w / 2.0;
        let cy = h / 2.0;
        let radius = cx.min(cy) - 8.0;
        let oval = Rect::from_xywh(cx - radius, cy - radius, radius * 2.0, radius * 2.0);

        let total_sweep = 360.0 * progress;
        let mut start_angle = -90.0_f32;

        for (i, dp) in self.data.iter().enumerate() {
            let sweep = (dp.value / total) as f32 * total_sweep;
            let color = dp.color.as_deref().unwrap_or_else(|| self.get_color(i));

            let mut paint = paint_from_hex(color);
            paint.set_style(PaintStyle::Fill);
            paint.set_anti_alias(true);

            let mut path = Path::new();
            path.move_to((cx, cy));
            path.arc_to(oval, start_angle, sweep, false);
            path.close();
            canvas.draw_path(&path, &paint);

            start_angle += sweep;
        }

        Ok(())
    }

    pub(super) fn render_donut(
        &self,
        canvas: &Canvas,
        w: f32,
        h: f32,
        progress: f32,
    ) -> Result<()> {
        // Draw pie first
        self.render_pie(canvas, w, h, progress)?;

        // Punch out center circle
        let cx = w / 2.0;
        let cy = h / 2.0;
        let outer_radius = cx.min(cy) - 8.0;
        let inner_r = outer_radius * self.inner_radius.clamp(0.1, 0.95) as f32;

        // Draw a circle filled with the background color (transparent black by default)
        // We use save/restore with a blend mode to cut out the center
        let mut center_paint = skia_safe::Paint::default();
        center_paint.set_style(PaintStyle::Fill);
        center_paint.set_anti_alias(true);
        center_paint.set_blend_mode(skia_safe::BlendMode::Clear);
        canvas.draw_circle((cx, cy), inner_r, &center_paint);

        Ok(())
    }
}
