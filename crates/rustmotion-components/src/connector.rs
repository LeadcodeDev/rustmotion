use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, Path, PathMeasure, Point};

use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::paint_from_hex;
use rustmotion_core::schema::LayerStyle;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

/// Connector component that draws a routed line between two points.
/// Supports straight, curved (bezier), and elbow (right-angle) routing.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Connector {
    /// Start point.
    pub from: ConnectorPoint,
    /// End point.
    pub to: ConnectorPoint,
    /// Routing mode (default: straight).
    #[serde(default)]
    pub routing: RoutingMode,
    /// Curvature intensity for curved routing (-1.0 to 1.0, default: 0.4).
    #[serde(default = "default_curvature")]
    pub curvature: f32,
    /// Stroke width.
    #[serde(default = "default_connector_width")]
    pub width: f32,
    /// Stroke color.
    #[serde(default = "default_connector_color")]
    pub color: String,
    /// Show arrowhead at end (default: true).
    #[serde(default = "default_true")]
    pub arrow_end: bool,
    /// Show arrowhead at start.
    #[serde(default)]
    pub arrow_start: bool,
    /// Arrowhead size.
    #[serde(default = "default_arrow_size")]
    pub arrow_size: f32,
    /// Dashed line intervals.
    #[serde(default)]
    pub dashed: Option<Vec<f32>>,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConnectorPoint {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum RoutingMode {
    /// Direct straight line.
    #[default]
    Straight,
    /// Smooth bezier curve.
    Curved,
    /// Right-angle elbow routing.
    Elbow,
}

fn default_curvature() -> f32 { 0.4 }
fn default_connector_width() -> f32 { 2.0 }
fn default_connector_color() -> String { "#FFFFFF".to_string() }
fn default_arrow_size() -> f32 { 10.0 }
fn default_true() -> bool { true }

rustmotion_core::impl_traits!(Connector {
    Animatable => style,
    Timed => timing,
    Styled => style,
});

impl Connector {
    fn build_path(&self) -> Path {
        let mut path = Path::new();
        let (x1, y1) = (self.from.x, self.from.y);
        let (x2, y2) = (self.to.x, self.to.y);

        match self.routing {
            RoutingMode::Straight => {
                path.move_to((x1, y1));
                path.line_to((x2, y2));
            }
            RoutingMode::Curved => {
                path.move_to((x1, y1));
                let dx = x2 - x1;
                let dy = y2 - y1;
                let len = (dx * dx + dy * dy).sqrt();
                let mid_x = (x1 + x2) / 2.0;
                let mid_y = (y1 + y2) / 2.0;
                // Perpendicular offset for curvature
                let perp_x = -dy / len * self.curvature * len * 0.3;
                let perp_y = dx / len * self.curvature * len * 0.3;
                path.quad_to((mid_x + perp_x, mid_y + perp_y), (x2, y2));
            }
            RoutingMode::Elbow => {
                path.move_to((x1, y1));
                // Route: horizontal first, then vertical
                let mid_x = (x1 + x2) / 2.0;
                path.line_to((mid_x, y1));
                path.line_to((mid_x, y2));
                path.line_to((x2, y2));
            }
        }

        path
    }

    fn draw_arrowhead(canvas: &Canvas, path: &Path, at_end: bool, size: f32, paint: &skia_safe::Paint) {
        let mut measure = PathMeasure::new(path, false, None);
        let total_len = measure.length();
        if total_len < 1.0 { return; }

        let (pos, tangent) = if at_end {
            match measure.pos_tan(total_len - 0.1) {
                Some((p, t)) => (p, t),
                None => return,
            }
        } else {
            match measure.pos_tan(0.1) {
                Some((p, t)) => (p, Point::new(-t.x, -t.y)),
                None => return,
            }
        };

        let angle = tangent.y.atan2(tangent.x);
        let half_angle = std::f32::consts::PI / 6.0;

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

impl Painter for Connector {
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
