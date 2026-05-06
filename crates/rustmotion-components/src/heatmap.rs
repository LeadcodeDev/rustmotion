use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, RRect, Rect};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::parse_hex_color;
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

fn default_cell_size() -> f32 {
    14.0
}

fn default_cell_gap() -> f32 {
    3.0
}

fn default_cell_radius() -> f32 {
    2.0
}

fn default_animated() -> bool {
    true
}

fn default_animation_duration() -> f64 {
    1.5
}

fn default_color_scale() -> Vec<String> {
    vec![
        "#161B22".to_string(),
        "#0E4429".to_string(),
        "#006D32".to_string(),
        "#26A641".to_string(),
        "#39D353".to_string(),
    ]
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Heatmap {
    /// 2D array of values (rows x columns).
    pub data: Vec<Vec<f64>>,
    /// Gradient colors for the heatmap scale.
    #[serde(default = "default_color_scale")]
    pub color_scale: Vec<String>,
    /// Size of each cell in pixels.
    #[serde(default = "default_cell_size")]
    pub cell_size: f32,
    /// Gap between cells in pixels.
    #[serde(default = "default_cell_gap")]
    pub cell_gap: f32,
    /// Corner radius of each cell.
    #[serde(default = "default_cell_radius")]
    pub cell_radius: f32,
    /// Whether the heatmap animates in.
    #[serde(default = "default_animated")]
    pub animated: bool,
    /// Duration of the reveal animation in seconds.
    #[serde(default = "default_animation_duration")]
    pub animation_duration: f64,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(Heatmap {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t).round().clamp(0.0, 255.0) as u8
}

fn interpolate_color(scale: &[String], t: f32) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    if scale.len() < 2 {
        let (r, g, b, _) = parse_hex_color(&scale[0]);
        return (r, g, b);
    }
    let n = scale.len() - 1;
    let segment = (t * n as f32).floor() as usize;
    let local_t = t * n as f32 - segment as f32;
    let i = segment.min(n - 1);
    let (r1, g1, b1, _) = parse_hex_color(&scale[i]);
    let (r2, g2, b2, _) = parse_hex_color(&scale[i + 1]);
    (
        lerp_u8(r1, r2, local_t),
        lerp_u8(g1, g2, local_t),
        lerp_u8(b1, b2, local_t),
    )
}

impl Heatmap {
    fn progress_at(&self, time: f64) -> f32 {
        if !self.animated {
            return 1.0;
        }
        let p = (time / self.animation_duration).clamp(0.0, 1.0) as f32;
        1.0 - (1.0 - p).powi(3)
    }

    fn paint(&self, canvas: &Canvas, layout_w: f32, layout_h: f32, time: f64) {
        let w = layout_w;
        let h = layout_h;

        if self.data.is_empty() || self.color_scale.is_empty() {
            return;
        }

        let progress = self.progress_at(time);

        // Find min/max across all cells
        let mut min_val = f64::MAX;
        let mut max_val = f64::MIN;
        for row in &self.data {
            for &val in row {
                min_val = min_val.min(val);
                max_val = max_val.max(val);
            }
        }
        let range = (max_val - min_val).max(0.001);

        // Animation: clip rect expanding from left to right
        let clip_w = w * progress;
        canvas.save();
        canvas.clip_rect(
            Rect::from_xywh(0.0, 0.0, clip_w, h),
            skia_safe::ClipOp::Intersect,
            false,
        );

        let step = self.cell_size + self.cell_gap;

        for (row_idx, row) in self.data.iter().enumerate() {
            for (col_idx, &val) in row.iter().enumerate() {
                let normalized = ((val - min_val) / range) as f32;
                let (r, g, b) = interpolate_color(&self.color_scale, normalized);

                let x = col_idx as f32 * step;
                let y = row_idx as f32 * step;

                let rect = Rect::from_xywh(x, y, self.cell_size, self.cell_size);
                let rrect = RRect::new_rect_xy(rect, self.cell_radius, self.cell_radius);

                let color = skia_safe::Color::from_rgb(r, g, b);
                let mut paint = skia_safe::Paint::default();
                paint.set_color(color);
                paint.set_style(PaintStyle::Fill);
                paint.set_anti_alias(true);

                canvas.draw_rrect(rrect, &paint);
            }
        }

        canvas.restore();
    }
}

impl Painter for Heatmap {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        self.paint(canvas, layout.width, layout.height, ctx.time);
    }
}
