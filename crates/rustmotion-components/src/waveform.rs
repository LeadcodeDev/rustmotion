use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Color, Paint, PaintStyle, Path};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::audio_analysis::audio_analysis_cache;
use rustmotion_core::engine::renderer::parse_hex_color;
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

fn default_color() -> String {
    "#38bdf8".to_string()
}
fn default_window() -> f32 {
    2.0
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DrawStyle {
    #[default]
    Line,
    Filled,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Waveform {
    /// Source audio track (src path). If None, uses the first track in the cache.
    #[serde(default)]
    pub track: Option<String>,
    /// Waveform color as hex string.
    #[serde(default = "default_color")]
    pub color: String,
    /// Visual style: line or filled.
    #[serde(default)]
    pub draw_style: DrawStyle,
    /// Time window in seconds, centered on ctx.time.
    #[serde(default = "default_window")]
    pub window: f32,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(Waveform {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Painter for Waveform {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        let w = layout.width;
        let h = layout.height;
        let (r, g, b, a) = parse_hex_color(&self.color);
        let color = Color::from_argb(a, r, g, b);

        let cache = audio_analysis_cache();
        let analysis = if let Some(ref src) = self.track {
            cache.get(src).map(|r| r.clone())
        } else {
            cache.iter().next().map(|r| r.value().clone())
        };

        let Some(analysis) = analysis else {
            // Graceful degradation: flat line at mid-height
            let mut paint = Paint::default();
            paint.set_color(color);
            paint.set_style(PaintStyle::Stroke);
            paint.set_stroke_width(1.0);
            paint.set_anti_alias(true);
            canvas.draw_line(
                skia_safe::Point::new(0.0, h / 2.0),
                skia_safe::Point::new(w, h / 2.0),
                &paint,
            );
            return;
        };

        let half_window = self.window as f64 / 2.0;
        let t_start = (ctx.time - half_window).max(0.0);
        let t_end = ctx.time + half_window;

        // Sample N points along the window
        let n_points = w as usize;
        let mut points: Vec<(f32, f32)> = Vec::with_capacity(n_points);
        for i in 0..n_points {
            let t = t_start + (i as f64 / n_points.max(1) as f64) * (t_end - t_start);
            let amp = analysis.amplitude_at(t);
            let x = i as f32;
            let y = h / 2.0 - amp * (h / 2.0 - 2.0);
            points.push((x, y));
        }

        let mut paint = Paint::default();
        paint.set_color(color);
        paint.set_anti_alias(true);

        match self.draw_style {
            DrawStyle::Line => {
                paint.set_style(PaintStyle::Stroke);
                paint.set_stroke_width(1.5);
                if points.len() < 2 {
                    return;
                }
                let mut path = Path::new();
                path.move_to((points[0].0, points[0].1));
                for &(x, y) in &points[1..] {
                    path.line_to((x, y));
                }
                canvas.draw_path(&path, &paint);
            }
            DrawStyle::Filled => {
                // Filled area
                let (r2, g2, b2, _) = parse_hex_color(&self.color);
                let fill_color = Color::from_argb(100, r2, g2, b2);
                paint.set_style(PaintStyle::Fill);
                paint.set_color(fill_color);
                if points.is_empty() {
                    return;
                }
                let mut fill_path = Path::new();
                fill_path.move_to((0.0, h / 2.0));
                for &(x, y) in &points {
                    fill_path.line_to((x, y));
                }
                fill_path.line_to((w, h / 2.0));
                fill_path.close();
                canvas.draw_path(&fill_path, &paint);

                // Outline
                paint.set_style(PaintStyle::Stroke);
                paint.set_stroke_width(1.5);
                paint.set_color(color);
                if points.len() >= 2 {
                    let mut outline = Path::new();
                    outline.move_to((points[0].0, points[0].1));
                    for &(x, y) in &points[1..] {
                        outline.line_to((x, y));
                    }
                    canvas.draw_path(&outline, &paint);
                }
            }
        }
    }
}
