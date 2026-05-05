use rustmotion_core::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, MaskFilter, PaintStyle, Rect};

use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::paint_from_hex;
use rustmotion_core::schema::LayerStyle;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ParticleType {
    Confetti,
    Snow,
    Stars,
    Bubbles,
    Halo,
}

impl Default for ParticleType {
    fn default() -> Self {
        Self::Confetti
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SizeRange {
    pub min: f32,
    pub max: f32,
}

impl Default for SizeRange {
    fn default() -> Self {
        Self { min: 4.0, max: 12.0 }
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Particle {
    pub particle_type: ParticleType,
    #[serde(default = "default_count")]
    pub count: u32,
    #[serde(default)]
    pub colors: Option<Vec<String>>,
    #[serde(default = "default_speed")]
    pub speed: f32,
    #[serde(default)]
    pub size_range: SizeRange,
    #[serde(default = "default_seed")]
    pub seed: u64,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

fn default_count() -> u32 {
    50
}
fn default_speed() -> f32 {
    1.0
}
fn default_seed() -> u64 {
    42
}

rustmotion_core::impl_traits!(Particle {
    Animatable => style,
    Timed => timing,
    Styled => style,
});

/// Deterministic pseudo-random number generator (splitmix64 for good distribution)
fn prng(seed: u64) -> (f64, u64) {
    let mut s = seed.wrapping_add(0x9E3779B97F4A7C15);
    s = (s ^ (s >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    s = (s ^ (s >> 27)).wrapping_mul(0x94D049BB133111EB);
    s ^= s >> 31;
    let value = (s as f64) / (u64::MAX as f64);
    (value.abs(), s)
}

/// Generate N random values from a seed
fn rand_values(seed: u64, n: usize) -> Vec<f64> {
    let mut values = Vec::with_capacity(n);
    let mut s = seed;
    for _ in 0..n {
        let (v, next) = prng(s);
        values.push(v);
        s = next;
    }
    values
}

impl Particle {
    fn default_colors(&self) -> Vec<String> {
        match self.particle_type {
            ParticleType::Confetti => vec![
                "#EF4444", "#3B82F6", "#22C55E", "#F59E0B", "#8B5CF6", "#EC4899", "#06B6D4",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            ParticleType::Snow => vec!["#FFFFFF", "#E0E7FF", "#DBEAFE"]
                .into_iter()
                .map(String::from)
                .collect(),
            ParticleType::Stars => vec!["#FCD34D", "#FBBF24", "#F59E0B", "#FFFFFF"]
                .into_iter()
                .map(String::from)
                .collect(),
            ParticleType::Bubbles => vec!["#60A5FA40", "#818CF840", "#A78BFA40"]
                .into_iter()
                .map(String::from)
                .collect(),
            ParticleType::Halo => vec!["#6366F140", "#8B5CF640", "#EC489940", "#3B82F640"]
                .into_iter()
                .map(String::from)
                .collect(),
        }
    }
}

impl Painter for Particle {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        let w = layout.width;
        let h = layout.height;
        let t = ctx.time as f32 * self.speed;

        let colors = self.colors.clone().unwrap_or_else(|| self.default_colors());
        if colors.is_empty() {
            return;
        }

        for i in 0..self.count {
            let seed = self.seed.wrapping_add(i as u64 * 7919);
            let rv = rand_values(seed, 6);

            let base_x = rv[0] as f32 * w;
            let base_y = rv[1] as f32 * h;
            let size = self.size_range.min + rv[2] as f32 * (self.size_range.max - self.size_range.min);
            let color_idx = (rv[3] * colors.len() as f64) as usize % colors.len();
            let phase = rv[4] as f32 * std::f32::consts::TAU;
            let speed_var = 0.7 + rv[5] as f32 * 0.6;

            let color = &colors[color_idx];
            let mut paint = paint_from_hex(color);
            paint.set_anti_alias(true);

            match self.particle_type {
                ParticleType::Confetti => {
                    let fall_speed = 80.0 * speed_var;
                    let x = base_x + (t * 1.5 + phase).sin() * 30.0;
                    let y = (base_y + t * fall_speed) % h;
                    let rotation = t * 200.0 * speed_var + phase * 360.0;

                    paint.set_style(PaintStyle::Fill);

                    canvas.save();
                    canvas.translate((x, y));
                    canvas.rotate(rotation, None);
                    let rect = Rect::from_xywh(-size / 2.0, -size / 4.0, size, size / 2.0);
                    canvas.draw_rect(rect, &paint);
                    canvas.restore();
                }
                ParticleType::Snow => {
                    let fall_speed = 40.0 * speed_var;
                    let x = base_x + (t * 0.8 + phase).sin() * 20.0;
                    let y = (base_y + t * fall_speed) % h;

                    let opacity = 0.5 + 0.5 * (t * 2.0 + phase).sin().abs();
                    paint.set_style(PaintStyle::Fill);
                    paint.set_alpha_f(opacity);

                    canvas.draw_circle((x, y), size / 2.0, &paint);
                }
                ParticleType::Stars => {
                    let x = base_x;
                    let y = base_y;
                    let twinkle = 0.3 + 0.7 * ((t * 3.0 * speed_var + phase).sin() * 0.5 + 0.5);

                    paint.set_style(PaintStyle::Fill);
                    paint.set_alpha_f(twinkle);

                    let s = size / 2.0;
                    let mut path = skia_safe::Path::new();
                    path.move_to((x, y - s));
                    path.line_to((x + s * 0.3, y - s * 0.3));
                    path.line_to((x + s, y));
                    path.line_to((x + s * 0.3, y + s * 0.3));
                    path.line_to((x, y + s));
                    path.line_to((x - s * 0.3, y + s * 0.3));
                    path.line_to((x - s, y));
                    path.line_to((x - s * 0.3, y - s * 0.3));
                    path.close();
                    canvas.draw_path(&path, &paint);
                }
                ParticleType::Bubbles => {
                    let rise_speed = 50.0 * speed_var;
                    let x = base_x + (t * 0.5 + phase).sin() * 15.0;
                    let y = h - ((base_y + t * rise_speed) % h);
                    let size_osc = size * (1.0 + 0.15 * (t * 2.0 + phase).sin());

                    paint.set_style(PaintStyle::Fill);
                    canvas.draw_circle((x, y), size_osc / 2.0, &paint);

                    let mut highlight = paint_from_hex("#FFFFFF");
                    highlight.set_style(PaintStyle::Fill);
                    highlight.set_alpha_f(0.3);
                    canvas.draw_circle(
                        (x - size_osc * 0.15, y - size_osc * 0.15),
                        size_osc * 0.15,
                        &highlight,
                    );
                }
                ParticleType::Halo => {
                    let drift_x = (t * 0.3 * speed_var + phase).sin() * w * 0.1;
                    let drift_y = (t * 0.2 * speed_var + phase * 1.3).cos() * h * 0.1;
                    let x = base_x + drift_x;
                    let y = base_y + drift_y;

                    let pulse = 0.4 + 0.6 * ((t * 1.5 * speed_var + phase).sin() * 0.5 + 0.5);
                    let radius = size * (0.8 + 0.4 * ((t * 0.8 + phase).sin() * 0.5 + 0.5));

                    paint.set_style(PaintStyle::Fill);
                    paint.set_alpha_f(pulse * 0.6);

                    let blur_sigma = radius * 0.6;
                    if let Some(mask) = MaskFilter::blur(skia_safe::BlurStyle::Normal, blur_sigma, false) {
                        paint.set_mask_filter(mask);
                    }

                    canvas.draw_circle((x, y), radius, &paint);
                }
            }
        }
    }
}
