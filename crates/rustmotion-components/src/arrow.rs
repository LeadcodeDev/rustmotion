use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, Path, PathBuilder, PathMeasure, Point};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::paint_from_hex;
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

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
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ControlPoint {
    pub x: f32,
    pub y: f32,
}

fn default_arrow_width() -> f32 {
    3.0
}
fn default_arrow_color() -> String {
    "#FFFFFF".to_string()
}
fn default_arrow_size() -> f32 {
    12.0
}
fn default_true() -> bool {
    true
}

rustmotion_core::impl_traits!(Arrow {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Arrow {
    fn build_path(&self) -> Path {
        let mut path = PathBuilder::new();
        path.move_to((self.x1, self.y1));

        if let (Some(cp1), Some(cp2)) = (&self.cp1, &self.cp2) {
            path.cubic_to((cp1.x, cp1.y), (cp2.x, cp2.y), (self.x2, self.y2));
        } else if let Some(cp) = &self.cp {
            path.quad_to((cp.x, cp.y), (self.x2, self.y2));
        } else if let Some(curve) = self.curve {
            let mid_x = (self.x1 + self.x2) / 2.0;
            let mid_y = (self.y1 + self.y2) / 2.0;
            let dx = self.x2 - self.x1;
            let dy = self.y2 - self.y1;
            let len = (dx * dx + dy * dy).sqrt();
            let perp_x = -dy / len * curve * len * 0.3;
            let perp_y = dx / len * curve * len * 0.3;
            path.quad_to((mid_x + perp_x, mid_y + perp_y), (self.x2, self.y2));
        } else {
            path.line_to((self.x2, self.y2));
        }

        path.detach()
    }

    fn draw_arrowhead(
        canvas: &Canvas,
        path: &Path,
        at_end: bool,
        size: f32,
        paint: &skia_safe::Paint,
    ) {
        let mut measure = PathMeasure::new(path, false, None);
        let total_len = measure.length();
        if total_len < 1.0 {
            return;
        }

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
        let half_angle = std::f32::consts::PI / 6.0;

        let mut arrow_path = PathBuilder::new();
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
        canvas.draw_path(&arrow_path.detach(), &arrow_paint);
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

        let drawing = props.draw_progress >= 0.0 && props.draw_progress < 1.0;
        let draw_start = props.draw_start.max(0.0);
        let draw_offset = props.draw_offset;
        let trimming = draw_start > 0.0 || draw_offset.abs() > 0.0005;
        let trim_start = (draw_start + draw_offset).clamp(0.0, 1.0);
        let trim_end = if drawing {
            (props.draw_progress.clamp(0.0, 1.0) + draw_offset).clamp(0.0, 1.0)
        } else {
            (1.0 + draw_offset).clamp(0.0, 1.0)
        };

        if trim_end <= trim_start && (drawing || trimming) {
            return;
        }

        if drawing || trimming {
            let trimmed =
                rustmotion_core::engine::renderer::trim_path_between(&path, trim_start, trim_end);
            canvas.draw_path(&trimmed, &paint);
        } else {
            canvas.draw_path(&path, &paint);
        }

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

#[cfg(test)]
mod tests {
    use super::*;
    use rustmotion_core::engine::layout_pass::Insets;

    const W: i32 = 100;
    const H: i32 = 100;

    fn test_layout() -> BoxLayout {
        BoxLayout {
            x: 0.0,
            y: 0.0,
            width: W as f32,
            height: H as f32,
            border: Insets::default(),
            padding: Insets::default(),
        }
    }

    fn test_ctx() -> PaintCtx {
        PaintCtx {
            time: 0.0,
            scenario_time: 0.0,
            scene_duration: 1.0,
            frame_index: 0,
            fps: 30,
            video_width: W as u32,
            video_height: H as u32,
            stagger_offset: 0.0,
        }
    }

    fn straight_arrow() -> Arrow {
        Arrow {
            x1: 10.0,
            y1: 50.0,
            x2: 90.0,
            y2: 50.0,
            cp: None,
            cp1: None,
            cp2: None,
            curve: None,
            width: 12.0,
            color: "#FFFFFF".to_string(),
            arrow_end: true,
            arrow_start: false,
            arrow_size: default_arrow_size(),
            dashed: None,
            timing: Default::default(),
            style: Default::default(),
            timeline: Vec::new(),
            stagger: None,
        }
    }

    fn render_alpha(arrow: &Arrow, props: &AnimatedProperties) -> Vec<u8> {
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        arrow.paint_content(surface.canvas(), &test_layout(), props, &test_ctx());
        let snapshot = surface.image_snapshot();
        let info = skia_safe::ImageInfo::new(
            (W, H),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Unpremul,
            None,
        );
        let mut buf = vec![0u8; (W * H * 4) as usize];
        let ok = snapshot.read_pixels(
            &info,
            &mut buf,
            (W * 4) as usize,
            skia_safe::IPoint::new(0, 0),
            skia_safe::image::CachingHint::Disallow,
        );
        assert!(ok, "pixel read should succeed");
        buf
    }

    fn lit_pixel_count(buf: &[u8]) -> usize {
        (0..(W * H) as usize)
            .filter(|&i| buf[i * 4 + 3] > 10)
            .count()
    }

    fn alpha_at(buf: &[u8], x: i32, y: i32) -> u8 {
        buf[((y * W + x) * 4 + 3) as usize]
    }

    #[test]
    fn draw_progress_zero_paints_nothing_not_even_a_dot() {
        let arrow = straight_arrow();
        let props = AnimatedProperties {
            draw_progress: 0.0,
            ..AnimatedProperties::default()
        };
        let lit = lit_pixel_count(&render_alpha(&arrow, &props));
        assert_eq!(
            lit, 0,
            "at draw_progress=0 a round-capped zero-length dash must not paint a solid dot, \
             got {lit} lit pixels"
        );
    }

    #[test]
    fn draw_progress_partial_paints_a_partial_stroke() {
        let arrow = straight_arrow();
        let props = AnimatedProperties {
            draw_progress: 0.5,
            ..AnimatedProperties::default()
        };
        let lit = lit_pixel_count(&render_alpha(&arrow, &props));
        assert!(
            lit > 0,
            "at draw_progress=0.5 the arrow must paint a partial stroke, got {lit} lit pixels"
        );
    }

    #[test]
    fn draw_start_erases_the_head_of_an_already_finished_arrow() {
        let arrow = straight_arrow();
        let props = AnimatedProperties {
            draw_start: 0.5,
            ..AnimatedProperties::default()
        };
        let buf = render_alpha(&arrow, &props);
        assert_eq!(
            alpha_at(&buf, 20, 50),
            0,
            "before a keyframe-driven draw_start=0.5 on a finished arrow must be erased"
        );
        assert!(
            alpha_at(&buf, 60, 50) > 40,
            "after draw_start=0.5 must stay painted"
        );
    }

    #[test]
    fn draw_offset_marches_the_drawn_window_along_the_path() {
        let arrow = straight_arrow();
        let no_offset = render_alpha(
            &arrow,
            &AnimatedProperties {
                draw_progress: 0.3,
                ..AnimatedProperties::default()
            },
        );
        let offset = render_alpha(
            &arrow,
            &AnimatedProperties {
                draw_progress: 0.3,
                draw_offset: 0.5,
                ..AnimatedProperties::default()
            },
        );
        assert!(
            alpha_at(&no_offset, 20, 50) > 40,
            "with no offset the window starts at the path's own beginning"
        );
        assert_eq!(
            alpha_at(&offset, 20, 50),
            0,
            "draw_offset=0.5 must march the window forward, leaving the path's start empty"
        );
        assert!(
            alpha_at(&offset, 70, 50) > 40,
            "and paint further along the path instead"
        );
    }
}
