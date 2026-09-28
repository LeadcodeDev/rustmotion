use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::{ease, AnimatedProperties};
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::paint_from_hex;
use rustmotion_core::schema::{EasingType, TimelineStep};
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

const MAX_PARTICLES: u64 = 6000;
const SPAWN_SEED_SALT: u64 = 0x9E3779B97F4A7C15;
const FADE_IN_SPAN: f32 = 0.06;
const FADE_OUT_SPAN: f32 = 0.18;
const MIN_VISIBLE_ALPHA: f32 = 0.004;

/// A point in the emitter's own box, in pixels from its top-left corner.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct EmitterOrigin {
    /// Horizontal offset from the box's left edge, in pixels.
    pub x: f32,
    /// Vertical offset from the box's top edge, in pixels.
    pub y: f32,
}

/// A closed range `[min, max]` a per-particle trait is drawn from. Order
/// does not matter on input — the smaller value always acts as the
/// minimum.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct EmitterRange(pub f32, pub f32);

impl EmitterRange {
    fn ordered(self) -> (f32, f32) {
        if self.0 <= self.1 {
            (self.0, self.1)
        } else {
            (self.1, self.0)
        }
    }
}

/// Where newly spawned particles appear and which way they travel.
/// `radial` is the only shape today: particles are born on a ring around
/// `origin` and travel in a straight line outward from it.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EmitterDirection {
    #[default]
    Radial,
}

/// The mark painted for each particle.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EmitterShape {
    /// A short line segment aligned with the direction of travel, from
    /// `length` px behind the particle's position to its position — a
    /// streak of light.
    #[default]
    Streak,
    /// A filled circle at the particle's position.
    Dot,
}

/// How fast a particle travels away from its birth point over its life.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct EmitterSpeed {
    /// Speed in pixels per second at birth (`progress == 0`).
    #[serde(default = "default_speed_from")]
    pub from: f32,
    /// Speed in pixels per second at death (`progress == 1`).
    #[serde(default = "default_speed_to")]
    pub to: f32,
    /// Shapes how the birth-to-death travel distributes over the
    /// particle's life. `linear` spends it evenly; `ease_in` holds most
    /// of it for the end, reading as acceleration.
    #[serde(default)]
    pub easing: EasingType,
}

impl Default for EmitterSpeed {
    fn default() -> Self {
        Self {
            from: default_speed_from(),
            to: default_speed_to(),
            easing: EasingType::default(),
        }
    }
}

/// A radial particle field with a real per-particle lifecycle: born,
/// travelling, dying and immediately respawned, continuously, so the
/// field at any instant is a mix of ages rather than one cohort.
///
/// Every particle's state is derived in closed form from `(seed, index,
/// time)` — nothing is simulated frame-to-frame — so any instant renders
/// independently and byte-identically no matter how it is reached
/// (`render`, `still --time`, or the same frame twice). Supersedes the
/// deprecated `particle`, whose fixed compositions had no lifecycle at
/// all.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Emitter {
    /// Point particles are born around, in the emitter's own box, in
    /// pixels from its top-left corner. Defaults to the box centre.
    #[serde(default)]
    pub origin: Option<EmitterOrigin>,
    /// Average number of particles born per second. Combined with
    /// `life`, this sets how many particles are alive at any instant
    /// (concurrency = rate * average lifetime) — there is no separate
    /// particle count to keep in sync by hand.
    #[serde(default = "default_rate")]
    pub rate: f32,
    /// Lifetime range in seconds, `[min, max]`. Each particle draws its
    /// own lifetime once, deterministically, from `seed` and its index.
    #[serde(default = "default_life")]
    pub life: EmitterRange,
    #[serde(default)]
    pub direction: EmitterDirection,
    #[serde(default)]
    pub speed: EmitterSpeed,
    /// Ring, in pixels from `origin`, particles are born on: `[min,
    /// max]`.
    #[serde(default = "default_spawn_radius")]
    pub spawn_radius: EmitterRange,
    #[serde(default)]
    pub shape: EmitterShape,
    /// Streak length range in pixels, `[min, max]`. Unused when `shape`
    /// is `dot`.
    #[serde(default = "default_length")]
    pub length: EmitterRange,
    /// Particle color as a hex string.
    #[serde(default = "default_color")]
    pub color: String,
    /// Stroke width in pixels for a `streak`, diameter for a `dot`.
    #[serde(default = "default_particle_width")]
    pub width: f32,
    /// Deterministic seed: the same seed and the same instant always
    /// paint the same pixels.
    #[serde(default = "default_seed")]
    pub seed: u64,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
}

fn default_rate() -> f32 {
    80.0
}

fn default_life() -> EmitterRange {
    EmitterRange(0.6, 1.4)
}

fn default_spawn_radius() -> EmitterRange {
    EmitterRange(0.0, 24.0)
}

fn default_length() -> EmitterRange {
    EmitterRange(24.0, 24.0)
}

fn default_color() -> String {
    "#FFFFFF".to_string()
}

fn default_particle_width() -> f32 {
    3.0
}

fn default_seed() -> u64 {
    1
}

fn default_speed_from() -> f32 {
    120.0
}

fn default_speed_to() -> f32 {
    480.0
}

rustmotion_core::impl_traits!(Emitter {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

struct EmitterFrame<'a> {
    emitter: &'a Emitter,
    origin: EmitterOrigin,
    life_min: f32,
    life_max: f32,
    radius_min: f32,
    radius_max: f32,
    length_min: f32,
    length_max: f32,
    travel: f32,
    capacity: u64,
}

struct ParticleState {
    head: (f32, f32),
    tail: (f32, f32),
    alpha: f32,
}

fn splitmix64_next(state: &mut u64) -> f64 {
    *state = state.wrapping_add(SPAWN_SEED_SALT);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^= z >> 31;
    (z as f64) / (u64::MAX as f64)
}

fn particle_rolls(seed: u64, index: u64) -> [f64; 5] {
    let mut state = seed.wrapping_add(index.wrapping_mul(SPAWN_SEED_SALT));
    std::array::from_fn(|_| splitmix64_next(&mut state))
}

fn spawn_fade(progress: f32) -> f32 {
    let fade_in = (progress / FADE_IN_SPAN).clamp(0.0, 1.0);
    let fade_out = ((1.0 - progress) / FADE_OUT_SPAN).clamp(0.0, 1.0);
    fade_in.min(fade_out)
}

impl Emitter {
    fn frame(&self, width: f32, height: f32) -> Option<EmitterFrame<'_>> {
        if width <= 0.0 || height <= 0.0 {
            return None;
        }
        let (life_min, life_max) = self.life.ordered();
        let life_min = life_min.max(0.01);
        let life_max = life_max.max(life_min);
        let life_avg = ((life_min + life_max) * 0.5) as f64;
        if life_avg <= 0.0 {
            return None;
        }

        let capacity = ((self.rate.max(0.0) as f64) * life_avg)
            .round()
            .clamp(0.0, MAX_PARTICLES as f64) as u64;

        let (radius_min, radius_max) = self.spawn_radius.ordered();
        let (length_min, length_max) = self.length.ordered();
        let travel = ((self.speed.from + self.speed.to) * 0.5).max(0.0);
        let origin = self.origin.unwrap_or(EmitterOrigin {
            x: width / 2.0,
            y: height / 2.0,
        });

        Some(EmitterFrame {
            emitter: self,
            origin,
            life_min,
            life_max,
            radius_min,
            radius_max,
            length_min,
            length_max,
            travel,
            capacity,
        })
    }
}

impl EmitterFrame<'_> {
    fn particle_at(&self, index: u64, time: f64) -> Option<ParticleState> {
        let rolls = particle_rolls(self.emitter.seed, index);

        let life = self.life_min + rolls[0] as f32 * (self.life_max - self.life_min);
        let life = life.max(0.01);
        let phase = rolls[1] as f32 * life;
        let age = (time.max(0.0) as f32 + phase).rem_euclid(life);
        let progress = (age / life).clamp(0.0, 1.0);

        let alpha = spawn_fade(progress);
        if alpha <= MIN_VISIBLE_ALPHA {
            return None;
        }

        let angle = rolls[2] as f32 * std::f32::consts::TAU;
        let birth_radius = self.radius_min + rolls[3] as f32 * (self.radius_max - self.radius_min);
        let streak_length = self.length_min + rolls[4] as f32 * (self.length_max - self.length_min);

        let eased = ease(progress as f64, &self.emitter.speed.easing) as f32;
        let distance = birth_radius + eased * self.travel * life;

        let (dx, dy) = (angle.cos(), angle.sin());
        let head = (self.origin.x + dx * distance, self.origin.y + dy * distance);
        let tail_distance = (distance - streak_length).max(0.0);
        let tail = (
            self.origin.x + dx * tail_distance,
            self.origin.y + dy * tail_distance,
        );

        Some(ParticleState { head, tail, alpha })
    }
}

impl Painter for Emitter {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        let Some(frame) = self.frame(layout.width, layout.height) else {
            return;
        };

        let mut paint = paint_from_hex(&self.color);
        paint.set_anti_alias(true);
        paint.set_stroke_cap(skia_safe::PaintCap::Round);
        paint.set_stroke_width(self.width.max(0.5));

        for index in 0..frame.capacity {
            let Some(particle) = frame.particle_at(index, ctx.time) else {
                continue;
            };
            paint.set_alpha_f(particle.alpha);
            match self.shape {
                EmitterShape::Dot => {
                    paint.set_style(PaintStyle::Fill);
                    canvas.draw_circle(particle.head, self.width.max(0.5), &paint);
                }
                EmitterShape::Streak => {
                    paint.set_style(PaintStyle::Stroke);
                    canvas.draw_line(particle.tail, particle.head, &paint);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use skia_safe::{surfaces, AlphaType, ColorType, ImageInfo};

    const W: i32 = 400;
    const H: i32 = 400;

    fn ctx_at(time: f64) -> PaintCtx {
        PaintCtx {
            time,
            scenario_time: time,
            scene_duration: 3.0,
            frame_index: 0,
            fps: 30,
            video_width: W as u32,
            video_height: H as u32,
            stagger_offset: 0.0,
        }
    }

    fn layout() -> BoxLayout {
        BoxLayout {
            width: W as f32,
            height: H as f32,
            ..Default::default()
        }
    }

    fn tunnel() -> Emitter {
        serde_json::from_value(serde_json::json!({
            "origin": { "x": 200.0, "y": 200.0 },
            "rate": 180.0,
            "life": [0.7, 1.2],
            "direction": "radial",
            "speed": { "from": 200.0, "to": 1600.0, "easing": "ease_in" },
            "spawn_radius": [10.0, 40.0],
            "shape": "streak",
            "length": [30.0, 90.0],
            "color": "#EEF4FF",
            "width": 3.0,
            "seed": 7
        }))
        .expect("valid emitter json")
    }

    fn render_at(emitter: &Emitter, time: f64) -> Vec<u8> {
        let info = ImageInfo::new((W, H), ColorType::RGBA8888, AlphaType::Unpremul, None);
        let mut surface = surfaces::raster(&info, None, None).expect("raster surface");
        surface.canvas().clear(skia_safe::Color::TRANSPARENT);
        emitter.paint_content(
            surface.canvas(),
            &layout(),
            &AnimatedProperties::default(),
            &ctx_at(time),
        );
        let row_bytes = W as usize * 4;
        let mut pixels = vec![0u8; row_bytes * H as usize];
        surface.read_pixels(&info, &mut pixels, row_bytes, (0, 0));
        pixels
    }

    fn painted_pixel_count(buf: &[u8]) -> usize {
        buf.as_chunks::<4>().0.iter().filter(|px| px[3] > 0).count()
    }

    #[test]
    fn a_live_emitter_paints_ink_an_empty_one_does_not() {
        let live = tunnel();
        let mut empty = tunnel();
        empty.rate = 0.0;

        let live_pixels = render_at(&live, 1.0);
        let empty_pixels = render_at(&empty, 1.0);

        assert!(
            painted_pixel_count(&live_pixels) > 0,
            "a live emitter must paint some ink"
        );
        assert_eq!(
            painted_pixel_count(&empty_pixels),
            0,
            "rate: 0 must leave the field completely empty, not just dim"
        );
        assert_eq!(
            empty_pixels,
            vec![0u8; empty_pixels.len()],
            "an empty emitter is byte-identical to an untouched transparent surface"
        );
    }

    #[test]
    fn the_same_instant_rendered_twice_is_byte_identical() {
        let emitter = tunnel();
        let first = render_at(&emitter, 1.234);
        let second = render_at(&emitter, 1.234);
        assert_eq!(
            first, second,
            "same file, same instant, must produce the same bytes every time"
        );
    }

    #[test]
    fn a_particle_travels_between_two_instants() {
        let emitter = tunnel();
        let frame = emitter.frame(W as f32, H as f32).expect("frame builds");

        let index = (0..frame.capacity)
            .find(|&i| frame.particle_at(i, 0.1).is_some() && frame.particle_at(i, 0.3).is_some())
            .expect("at least one particle is alive at both instants");

        let early = frame.particle_at(index, 0.1).unwrap();
        let late = frame.particle_at(index, 0.3).unwrap();

        let dist =
            |p: (f32, f32), o: (f32, f32)| ((p.0 - o.0).powi(2) + (p.1 - o.1).powi(2)).sqrt();
        let early_distance = dist(early.head, (frame.origin.x, frame.origin.y));
        let late_distance = dist(late.head, (frame.origin.x, frame.origin.y));

        assert!(
            (early_distance - late_distance).abs() > 1.0,
            "a travelling particle must be at a different distance from the \
             origin at two different instants (early={early_distance}, \
             late={late_distance})"
        );
    }

    #[test]
    fn particle_rolls_are_pure_functions_of_seed_and_index() {
        assert_eq!(particle_rolls(7, 42), particle_rolls(7, 42));
        assert_ne!(particle_rolls(7, 42), particle_rolls(7, 43));
        assert_ne!(particle_rolls(7, 42), particle_rolls(8, 42));
    }

    #[test]
    fn capacity_follows_rate_times_average_life() {
        let emitter = tunnel();
        let frame = emitter.frame(W as f32, H as f32).unwrap();
        assert_eq!(frame.capacity, (180.0 * 0.95_f64).round() as u64);
    }

    #[test]
    fn rate_and_life_deserialize_from_bare_json_arrays() {
        let emitter: Emitter = serde_json::from_value(serde_json::json!({
            "life": [0.5, 1.5],
            "spawn_radius": [1.0, 2.0],
            "length": [3.0, 4.0]
        }))
        .expect("arrays deserialize into EmitterRange");
        assert_eq!(emitter.life, EmitterRange(0.5, 1.5));
        assert_eq!(emitter.spawn_radius, EmitterRange(1.0, 2.0));
        assert_eq!(emitter.length, EmitterRange(3.0, 4.0));
    }
}
