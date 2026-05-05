use rustmotion_core::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, Path, PathMeasure, Point};

use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::paint_from_hex;
use rustmotion_core::layout::{Constraints, LayoutNode};
use rustmotion_core::schema::LayerStyle;
use rustmotion_core::traits::{PaintCtx, Painter, RenderContext, TimingConfig, Widget};

/// Curved arrow component with optional bezier control points and oriented arrowhead.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Arrow {
    #[serde(default)]
    pub x1: f32,
    #[serde(default)]
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
    /// Bezier control point (for quadratic curve). Mutually exclusive with cp1/cp2.
    #[serde(default)]
    pub cp: Option<ControlPoint>,
    /// First bezier control point (for cubic curve).
    #[serde(default)]
    pub cp1: Option<ControlPoint>,
    /// Second bezier control point (for cubic curve).
    #[serde(default)]
    pub cp2: Option<ControlPoint>,
    /// Curvature intensity for auto-generated control point (-1.0 to 1.0).
    /// Positive = curve upward, negative = curve downward.
    /// Only used when no explicit cp/cp1/cp2 is provided.
    #[serde(default)]
    pub curve: Option<f32>,
    /// Stroke width.
    #[serde(default = "default_arrow_width")]
    pub width: f32,
    /// Stroke color.
    #[serde(default = "default_arrow_color")]
    pub color: String,
    /// Show arrowhead at end (default: true).
    #[serde(default = "default_true")]
    pub arrow_end: bool,
    /// Show arrowhead at start.
    #[serde(default)]
    pub arrow_start: bool,
    /// Size of the arrowhead (default: 12.0).
    #[serde(default = "default_arrow_size")]
    pub arrow_size: f32,
    #[serde(default)]
    pub dashed: Option<Vec<f32>>,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ControlPoint {
    pub x: f32,
    pub y: f32,
}

fn default_arrow_width() -> f32 { 3.0 }
fn default_arrow_color() -> String { "#FFFFFF".to_string() }
fn default_arrow_size() -> f32 { 12.0 }
fn default_true() -> bool { true }

rustmotion_core::impl_traits!(Arrow {
    Animatable => style,
    Timed => timing,
    Styled => style,
});

impl Arrow {
    /// Build the bezier path for this arrow (without arrowheads).
    fn build_path(&self) -> Path {
        let mut path = Path::new();
        path.move_to((self.x1, self.y1));

        if let (Some(cp1), Some(cp2)) = (&self.cp1, &self.cp2) {
            // Cubic bezier
            path.cubic_to((cp1.x, cp1.y), (cp2.x, cp2.y), (self.x2, self.y2));
        } else if let Some(cp) = &self.cp {
            // Quadratic bezier
            path.quad_to((cp.x, cp.y), (self.x2, self.y2));
        } else if let Some(curve) = self.curve {
            // Auto-generate a quadratic control point based on curve intensity
            let mid_x = (self.x1 + self.x2) / 2.0;
            let mid_y = (self.y1 + self.y2) / 2.0;
            let dx = self.x2 - self.x1;
            let dy = self.y2 - self.y1;
            let len = (dx * dx + dy * dy).sqrt();
            // Perpendicular offset
            let perp_x = -dy / len * curve * len * 0.3;
            let perp_y = dx / len * curve * len * 0.3;
            path.quad_to((mid_x + perp_x, mid_y + perp_y), (self.x2, self.y2));
        } else {
            // Straight line
            path.line_to((self.x2, self.y2));
        }

        path
    }

    /// Draw an arrowhead at the given position along the path.
    fn draw_arrowhead(canvas: &Canvas, path: &Path, at_end: bool, size: f32, paint: &skia_safe::Paint) {
        let mut measure = PathMeasure::new(path, false, None);
        let total_len = measure.length();
        if total_len < 1.0 { return; }

        let (pos, tangent) = if at_end {
            let dist = total_len - 0.1;
            match measure.pos_tan(dist) {
                Some((p, t)) => (p, t),
                None => return,
            }
        } else {
            let dist = 0.1;
            match measure.pos_tan(dist) {
                Some((p, t)) => (p, Point::new(-t.x, -t.y)),
                None => return,
            }
        };

        let angle = tangent.y.atan2(tangent.x);
        let half_angle = std::f32::consts::PI / 6.0; // 30 degrees

        let mut arrow_path = Path::new();
        arrow_path.move_to(pos);
        arrow_path.line_to((
            pos.x - size * (angle - half_angle).cos(),
            pos.y - size * (angle - half_angle).sin(),
        ));
        arrow_path.move_to(pos);
        arrow_path.line_to((
            pos.x - size * (angle + half_angle).cos(),
            pos.y - size * (angle + half_angle).sin(),
        ));

        let mut arrow_paint = paint.clone();
        arrow_paint.set_path_effect(None);
        arrow_paint.set_stroke_cap(skia_safe::PaintCap::Round);
        canvas.draw_path(&arrow_path, &arrow_paint);
    }

    fn paint(&self, canvas: &Canvas, props: &AnimatedProperties) {
        let path = self.build_path();

        let mut paint = paint_from_hex(&self.color);
        paint.set_style(PaintStyle::Stroke);
        paint.set_stroke_width(self.width);
        paint.set_anti_alias(true);
        paint.set_stroke_cap(skia_safe::PaintCap::Round);
        paint.set_stroke_join(skia_safe::PaintJoin::Round);

        if let Some(ref intervals) = self.dashed {
            if intervals.len() >= 2 {
                if let Some(dash) = skia_safe::PathEffect::dash(intervals, 0.0) {
                    paint.set_path_effect(dash);
                }
            }
        }

        if props.draw_progress >= 0.0 && props.draw_progress < 1.0 {
            let mut measure = PathMeasure::new(&path, false, None);
            let total_len = measure.length();
            if total_len > 0.0 {
                let draw_len = total_len * props.draw_progress.clamp(0.0, 1.0);
                let intervals = [draw_len, total_len - draw_len + 0.01];
                if let Some(dash) = skia_safe::PathEffect::dash(&intervals, 0.0) {
                    paint.set_path_effect(dash);
                }
            }
        }

        canvas.draw_path(&path, &paint);

        let show_arrows = props.draw_progress < 0.0 || props.draw_progress >= 0.95;
        if show_arrows {
            if self.arrow_end {
                Self::draw_arrowhead(canvas, &path, true, self.arrow_size, &paint);
            }
            if self.arrow_start {
                Self::draw_arrowhead(canvas, &path, false, self.arrow_size, &paint);
            }
        }
    }
}

impl Widget for Arrow {
    fn render(
        &self,
        canvas: &Canvas,
        _layout: &LayoutNode,
        _ctx: &RenderContext,
        props: &AnimatedProperties,
        _pipeline: &dyn rustmotion_core::traits::RenderPipeline,
    ) -> Result<()> {
        self.paint(canvas, props);
        Ok(())
    }

    fn measure(&self, _constraints: &Constraints) -> (f32, f32) {
        let mut min_x = self.x1.min(self.x2);
        let mut max_x = self.x1.max(self.x2);
        let mut min_y = self.y1.min(self.y2);
        let mut max_y = self.y1.max(self.y2);

        // Include control points in bounds
        if let Some(ref cp) = self.cp {
            min_x = min_x.min(cp.x);
            max_x = max_x.max(cp.x);
            min_y = min_y.min(cp.y);
            max_y = max_y.max(cp.y);
        }
        if let Some(ref cp1) = self.cp1 {
            min_x = min_x.min(cp1.x);
            max_x = max_x.max(cp1.x);
            min_y = min_y.min(cp1.y);
            max_y = max_y.max(cp1.y);
        }
        if let Some(ref cp2) = self.cp2 {
            min_x = min_x.min(cp2.x);
            max_x = max_x.max(cp2.x);
            min_y = min_y.min(cp2.y);
            max_y = max_y.max(cp2.y);
        }

        let w = (max_x - min_x).max(1.0);
        let h = (max_y - min_y).max(1.0);
        (w, h)
    }
}

impl Painter for Arrow {
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
