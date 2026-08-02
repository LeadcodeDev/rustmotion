use rustmotion_core::error::Result;
use skia_safe::{Canvas, PaintStyle, Path};

use rustmotion_core::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, measure_text_with_fallback, paint_from_hex,
};

use super::Chart;

impl Chart {
    pub(super) fn render_radar(
        &self,
        canvas: &Canvas,
        w: f32,
        h: f32,
        progress: f32,
    ) -> Result<()> {
        let n_axes = self.axes.len();
        if n_axes < 3 || self.radar_data.is_empty() {
            return Ok(());
        }

        let cx = w / 2.0;
        let cy = h / 2.0;
        let radius = cx.min(cy) - 24.0;

        let angle_step = std::f32::consts::TAU / n_axes as f32;

        // Draw concentric grid polygons
        let grid_levels = 4;
        for level in 1..=grid_levels {
            let r = radius * (level as f32 / grid_levels as f32);
            let mut grid_path = Path::new();
            for i in 0..n_axes {
                let angle = -std::f32::consts::FRAC_PI_2 + i as f32 * angle_step;
                let px = cx + r * angle.cos();
                let py = cy + r * angle.sin();
                if i == 0 {
                    grid_path.move_to((px, py));
                } else {
                    grid_path.line_to((px, py));
                }
            }
            grid_path.close();

            let mut grid_paint = paint_from_hex(&self.grid_color);
            if grid_paint.alpha() < 10 {
                grid_paint = paint_from_hex("#FFFFFF20");
            }
            grid_paint.set_style(PaintStyle::Stroke);
            grid_paint.set_stroke_width(1.0);
            grid_paint.set_anti_alias(true);
            canvas.draw_path(&grid_path, &grid_paint);
        }

        // Draw axis lines
        for i in 0..n_axes {
            let angle = -std::f32::consts::FRAC_PI_2 + i as f32 * angle_step;
            let px = cx + radius * angle.cos();
            let py = cy + radius * angle.sin();
            let mut axis_paint = paint_from_hex("#FFFFFF15");
            axis_paint.set_style(PaintStyle::Stroke);
            axis_paint.set_stroke_width(1.0);
            axis_paint.set_anti_alias(true);
            canvas.draw_line((cx, cy), (px, py), &axis_paint);
        }

        // Draw axis labels
        let Some(font) = self.make_label_font() else {
            return Ok(());
        };
        let emoji_font =
            emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, self.label_font_size));
        let (_, metrics) = font.metrics();
        let ascent = -metrics.ascent;

        for (i, label) in self.axes.iter().enumerate() {
            let angle = -std::f32::consts::FRAC_PI_2 + i as f32 * angle_step;
            let label_r = radius + 12.0;
            let px = cx + label_r * angle.cos();
            let py = cy + label_r * angle.sin();

            let mut label_paint = paint_from_hex(&self.label_color);
            label_paint.set_anti_alias(true);
            let label_w = measure_text_with_fallback(label, &font, &emoji_font, 0.0);
            // Anchor by side — centring every label pushed the east/west ones
            // out of the component box (measured: 389 ink pixels outside a
            // 400x400 box, reaching 28px left and 45px right of it). Labels on
            // the right start at the vertex, labels on the left end at it, and
            // the result is clamped into the box as a backstop.
            let cos = angle.cos();
            let anchored = if cos > 0.15 {
                px
            } else if cos < -0.15 {
                px - label_w
            } else {
                px - label_w / 2.0
            };
            let lx = anchored.clamp(0.0, (w - label_w).max(0.0));
            let ly = py + ascent / 2.0;
            draw_text_with_fallback(canvas, label, &font, &emoji_font, 0.0, lx, ly, &label_paint);
        }

        // One scale for every series. Normalising each polygon by its own max
        // made a [3, 2, 1, 2] series fill nearly the same area as a [10, 8, 6,
        // 9] one, so overlaid series could not be compared at all — the single
        // thing a radar chart exists to do.
        let global_max = self
            .radar_data
            .iter()
            .flat_map(|rd| rd.values.iter().copied())
            .fold(0.0_f64, f64::max)
            .max(0.001);

        // Draw data polygons
        for (di, rd) in self.radar_data.iter().enumerate() {
            if rd.values.len() != n_axes {
                continue;
            }

            let color_str = rd.color.as_deref().unwrap_or_else(|| self.get_color(di));

            let mut data_path = Path::new();
            for (i, &val) in rd.values.iter().enumerate() {
                let norm = (val.max(0.0) / global_max) as f32 * progress;
                let angle = -std::f32::consts::FRAC_PI_2 + i as f32 * angle_step;
                let r = radius * norm;
                let px = cx + r * angle.cos();
                let py = cy + r * angle.sin();
                if i == 0 {
                    data_path.move_to((px, py));
                } else {
                    data_path.line_to((px, py));
                }
            }
            data_path.close();

            // Fill
            let mut fill_paint = paint_from_hex(color_str);
            fill_paint.set_style(PaintStyle::Fill);
            fill_paint.set_alpha_f(0.3);
            fill_paint.set_anti_alias(true);
            canvas.draw_path(&data_path, &fill_paint);

            // Stroke
            let mut stroke_paint = paint_from_hex(color_str);
            stroke_paint.set_style(PaintStyle::Stroke);
            stroke_paint.set_stroke_width(2.0);
            stroke_paint.set_anti_alias(true);
            canvas.draw_path(&data_path, &stroke_paint);
        }

        Ok(())
    }
}
