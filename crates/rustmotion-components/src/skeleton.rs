use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Color, PaintStyle, Point, Rect};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::paint_from_hex;
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

fn default_base_color() -> String {
    "#1E293B".to_string()
}

fn default_shimmer_color() -> String {
    "#334155".to_string()
}

fn default_border_radius() -> f32 {
    8.0
}

fn default_speed() -> f32 {
    1.5
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SkeletonVariant {
    Rectangle,
    Circle,
    Text,
}

impl Default for SkeletonVariant {
    fn default() -> Self {
        Self::Rectangle
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Skeleton {
    #[serde(default)]
    pub variant: SkeletonVariant,
    #[serde(default = "default_base_color")]
    pub base_color: String,
    #[serde(default = "default_shimmer_color")]
    pub shimmer_color: String,
    #[serde(default = "default_border_radius")]
    pub border_radius: f32,
    #[serde(default = "default_speed")]
    pub speed: f32,
    /// Number of text lines to render (only for `text` variant).
    #[serde(default = "default_lines")]
    pub lines: u32,
    /// Line height for text variant.
    #[serde(default = "default_line_height")]
    pub line_height: f32,
    /// Gap between text lines.
    #[serde(default = "default_line_gap")]
    pub line_gap: f32,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

fn default_lines() -> u32 {
    3
}

fn default_line_height() -> f32 {
    16.0
}

fn default_line_gap() -> f32 {
    12.0
}

rustmotion_core::impl_traits!(Skeleton {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Skeleton {
    fn draw_shimmer_rect(&self, canvas: &Canvas, rect: Rect, radius: f32, time: f64) {
        let w = rect.width();

        // Base color
        let mut base_paint = paint_from_hex(&self.base_color);
        base_paint.set_style(PaintStyle::Fill);
        base_paint.set_anti_alias(true);
        let rrect = skia_safe::RRect::new_rect_xy(rect, radius, radius);
        canvas.draw_rrect(rrect, &base_paint);

        // Shimmer gradient sweep
        let cycle = (time as f32 / self.speed).fract();
        // The shimmer band sweeps from left to right
        let shimmer_w = w * 0.4;
        let shimmer_x = rect.left + (w + shimmer_w) * cycle - shimmer_w;

        let (r, g, b, _) = rustmotion_core::engine::renderer::parse_hex_color(&self.shimmer_color);
        let transparent = Color::from_argb(0, r, g, b);
        let highlight = Color::from_argb(180, r, g, b);

        let shader = skia_safe::shader::Shader::linear_gradient(
            (
                Point::new(shimmer_x, 0.0),
                Point::new(shimmer_x + shimmer_w, 0.0),
            ),
            skia_safe::gradient_shader::GradientShaderColors::Colors(&[
                transparent,
                highlight,
                transparent,
            ]),
            None,
            skia_safe::TileMode::Clamp,
            None,
            None,
        );

        if let Some(shader) = shader {
            canvas.save();
            canvas.clip_rrect(rrect, skia_safe::ClipOp::Intersect, true);
            let mut shimmer_paint = skia_safe::Paint::default();
            shimmer_paint.set_style(PaintStyle::Fill);
            shimmer_paint.set_anti_alias(true);
            shimmer_paint.set_shader(shader);
            canvas.draw_rect(rect, &shimmer_paint);
            canvas.restore();
        }
    }
}

impl Skeleton {
    fn paint(&self, canvas: &Canvas, layout_w: f32, layout_h: f32, time: f64) {
        let w = layout_w;
        let h = layout_h;

        match self.variant {
            SkeletonVariant::Rectangle => {
                let rect = Rect::from_xywh(0.0, 0.0, w, h);
                self.draw_shimmer_rect(canvas, rect, self.border_radius, time);
            }
            SkeletonVariant::Circle => {
                let diameter = w.min(h);
                let rect = Rect::from_xywh(0.0, 0.0, diameter, diameter);
                self.draw_shimmer_rect(canvas, rect, diameter / 2.0, time);
            }
            SkeletonVariant::Text => {
                let total_lines = self.lines.max(1);
                for i in 0..total_lines {
                    let y = i as f32 * (self.line_height + self.line_gap);
                    let line_w = if i == total_lines - 1 { w * 0.6 } else { w };
                    let rect = Rect::from_xywh(0.0, y, line_w, self.line_height);
                    self.draw_shimmer_rect(canvas, rect, self.border_radius * 0.5, time);
                }
            }
        }
    }
}

impl Painter for Skeleton {
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
