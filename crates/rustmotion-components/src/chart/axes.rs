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
    } else if (val - val.round()).abs() < 1e-9 {
        format!("{}", val.round() as i64)
    } else {
        format!("{:.1}", val)
    }
}

fn tick_decimals(step: f64) -> usize {
    let step = step.abs();
    if !step.is_finite() || step <= 0.0 {
        return 0;
    }
    let mut decimals = 0usize;
    let mut scaled = step;
    while scaled < 0.999_999 && decimals < 6 {
        scaled *= 10.0;
        decimals += 1;
    }
    decimals
}

pub(super) fn format_tick_for_step(val: f64, step: f64) -> String {
    if val.abs() >= 1_000_000.0 {
        return format!("{:.1}M", val / 1_000_000.0);
    }
    if val.abs() >= 1_000.0 {
        return format!("{:.1}K", val / 1_000.0);
    }
    let decimals = tick_decimals(step);
    format!("{val:.decimals$}")
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
        let tick_step = range / grid_steps as f64;

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
                let label = format_tick_for_step(val, tick_step);
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
                // Centring the first/last label on the axis end pushes half of
                // it outside the component's own box (measured: the last label
                // of a 5-point line chart bled 5px past the right edge). Clamp
                // into [0, box_width] — `chart_margins()` always returns a
                // right margin of 8, so the box ends at chart_x + chart_w + 8.
                let box_w = chart_x + chart_w + 8.0;
                let lx = (x - label_w / 2.0).clamp(0.0, (box_w - label_w).max(0.0));
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

    /// Draw the *value* axis of a transposed (horizontal) cartesian chart:
    /// vertical grid lines and value tick labels along the bottom.
    ///
    /// [`draw_axes`] assumes the value axis is vertical (grid lines
    /// horizontal, values down the left gutter), which is wrong for
    /// `horizontal_bar` — there the value axis runs left-to-right and the
    /// category axis is the vertical one.
    pub(super) fn draw_value_axis_x(
        &self,
        canvas: &Canvas,
        chart_x: f32,
        chart_y: f32,
        chart_w: f32,
        chart_h: f32,
        min_val: f64,
        max_val: f64,
    ) {
        if !self.show_grid && !self.show_x_labels {
            return;
        }
        let Some(font) = self.make_label_font() else {
            return;
        };
        let emoji_font =
            emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, self.label_font_size));
        let (_, metrics) = font.metrics();
        let ascent = -metrics.ascent;

        let grid_steps = 5;
        let range = max_val - min_val;
        let tick_step = range / grid_steps as f64;

        for i in 0..=grid_steps {
            let frac = i as f32 / grid_steps as f32;
            let x = chart_x + frac * chart_w;

            if self.show_grid {
                let mut grid_paint = paint_from_hex(&self.grid_color);
                grid_paint.set_style(PaintStyle::Stroke);
                grid_paint.set_stroke_width(1.0);
                grid_paint.set_anti_alias(true);
                canvas.draw_line((x, chart_y), (x, chart_y + chart_h), &grid_paint);
            }

            if self.show_x_labels {
                let label = format_tick_for_step(min_val + range * frac as f64, tick_step);
                let mut label_paint = paint_from_hex(&self.label_color);
                label_paint.set_anti_alias(true);
                let label_w = measure_text_with_fallback(&label, &font, &emoji_font, 0.0);
                let box_w = chart_x + chart_w + 8.0;
                let lx = (x - label_w / 2.0).clamp(0.0, (box_w - label_w).max(0.0));
                let ly = chart_y + chart_h + ascent + 6.0;
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_number_does_not_flip_a_float_noise_integer_to_a_different_branch_than_its_exact_neighbour(
    ) {
        assert_eq!(format_number(-2.0), "-2");
        assert_eq!(format_number(-1.999_999_999_999_999_6), "-2");
    }

    #[test]
    fn format_tick_for_step_keeps_enough_decimals_to_tell_sub_unit_ticks_apart() {
        assert_eq!(format_tick_for_step(0.01, 0.01), "0.01");
        assert_eq!(format_tick_for_step(0.0, 0.01), "0.00");
        assert_ne!(
            format_tick_for_step(0.01, 0.01),
            format_tick_for_step(0.0, 0.01),
            "two ticks a step apart must not render identically"
        );
    }

    #[test]
    fn format_tick_for_step_does_not_flip_branch_on_float_noise() {
        assert_eq!(format_tick_for_step(-1.999_999_999_999_999_6, 1.0), "-2");
    }

    #[test]
    fn format_tick_for_step_uses_whole_numbers_for_a_whole_number_step() {
        assert_eq!(format_tick_for_step(4.0, 2.0), "4");
    }
}
