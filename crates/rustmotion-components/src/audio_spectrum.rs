use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Color, Paint, PaintStyle, Point, Rect};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::audio_analysis::audio_analysis_cache;
use rustmotion_core::engine::renderer::parse_hex_color;
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

fn default_bars() -> u32 {
    16
}
fn default_color() -> String {
    "#38bdf8".to_string()
}
fn default_bar_gap() -> f32 {
    2.0
}
fn default_min_height() -> f32 {
    2.0
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SpectrumMode {
    #[default]
    Bars,
    Radial,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct AudioSpectrum {
    /// Source audio track (src path). If None, uses the first track in the cache.
    #[serde(default)]
    pub track: Option<String>,
    /// Number of frequency bars to display (resampled from 16 internal bands).
    #[serde(default = "default_bars")]
    pub bars: u32,
    /// Display mode: bars (vertical) or radial.
    #[serde(default)]
    pub mode: SpectrumMode,
    /// Bar color as hex string.
    #[serde(default = "default_color")]
    pub color: String,
    /// Gap between bars in pixels.
    #[serde(default = "default_bar_gap")]
    pub bar_gap: f32,
    /// Minimum bar height in pixels.
    #[serde(default = "default_min_height")]
    pub min_height: f32,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(AudioSpectrum {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl AudioSpectrum {
    fn get_band_values(&self, time: f64) -> Vec<f32> {
        let cache = audio_analysis_cache();
        let analysis = if let Some(ref src) = self.track {
            cache.get(src).map(|r| r.clone())
        } else {
            cache.iter().next().map(|r| r.value().clone())
        };

        let n = self.bars.max(1) as usize;

        // Empty cache / missing track → all-zero values; the painter clamps
        // each bar to `min_height`, so this degrades to a flat baseline.
        let Some(analysis) = analysis else {
            return vec![0.0; n];
        };

        // Resample from 16 bands to n bars
        let num_bands = 16usize;
        (0..n)
            .map(|i| {
                let band_f = i as f32 * (num_bands as f32 - 1.0) / (n as f32 - 1.0).max(1.0);
                let band_lo = band_f as usize;
                let band_hi = (band_lo + 1).min(num_bands - 1);
                let frac = band_f - band_lo as f32;
                let v_lo = analysis.band_at(time, band_lo as u8);
                let v_hi = analysis.band_at(time, band_hi as u8);
                v_lo * (1.0 - frac) + v_hi * frac
            })
            .collect()
    }
}

impl Painter for AudioSpectrum {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        let w = layout.width;
        let h = layout.height;
        let n = self.bars.max(1) as usize;
        let values = self.get_band_values(ctx.time);
        let (r, g, b, a) = parse_hex_color(&self.color);
        let color = Color::from_argb(a, r, g, b);

        match self.mode {
            SpectrumMode::Bars => {
                let total_gap = self.bar_gap * (n as f32 - 1.0);
                let bar_w = ((w - total_gap) / n as f32).max(1.0);
                let mut paint = Paint::default();
                paint.set_color(color);
                paint.set_style(PaintStyle::Fill);
                paint.set_anti_alias(true);

                for (i, &v) in values.iter().enumerate() {
                    let bar_h = (v * h).max(self.min_height);
                    let x = i as f32 * (bar_w + self.bar_gap);
                    let y = h - bar_h;
                    canvas.draw_rect(Rect::from_xywh(x, y, bar_w, bar_h), &paint);
                }
            }
            SpectrumMode::Radial => {
                let cx = w / 2.0;
                let cy = h / 2.0;
                let max_r = cx.min(cy);
                let inner_r = max_r * 0.3;

                let mut paint = Paint::default();
                paint.set_color(color);
                paint.set_style(PaintStyle::Stroke);
                paint.set_stroke_width(2.0);
                paint.set_anti_alias(true);

                for (i, &v) in values.iter().enumerate() {
                    let angle = (i as f32 / n as f32) * 2.0 * std::f32::consts::PI;
                    let bar_len = (v * (max_r - inner_r)).max(self.min_height);
                    let x0 = cx + inner_r * angle.cos();
                    let y0 = cy + inner_r * angle.sin();
                    let x1 = cx + (inner_r + bar_len) * angle.cos();
                    let y1 = cy + (inner_r + bar_len) * angle.sin();
                    canvas.draw_line(Point::new(x0, y0), Point::new(x1, y1), &paint);
                }
            }
        }
    }
}
