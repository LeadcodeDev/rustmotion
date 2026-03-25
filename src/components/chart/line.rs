use crate::error::Result;
use skia_safe::{Canvas, Color, PaintStyle, Path, Point, Rect};

use crate::engine::renderer::{paint_from_hex, parse_hex_color};

use super::Chart;

impl Chart {
    pub(super) fn render_line(&self, canvas: &Canvas, w: f32, h: f32, progress: f32) -> Result<()> {
        let (mt, mr, mb, ml) = self.chart_margins();
        let chart_w = w - ml - mr;
        let chart_h = h - mt - mb;

        let max_val = self
            .data
            .iter()
            .map(|d| d.value)
            .fold(0.0_f64, f64::max)
            .max(0.001);
        let min_val = self
            .data
            .iter()
            .map(|d| d.value)
            .fold(f64::MAX, f64::min);
        let range = (max_val - min_val).max(0.001);

        let n = self.data.len();
        if n < 2 {
            return Ok(());
        }

        let x_labels: Vec<String> = self
            .data
            .iter()
            .filter_map(|d| d.label.clone())
            .collect();
        self.draw_axes(canvas, ml, mt, chart_w, chart_h, min_val, max_val, &x_labels);

        let mut path = Path::new();
        let mut fill_path = Path::new();

        for (i, dp) in self.data.iter().enumerate() {
            let x = ml + (i as f32 / (n - 1) as f32) * chart_w;
            let y = mt + chart_h - ((dp.value - min_val) / range) as f32 * chart_h;

            if i == 0 {
                path.move_to((x, y));
                fill_path.move_to((x, mt + chart_h));
                fill_path.line_to((x, y));
            } else {
                path.line_to((x, y));
                fill_path.line_to((x, y));
            }
        }

        let last_x = ml + chart_w;
        fill_path.line_to((last_x, mt + chart_h));
        fill_path.close();

        // Clip for animation
        let clip_w = w * progress;
        canvas.save();
        canvas.clip_rect(
            Rect::from_xywh(0.0, 0.0, clip_w, h),
            skia_safe::ClipOp::Intersect,
            false,
        );

        // Fill under line
        let line_color = self.get_color(0);
        let mut fill_paint = paint_from_hex(line_color);
        fill_paint.set_style(PaintStyle::Fill);
        fill_paint.set_alpha_f(0.15);
        canvas.draw_path(&fill_path, &fill_paint);

        // Line stroke
        let mut line_paint = paint_from_hex(line_color);
        line_paint.set_style(PaintStyle::Stroke);
        line_paint.set_stroke_width(2.5);
        line_paint.set_anti_alias(true);
        canvas.draw_path(&path, &line_paint);

        // Dots
        for (i, dp) in self.data.iter().enumerate() {
            let x = ml + (i as f32 / (n - 1) as f32) * chart_w;
            let y = mt + chart_h - ((dp.value - min_val) / range) as f32 * chart_h;

            let dot_color = dp.color.as_deref().unwrap_or(line_color);
            let mut dot_paint = paint_from_hex(dot_color);
            dot_paint.set_style(PaintStyle::Fill);
            dot_paint.set_anti_alias(true);
            canvas.draw_circle((x, y), 4.0, &dot_paint);
        }

        canvas.restore();
        Ok(())
    }

    pub(super) fn render_area(&self, canvas: &Canvas, w: f32, h: f32, progress: f32) -> Result<()> {
        let (mt, mr, mb, ml) = self.chart_margins();
        let chart_w = w - ml - mr;
        let chart_h = h - mt - mb;

        let max_val = self
            .data
            .iter()
            .map(|d| d.value)
            .fold(0.0_f64, f64::max)
            .max(0.001);
        let min_val = self
            .data
            .iter()
            .map(|d| d.value)
            .fold(f64::MAX, f64::min);
        let range = (max_val - min_val).max(0.001);

        let n = self.data.len();
        if n < 2 {
            return Ok(());
        }

        let x_labels: Vec<String> = self
            .data
            .iter()
            .filter_map(|d| d.label.clone())
            .collect();
        self.draw_axes(canvas, ml, mt, chart_w, chart_h, min_val, max_val, &x_labels);

        // Compute points
        let pts: Vec<(f32, f32)> = self
            .data
            .iter()
            .enumerate()
            .map(|(i, dp)| {
                let x = ml + (i as f32 / (n - 1) as f32) * chart_w;
                let y = mt + chart_h - ((dp.value - min_val) / range) as f32 * chart_h;
                (x, y)
            })
            .collect();

        let mut line_path = Path::new();
        let mut fill_path = Path::new();

        if self.smooth && pts.len() >= 3 {
            // Catmull-Rom -> cubic bezier for smooth curves
            line_path.move_to(pts[0]);
            fill_path.move_to((pts[0].0, mt + chart_h));
            fill_path.line_to(pts[0]);

            for i in 0..pts.len() - 1 {
                let p0 = if i > 0 { pts[i - 1] } else { pts[i] };
                let p1 = pts[i];
                let p2 = pts[i + 1];
                let p3 = if i + 2 < pts.len() {
                    pts[i + 2]
                } else {
                    pts[i + 1]
                };

                let cp1x = p1.0 + (p2.0 - p0.0) / 6.0;
                let cp1y = p1.1 + (p2.1 - p0.1) / 6.0;
                let cp2x = p2.0 - (p3.0 - p1.0) / 6.0;
                let cp2y = p2.1 - (p3.1 - p1.1) / 6.0;

                line_path.cubic_to((cp1x, cp1y), (cp2x, cp2y), p2);
                fill_path.cubic_to((cp1x, cp1y), (cp2x, cp2y), p2);
            }
        } else {
            for (i, &(x, y)) in pts.iter().enumerate() {
                if i == 0 {
                    line_path.move_to((x, y));
                    fill_path.move_to((x, mt + chart_h));
                    fill_path.line_to((x, y));
                } else {
                    line_path.line_to((x, y));
                    fill_path.line_to((x, y));
                }
            }
        }

        let last_x = pts.last().map(|p| p.0).unwrap_or(ml + chart_w);
        fill_path.line_to((last_x, mt + chart_h));
        fill_path.close();

        // Clip for animation
        let clip_w = w * progress;
        canvas.save();
        canvas.clip_rect(
            Rect::from_xywh(0.0, 0.0, clip_w, h),
            skia_safe::ClipOp::Intersect,
            false,
        );

        // Gradient fill
        let line_color = self.get_color(0);
        let (r, g, b, _) = parse_hex_color(line_color);
        let top_color = Color::from_argb((self.fill_opacity * 255.0) as u8, r, g, b);
        let bottom_color = Color::from_argb(0, r, g, b);

        let shader = skia_safe::shader::Shader::linear_gradient(
            (Point::new(0.0, mt), Point::new(0.0, mt + chart_h)),
            skia_safe::gradient_shader::GradientShaderColors::Colors(&[top_color, bottom_color]),
            None,
            skia_safe::TileMode::Clamp,
            None,
            None,
        );

        if let Some(shader) = shader {
            let mut fill_paint = skia_safe::Paint::default();
            fill_paint.set_style(PaintStyle::Fill);
            fill_paint.set_anti_alias(true);
            fill_paint.set_shader(shader);
            canvas.draw_path(&fill_path, &fill_paint);
        }

        // Line stroke
        let mut line_paint = paint_from_hex(line_color);
        line_paint.set_style(PaintStyle::Stroke);
        line_paint.set_stroke_width(2.5);
        line_paint.set_anti_alias(true);
        canvas.draw_path(&line_path, &line_paint);

        // Dots
        for &(x, y) in &pts {
            let mut dot_paint = paint_from_hex(line_color);
            dot_paint.set_style(PaintStyle::Fill);
            dot_paint.set_anti_alias(true);
            canvas.draw_circle((x, y), 4.0, &dot_paint);
        }

        canvas.restore();
        Ok(())
    }
}
