use rustmotion_core::error::Result;
use skia_safe::gradient::{self, Colors, Gradient};
use skia_safe::{Canvas, Color, Color4f, PaintStyle, PathBuilder, Point, Rect};

use rustmotion_core::engine::renderer::{paint_from_hex, parse_hex_color};

use super::Chart;

/// `(min, max, normalize)` for a min–max scaled series.
///
/// `normalize` maps a value to `0.0..=1.0` (0 = bottom of the plot area).
/// When every value is identical the series is centred rather than pinned to
/// the axis: dividing by a `max(0.001)` floor mapped a constant series to 0,
/// so a flat line at, say, 7 rendered sitting exactly on the zero gridline and
/// read as a series of zeroes.
pub(super) fn series_scale(
    values: impl Iterator<Item = f64> + Clone,
) -> (f64, f64, impl Fn(f64) -> f32) {
    let min_val = values.clone().fold(f64::INFINITY, f64::min);
    let max_val = values.fold(f64::NEG_INFINITY, f64::max);
    let (min_val, max_val) = if min_val.is_finite() && max_val.is_finite() {
        (min_val, max_val)
    } else {
        (0.0, 0.0)
    };
    let span = max_val - min_val;
    let flat = span.abs() < f64::EPSILON;
    let range = if flat { 1.0 } else { span };
    (min_val, max_val, move |v: f64| {
        if flat {
            0.5
        } else {
            ((v - min_val) / range) as f32
        }
    })
}

pub(super) fn zero_anchored_scale(
    values: impl Iterator<Item = f64> + Clone,
) -> (f64, f64, impl Fn(f64) -> f32) {
    let min_val = values.clone().fold(f64::INFINITY, f64::min);
    let max_val = values.fold(f64::NEG_INFINITY, f64::max);
    let (min_val, max_val) = if min_val.is_finite() && max_val.is_finite() {
        (min_val, max_val)
    } else {
        (0.0, 0.0)
    };
    let flat = (max_val - min_val).abs() < f64::EPSILON;
    let domain_min = if flat { min_val } else { min_val.min(0.0) };
    let domain_max = if flat { max_val } else { max_val.max(0.0) };
    let range = if flat { 1.0 } else { domain_max - domain_min };
    (domain_min, domain_max, move |v: f64| {
        if flat {
            0.5
        } else {
            ((v - domain_min) / range) as f32
        }
    })
}

impl Chart {
    pub(super) fn render_line(&self, canvas: &Canvas, w: f32, h: f32, progress: f32) -> Result<()> {
        let (mt, mr, mb, ml) = self.chart_margins();
        let chart_w = w - ml - mr;
        let chart_h = h - mt - mb;

        let (min_val, max_val, norm) = zero_anchored_scale(self.data.iter().map(|d| d.value));
        let zero_y = mt + chart_h - norm(0.0) * chart_h;

        let n = self.data.len();
        let x_labels: Vec<String> = self
            .data
            .iter()
            .map(|d| d.label.clone().unwrap_or_default())
            .collect();
        self.draw_axes(
            canvas, ml, mt, chart_w, chart_h, min_val, max_val, &x_labels, false,
        );
        if n < 2 {
            // A one-point series has no line to draw, but returning before the
            // axes were painted made the whole component render nothing at all
            // (measured: 0 ink pixels, and `validate` clean). Plot the point.
            if let Some(dp) = self.data.first() {
                let x = ml + chart_w / 2.0;
                let y = mt + chart_h - norm(dp.value) * chart_h;
                let color = dp.color.as_deref().unwrap_or_else(|| self.get_color(0));
                let mut dot_paint = paint_from_hex(color);
                dot_paint.set_style(PaintStyle::Fill);
                dot_paint.set_anti_alias(true);
                canvas.draw_circle((x, y), 4.0, &dot_paint);
            }
            return Ok(());
        }

        let mut path = PathBuilder::new();
        let mut fill_path = PathBuilder::new();

        for (i, dp) in self.data.iter().enumerate() {
            let x = ml + (i as f32 / (n - 1) as f32) * chart_w;
            let y = mt + chart_h - norm(dp.value) * chart_h;

            if i == 0 {
                path.move_to((x, y));
                fill_path.move_to((x, zero_y));
                fill_path.line_to((x, y));
            } else {
                path.line_to((x, y));
                fill_path.line_to((x, y));
            }
        }

        let last_x = ml + chart_w;
        fill_path.line_to((last_x, zero_y));
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
        canvas.draw_path(&fill_path.detach(), &fill_paint);

        // Line stroke
        let mut line_paint = paint_from_hex(line_color);
        line_paint.set_style(PaintStyle::Stroke);
        line_paint.set_stroke_width(2.5);
        line_paint.set_anti_alias(true);
        canvas.draw_path(&path.detach(), &line_paint);

        // Dots
        for (i, dp) in self.data.iter().enumerate() {
            let x = ml + (i as f32 / (n - 1) as f32) * chart_w;
            let y = mt + chart_h - norm(dp.value) * chart_h;

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

        let (min_val, max_val, norm) = zero_anchored_scale(self.data.iter().map(|d| d.value));
        let zero_y = mt + chart_h - norm(0.0) * chart_h;

        let n = self.data.len();
        let x_labels: Vec<String> = self
            .data
            .iter()
            .map(|d| d.label.clone().unwrap_or_default())
            .collect();
        self.draw_axes(
            canvas, ml, mt, chart_w, chart_h, min_val, max_val, &x_labels, false,
        );
        if n < 2 {
            // See `render_line`: draw the lone point rather than nothing.
            if let Some(dp) = self.data.first() {
                let x = ml + chart_w / 2.0;
                let y = mt + chart_h - norm(dp.value) * chart_h;
                let mut dot_paint = paint_from_hex(self.get_color(0));
                dot_paint.set_style(PaintStyle::Fill);
                dot_paint.set_anti_alias(true);
                canvas.draw_circle((x, y), 4.0, &dot_paint);
            }
            return Ok(());
        }

        // Compute points
        let pts: Vec<(f32, f32)> = self
            .data
            .iter()
            .enumerate()
            .map(|(i, dp)| {
                let x = ml + (i as f32 / (n - 1) as f32) * chart_w;
                let y = mt + chart_h - norm(dp.value) * chart_h;
                (x, y)
            })
            .collect();

        let mut line_path = PathBuilder::new();
        let mut fill_path = PathBuilder::new();

        if self.smooth && pts.len() >= 3 {
            // Catmull-Rom -> cubic bezier for smooth curves
            line_path.move_to(pts[0]);
            fill_path.move_to((pts[0].0, zero_y));
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
                    fill_path.move_to((x, zero_y));
                    fill_path.line_to((x, y));
                } else {
                    line_path.line_to((x, y));
                    fill_path.line_to((x, y));
                }
            }
        }

        let last_x = pts.last().map(|p| p.0).unwrap_or(ml + chart_w);
        fill_path.line_to((last_x, zero_y));
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

        let colors4f = [Color4f::from(top_color), Color4f::from(bottom_color)];
        let stops = Colors::new(&colors4f, None, skia_safe::TileMode::Clamp, None);
        let grad = Gradient::new(stops, gradient::Interpolation::default());
        let shader = gradient::shaders::linear_gradient(
            (Point::new(0.0, mt), Point::new(0.0, mt + chart_h)),
            &grad,
            None,
        );

        if let Some(shader) = shader {
            let mut fill_paint = skia_safe::Paint::default();
            fill_paint.set_style(PaintStyle::Fill);
            fill_paint.set_anti_alias(true);
            fill_paint.set_shader(shader);
            canvas.draw_path(&fill_path.detach(), &fill_paint);
        }

        // Line stroke
        let mut line_paint = paint_from_hex(line_color);
        line_paint.set_style(PaintStyle::Stroke);
        line_paint.set_stroke_width(2.5);
        line_paint.set_anti_alias(true);
        canvas.draw_path(&line_path.detach(), &line_paint);

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chart::{ChartDataPoint, ChartType};
    use rustmotion_core::css::CssStyle;
    use rustmotion_core::traits::TimingConfig;

    fn base_chart(chart_type: ChartType, data: Vec<ChartDataPoint>) -> Chart {
        Chart {
            chart_type,
            data,
            animated: false,
            animation_duration: 1.5,
            colors: None,
            inner_radius: 0.6,
            max: 100.0,
            fill_opacity: 0.3,
            smooth: false,
            categories: Vec::new(),
            series: Vec::new(),
            axes: Vec::new(),
            radar_data: Vec::new(),
            points: Vec::new(),
            direction: None,
            show_grid: false,
            show_x_labels: false,
            show_y_labels: false,
            grid_color: "#FFFFFF15".to_string(),
            label_color: "#888888".to_string(),
            label_font_size: 18.0,
            show_labels: false,
            timing: TimingConfig::default(),
            style: CssStyle::default(),
            timeline: Vec::new(),
            stagger: None,
        }
    }

    fn point(value: f64) -> ChartDataPoint {
        ChartDataPoint {
            value,
            label: None,
            color: None,
        }
    }

    #[test]
    fn zero_anchored_scale_puts_zero_on_the_domain_edge_instead_of_the_series_minimum() {
        let (min_val, max_val, norm) = zero_anchored_scale([100.0, 105.0].into_iter());
        assert_eq!(min_val, 0.0);
        assert_eq!(max_val, 105.0);
        assert!(
            norm(100.0) > 0.9,
            "100 out of a 0..105 domain must sit near the top, got {}",
            norm(100.0)
        );
    }

    #[test]
    fn zero_anchored_scale_keeps_a_flat_series_centred() {
        let (min_val, max_val, norm) = zero_anchored_scale([7.0, 7.0, 7.0].into_iter());
        assert_eq!(min_val, 7.0);
        assert_eq!(max_val, 7.0);
        assert_eq!(norm(7.0), 0.5);
    }

    #[test]
    fn a_close_pair_of_far_from_zero_values_does_not_paint_the_smaller_one_at_the_axis_floor() {
        const W: i32 = 216;
        const H: i32 = 216;
        let chart = base_chart(ChartType::Line, vec![point(100.0), point(105.0)]);

        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        {
            let canvas = surface.canvas();
            chart
                .render_line(canvas, W as f32, H as f32, 1.0)
                .expect("paint");
        }
        let snapshot = surface.image_snapshot();
        let info = skia_safe::ImageInfo::new(
            (W, H),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut buf = vec![0u8; (W * H * 4) as usize];
        snapshot.read_pixels(
            &info,
            &mut buf,
            (W * 4) as usize,
            skia_safe::IPoint::new(0, 0),
            skia_safe::image::CachingHint::Disallow,
        );
        let is_dot_ink = |x: i32, y: i32| {
            let idx = ((y * W + x) * 4) as usize;
            let (r, g, b, a) = (buf[idx], buf[idx + 1], buf[idx + 2], buf[idx + 3]);
            a > 200 && r < 100 && g > 100 && g < 160 && b > 200
        };
        let first_point_x = 8;
        let near_bottom_y = H - 10;
        assert!(
            !is_dot_ink(first_point_x, near_bottom_y),
            "value 100 of a series that only spans 100..105 must not be painted at \
             the chart floor, as if it were close to zero"
        );
    }
}
