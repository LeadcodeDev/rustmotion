use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Color, PaintStyle, Path, Point, Rect};

use crate::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, font_mgr, measure_text_with_fallback,
    paint_from_hex, parse_hex_color,
};
use crate::layout::{Constraints, LayoutNode};
use crate::schema::{LayerStyle, Size};
use crate::traits::{RenderContext, TimingConfig, Widget};

const DEFAULT_PALETTE: &[&str] = &[
    "#3B82F6", "#EF4444", "#22C55E", "#F59E0B", "#8B5CF6", "#EC4899", "#06B6D4", "#F97316",
];

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChartType {
    Bar,
    Line,
    Pie,
    Donut,
    HorizontalBar,
    Area,
    StackedBar,
    Radar,
    Scatter,
    RadialBar,
    Funnel,
    Waterfall,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ChartDataPoint {
    pub value: f64,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
}

/// A data point for scatter charts with explicit x/y coordinates.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ScatterPoint {
    pub x: f64,
    pub y: f64,
    #[serde(default = "default_scatter_size")]
    pub size: f32,
    #[serde(default)]
    pub color: Option<String>,
}

fn default_scatter_size() -> f32 {
    8.0
}

/// A data series for stacked bar charts.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ChartSeries {
    pub name: String,
    pub data: Vec<f64>,
    #[serde(default)]
    pub color: Option<String>,
}

/// A data series for radar charts.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RadarData {
    pub values: Vec<f64>,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Chart {
    pub chart_type: ChartType,
    #[serde(default)]
    pub data: Vec<ChartDataPoint>,
    #[serde(default)]
    pub size: Option<Size>,
    #[serde(default = "default_animated")]
    pub animated: bool,
    #[serde(default = "default_animation_duration")]
    pub animation_duration: f64,
    #[serde(default)]
    pub colors: Option<Vec<String>>,

    // Donut-specific
    #[serde(default = "default_inner_radius")]
    pub inner_radius: f64,

    // Area-specific
    #[serde(default = "default_fill_opacity")]
    pub fill_opacity: f32,
    #[serde(default)]
    pub smooth: bool,

    // Stacked bar
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub series: Vec<ChartSeries>,

    // Radar
    #[serde(default)]
    pub axes: Vec<String>,
    #[serde(default)]
    pub radar_data: Vec<RadarData>,

    // Scatter
    #[serde(default)]
    pub points: Vec<ScatterPoint>,

    // Funnel direction
    /// Direction for funnel chart: "vertical" (default) or "horizontal".
    #[serde(default)]
    pub direction: Option<String>,

    // Axes, grid, labels
    #[serde(default)]
    pub show_grid: bool,
    #[serde(default)]
    pub show_x_labels: bool,
    #[serde(default)]
    pub show_y_labels: bool,
    #[serde(default = "default_grid_color")]
    pub grid_color: String,
    #[serde(default = "default_label_color")]
    pub label_color: String,
    #[serde(default = "default_label_font_size")]
    pub label_font_size: f32,
    #[serde(default)]
    pub show_labels: bool,

    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

fn default_animated() -> bool {
    true
}

fn default_animation_duration() -> f64 {
    1.5
}

fn default_inner_radius() -> f64 {
    0.6
}

fn default_fill_opacity() -> f32 {
    0.3
}

fn default_grid_color() -> String {
    "#FFFFFF15".to_string()
}

fn default_label_color() -> String {
    "#888888".to_string()
}

fn default_label_font_size() -> f32 {
    12.0
}

crate::impl_traits!(Chart {
    Animatable => style,
    Timed => timing,
    Styled => style,
});

impl Chart {
    fn get_color(&self, index: usize) -> &str {
        if let Some(colors) = &self.colors {
            if !colors.is_empty() {
                return &colors[index % colors.len()];
            }
        }
        DEFAULT_PALETTE[index % DEFAULT_PALETTE.len()]
    }

    fn progress(&self, ctx: &RenderContext) -> f32 {
        if !self.animated {
            return 1.0;
        }
        let p = (ctx.time / self.animation_duration).clamp(0.0, 1.0) as f32;
        // ease_out_cubic
        1.0 - (1.0 - p).powi(3)
    }

    /// Compute margins for axes/labels area.
    fn chart_margins(&self) -> (f32, f32, f32, f32) {
        let left = if self.show_y_labels {
            self.label_font_size * 3.5
        } else {
            0.0
        };
        let bottom = if self.show_x_labels {
            self.label_font_size * 2.0
        } else {
            0.0
        };
        // top, right, bottom, left
        (8.0, 8.0, bottom + 8.0, left + 8.0)
    }

    fn make_label_font(&self) -> skia_safe::Font {
        let fm = font_mgr();
        let font_style = skia_safe::FontStyle::normal();
        let typeface = fm
            .match_family_style("Inter", font_style)
            .or_else(|| fm.match_family_style("Helvetica", font_style))
            .or_else(|| fm.match_family_style("Arial", font_style))
            .or_else(|| fm.match_family_style("sans-serif", font_style))
            .unwrap_or_else(|| fm.legacy_make_typeface(None, font_style).unwrap());
        skia_safe::Font::from_typeface(typeface, self.label_font_size)
    }

    /// Draw grid lines and axis labels for cartesian charts.
    fn draw_axes(
        &self,
        canvas: &Canvas,
        chart_x: f32,
        chart_y: f32,
        chart_w: f32,
        chart_h: f32,
        min_val: f64,
        max_val: f64,
        x_labels: &[String],
    ) {
        let font = self.make_label_font();
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
                let label_w =
                    measure_text_with_fallback(&label, &font, &emoji_font, 0.0);
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
                } else {
                    chart_x + (i as f32 / (n - 1).max(1) as f32) * chart_w
                };
                let label_w =
                    measure_text_with_fallback(label, &font, &emoji_font, 0.0);
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

impl Widget for Chart {
    fn render(
        &self,
        canvas: &Canvas,
        layout: &LayoutNode,
        ctx: &RenderContext,
        _props: &crate::engine::animator::AnimatedProperties,
    ) -> Result<()> {
        let w = layout.width;
        let h = layout.height;
        let progress = self.progress(ctx);

        match self.chart_type {
            ChartType::Bar => {
                if self.data.is_empty() {
                    return Ok(());
                }
                self.render_bar(canvas, w, h, progress)
            }
            ChartType::Line => {
                if self.data.is_empty() {
                    return Ok(());
                }
                self.render_line(canvas, w, h, progress)
            }
            ChartType::Pie => {
                if self.data.is_empty() {
                    return Ok(());
                }
                self.render_pie(canvas, w, h, progress)
            }
            ChartType::Donut => {
                if self.data.is_empty() {
                    return Ok(());
                }
                self.render_donut(canvas, w, h, progress)
            }
            ChartType::HorizontalBar => {
                if self.data.is_empty() {
                    return Ok(());
                }
                self.render_horizontal_bar(canvas, w, h, progress)
            }
            ChartType::Area => {
                if self.data.is_empty() {
                    return Ok(());
                }
                self.render_area(canvas, w, h, progress)
            }
            ChartType::StackedBar => self.render_stacked_bar(canvas, w, h, progress),
            ChartType::Radar => self.render_radar(canvas, w, h, progress),
            ChartType::Scatter => self.render_scatter(canvas, w, h, progress),
            ChartType::RadialBar => {
                if self.data.is_empty() {
                    return Ok(());
                }
                self.render_radial_bar(canvas, w, h, progress)
            }
            ChartType::Funnel => {
                if self.data.is_empty() {
                    return Ok(());
                }
                self.render_funnel(canvas, w, h, progress)
            }
            ChartType::Waterfall => {
                if self.data.is_empty() {
                    return Ok(());
                }
                self.render_waterfall(canvas, w, h, progress)
            }
        }
    }

    fn measure(&self, _constraints: &Constraints) -> (f32, f32) {
        if let Some(size) = &self.size {
            return (size.width, size.height);
        }
        (300.0, 200.0)
    }
}

// ─── Render methods ──────────────────────────────────────────────────────────

impl Chart {
    fn render_bar(&self, canvas: &Canvas, w: f32, h: f32, progress: f32) -> Result<()> {
        let (mt, mr, mb, ml) = self.chart_margins();
        let chart_w = w - ml - mr;
        let chart_h = h - mt - mb;

        let max_val = self
            .data
            .iter()
            .map(|d| d.value)
            .fold(0.0_f64, f64::max)
            .max(0.001);

        let x_labels: Vec<String> = self
            .data
            .iter()
            .filter_map(|d| d.label.clone())
            .collect();

        self.draw_axes(canvas, ml, mt, chart_w, chart_h, 0.0, max_val, &x_labels);

        let n = self.data.len();
        let gap = 8.0;
        let bar_w = (chart_w - gap * (n + 1) as f32) / n as f32;

        for (i, dp) in self.data.iter().enumerate() {
            let color = dp.color.as_deref().unwrap_or_else(|| self.get_color(i));
            let bar_h = (dp.value / max_val) as f32 * chart_h * progress;
            let x = ml + gap + i as f32 * (bar_w + gap);
            let y = mt + chart_h - bar_h;

            let mut paint = paint_from_hex(color);
            paint.set_style(PaintStyle::Fill);
            paint.set_anti_alias(true);

            let rect = Rect::from_xywh(x, y, bar_w, bar_h);
            let radius = (bar_w * 0.15).min(8.0);
            let rrect = skia_safe::RRect::new_rect_radii(
                rect,
                &[
                    (radius, radius).into(),
                    (radius, radius).into(),
                    (0.0, 0.0).into(),
                    (0.0, 0.0).into(),
                ],
            );
            canvas.draw_rrect(rrect, &paint);
        }

        Ok(())
    }

    fn render_line(&self, canvas: &Canvas, w: f32, h: f32, progress: f32) -> Result<()> {
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

    fn render_pie(&self, canvas: &Canvas, w: f32, h: f32, progress: f32) -> Result<()> {
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

    fn render_donut(&self, canvas: &Canvas, w: f32, h: f32, progress: f32) -> Result<()> {
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

    fn render_horizontal_bar(
        &self,
        canvas: &Canvas,
        w: f32,
        h: f32,
        progress: f32,
    ) -> Result<()> {
        let (mt, mr, mb, ml) = self.chart_margins();
        let chart_w = w - ml - mr;
        let chart_h = h - mt - mb;

        let max_val = self
            .data
            .iter()
            .map(|d| d.value)
            .fold(0.0_f64, f64::max)
            .max(0.001);

        let n = self.data.len();
        let gap = 8.0;
        let bar_h = (chart_h - gap * (n + 1) as f32) / n as f32;

        let font = self.make_label_font();
        let emoji_font =
            emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, self.label_font_size));
        let (_, metrics) = font.metrics();
        let ascent = -metrics.ascent;

        for (i, dp) in self.data.iter().enumerate() {
            let color = dp.color.as_deref().unwrap_or_else(|| self.get_color(i));
            let bar_w = (dp.value / max_val) as f32 * chart_w * progress;
            let x = ml;
            let y = mt + gap + i as f32 * (bar_h + gap);

            let mut paint = paint_from_hex(color);
            paint.set_style(PaintStyle::Fill);
            paint.set_anti_alias(true);

            let rect = Rect::from_xywh(x, y, bar_w, bar_h);
            let radius = (bar_h * 0.15).min(8.0);
            let rrect = skia_safe::RRect::new_rect_radii(
                rect,
                &[
                    (0.0, 0.0).into(),
                    (radius, radius).into(),
                    (radius, radius).into(),
                    (0.0, 0.0).into(),
                ],
            );
            canvas.draw_rrect(rrect, &paint);

            // Draw label inside the bar
            if self.show_labels || self.show_x_labels {
                if let Some(label) = &dp.label {
                    let contrast_color = contrast_text_color(color);
                    let mut label_paint = paint_from_hex(&contrast_color);
                    label_paint.set_anti_alias(true);
                    let lx = x + 10.0;
                    let ly = y + bar_h / 2.0 + ascent / 2.0;
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

        Ok(())
    }

    fn render_area(&self, canvas: &Canvas, w: f32, h: f32, progress: f32) -> Result<()> {
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
            // Catmull-Rom → cubic bezier for smooth curves
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

    fn render_stacked_bar(
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
        // Find max stacked total
        let max_val = (0..n_cats)
            .map(|ci| {
                self.series
                    .iter()
                    .map(|s| s.data.get(ci).copied().unwrap_or(0.0))
                    .sum::<f64>()
            })
            .fold(0.0_f64, f64::max)
            .max(0.001);

        let x_labels: Vec<String> = self.categories.clone();
        self.draw_axes(canvas, ml, mt, chart_w, chart_h, 0.0, max_val, &x_labels);

        let gap = 8.0;
        let bar_w = (chart_w - gap * (n_cats + 1) as f32) / n_cats as f32;

        for ci in 0..n_cats {
            let x = ml + gap + ci as f32 * (bar_w + gap);
            let mut cumulative_h = 0.0_f32;

            for (si, series) in self.series.iter().enumerate() {
                let val = series.data.get(ci).copied().unwrap_or(0.0);
                let seg_h = (val / max_val) as f32 * chart_h * progress;
                let y = mt + chart_h - cumulative_h - seg_h;

                let color = series
                    .color
                    .as_deref()
                    .unwrap_or_else(|| self.get_color(si));
                let mut paint = paint_from_hex(color);
                paint.set_style(PaintStyle::Fill);
                paint.set_anti_alias(true);

                let rect = Rect::from_xywh(x, y, bar_w, seg_h);
                // Rounded top on the topmost segment only
                if si == self.series.len() - 1 {
                    let radius = (bar_w * 0.15).min(8.0);
                    let rrect = skia_safe::RRect::new_rect_radii(
                        rect,
                        &[
                            (radius, radius).into(),
                            (radius, radius).into(),
                            (0.0, 0.0).into(),
                            (0.0, 0.0).into(),
                        ],
                    );
                    canvas.draw_rrect(rrect, &paint);
                } else {
                    canvas.draw_rect(rect, &paint);
                }

                cumulative_h += seg_h;
            }
        }

        Ok(())
    }

    fn render_radar(&self, canvas: &Canvas, w: f32, h: f32, progress: f32) -> Result<()> {
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
        let font = self.make_label_font();
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
            let lx = px - label_w / 2.0;
            let ly = py + ascent / 2.0;
            draw_text_with_fallback(canvas, label, &font, &emoji_font, 0.0, lx, ly, &label_paint);
        }

        // Draw data polygons
        for (di, rd) in self.radar_data.iter().enumerate() {
            if rd.values.len() != n_axes {
                continue;
            }
            let max_val = rd
                .values
                .iter()
                .fold(0.0_f64, |a, &b| a.max(b))
                .max(0.001);

            let color_str = rd
                .color
                .as_deref()
                .unwrap_or_else(|| self.get_color(di));

            let mut data_path = Path::new();
            for (i, &val) in rd.values.iter().enumerate() {
                let norm = (val / max_val) as f32 * progress;
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

    fn render_scatter(&self, canvas: &Canvas, w: f32, h: f32, progress: f32) -> Result<()> {
        if self.points.is_empty() {
            return Ok(());
        }

        let (mt, mr, mb, ml) = self.chart_margins();
        let chart_w = w - ml - mr;
        let chart_h = h - mt - mb;

        let min_x = self
            .points
            .iter()
            .map(|p| p.x)
            .fold(f64::MAX, f64::min);
        let max_x = self
            .points
            .iter()
            .map(|p| p.x)
            .fold(f64::MIN, f64::max);
        let min_y = self
            .points
            .iter()
            .map(|p| p.y)
            .fold(f64::MAX, f64::min);
        let max_y = self
            .points
            .iter()
            .map(|p| p.y)
            .fold(f64::MIN, f64::max);

        let range_x = (max_x - min_x).max(0.001);
        let range_y = (max_y - min_y).max(0.001);

        self.draw_axes(canvas, ml, mt, chart_w, chart_h, min_y, max_y, &[]);

        for (i, pt) in self.points.iter().enumerate() {
            let px = ml + ((pt.x - min_x) / range_x) as f32 * chart_w;
            let py = mt + chart_h - ((pt.y - min_y) / range_y) as f32 * chart_h;

            let color = pt
                .color
                .as_deref()
                .unwrap_or_else(|| self.get_color(i));
            let mut paint = paint_from_hex(color);
            paint.set_style(PaintStyle::Fill);
            paint.set_anti_alias(true);
            paint.set_alpha_f(progress);

            canvas.draw_circle((px, py), pt.size * progress, &paint);
        }

        Ok(())
    }

    fn render_radial_bar(
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

    fn render_funnel(&self, canvas: &Canvas, w: f32, h: f32, progress: f32) -> Result<()> {
        let horizontal = self
            .direction
            .as_deref()
            .map(|d| d == "horizontal")
            .unwrap_or(false);

        if horizontal {
            self.render_funnel_horizontal(canvas, w, h, progress)
        } else {
            self.render_funnel_vertical(canvas, w, h, progress)
        }
    }

    fn render_funnel_vertical(&self, canvas: &Canvas, w: f32, h: f32, progress: f32) -> Result<()> {
        let n = self.data.len();
        let max_val = self
            .data
            .iter()
            .map(|d| d.value)
            .fold(0.0_f64, f64::max)
            .max(0.001);

        let gap = 4.0;
        let total_gap = gap * (n.saturating_sub(1)) as f32;
        let seg_h = (h - total_gap) / n as f32;

        let font = self.make_label_font();
        let emoji_font =
            emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, self.label_font_size));
        let (_, metrics) = font.metrics();
        let ascent = -metrics.ascent;

        for (i, dp) in self.data.iter().enumerate() {
            let ratio = (dp.value / max_val) as f32 * progress;
            let next_ratio = if i + 1 < n {
                (self.data[i + 1].value / max_val) as f32 * progress
            } else {
                ratio * 0.6
            };

            let top_w = w * ratio;
            let bot_w = w * next_ratio;
            let top_x = (w - top_w) / 2.0;
            let bot_x = (w - bot_w) / 2.0;
            let y_top = i as f32 * (seg_h + gap);
            let y_bot = y_top + seg_h;

            let color = dp.color.as_deref().unwrap_or_else(|| self.get_color(i));
            let mut paint = paint_from_hex(color);
            paint.set_style(PaintStyle::Fill);
            paint.set_anti_alias(true);

            let mut path = Path::new();
            path.move_to((top_x, y_top));
            path.line_to((top_x + top_w, y_top));
            path.line_to((bot_x + bot_w, y_bot));
            path.line_to((bot_x, y_bot));
            path.close();
            canvas.draw_path(&path, &paint);

            if self.show_labels {
                if let Some(label) = &dp.label {
                    let contrast = contrast_text_color(color);
                    let mut label_paint = paint_from_hex(&contrast);
                    label_paint.set_anti_alias(true);
                    let label_w = measure_text_with_fallback(label, &font, &emoji_font, 0.0);
                    let lx = (w - label_w) / 2.0;
                    let ly = y_top + seg_h / 2.0 + ascent / 2.0;
                    draw_text_with_fallback(
                        canvas, label, &font, &emoji_font, 0.0, lx, ly, &label_paint,
                    );
                }
            }
        }

        Ok(())
    }

    fn render_funnel_horizontal(&self, canvas: &Canvas, w: f32, h: f32, progress: f32) -> Result<()> {
        let n = self.data.len();
        let max_val = self
            .data
            .iter()
            .map(|d| d.value)
            .fold(0.0_f64, f64::max)
            .max(0.001);

        let gap = 4.0;
        let total_gap = gap * (n.saturating_sub(1)) as f32;
        let seg_w = (w - total_gap) / n as f32;

        let font = self.make_label_font();
        let emoji_font =
            emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, self.label_font_size));
        let (_, metrics) = font.metrics();
        let ascent = -metrics.ascent;

        for (i, dp) in self.data.iter().enumerate() {
            let ratio = (dp.value / max_val) as f32 * progress;
            let next_ratio = if i + 1 < n {
                (self.data[i + 1].value / max_val) as f32 * progress
            } else {
                ratio * 0.6
            };

            let left_h = h * ratio;
            let right_h = h * next_ratio;
            let left_y = (h - left_h) / 2.0;
            let right_y = (h - right_h) / 2.0;
            let x_left = i as f32 * (seg_w + gap);
            let x_right = x_left + seg_w;

            let color = dp.color.as_deref().unwrap_or_else(|| self.get_color(i));
            let mut paint = paint_from_hex(color);
            paint.set_style(PaintStyle::Fill);
            paint.set_anti_alias(true);

            let mut path = Path::new();
            path.move_to((x_left, left_y));
            path.line_to((x_right, right_y));
            path.line_to((x_right, right_y + right_h));
            path.line_to((x_left, left_y + left_h));
            path.close();
            canvas.draw_path(&path, &paint);

            if self.show_labels {
                if let Some(label) = &dp.label {
                    let contrast = contrast_text_color(color);
                    let mut label_paint = paint_from_hex(&contrast);
                    label_paint.set_anti_alias(true);
                    let label_w = measure_text_with_fallback(label, &font, &emoji_font, 0.0);
                    let lx = x_left + (seg_w - label_w) / 2.0;
                    let ly = h / 2.0 + ascent / 2.0;
                    draw_text_with_fallback(
                        canvas, label, &font, &emoji_font, 0.0, lx, ly, &label_paint,
                    );
                }
            }
        }

        Ok(())
    }

    fn render_waterfall(&self, canvas: &Canvas, w: f32, h: f32, progress: f32) -> Result<()> {
        let (mt, mr, mb, ml) = self.chart_margins();
        let chart_w = w - ml - mr;
        let chart_h = h - mt - mb;

        // Calculate cumulative values and find range
        let mut cumulative = Vec::with_capacity(self.data.len());
        let mut running = 0.0_f64;
        for dp in &self.data {
            let prev = running;
            running += dp.value;
            cumulative.push((prev, running));
        }

        let all_vals: Vec<f64> = cumulative
            .iter()
            .flat_map(|(a, b)| vec![*a, *b])
            .chain(std::iter::once(0.0))
            .collect();
        let min_val = all_vals.iter().fold(f64::MAX, |a, &b| a.min(b));
        let max_val = all_vals.iter().fold(f64::MIN, |a, &b| a.max(b));
        let range = (max_val - min_val).max(0.001);

        let x_labels: Vec<String> = self
            .data
            .iter()
            .filter_map(|d| d.label.clone())
            .collect();
        self.draw_axes(canvas, ml, mt, chart_w, chart_h, min_val, max_val, &x_labels);

        let n = self.data.len();
        let gap = 6.0;
        let bar_w = (chart_w - gap * (n + 1) as f32) / n as f32;

        // Zero baseline
        let _zero_y = mt + chart_h - ((0.0 - min_val) / range) as f32 * chart_h;

        for (i, dp) in self.data.iter().enumerate() {
            let (start_val, end_val) = cumulative[i];
            let y_start = mt + chart_h - ((start_val - min_val) / range) as f32 * chart_h;
            let y_end = mt + chart_h - ((end_val - min_val) / range) as f32 * chart_h;

            let bar_top = y_start.min(y_end);
            let bar_bottom = y_start.max(y_end);
            let bar_h = (bar_bottom - bar_top) * progress;
            let x = ml + gap + i as f32 * (bar_w + gap);

            // Green for positive, red for negative
            let color = dp.color.as_deref().unwrap_or_else(|| {
                if dp.value >= 0.0 {
                    "#22C55E"
                } else {
                    "#EF4444"
                }
            });

            let mut paint = paint_from_hex(color);
            paint.set_style(PaintStyle::Fill);
            paint.set_anti_alias(true);

            // Animate from the start position
            let animated_top = if dp.value >= 0.0 {
                y_start - bar_h
            } else {
                y_start
            };

            let rect = Rect::from_xywh(x, animated_top, bar_w, bar_h);
            let radius = (bar_w * 0.1).min(4.0);
            let rrect = skia_safe::RRect::new_rect_xy(rect, radius, radius);
            canvas.draw_rrect(rrect, &paint);

            // Connector line to next bar
            if i + 1 < n {
                let next_x = ml + gap + (i + 1) as f32 * (bar_w + gap);
                let connector_y = if dp.value >= 0.0 {
                    animated_top
                } else {
                    animated_top + bar_h
                };
                let mut connector_paint = paint_from_hex("#FFFFFF30");
                connector_paint.set_style(PaintStyle::Stroke);
                connector_paint.set_stroke_width(1.0);
                connector_paint.set_anti_alias(true);
                // Dashed
                let intervals = [4.0_f32, 4.0];
                if let Some(effect) = skia_safe::PathEffect::dash(&intervals, 0.0) {
                    connector_paint.set_path_effect(effect);
                }
                canvas.draw_line(
                    (x + bar_w, connector_y),
                    (next_x, connector_y),
                    &connector_paint,
                );
            }
        }

        Ok(())
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Returns "#000000" or "#FFFFFF" depending on the perceived luminance of `hex`.
fn contrast_text_color(hex: &str) -> String {
    let (r, g, b, _) = parse_hex_color(hex);
    // Relative luminance (sRGB)
    let lum = 0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64;
    if lum > 150.0 {
        "#000000".to_string()
    } else {
        "#FFFFFF".to_string()
    }
}

fn format_number(val: f64) -> String {
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
