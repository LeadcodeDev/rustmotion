use rustmotion_core::error::Result;
use skia_safe::{Canvas, PaintStyle, Rect};

use rustmotion_core::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, measure_text_with_fallback, paint_from_hex,
};

use super::axes::{contrast_text_color, format_number};
use super::Chart;

impl Chart {
    pub(super) fn render_bar(&self, canvas: &Canvas, w: f32, h: f32, progress: f32) -> Result<()> {
        let (mt, mr, mb, ml) = self.chart_margins();
        let chart_w = w - ml - mr;
        let chart_h = h - mt - mb;

        let (min_val, max_val, range) = self.value_extent();

        let x_labels: Vec<String> = self
            .data
            .iter()
            .map(|d| d.label.clone().unwrap_or_default())
            .collect();

        self.draw_axes(
            canvas, ml, mt, chart_w, chart_h, min_val, max_val, &x_labels, true,
        );

        let n = self.data.len();
        let gap = 8.0;
        let bar_w = (chart_w - gap * (n + 1) as f32) / n as f32;
        let zero_y = mt + chart_h - ((0.0 - min_val) / range) as f32 * chart_h;

        for (i, dp) in self.data.iter().enumerate() {
            let color = dp.color.as_deref().unwrap_or_else(|| self.get_color(i));
            let bar_h = (dp.value.abs() / range) as f32 * chart_h * progress;
            let x = ml + gap + i as f32 * (bar_w + gap);
            let negative = dp.value < 0.0;
            let y = if negative { zero_y } else { zero_y - bar_h };

            let mut paint = paint_from_hex(color);
            paint.set_style(PaintStyle::Fill);
            paint.set_anti_alias(true);

            let rect = Rect::from_xywh(x, y, bar_w, bar_h);
            let radius = (bar_w * 0.15).min(8.0);
            let round = (radius, radius).into();
            let square = (0.0, 0.0).into();
            let radii = if negative {
                [square, square, round, round]
            } else {
                [round, round, square, square]
            };
            let rrect = skia_safe::RRect::new_rect_radii(rect, &radii);
            canvas.draw_rrect(rrect, &paint);
        }

        Ok(())
    }

    pub(super) fn value_extent(&self) -> (f64, f64, f64) {
        let min_val = self
            .data
            .iter()
            .map(|d| d.value)
            .fold(0.0_f64, f64::min)
            .min(0.0);
        let max_val = self
            .data
            .iter()
            .map(|d| d.value)
            .fold(0.0_f64, f64::max)
            .max(0.0);
        (min_val, max_val, (max_val - min_val).max(0.001))
    }

    pub(super) fn render_horizontal_bar(
        &self,
        canvas: &Canvas,
        w: f32,
        h: f32,
        progress: f32,
    ) -> Result<()> {
        let Some(font) = self.make_label_font() else {
            return Ok(());
        };
        let emoji_font =
            emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, self.label_font_size));
        let (_, metrics) = font.metrics();
        let ascent = -metrics.ascent;
        let measure = |s: &str| measure_text_with_fallback(s, &font, &emoji_font, 0.0);

        let mt = 8.0;
        let mr = 8.0;
        let mb = if self.show_x_labels {
            self.label_font_size * 2.0 + 8.0
        } else {
            8.0
        };
        let gutter = if self.show_y_labels {
            let widest = self
                .data
                .iter()
                .filter_map(|d| d.label.as_deref())
                .map(measure)
                .fold(0.0_f32, f32::max);
            (widest + 14.0).min(w * 0.45)
        } else {
            0.0
        };
        let ml = gutter + 8.0;
        let chart_w = (w - ml - mr).max(1.0);
        let chart_h = (h - mt - mb).max(1.0);

        let (min_val, max_val, range) = self.value_extent();
        self.draw_value_axis_x(canvas, ml, mt, chart_w, chart_h, min_val, max_val);

        let n = self.data.len();
        let gap = 8.0;
        let row_h = (chart_h - gap * (n + 1) as f32) / n as f32;
        if row_h <= 0.0 {
            return Ok(());
        }
        let zero_x = ml + ((0.0 - min_val) / range) as f32 * chart_w;

        for (i, dp) in self.data.iter().enumerate() {
            let color = dp.color.as_deref().unwrap_or_else(|| self.get_color(i));
            let bar_len = (dp.value.abs() / range) as f32 * chart_w * progress;
            let negative = dp.value < 0.0;
            let x = if negative { zero_x - bar_len } else { zero_x };
            let y = mt + gap + i as f32 * (row_h + gap);
            let radius = (row_h * 0.15).min(8.0);

            let mut track_paint = paint_from_hex("#FFFFFF");
            track_paint.set_style(PaintStyle::Fill);
            track_paint.set_anti_alias(true);
            track_paint.set_alpha_f(0.06);
            canvas.draw_rrect(
                skia_safe::RRect::new_rect_xy(
                    Rect::from_xywh(ml, y, chart_w, row_h),
                    radius,
                    radius,
                ),
                &track_paint,
            );

            let mut paint = paint_from_hex(color);
            paint.set_style(PaintStyle::Fill);
            paint.set_anti_alias(true);

            let rect = Rect::from_xywh(x, y, bar_len, row_h);
            let round = (radius, radius).into();
            let square = (0.0, 0.0).into();
            let radii = if negative {
                [round, square, square, round]
            } else {
                [square, round, round, square]
            };
            canvas.draw_rrect(skia_safe::RRect::new_rect_radii(rect, &radii), &paint);

            let baseline_y = y + row_h / 2.0 + ascent / 2.0;

            let mut value_left = ml + chart_w;
            if self.show_labels {
                let text = format_number(dp.value);
                let text_w = measure(&text);
                value_left = ml + chart_w - text_w - 10.0;
                let on_bar = !negative && x + bar_len >= value_left;
                let color_hex = if on_bar {
                    contrast_text_color(color)
                } else {
                    self.label_color.clone()
                };
                let mut value_paint = paint_from_hex(&color_hex);
                value_paint.set_anti_alias(true);
                draw_text_with_fallback(
                    canvas,
                    &text,
                    &font,
                    &emoji_font,
                    0.0,
                    value_left,
                    baseline_y,
                    &value_paint,
                );
            }

            let Some(label) = &dp.label else { continue };

            if gutter > 0.0 {
                let mut label_paint = paint_from_hex(&self.label_color);
                label_paint.set_anti_alias(true);
                let lx = (gutter - measure(label)).max(0.0);
                draw_text_with_fallback(
                    canvas,
                    label,
                    &font,
                    &emoji_font,
                    0.0,
                    lx,
                    baseline_y,
                    &label_paint,
                );
            } else if self.show_labels || self.show_x_labels {
                let label_w = measure(label);
                let fits = bar_len >= label_w + 20.0;
                let (lx, color_hex) = if fits {
                    (x + 10.0, contrast_text_color(color))
                } else {
                    (x + bar_len + 10.0, self.label_color.clone())
                };
                if lx + label_w <= value_left - 6.0 || fits {
                    let mut label_paint = paint_from_hex(&color_hex);
                    label_paint.set_anti_alias(true);
                    draw_text_with_fallback(
                        canvas,
                        label,
                        &font,
                        &emoji_font,
                        0.0,
                        lx,
                        baseline_y,
                        &label_paint,
                    );
                }
            }
        }

        Ok(())
    }

    pub(super) fn render_stacked_bar(
        &self,
        canvas: &Canvas,
        w: f32,
        h: f32,
        progress: f32,
    ) -> Result<()> {
        if self.series.is_empty() || self.categories.is_empty() {
            return Ok(());
        }

        let (mt, mr, mb, ml) = self.chart_margins();
        let chart_w = w - ml - mr;
        let chart_h = h - mt - mb;

        let n_cats = self.categories.len();
        let totals: Vec<(f64, f64)> = (0..n_cats)
            .map(|ci| {
                self.series
                    .iter()
                    .fold((0.0_f64, 0.0_f64), |(pos, neg), s| {
                        let v = s.data.get(ci).copied().unwrap_or(0.0);
                        if v >= 0.0 {
                            (pos + v, neg)
                        } else {
                            (pos, neg + v)
                        }
                    })
            })
            .collect();
        let min_val = totals.iter().map(|(_, neg)| *neg).fold(0.0_f64, f64::min);
        let max_val = totals.iter().map(|(pos, _)| *pos).fold(0.0_f64, f64::max);
        let range = (max_val - min_val).max(0.001);

        let x_labels: Vec<String> = self.categories.clone();
        self.draw_axes(
            canvas, ml, mt, chart_w, chart_h, min_val, max_val, &x_labels, true,
        );

        let gap = 8.0;
        let bar_w = (chart_w - gap * (n_cats + 1) as f32) / n_cats as f32;
        let zero_y = mt + chart_h - ((0.0 - min_val) / range) as f32 * chart_h;

        for ci in 0..n_cats {
            let x = ml + gap + ci as f32 * (bar_w + gap);
            let mut pos_top = zero_y;
            let mut neg_bottom = zero_y;
            let last_pos_si =
                self.series.iter().enumerate().rev().find_map(|(si, s)| {
                    (s.data.get(ci).copied().unwrap_or(0.0) > 0.0).then_some(si)
                });
            let last_neg_si =
                self.series.iter().enumerate().rev().find_map(|(si, s)| {
                    (s.data.get(ci).copied().unwrap_or(0.0) < 0.0).then_some(si)
                });

            for (si, series) in self.series.iter().enumerate() {
                let val = series.data.get(ci).copied().unwrap_or(0.0);
                if val == 0.0 {
                    continue;
                }
                let seg_h = (val.abs() / range) as f32 * chart_h * progress;
                let negative = val < 0.0;
                let y = if negative {
                    neg_bottom
                } else {
                    pos_top - seg_h
                };

                let color = series
                    .color
                    .as_deref()
                    .unwrap_or_else(|| self.get_color(si));
                let mut paint = paint_from_hex(color);
                paint.set_style(PaintStyle::Fill);
                paint.set_anti_alias(true);

                let rect = Rect::from_xywh(x, y, bar_w, seg_h);
                let is_outer_edge = if negative {
                    Some(si) == last_neg_si
                } else {
                    Some(si) == last_pos_si
                };
                if is_outer_edge {
                    let radius = (bar_w * 0.15).min(8.0);
                    let round = (radius, radius).into();
                    let square = (0.0, 0.0).into();
                    let radii = if negative {
                        [square, square, round, round]
                    } else {
                        [round, round, square, square]
                    };
                    let rrect = skia_safe::RRect::new_rect_radii(rect, &radii);
                    canvas.draw_rrect(rrect, &paint);
                } else {
                    canvas.draw_rect(rect, &paint);
                }

                if negative {
                    neg_bottom += seg_h;
                } else {
                    pos_top -= seg_h;
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chart::{ChartDataPoint, ChartSeries, ChartType};
    use rustmotion_core::css::CssStyle;
    use rustmotion_core::traits::TimingConfig;

    fn base_chart(chart_type: ChartType) -> Chart {
        Chart {
            chart_type,
            data: Vec::new(),
            animated: true,
            animation_duration: 1.5,
            colors: None,
            inner_radius: 0.6,
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

    fn ink_bounds(
        surface: &mut skia_safe::Surface,
        w: i32,
        h: i32,
    ) -> Option<(i32, i32, i32, i32)> {
        let snapshot = surface.image_snapshot();
        let info = skia_safe::ImageInfo::new(
            (w, h),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let ok = snapshot.read_pixels(
            &info,
            &mut buf,
            (w * 4) as usize,
            skia_safe::IPoint::new(0, 0),
            skia_safe::image::CachingHint::Disallow,
        );
        assert!(ok, "pixel read should succeed");
        let (mut minx, mut maxx, mut miny, mut maxy) = (i32::MAX, i32::MIN, i32::MAX, i32::MIN);
        for y in 0..h {
            for x in 0..w {
                if buf[((y * w + x) * 4 + 3) as usize] > 0 {
                    minx = minx.min(x);
                    maxx = maxx.max(x);
                    miny = miny.min(y);
                    maxy = maxy.max(y);
                }
            }
        }
        (minx <= maxx).then_some((minx, maxx, miny, maxy))
    }

    fn colored_columns(
        surface: &mut skia_safe::Surface,
        w: i32,
        h: i32,
        matches_color: impl Fn(u8, u8, u8) -> bool,
    ) -> Vec<i32> {
        let snapshot = surface.image_snapshot();
        let info = skia_safe::ImageInfo::new(
            (w, h),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut buf = vec![0u8; (w * h * 4) as usize];
        snapshot.read_pixels(
            &info,
            &mut buf,
            (w * 4) as usize,
            skia_safe::IPoint::new(0, 0),
            skia_safe::image::CachingHint::Disallow,
        );
        let mut cols = vec![];
        for x in 0..w {
            let mut any = false;
            for y in 0..h {
                let idx = ((y * w + x) * 4) as usize;
                let (r, g, b, a) = (buf[idx], buf[idx + 1], buf[idx + 2], buf[idx + 3]);
                if a > 0 && matches_color(r, g, b) {
                    any = true;
                    break;
                }
            }
            if any {
                cols.push(x);
            }
        }
        cols
    }

    fn stacked_bar_ink_local_range(chart: &Chart, box_w: i32, box_h: i32, time: f64) -> (i32, i32) {
        const MARGIN: i32 = 300;
        let surface_h = box_h + MARGIN * 2;
        let mut surface =
            skia_safe::surfaces::raster_n32_premul((box_w, surface_h)).expect("raster surface");
        {
            let canvas = surface.canvas();
            canvas.translate((0.0, MARGIN as f32));
            chart
                .paint(canvas, box_w as f32, box_h as f32, time)
                .expect("paint must not error");
        }
        let (_minx, _maxx, miny, maxy) = ink_bounds(&mut surface, box_w, surface_h)
            .expect("stacked bar chart must paint visible bars");
        (miny - MARGIN, maxy - MARGIN)
    }

    #[test]
    fn stacked_bar_with_all_negative_totals_stays_inside_the_box() {
        let mut chart = base_chart(ChartType::StackedBar);
        chart.categories = vec!["Q1".to_string(), "Q2".to_string()];
        chart.series = vec![ChartSeries {
            name: "net".to_string(),
            data: vec![-5.0, -3.0],
            color: None,
        }];

        let (local_min, local_max) = stacked_bar_ink_local_range(&chart, 400, 300, 10.0);
        assert!(
            local_min >= 0 && local_max < 300,
            "ink escaped the 400x300 box vertically: local y=[{local_min}..{local_max}]"
        );
    }

    #[test]
    fn stacked_bar_with_mixed_sign_series_stays_inside_the_box() {
        let mut chart = base_chart(ChartType::StackedBar);
        chart.categories = vec!["Q1".to_string(), "Q2".to_string()];
        chart.series = vec![
            ChartSeries {
                name: "revenue".to_string(),
                data: vec![10.0, 8.0],
                color: None,
            },
            ChartSeries {
                name: "cost".to_string(),
                data: vec![-5.0, -3.0],
                color: None,
            },
        ];

        let (local_min, local_max) = stacked_bar_ink_local_range(&chart, 400, 300, 10.0);
        assert!(
            local_min >= 0 && local_max < 300,
            "ink escaped the 400x300 box vertically: local y=[{local_min}..{local_max}]"
        );
    }

    #[test]
    fn bar_x_labels_align_with_their_own_bar_when_some_points_are_unlabeled() {
        let mut chart = base_chart(ChartType::Bar);
        chart.show_x_labels = true;
        chart.data = vec![
            ChartDataPoint {
                value: 50.0,
                label: Some("AAA".to_string()),
                color: Some("#0000FF".to_string()),
            },
            ChartDataPoint {
                value: 50.0,
                label: None,
                color: Some("#0000FF".to_string()),
            },
            ChartDataPoint {
                value: 50.0,
                label: None,
                color: Some("#0000FF".to_string()),
            },
            ChartDataPoint {
                value: 50.0,
                label: Some("DDD".to_string()),
                color: Some("#0000FF".to_string()),
            },
        ];

        const W: i32 = 400;
        const H: i32 = 300;
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        let bar_cols = {
            let canvas = surface.canvas();
            chart
                .paint(canvas, W as f32, H as f32, 10.0)
                .expect("paint must not error");
            colored_columns(&mut surface, W, H, |r, g, b| r < 40 && g < 40 && b > 200)
        };
        assert!(!bar_cols.is_empty(), "no bars painted");
        let mut runs: Vec<(i32, i32)> = vec![];
        for x in bar_cols {
            match runs.last_mut() {
                Some((_, end)) if x <= *end + 1 => *end = x,
                _ => runs.push((x, x)),
            }
        }
        assert_eq!(runs.len(), 4, "expected 4 bar slots, got {runs:?}");
        let bar0_center = (runs[0].0 + runs[0].1) as f32 / 2.0;
        let bar3_center = (runs[3].0 + runs[3].1) as f32 / 2.0;

        let mut label_surface =
            skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        let label_cols = {
            let canvas = label_surface.canvas();
            chart
                .paint(canvas, W as f32, H as f32, 10.0)
                .expect("paint must not error");
            colored_columns(&mut label_surface, W, H, |r, g, b| {
                (120..160).contains(&r) && (120..160).contains(&g) && (120..160).contains(&b)
            })
        };
        assert!(!label_cols.is_empty(), "no labels painted");
        let mut label_runs: Vec<(i32, i32)> = vec![];
        for x in label_cols {
            match label_runs.last_mut() {
                Some((_, end)) if x <= *end + 4 => *end = x,
                _ => label_runs.push((x, x)),
            }
        }
        assert_eq!(
            label_runs.len(),
            2,
            "expected 2 label runs (AAA, DDD), got {label_runs:?}"
        );
        let label_aaa_center = (label_runs[0].0 + label_runs[0].1) as f32 / 2.0;
        let label_ddd_center = (label_runs[1].0 + label_runs[1].1) as f32 / 2.0;

        assert!(
            (label_aaa_center - bar0_center).abs() < 15.0,
            "AAA label (center {label_aaa_center}) should sit under bar 0 (center {bar0_center})"
        );
        assert!(
            (label_ddd_center - bar3_center).abs() < 15.0,
            "DDD label (center {label_ddd_center}) should sit under bar 3 (center {bar3_center})"
        );
    }
}
