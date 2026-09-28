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
        let trim_start = props.draw_start.clamp(0.0, 1.0);
        let trimming = props.draw_start > 0.0;
        let trim_end = if drawing {
            props.draw_progress.clamp(0.0, 1.0)
        } else {
            1.0
        };
        if trim_end <= trim_start && (drawing || trimming) {
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

        if drawing || trimming {
            let mut builder = skia_safe::PathBuilder::new();
            builder.move_to((self.x1, self.y1));
            builder.line_to((self.x2, self.y2));
            let trimmed = rustmotion_core::engine::renderer::trim_path_between(
                &builder.detach(),
                trim_start,
                trim_end,
            );
            canvas.draw_path(&trimmed, &paint);
            return;
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
#[cfg(test)]
mod draw_start_tests {
    use super::*;

    fn line_at(draw_start: f32, draw_progress: f32) -> Vec<u8> {
        let line = Line {
            x1: 10.0,
            y1: 50.0,
            x2: 90.0,
            y2: 50.0,
            color: "#FFFFFF".into(),
            width: 8.0,
            dashed: None,
            timing: Default::default(),
            style: Default::default(),
            timeline: Vec::new(),
            stagger: None,
        };
        let mut surface = skia_safe::surfaces::raster_n32_premul((100, 100)).unwrap();
        let props = AnimatedProperties {
            draw_start,
            draw_progress,
            ..Default::default()
        };
        line.paint(surface.canvas(), &props);

        let info = skia_safe::ImageInfo::new(
            (100, 100),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Unpremul,
            None,
        );
        let mut buf = vec![0u8; 100 * 100 * 4];
        surface.read_pixels(&info, &mut buf, 100 * 4, (0, 0));
        buf
    }

    fn lit_in_columns(buf: &[u8], x0: usize, x1: usize) -> usize {
        (0..100)
            .flat_map(|y| (x0..x1).map(move |x| (x, y)))
            .filter(|&(x, y)| buf[(y * 100 + x) * 4 + 3] > 40)
            .count()
    }

    #[test]
    fn draw_start_leaves_the_first_half_empty() {
        let buf = line_at(0.5, -1.0);
        assert_eq!(
            lit_in_columns(&buf, 0, 44),
            0,
            "draw_start 0.5 must erase the first half of the stroke. The probe stops at 44 \
             rather than 50 because the round cap extends half the 8px stroke back past the \
             cut, which is the cap doing its job and not the trim failing"
        );
        assert!(
            lit_in_columns(&buf, 52, 100) > 100,
            "and leave the second half painted"
        );
    }

    #[test]
    fn draw_start_and_draw_progress_bound_a_window() {
        let buf = line_at(0.3, 0.7);
        assert_eq!(lit_in_columns(&buf, 0, 30), 0, "before the window: empty");
        assert_eq!(lit_in_columns(&buf, 78, 100), 0, "after the window: empty");
        assert!(
            lit_in_columns(&buf, 40, 60) > 80,
            "inside the window: painted"
        );
    }

    #[test]
    fn no_draw_start_renders_exactly_as_before() {
        assert_eq!(
            line_at(-1.0, -1.0),
            line_at(0.0, -1.0),
            "draw_start absent and draw_start 0 must both paint the whole line, byte for byte"
        );
    }
}
