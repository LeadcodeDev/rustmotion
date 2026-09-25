use rustmotion_core::error::Result;
use skia_safe::{Canvas, PaintStyle, Rect};

use rustmotion_core::engine::renderer::paint_from_hex;

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
        let track_width = (max_radius / (n as f32 * 1.5)).clamp(6.0, 20.0);
        let ring_gap = track_width * 0.5;

        let max_val = self.max.max(0.001);

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
            let ratio = (dp.value.max(0.0) / max_val).clamp(0.0, 1.0) as f32;
            let sweep = ratio * 360.0 * progress;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chart::{ChartDataPoint, ChartType};
    use rustmotion_core::css::CssStyle;
    use rustmotion_core::traits::TimingConfig;

    fn base_chart(chart_type: ChartType) -> Chart {
        Chart {
            chart_type,
            data: Vec::new(),
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

    fn pixel(buf: &[u8], w: i32, x: i32, y: i32) -> (u8, u8, u8, u8) {
        let idx = ((y * w + x) * 4) as usize;
        (buf[idx], buf[idx + 1], buf[idx + 2], buf[idx + 3])
    }

    #[test]
    fn a_single_datum_below_max_does_not_close_the_ring() {
        const W: i32 = 300;
        const H: i32 = 300;
        let mut chart = base_chart(ChartType::RadialBar);
        chart.max = 100.0;
        chart.data = vec![ChartDataPoint {
            value: 25.0,
            label: None,
            color: Some("#FF0000".to_string()),
        }];

        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        {
            let canvas = surface.canvas();
            chart
                .render_radial_bar(canvas, W as f32, H as f32, 1.0)
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

        let cx = W / 2;
        let cy = H / 2;
        let max_radius = (cx.min(cy) as f32) - 16.0;
        let bottom_y = cy + max_radius.round() as i32;
        let (r, g, b, a) = pixel(&buf, W, cx, bottom_y.min(H - 1));
        let is_red_fill = a > 40 && r > 180 && g < 60 && b < 60;
        assert!(
            !is_red_fill,
            "value 25 of max 100 must leave the far side of the ring unfilled, \
             found the fill color at the bottom of the ring: rgba=({r},{g},{b},{a})"
        );
    }
}
