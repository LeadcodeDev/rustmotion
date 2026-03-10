use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, Path, Rect};

use crate::engine::renderer::paint_from_hex;
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
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ChartDataPoint {
    pub value: f64,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Chart {
    pub chart_type: ChartType,
    pub data: Vec<ChartDataPoint>,
    #[serde(default)]
    pub size: Option<Size>,
    #[serde(default = "default_animated")]
    pub animated: bool,
    #[serde(default = "default_animation_duration")]
    pub animation_duration: f64,
    #[serde(default)]
    pub colors: Option<Vec<String>>,
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
}

impl Widget for Chart {
    fn render(
        &self,
        canvas: &Canvas,
        layout: &LayoutNode,
        ctx: &RenderContext,
        _props: &crate::engine::animator::AnimatedProperties,
    ) -> Result<()> {
        if self.data.is_empty() {
            return Ok(());
        }

        let w = layout.width;
        let h = layout.height;
        let progress = self.progress(ctx);

        match self.chart_type {
            ChartType::Bar => self.render_bar(canvas, w, h, progress),
            ChartType::Line => self.render_line(canvas, w, h, progress),
            ChartType::Pie => self.render_pie(canvas, w, h, progress),
        }
    }

    fn measure(&self, _constraints: &Constraints) -> (f32, f32) {
        if let Some(size) = &self.size {
            return (size.width, size.height);
        }
        (300.0, 200.0)
    }
}

impl Chart {
    fn render_bar(&self, canvas: &Canvas, w: f32, h: f32, progress: f32) -> Result<()> {
        let max_val = self
            .data
            .iter()
            .map(|d| d.value)
            .fold(0.0_f64, f64::max)
            .max(0.001);

        let n = self.data.len();
        let gap = 8.0;
        let bar_w = (w - gap * (n + 1) as f32) / n as f32;

        for (i, dp) in self.data.iter().enumerate() {
            let color = dp.color.as_deref().unwrap_or_else(|| self.get_color(i));
            let bar_h = (dp.value / max_val) as f32 * h * progress;
            let x = gap + i as f32 * (bar_w + gap);
            let y = h - bar_h;

            let mut paint = paint_from_hex(color);
            paint.set_style(PaintStyle::Fill);
            paint.set_anti_alias(true);

            let rect = Rect::from_xywh(x, y, bar_w, bar_h);

            // Rounded top corners
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

        let padding = 16.0;
        let chart_w = w - padding * 2.0;
        let chart_h = h - padding * 2.0;

        // Build path
        let mut path = Path::new();
        let mut fill_path = Path::new();

        for (i, dp) in self.data.iter().enumerate() {
            let x = padding + (i as f32 / (n - 1) as f32) * chart_w;
            let y = padding + chart_h - ((dp.value - min_val) / range) as f32 * chart_h;

            if i == 0 {
                path.move_to((x, y));
                fill_path.move_to((x, h - padding));
                fill_path.line_to((x, y));
            } else {
                path.line_to((x, y));
                fill_path.line_to((x, y));
            }
        }

        // Close fill path
        let last_x = padding + chart_w;
        fill_path.line_to((last_x, h - padding));
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
            let x = padding + (i as f32 / (n - 1) as f32) * chart_w;
            let y = padding + chart_h - ((dp.value - min_val) / range) as f32 * chart_h;

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
}
