use skia_safe::{Canvas, PaintStyle};

use rustmotion_core::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, measure_text_with_fallback, paint_from_hex,
    parse_hex_color,
};

use super::Chart;

/// Returns "#000000" or "#FFFFFF" depending on the perceived luminance of `hex`.
pub(super) fn contrast_text_color(hex: &str) -> String {
    let (r, g, b, _) = parse_hex_color(hex);
    // Relative luminance (sRGB)
    let lum = 0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64;
    if lum > 150.0 {
        "#000000".to_string()
    } else {
        "#FFFFFF".to_string()
    }
}

pub(super) fn format_number(val: f64) -> String {
    if val.abs() >= 1_000_000.0 {
        format!("{:.1}M", val / 1_000_000.0)
    } else if val.abs() >= 1_000.0 {
        format!("{:.1}K", val / 1_000.0)
    } else if val.fract().abs() < 0.01 {
        format!("{}", val as i64)
    } else {
        format!("{:.1}", val)
    }
}

impl Chart {
    /// Draw grid lines and axis labels for cartesian charts.
    ///
    /// When `categorical` is true, x-labels are placed at the center of evenly
    /// distributed slots (`(i+0.5)/n * chart_w`) — appropriate for bar charts
    /// where labels belong to a discrete bar slot. When false, labels are
    /// placed at proportional positions (`i/(n-1) * chart_w`), spanning the
    /// full chart width — appropriate for line/scatter where labels mark
    /// points on a continuous axis.
    pub(super) fn draw_axes(
        &self,
        canvas: &Canvas,
        chart_x: f32,
        chart_y: f32,
        chart_w: f32,
        chart_h: f32,
        min_val: f64,
        max_val: f64,
        x_labels: &[String],
        categorical: bool,
    ) {
        let Some(font) = self.make_label_font() else {
            return;
        };
        let emoji_font =
            emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, self.label_font_size));
        let (_, metrics) = font.metrics();
        let ascent = -metrics.ascent;

        // Grid lines + Y labels
        let grid_steps = 5;
        let range = max_val - min_val;

        for i in 0..=grid_steps {
            let frac = i as f32 / grid_steps as f32;
            let y = chart_y + chart_h - frac * chart_h;

            if self.show_grid {
                let mut grid_paint = paint_from_hex(&self.grid_color);
                grid_paint.set_style(PaintStyle::Stroke);
                grid_paint.set_stroke_width(1.0);
                grid_paint.set_anti_alias(true);
                canvas.draw_line((chart_x, y), (chart_x + chart_w, y), &grid_paint);
            }

            if self.show_y_labels {
                let val = min_val + range * frac as f64;
                let label = format_number(val);
                let mut label_paint = paint_from_hex(&self.label_color);
                label_paint.set_anti_alias(true);
                let label_w = measure_text_with_fallback(&label, &font, &emoji_font, 0.0);
                let lx = chart_x - label_w - 6.0;
                let ly = y + ascent / 2.0;
                draw_text_with_fallback(
                    canvas,
                    &label,
                    &font,
                    &emoji_font,
                    0.0,
                    lx,
                    ly,
                    &label_paint,
                );
            }
        }

        // X labels
        if self.show_x_labels && !x_labels.is_empty() {
            let mut label_paint = paint_from_hex(&self.label_color);
            label_paint.set_anti_alias(true);
            let n = x_labels.len();

            for (i, label) in x_labels.iter().enumerate() {
                let x = if n == 1 {
                    chart_x + chart_w / 2.0
                } else if categorical {
                    chart_x + ((i as f32 + 0.5) / n as f32) * chart_w
                } else {
                    chart_x + (i as f32 / (n - 1).max(1) as f32) * chart_w
                };
                let label_w = measure_text_with_fallback(label, &font, &emoji_font, 0.0);
                let lx = x - label_w / 2.0;
                let ly = chart_y + chart_h + ascent + 6.0;
                draw_text_with_fallback(
                    canvas,
                    label,
                    &font,
                    &emoji_font,
                    0.0,
                    lx,
                    ly,
                    &label_paint,
                );
            }
        }
    }
}
