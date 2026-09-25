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
        let values = self.get_band_values(ctx.scenario_time);
        let (r, g, b, a) = parse_hex_color(&self.color);
        let color = Color::from_argb(a, r, g, b);

        canvas.save();
        canvas.clip_rect(
            Rect::from_xywh(0.0, 0.0, w.max(0.0), h.max(0.0)),
            skia_safe::ClipOp::Intersect,
            true,
        );

        match self.mode {
            SpectrumMode::Bars => {
                let stride = w / n as f32;
                let bar_w = (stride - self.bar_gap).max(1.0);
                let mut paint = Paint::default();
                paint.set_color(color);
                paint.set_style(PaintStyle::Fill);
                paint.set_anti_alias(true);

                for (i, &v) in values.iter().enumerate() {
                    let bar_h = (v * h).max(self.min_height).min(h);
                    let x = i as f32 * stride;
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
                    let bar_len = (v * (max_r - inner_r))
                        .max(self.min_height)
                        .min(max_r - inner_r);
                    let x0 = cx + inner_r * angle.cos();
                    let y0 = cy + inner_r * angle.sin();
                    let x1 = cx + (inner_r + bar_len) * angle.cos();
                    let y1 = cy + (inner_r + bar_len) * angle.sin();
                    canvas.draw_line(Point::new(x0, y0), Point::new(x1, y1), &paint);
                }
            }
        }

        canvas.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ink_bounds(surface: &mut skia_safe::Surface, w: i32, h: i32) -> (i32, i32, i32, i32) {
        let snapshot = surface.image_snapshot();
        let info = skia_safe::ImageInfo::new(
            (w, h),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let ok = snapshot.read_pixels(
            &info,
            &mut buf,
            (w * 4) as usize,
            skia_safe::IPoint::new(0, 0),
            skia_safe::image::CachingHint::Disallow,
        );
        assert!(ok, "pixel read should succeed");
        let (mut minx, mut miny, mut maxx, mut maxy) = (w, h, 0, 0);
        for y in 0..h {
            for x in 0..w {
                if buf[((y * w + x) * 4 + 3) as usize] > 0 {
                    minx = minx.min(x);
                    miny = miny.min(y);
                    maxx = maxx.max(x);
                    maxy = maxy.max(y);
                }
            }
        }
        (minx, miny, maxx, maxy)
    }

    #[test]
    fn bars_overflowing_the_box_are_kept_inside_it() {
        const CANVAS_W: i32 = 800;
        const CANVAS_H: i32 = 700;
        const OFFSET_X: f32 = 200.0;
        const OFFSET_Y: f32 = 500.0;
        const BOX_W: f32 = 300.0;
        const BOX_H: f32 = 100.0;

        let spectrum = AudioSpectrum {
            track: None,
            bars: 64,
            mode: SpectrumMode::Bars,
            color: "#FFFFFF".to_string(),
            bar_gap: 8.0,
            min_height: 500.0,
            timing: Default::default(),
            style: CssStyle::default(),
            timeline: Vec::new(),
            stagger: None,
        };

        let mut surface =
            skia_safe::surfaces::raster_n32_premul((CANVAS_W, CANVAS_H)).expect("raster surface");
        {
            let canvas = surface.canvas();
            canvas.translate((OFFSET_X, OFFSET_Y));
            let layout = BoxLayout {
                x: 0.0,
                y: 0.0,
                width: BOX_W,
                height: BOX_H,
                ..Default::default()
            };
            let ctx = PaintCtx {
                time: 0.0,
                scenario_time: 0.0,
                scene_duration: 1.0,
                frame_index: 0,
                fps: 30,
                video_width: CANVAS_W as u32,
                video_height: CANVAS_H as u32,
                stagger_offset: 0.0,
            };
            spectrum.paint_content(canvas, &layout, &AnimatedProperties::default(), &ctx);
        }

        let (minx, miny, maxx, maxy) = ink_bounds(&mut surface, CANVAS_W, CANVAS_H);

        assert!(
            minx as f32 >= OFFSET_X,
            "ink must not start left of the box (x={minx}, box left={OFFSET_X})"
        );
        assert!(
            maxx as f32 <= OFFSET_X + BOX_W,
            "ink must not run right of the box (x={maxx}, box right={})",
            OFFSET_X + BOX_W
        );
        assert!(
            miny as f32 >= OFFSET_Y,
            "min_height must not push bars above the box's top edge (y={miny}, box top={OFFSET_Y})"
        );
        assert!(
            maxy as f32 <= OFFSET_Y + BOX_H,
            "ink must not run below the box (y={maxy}, box bottom={})",
            OFFSET_Y + BOX_H
        );
    }
}
