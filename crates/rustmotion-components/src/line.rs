use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::paint_from_hex;
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

/// A line component that draws a line from (x1, y1) to (x2, y2).
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Line {
    #[serde(default)]
    pub x1: f32,
    #[serde(default)]
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
    #[serde(default = "default_line_width")]
    pub width: f32,
    #[serde(default = "default_line_color")]
    pub color: String,
    #[serde(default)]
    pub dashed: Option<Vec<f32>>,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

fn default_line_width() -> f32 {
    2.0
}

fn default_line_color() -> String {
    "#FFFFFF".to_string()
}

rustmotion_core::impl_traits!(Line {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Line {
    fn paint(&self, canvas: &Canvas, props: &AnimatedProperties) {
        let mut paint = paint_from_hex(&self.color);
        paint.set_style(PaintStyle::Stroke);
        paint.set_stroke_width(self.width);
        paint.set_anti_alias(true);
        paint.set_stroke_cap(skia_safe::PaintCap::Round);

        // Apply dashed style
        if let Some(ref intervals) = self.dashed {
            if intervals.len() >= 2 {
                if let Some(dash) = skia_safe::PathEffect::dash(intervals, 0.0) {
                    paint.set_path_effect(dash);
                }
            }
        }

        // Apply draw_progress
        if props.draw_progress >= 0.0 && props.draw_progress < 1.0 {
            let dx = self.x2 - self.x1;
            let dy = self.y2 - self.y1;
            let length = (dx * dx + dy * dy).sqrt();
            let draw_len = length * props.draw_progress.clamp(0.0, 1.0);
            let intervals = [draw_len, length - draw_len + 0.01];
            if let Some(dash) = skia_safe::PathEffect::dash(&intervals, 0.0) {
                paint.set_path_effect(dash);
            }
        }

        canvas.draw_line(
            (self.x1, self.y1),
            (self.x2, self.y2),
            &paint,
        );
    }
}

impl Painter for Line {
    fn paint_content(
        &self,
        canvas: &Canvas,
        _layout: &BoxLayout,
        props: &AnimatedProperties,
        _ctx: &PaintCtx,
    ) {
        self.paint(canvas, props);
    }
}
