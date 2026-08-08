use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Color, PaintStyle, Path, Point, Rect};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{paint_from_hex, parse_hex_color};
use rustmotion_core::schema::TimelineStep;
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
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(Sparkline {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

/// `(min, max, normalize)` for a min-max scaled series, matching
/// `chart::line::series_scale`'s flat-series handling: a constant series is
/// centred (`0.5`) instead of collapsing to the bottom edge. Dividing by a
/// `max(0.001)` floor mapped a constant series to 0, so a flat series read
/// as "all zero" instead of "constant at some value". Duplicated locally —
/// `chart::line::series_scale` is `pub(super)` to the `chart` module, not
/// reachable from here.
fn series_scale(values: impl Iterator<Item = f64> + Clone) -> (f64, f64, impl Fn(f64) -> f32) {
    let min_val = values.clone().fold(f64::INFINITY, f64::min);
    let max_val = values.fold(f64::NEG_INFINITY, f64::max);
    let (min_val, max_val) = if min_val.is_finite() && max_val.is_finite() {
        (min_val, max_val)
    } else {
        (0.0, 0.0)
    };
    let span = max_val - min_val;
    let flat = span.abs() < f64::EPSILON;
    let range = if flat { 1.0 } else { span };
    (min_val, max_val, move |v: f64| {
        if flat {
            0.5
        } else {
            ((v - min_val) / range) as f32
        }
    })
}

impl Sparkline {
    fn progress_at(&self, time: f64) -> f32 {
        if !self.animated {
            return 1.0;
        }
        // Ramp measured from `start_at`, not from scene time zero — matches
        // `Counter::ramp_progress`. A sparkline delayed with `start_at` used
        // to read raw scene time, so it was already fully revealed on the
        // very first frame it became visible.
        let start = self.timing.start_at.unwrap_or(0.0);
        let elapsed = (time - start).max(0.0);
        let p = (elapsed / self.animation_duration).clamp(0.0, 1.0) as f32;
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

        let (_, _, norm) = series_scale(self.data.iter().copied());

        let pad = self.stroke_width;

        let mut line_path = Path::new();
        let mut fill_path = Path::new();

        for (i, &val) in self.data.iter().enumerate() {
            let x = pad + (i as f32 / (n - 1) as f32) * (w - pad * 2.0);
            let y = pad + (h - pad * 2.0) - norm(val) * (h - pad * 2.0);

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

#[cfg(test)]
mod tests {
    use super::*;
    use rustmotion_core::traits::TimingConfig;

    fn base_sparkline(data: Vec<f64>) -> Sparkline {
        Sparkline {
            data,
            color: default_color(),
            fill: false,
            fill_opacity: default_fill_opacity(),
            stroke_width: default_stroke_width(),
            animated: true,
            animation_duration: 1.0,
            timing: TimingConfig::default(),
            style: CssStyle::default(),
            timeline: Vec::new(),
            stagger: None,
        }
    }

    fn ink_bounds(
        surface: &mut skia_safe::Surface,
        w: i32,
        h: i32,
    ) -> Option<(i32, i32, i32, i32)> {
        let snapshot = surface.image_snapshot();
        let info = skia_safe::ImageInfo::new(
            (w, h),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut buf = vec![0u8; (w * h * 4) as usize];
        snapshot.read_pixels(
            &info,
            &mut buf,
            (w * 4) as usize,
            skia_safe::IPoint::new(0, 0),
            skia_safe::image::CachingHint::Disallow,
        );
        let (mut minx, mut maxx, mut miny, mut maxy) = (i32::MAX, i32::MIN, i32::MAX, i32::MIN);
        for y in 0..h {
            for x in 0..w {
                if buf[((y * w + x) * 4 + 3) as usize] > 0 {
                    minx = minx.min(x);
                    maxx = maxx.max(x);
                    miny = miny.min(y);
                    maxy = maxy.max(y);
                }
            }
        }
        (minx <= maxx).then_some((minx, maxx, miny, maxy))
    }

    #[test]
    fn a_flat_series_is_centered_not_pinned_to_the_bottom_edge() {
        // #7's exact repro: with every value equal, `(val - min_val)` is
        // 0 for every point, so the line was drawn on the bottom edge
        // (`h - pad`) — reading as "a series of zeroes" instead of "a
        // constant series at some value". `chart::line::series_scale`
        // was fixed to center a flat series (0.5) for exactly this reason.
        const H: i32 = 40;
        let flat = base_sparkline(vec![7.0, 7.0, 7.0, 7.0, 7.0]);
        let mut surface = skia_safe::surfaces::raster_n32_premul((120, H)).expect("raster surface");
        {
            let canvas = surface.canvas();
            flat.paint(canvas, 120.0, H as f32, 10.0);
        }
        let (_minx, _maxx, miny, maxy) =
            ink_bounds(&mut surface, 120, H).expect("flat sparkline must still paint a line");
        let mid = (miny + maxy) as f32 / 2.0;
        let bottom_edge = H as f32 - flat.stroke_width;
        assert!(
            (mid - H as f32 / 2.0).abs() < 6.0,
            "flat series line should sit near vertical center (y~{}), got y=[{miny}..{maxy}]",
            H / 2
        );
        assert!(
            (bottom_edge - maxy as f32).abs() > 6.0,
            "flat series line must not be pinned to the bottom edge: y=[{miny}..{maxy}], bottom={bottom_edge}"
        );
    }

    #[test]
    fn progress_ramp_starts_at_start_at_not_at_scene_time_zero() {
        let mut sparkline = base_sparkline(vec![1.0, 2.0, 3.0]);
        sparkline.animation_duration = 1.5;
        sparkline.timing = TimingConfig {
            start_at: Some(2.0),
            end_at: None,
        };
        assert_eq!(sparkline.progress_at(2.0), 0.0);
        assert!(sparkline.progress_at(2.75) < 1.0);
        assert_eq!(sparkline.progress_at(3.5), 1.0);
    }
}
