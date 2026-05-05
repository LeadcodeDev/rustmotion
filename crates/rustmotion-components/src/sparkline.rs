use rustmotion_core::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Color, PaintStyle, Path, Point, Rect};

use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{paint_from_hex, parse_hex_color};
use rustmotion_core::schema::{LayerStyle, Size};
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

fn default_color() -> String {
    "#22C55E".to_string()
}

fn default_stroke_width() -> f32 {
    2.0
}

fn default_fill_opacity() -> f32 {
    0.2
}

fn default_animated() -> bool {
    true
}

fn default_animation_duration() -> f64 {
    1.0
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Sparkline {
    pub data: Vec<f64>,
    #[serde(default)]
    pub size: Option<Size>,
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default)]
    pub fill: bool,
    #[serde(default = "default_fill_opacity")]
    pub fill_opacity: f32,
    #[serde(default = "default_stroke_width")]
    pub stroke_width: f32,
    #[serde(default = "default_animated")]
    pub animated: bool,
    #[serde(default = "default_animation_duration")]
    pub animation_duration: f64,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

rustmotion_core::impl_traits!(Sparkline {
    Animatable => style,
    Timed => timing,
    Styled => style,
});

impl Sparkline {
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
        let n = self.data.len();
        if n < 2 {
            return;
        }

        let progress = self.progress_at(time);

        let max_val = self.data.iter().fold(f64::MIN, |a, &b| a.max(b));
        let min_val = self.data.iter().fold(f64::MAX, |a, &b| a.min(b));
        let range = (max_val - min_val).max(0.001);

        let pad = self.stroke_width;

        let mut line_path = Path::new();
        let mut fill_path = Path::new();

        for (i, &val) in self.data.iter().enumerate() {
            let x = pad + (i as f32 / (n - 1) as f32) * (w - pad * 2.0);
            let y = pad + (h - pad * 2.0) - ((val - min_val) / range) as f32 * (h - pad * 2.0);

            if i == 0 {
                line_path.move_to((x, y));
                fill_path.move_to((x, h - pad));
                fill_path.line_to((x, y));
            } else {
                line_path.line_to((x, y));
                fill_path.line_to((x, y));
            }
        }

        let last_x = pad + (w - pad * 2.0);
        fill_path.line_to((last_x, h - pad));
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
        if self.fill {
            let (r, g, b, _) = parse_hex_color(&self.color);
            let top_color = Color::from_argb((self.fill_opacity * 255.0) as u8, r, g, b);
            let bottom_color = Color::from_argb(0, r, g, b);

            let shader = skia_safe::shader::Shader::linear_gradient(
                (Point::new(0.0, 0.0), Point::new(0.0, h)),
                skia_safe::gradient_shader::GradientShaderColors::Colors(&[
                    top_color,
                    bottom_color,
                ]),
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
        }

        // Line stroke
        let mut line_paint = paint_from_hex(&self.color);
        line_paint.set_style(PaintStyle::Stroke);
        line_paint.set_stroke_width(self.stroke_width);
        line_paint.set_anti_alias(true);
        line_paint.set_stroke_cap(skia_safe::paint::Cap::Round);
        line_paint.set_stroke_join(skia_safe::paint::Join::Round);
        canvas.draw_path(&line_path, &line_paint);

        canvas.restore();
    }
}

impl Painter for Sparkline {
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
