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
        let drawing = props.draw_progress >= 0.0 && props.draw_progress < 1.0;
        if drawing && props.draw_progress <= 0.0 {
            return;
        }

        let mut paint = paint_from_hex(&self.color);
        paint.set_style(PaintStyle::Stroke);
        paint.set_stroke_width(self.width);
        paint.set_anti_alias(true);
        paint.set_stroke_cap(skia_safe::PaintCap::Round);

        if let Some(ref intervals) = self.dashed {
            if intervals.len() >= 2 {
                if let Some(dash) = skia_safe::PathEffect::dash(intervals, 0.0) {
                    paint.set_path_effect(dash);
                }
            }
        }

        if drawing {
            let dx = self.x2 - self.x1;
            let dy = self.y2 - self.y1;
            let length = (dx * dx + dy * dy).sqrt();
            let draw_len = length * props.draw_progress.clamp(0.0, 1.0);
            let intervals = [draw_len, length - draw_len + 0.01];
            if let Some(dash) = skia_safe::PathEffect::dash(&intervals, 0.0) {
                paint.set_path_effect(dash);
            }
        }

        canvas.draw_line((self.x1, self.y1), (self.x2, self.y2), &paint);
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
            video_width: 1920,
            video_height: 1080,
            stagger_offset: 0.0,
        }
    }

    fn drawable_line() -> Line {
        Line {
            x1: 10.0,
            y1: 50.0,
            x2: 90.0,
            y2: 50.0,
            width: 12.0,
            color: "#000000".to_string(),
            dashed: None,
            timing: Default::default(),
            style: Default::default(),
            timeline: Vec::new(),
            stagger: None,
        }
    }

    fn lit_pixel_count(line: &Line, draw_progress: f32) -> usize {
        let layout = test_layout();
        let props = AnimatedProperties {
            draw_progress,
            ..Default::default()
        };
        let ctx = test_ctx();

        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        {
            let canvas = surface.canvas();
            line.paint_content(canvas, &layout, &props, &ctx);
        }

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

        (0..(W * H) as usize)
            .filter(|&i| buf[i * 4 + 3] > 10)
            .count()
    }

    #[test]
    fn draw_progress_zero_paints_nothing() {
        let line = drawable_line();
        let lit = lit_pixel_count(&line, 0.0);
        assert_eq!(
            lit, 0,
            "at draw_progress=0 the line must not paint a zero-length dash as a dot, got {lit} lit pixels"
        );
    }

    #[test]
    fn draw_progress_partial_paints_a_partial_stroke() {
        let line = drawable_line();
        let lit = lit_pixel_count(&line, 0.5);
        assert!(
            lit > 0,
            "at draw_progress=0.5 the line must paint a partial stroke, got {lit} lit pixels"
        );
    }
}
