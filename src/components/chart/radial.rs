use crate::error::Result;
use skia_safe::{Canvas, PaintStyle, Rect};

use crate::engine::renderer::paint_from_hex;

use super::Chart;

impl Chart {
    pub(super) fn render_radial_bar(
        &self,
        canvas: &Canvas,
        w: f32,
        h: f32,
        progress: f32,
    ) -> Result<()> {
        let cx = w / 2.0;
        let cy = h / 2.0;
        let max_radius = cx.min(cy) - 16.0;
        let n = self.data.len();
        let track_width = (max_radius / (n as f32 * 1.5)).min(20.0).max(6.0);
        let ring_gap = track_width * 0.5;

        let max_val = self
            .data
            .iter()
            .map(|d| d.value)
            .fold(0.0_f64, f64::max)
            .max(0.001);

        for (i, dp) in self.data.iter().enumerate() {
            let r = max_radius - i as f32 * (track_width + ring_gap);
            if r < track_width {
                break;
            }

            let oval = Rect::from_xywh(cx - r, cy - r, r * 2.0, r * 2.0);

            // Track (background)
            let mut track_paint = paint_from_hex("#333333");
            track_paint.set_style(PaintStyle::Stroke);
            track_paint.set_stroke_width(track_width);
            track_paint.set_stroke_cap(skia_safe::paint::Cap::Round);
            track_paint.set_anti_alias(true);
            track_paint.set_alpha_f(0.3);
            canvas.draw_arc(oval, -90.0, 360.0, false, &track_paint);

            // Fill arc
            let color = dp.color.as_deref().unwrap_or_else(|| self.get_color(i));
            let sweep = (dp.value / max_val) as f32 * 360.0 * progress;

            let mut fill_paint = paint_from_hex(color);
            fill_paint.set_style(PaintStyle::Stroke);
            fill_paint.set_stroke_width(track_width);
            fill_paint.set_stroke_cap(skia_safe::paint::Cap::Round);
            fill_paint.set_anti_alias(true);
            canvas.draw_arc(oval, -90.0, sweep, false, &fill_paint);
        }

        Ok(())
    }
}
