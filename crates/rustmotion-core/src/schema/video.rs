use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::animation::{Animation, AnimationPreset, EasingType, PresetConfig, SpringConfig};
use super::style::{FontWeight, TextAlign, VerticalAlign};

// --- Animation effects (nested inside CssStyle as typed array) ---

/// A single animation effect. Discriminated by `"type"` in JSON.
/// Each preset name is a valid type, plus special types: glow, wiggle, keyframes, motion_blur.
///
/// Examples:
/// ```json
/// { "name": "fade_in_up", "delay": 0.3, "duration": 0.6 }
/// { "name": "glow", "color": "#F68F2B", "radius": 16, "intensity": 1.2 }
/// { "name": "wiggle", "property": "translate_y", "amplitude": 5, "frequency": 2 }
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "name", rename_all = "snake_case")]
pub enum AnimationEffect {
    // --- Entrance presets ---
    FadeIn(AnimationTiming),
    FadeInUp(AnimationTiming),
    FadeInDown(AnimationTiming),
    FadeInLeft(AnimationTiming),
    FadeInRight(AnimationTiming),
    SlideInLeft(AnimationTiming),
    SlideInRight(AnimationTiming),
    SlideInUp(AnimationTiming),
    SlideInDown(AnimationTiming),
    ScaleIn(AnimationTiming),
    BounceIn(AnimationTiming),
    BlurIn(AnimationTiming),
    RotateIn(AnimationTiming),
    ElasticIn(AnimationTiming),
    // --- Exit presets ---
    FadeOut(AnimationTiming),
    FadeOutUp(AnimationTiming),
    FadeOutDown(AnimationTiming),
    SlideOutLeft(AnimationTiming),
    SlideOutRight(AnimationTiming),
    SlideOutUp(AnimationTiming),
    SlideOutDown(AnimationTiming),
    ScaleOut(AnimationTiming),
    BounceOut(AnimationTiming),
    BlurOut(AnimationTiming),
    RotateOut(AnimationTiming),
    // --- Continuous presets ---
    Pulse(AnimationTiming),
    Float(AnimationTiming),
    Shake(AnimationTiming),
    Spin(AnimationTiming),
    // --- 3D presets ---
    FlipInX(AnimationTiming),
    FlipInY(AnimationTiming),
    FlipOutX(AnimationTiming),
    FlipOutY(AnimationTiming),
    TiltIn(TiltInConfig),
    // --- Stroke presets ---
    DrawIn(AnimationTiming),
    StrokeReveal(AnimationTiming),
    // --- Special presets ---
    Typewriter(AnimationTiming),
    WipeLeft(AnimationTiming),
    WipeRight(AnimationTiming),
    // --- Floating/orbit presets ---
    #[serde(alias = "float_3d")]
    Float3d(AnimationTiming),
    // --- Char animation presets ---
    CharScaleIn(CharAnimationTiming),
    CharFadeIn(CharAnimationTiming),
    CharWave(CharAnimationTiming),
    CharBounce(CharAnimationTiming),
    CharRotateIn(CharAnimationTiming),
    CharSlideUp(CharAnimationTiming),
    /// Each word (or char) arrives blurred and settles sharp, combined with
    /// a slight upward translate and an opacity ramp — one continuous
    /// per-unit animation driven by the same progress value, not three
    /// independently-timed effects. `CharAnimationTiming.blur` sets the
    /// starting blur sigma in px (default tuned for 100px+ display type;
    /// see `crates/rustmotion-components/src/text.rs`).
    ///
    /// Resolved directly off `style.animation` inside
    /// `rustmotion_components::text::Text::paint` rather than through
    /// `engine::animator::extract_effects`/`ResolvedCharAnimation` like its
    /// five siblings — that resolution path lives in a file outside this
    /// workstream's scope (chantier #117 W1). Functionally equivalent for
    /// the documented use (a top-level `style.animation` entry); it does
    /// not participate in container-level stagger delay shifting the way
    /// the other char presets do when nested in a staggered list/grid.
    CharBlurIn(CharAnimationTiming),
    // --- Non-preset effects ---
    Glow(GlowConfig),
    Wiggle(WiggleConfig),
    Orbit(OrbitConfig),
    Keyframes(KeyframesConfig),
    MotionBlur(MotionBlurConfig),
    /// Temporal trail effect: paints copies of the component at prior times
    /// with decaying opacity, creating a persistence-of-vision ghost trail.
    Trail(TrailConfig),
}

impl AnimationEffect {
    /// Shift the effect's start delay by `by` seconds. Used by `timeline`
    /// steps, whose animations run relative to the step's `at`. Continuous
    /// effects without a delay concept (glow, wiggle, orbit, motion blur)
    /// are unaffected.
    pub fn shift_delay(&mut self, by: f64) {
        use AnimationEffect::*;
        match self {
            FadeIn(t) | FadeInUp(t) | FadeInDown(t) | FadeInLeft(t) | FadeInRight(t)
            | SlideInLeft(t) | SlideInRight(t) | SlideInUp(t) | SlideInDown(t) | ScaleIn(t)
            | BounceIn(t) | BlurIn(t) | RotateIn(t) | ElasticIn(t) | FadeOut(t) | FadeOutUp(t)
            | FadeOutDown(t) | SlideOutLeft(t) | SlideOutRight(t) | SlideOutUp(t)
            | SlideOutDown(t) | ScaleOut(t) | BounceOut(t) | BlurOut(t) | RotateOut(t)
            | Pulse(t) | Float(t) | Shake(t) | Spin(t) | FlipInX(t) | FlipInY(t) | FlipOutX(t)
            | FlipOutY(t) | DrawIn(t) | StrokeReveal(t) | Typewriter(t) | WipeLeft(t)
            | WipeRight(t) | Float3d(t) => t.delay += by,
            TiltIn(c) => c.delay += by,
            CharScaleIn(c) | CharFadeIn(c) | CharWave(c) | CharBounce(c) | CharRotateIn(c)
            | CharSlideUp(c) | CharBlurIn(c) => c.delay += by,
            Keyframes(c) => c.delay += by,
            Glow(_) | Wiggle(_) | Orbit(_) | MotionBlur(_) | Trail(_) => {}
        }
    }

    /// If this is a preset variant, return the corresponding AnimationPreset and timing.
    pub fn as_preset(&self) -> Option<(AnimationPreset, &AnimationTiming)> {
        match self {
            Self::FadeIn(t) => Some((AnimationPreset::FadeIn, t)),
            Self::FadeInUp(t) => Some((AnimationPreset::FadeInUp, t)),
            Self::FadeInDown(t) => Some((AnimationPreset::FadeInDown, t)),
            Self::FadeInLeft(t) => Some((AnimationPreset::FadeInLeft, t)),
            Self::FadeInRight(t) => Some((AnimationPreset::FadeInRight, t)),
            Self::SlideInLeft(t) => Some((AnimationPreset::SlideInLeft, t)),
            Self::SlideInRight(t) => Some((AnimationPreset::SlideInRight, t)),
            Self::SlideInUp(t) => Some((AnimationPreset::SlideInUp, t)),
            Self::SlideInDown(t) => Some((AnimationPreset::SlideInDown, t)),
            Self::ScaleIn(t) => Some((AnimationPreset::ScaleIn, t)),
            Self::BounceIn(t) => Some((AnimationPreset::BounceIn, t)),
            Self::BlurIn(t) => Some((AnimationPreset::BlurIn, t)),
            Self::RotateIn(t) => Some((AnimationPreset::RotateIn, t)),
            Self::ElasticIn(t) => Some((AnimationPreset::ElasticIn, t)),
            Self::FadeOut(t) => Some((AnimationPreset::FadeOut, t)),
            Self::FadeOutUp(t) => Some((AnimationPreset::FadeOutUp, t)),
            Self::FadeOutDown(t) => Some((AnimationPreset::FadeOutDown, t)),
            Self::SlideOutLeft(t) => Some((AnimationPreset::SlideOutLeft, t)),
            Self::SlideOutRight(t) => Some((AnimationPreset::SlideOutRight, t)),
            Self::SlideOutUp(t) => Some((AnimationPreset::SlideOutUp, t)),
            Self::SlideOutDown(t) => Some((AnimationPreset::SlideOutDown, t)),
            Self::ScaleOut(t) => Some((AnimationPreset::ScaleOut, t)),
            Self::BounceOut(t) => Some((AnimationPreset::BounceOut, t)),
            Self::BlurOut(t) => Some((AnimationPreset::BlurOut, t)),
            Self::RotateOut(t) => Some((AnimationPreset::RotateOut, t)),
            Self::Pulse(t) => Some((AnimationPreset::Pulse, t)),
            Self::Float(t) => Some((AnimationPreset::Float, t)),
            Self::Shake(t) => Some((AnimationPreset::Shake, t)),
            Self::Spin(t) => Some((AnimationPreset::Spin, t)),
            Self::FlipInX(t) => Some((AnimationPreset::FlipInX, t)),
            Self::FlipInY(t) => Some((AnimationPreset::FlipInY, t)),
            Self::FlipOutX(t) => Some((AnimationPreset::FlipOutX, t)),
            Self::FlipOutY(t) => Some((AnimationPreset::FlipOutY, t)),
            Self::TiltIn(_) => None,
            Self::DrawIn(t) => Some((AnimationPreset::DrawIn, t)),
            Self::StrokeReveal(t) => Some((AnimationPreset::StrokeReveal, t)),
            Self::Float3d(t) => Some((AnimationPreset::Float3d, t)),
            Self::Typewriter(t) => Some((AnimationPreset::Typewriter, t)),
            Self::WipeLeft(t) => Some((AnimationPreset::WipeLeft, t)),
            Self::WipeRight(t) => Some((AnimationPreset::WipeRight, t)),
            _ => None,
        }
    }
}

/// Timing configuration for preset animations.
// `deny_unknown_fields` (constat #8): this is the `AnimationTiming` payload
// of an internally-tagged `AnimationEffect` variant (`#[serde(tag = "name")]`
// on the enum). Serde's tagged-enum deserializer buffers the object and
// re-drives it through the variant's own `Deserialize` impl *without* the
// `name` tag key, so `deny_unknown_fields` here rejects a typo'd field (e.g.
// `duratoin`) without ever seeing/rejecting `name` itself — verified with a
// minimal repro before relying on it. Without this, `validate_attrs.rs`
// never sees inside `style.animation[*]` (it only walks component-level
// keys), so a typo silently no-ops instead of erroring.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct AnimationTiming {
    /// Delay before animation starts (seconds).
    #[serde(default)]
    pub delay: f64,
    /// Animation duration (seconds).
    #[serde(default = "default_animation_duration")]
    pub duration: f64,
    /// Loop the animation continuously.
    #[serde(default, rename = "loop")]
    pub repeat: bool,
    /// Overshoot/anticipation intensity for scale_in/scale_out (0.0 = none, default 0.08 = 8%).
    #[serde(default)]
    pub overshoot: Option<f64>,
    /// Spring physics for the preset's motion keyframes (translate/scale/
    /// rotate — opacity keeps its ease to avoid alpha overshoot flashes).
    /// `bounce_in` / `elastic_in` use their built-in springs as defaults;
    /// this overrides them.
    #[serde(default)]
    pub spring: Option<SpringConfig>,
    /// Travel of an oscillating preset, in pixels (`float_3d` only; default
    /// 12). Threaded through `to_preset_config` into `PresetConfig::amplitude`,
    /// which `expand_preset_inner` already reads — this field is what makes
    /// an author-supplied amplitude actually reach it instead of the
    /// hardcoded default on every element.
    #[serde(default)]
    pub amplitude: Option<f64>,
}

fn default_animation_duration() -> f64 {
    0.8
}

/// Configuration for the `tilt_in` animation with configurable 3D transform values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TiltInConfig {
    #[serde(default)]
    pub delay: f64,
    #[serde(default = "default_animation_duration")]
    pub duration: f64,
    #[serde(default, rename = "loop")]
    pub repeat: bool,
    /// Initial rotate_x angle in degrees (default: 15.0).
    #[serde(default)]
    pub rotate_x: Option<f64>,
    /// Initial rotate_y angle in degrees (default: -15.0).
    #[serde(default)]
    pub rotate_y: Option<f64>,
    /// Perspective depth in px (default: 1000.0).
    #[serde(default)]
    pub perspective: Option<f64>,
    /// Initial scale value (default: 0.9).
    #[serde(default)]
    pub scale_from: Option<f64>,
}

impl Default for AnimationTiming {
    fn default() -> Self {
        Self {
            delay: 0.0,
            duration: 0.8,
            repeat: false,
            overshoot: None,
            spring: None,
            amplitude: None,
        }
    }
}

/// Timing configuration for char animation effect variants (used inside style.animation).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CharAnimationTiming {
    /// Delay before animation starts (seconds).
    #[serde(default)]
    pub delay: f64,
    /// Duration of each unit's animation in seconds.
    #[serde(default = "default_char_duration_f64")]
    pub duration: f64,
    /// Delay between each unit (char or word) in seconds.
    #[serde(default = "default_char_stagger_f64")]
    pub stagger: f64,
    /// Granularity: animate per character or per word.
    #[serde(default)]
    pub granularity: TextAnimGranularity,
    /// Easing function for each unit's animation.
    #[serde(default)]
    pub easing: EasingType,
    /// Overshoot intensity for char_scale_in/char_bounce (0.0 = none, default 0.08 = 8%).
    #[serde(default)]
    pub overshoot: Option<f64>,
    /// Starting blur sigma in px for char_blur_in — the word (or char)
    /// begins blurred by this amount and settles to sharp (sigma 0) by the
    /// end of its unit animation. Unused by the other five char presets.
    /// Defaults to 14px sigma, chosen to read clearly at 100px+ display
    /// type without collapsing into a featureless blob (see render proof
    /// in issue #118).
    #[serde(default)]
    pub blur: Option<f64>,
}

fn default_char_stagger_f64() -> f64 {
    0.03
}

fn default_char_duration_f64() -> f64 {
    0.4
}

/// Per-character or per-word text animation configuration (legacy root-level prop).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
pub struct CharAnimation {
    /// Animation preset: "scale_in", "fade_in", "wave", "bounce", "rotate_in", "slide_up", "blur_in".
    #[serde(default = "default_char_preset")]
    pub preset: CharAnimPreset,
    /// Granularity: animate per character or per word.
    #[serde(default)]
    pub granularity: TextAnimGranularity,
    /// Delay between each unit (char or word) in seconds.
    #[serde(default = "default_char_stagger")]
    pub stagger: f32,
    /// Duration of each unit's animation in seconds.
    #[serde(default = "default_char_duration")]
    pub duration: f32,
    /// Easing function.
    #[serde(default)]
    pub easing: EasingType,
    /// Initial delay before the first unit starts.
    #[serde(default)]
    pub delay: f32,
}

/// Granularity for text animation: per character or per word.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
#[derive(PartialEq)]
pub enum TextAnimGranularity {
    /// Animate each character individually (default).
    #[default]
    Char,
    /// Animate each word as a unit.
    Word,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
#[derive(PartialEq)]
pub enum CharAnimPreset {
    /// Each character scales from 0 to 1.
    #[default]
    ScaleIn,
    /// Each character fades from 0 to 1 opacity.
    FadeIn,
    /// Characters oscillate vertically in a wave pattern.
    Wave,
    /// Each character bounces in (scale overshoot).
    Bounce,
    /// Each character rotates in from a random angle.
    RotateIn,
    /// Each character slides up from below.
    SlideUp,
    /// Each character/word arrives blurred and settles sharp, with a
    /// slight upward translate and opacity ramp driven by the same
    /// progress value (see `AnimationEffect::CharBlurIn`).
    BlurIn,
}

fn default_char_preset() -> CharAnimPreset {
    CharAnimPreset::ScaleIn
}

fn default_char_stagger() -> f32 {
    0.03
}

fn default_char_duration() -> f32 {
    0.4
}

impl AnimationTiming {
    /// Convert to PresetConfig for compatibility with resolve_animations.
    pub fn to_preset_config(&self) -> PresetConfig {
        PresetConfig {
            amplitude: self.amplitude,
            delay: self.delay,
            duration: self.duration,
            repeat: self.repeat,
            overshoot: self.overshoot,
            spring: self.spring.clone(),
        }
    }
}

/// Constat #4: every `property` name `engine::animator::{apply_property,
/// get_property_value}` (read-only for this workstream — the solver logic
/// itself stays there) actually recognises for `wiggle`/`keyframes`
/// animations. Anything outside this set has always been a silent no-op in
/// the solver (`_ => {}` / `_ => 0.0`): the animation plays as if the
/// property doesn't exist, with no error and no visual signal that
/// something is wrong. `WiggleConfig.property` and `Animation.property`
/// (the latter via `KeyframesConfig.keyframes`'s `deserialize_with`, since
/// `Animation` itself lives in `schema/animation.rs`, which this workstream
/// may only touch for `deny_unknown_fields`) are validated against this set
/// at parse time instead — turning the silent no-op into a named error, so
/// a mixed-convention typo (`"translateX"`, `"positionX"`, `"Rotation"`) or
/// a wholesale unsupported name is caught immediately.
///
/// `"color"` is included because `resolve_animations` special-cases
/// `anim.property == "color"` outside `apply_property`/`get_property_value`
/// — it is a real, solver-recognised value for `Animation`, just resolved on
/// a different path than the numeric properties.
const KNOWN_MOTION_PROPERTIES: &[&str] = &[
    "opacity",
    "position.x",
    "translate_x",
    "position.y",
    "translate_y",
    "scale",
    "scale.x",
    "scale.y",
    "rotation",
    "rotate_x",
    "rotate_y",
    "blur",
    "visible_chars",
    "visible_chars_progress",
    "border_radius",
    "font_size",
    "width",
    "height",
    "gap",
    "padding",
    "stroke_width",
    "shadow_blur",
    "glow_radius",
    "glow_intensity",
    "perspective",
    "draw_progress",
    "motion_progress",
    "color",
];

/// Reject a `property` value the solver doesn't recognise, with a
/// "did-you-mean" nudge when the only mismatch is casing/separator
/// convention (`translateX` / `translate-x` vs `translate_x`) — the exact
/// trap constat #4 names: this project mixes kebab-case (CSS-style, most of
/// `CssStyle`) and snake_case (these property names) conventions, and an
/// author reasoning from the former naturally reaches for the latter's
/// kebab or camelCase spelling.
fn validate_motion_property<E: serde::de::Error>(value: &str) -> Result<(), E> {
    if KNOWN_MOTION_PROPERTIES.contains(&value) {
        return Ok(());
    }
    let normalize = |s: &str| s.replace(['-', ' '], "_").to_lowercase();
    let normalized = normalize(value);
    if let Some(suggestion) = KNOWN_MOTION_PROPERTIES
        .iter()
        .find(|known| normalize(known) == normalized)
    {
        Err(E::custom(format!(
            "unknown animation property '{value}' — did you mean '{suggestion}'?"
        )))
    } else {
        Err(E::custom(format!(
            "unknown animation property '{value}': expected one of {}",
            KNOWN_MOTION_PROPERTIES.join(", ")
        )))
    }
}

fn deserialize_motion_property<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    validate_motion_property::<D::Error>(&s)?;
    Ok(s)
}

/// Validates every keyframe's `property` the same way
/// [`deserialize_motion_property`] does for `WiggleConfig` — `Animation`
/// itself lives in `schema/animation.rs`, out of reach for anything beyond
/// `deny_unknown_fields` in this workstream, so the check is applied here,
/// at the one field that actually consumes `Vec<Animation>` in this file.
fn deserialize_validated_keyframes<'de, D>(deserializer: D) -> Result<Vec<Animation>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let animations = Vec::<Animation>::deserialize(deserializer)?;
    for anim in &animations {
        validate_motion_property::<D::Error>(&anim.property)?;
    }
    Ok(animations)
}

/// Custom keyframe animations configuration.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct KeyframesConfig {
    #[serde(deserialize_with = "deserialize_validated_keyframes")]
    pub keyframes: Vec<Animation>,
    #[serde(default)]
    pub delay: f64,
    #[serde(default = "default_animation_duration")]
    pub duration: f64,
    #[serde(default, rename = "loop")]
    pub repeat: bool,
}

/// Motion blur configuration.
///
/// Ghost nodes are sampled at times `t - i * (shutter / fps) / samples` for
/// `i` in `1..=samples`. Each ghost and the principal node are painted with
/// opacity `1.0 / (samples + 1)` so their premultiplied-alpha sum approximates
/// the shutter-averaged exposure.
///
/// `samples = 1` is the degenerate case: the single ghost falls at `t - 0` and
/// superimposes exactly on the principal → visually equivalent to no blur.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MotionBlurConfig {
    /// Reserved for future intensity scaling (currently unused by the ghost
    /// sampler — the `samples` parameter controls quality). Kept for schema
    /// compatibility with existing JSON that may carry this field.
    #[serde(default)]
    pub intensity: f32,
    /// Number of ghost samples in the shutter window (default 6, clamped 1..=16).
    /// Use 1 to effectively disable (degenerate: ghost = principal position).
    #[serde(default = "default_motion_blur_samples")]
    pub samples: u32,
    /// Fraction of one frame duration used as the shutter window (default 0.5).
    /// The temporal spread equals `shutter / fps` seconds.
    #[serde(default = "default_motion_blur_shutter")]
    pub shutter: f64,
}

fn default_motion_blur_samples() -> u32 {
    6
}
fn default_motion_blur_shutter() -> f64 {
    0.5
}

/// Trail effect configuration.
///
/// Produces `copies` ghost nodes behind the principal, each offset in time by
/// `i * spacing` seconds. The i-th ghost (1-based) is painted with opacity
/// `base_opacity * falloff^i`; the principal is unchanged. Unlike motion blur,
/// the trail is additive: the principal retains its full opacity.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TrailConfig {
    /// Number of trailing ghost copies (default 4, clamped 1..=12).
    #[serde(default = "default_trail_copies")]
    pub copies: u32,
    /// Time gap between successive ghost copies in seconds (default 0.05).
    #[serde(default = "default_trail_spacing")]
    pub spacing: f64,
    /// Opacity multiplier applied per copy: ghost `i` uses `falloff^i` times
    /// the component's base opacity (default 0.6).
    #[serde(default = "default_trail_falloff")]
    pub falloff: f32,
}

fn default_trail_copies() -> u32 {
    4
}
fn default_trail_spacing() -> f64 {
    0.05
}
fn default_trail_falloff() -> f32 {
    0.6
}

// --- Orbit Config ---

/// Configuration for a 3D orbit/floating animation effect.
/// Creates circular or elliptical motion with pseudo-depth (scale + opacity modulation).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct OrbitConfig {
    /// Horizontal radius of the orbit in pixels.
    #[serde(default = "default_orbit_radius")]
    pub radius_x: f64,
    /// Vertical radius of the orbit in pixels.
    #[serde(default = "default_orbit_radius")]
    pub radius_y: f64,
    /// Orbit speed in revolutions per second.
    #[serde(default = "default_orbit_speed")]
    pub speed: f64,
    /// Starting angle in degrees (0 = right, 90 = bottom).
    #[serde(default)]
    pub start_angle: f64,
    /// Scale modulation depth (0.0 = none, 0.2 = 20% size variation for depth effect).
    #[serde(default = "default_orbit_depth")]
    pub depth: f64,
    /// Opacity modulation depth (0.0 = none, 0.3 = 30% opacity variation for depth).
    #[serde(default)]
    pub opacity_depth: f64,
    /// Tilt angle in degrees — tilts the orbit plane for a 3D perspective look.
    #[serde(default)]
    pub tilt: f64,
    /// Phase offset (0.0 to 1.0) — offsets the starting position along the orbit.
    #[serde(default)]
    pub phase: f64,
}

fn default_orbit_radius() -> f64 {
    30.0
}
fn default_orbit_speed() -> f64 {
    0.5
}
fn default_orbit_depth() -> f64 {
    0.15
}

// --- Wiggle Config ---

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WiggleConfig {
    #[serde(deserialize_with = "deserialize_motion_property")]
    pub property: String,
    pub amplitude: f64,
    pub frequency: f64,
    #[serde(default)]
    pub seed: u64,
    /// Number of noise octaves (higher = more detail, default 3)
    #[serde(default)]
    pub octaves: Option<u32>,
    /// Phase offset in seconds
    #[serde(default)]
    pub phase: Option<f64>,
    /// Exponential decay rate (amplitude diminishes over time)
    #[serde(default)]
    pub decay: Option<f64>,
    /// Easing function to reshape the noise curve
    #[serde(default)]
    pub easing: Option<EasingType>,
    /// Oscillation mode: "noise" (default, layered simplex) or "sine" (pure sine wave)
    #[serde(default)]
    pub mode: Option<String>,
}

// --- Supporting types ---

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ShapeType {
    Rect,
    Circle,
    RoundedRect,
    Ellipse,
    Triangle,
    Star {
        #[serde(default = "default_star_points")]
        points: u32,
    },
    Polygon {
        #[serde(default = "default_polygon_sides")]
        sides: u32,
    },
    Path {
        data: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Fill {
    Solid(String),
    Gradient(Gradient),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Gradient {
    #[serde(rename = "type")]
    pub gradient_type: GradientType,
    pub colors: Vec<String>,
    #[serde(default)]
    pub stops: Option<Vec<f32>>,
    #[serde(default)]
    pub angle: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GradientType {
    Linear,
    Radial,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Stroke {
    pub color: String,
    #[serde(default = "default_stroke_width")]
    pub width: f32,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum ImageFit {
    Cover,
    #[default]
    Contain,
    Fill,
}

// --- Shape Text ---

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ShapeText {
    pub content: String,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default = "default_font_family")]
    pub font_family: String,
    #[serde(default)]
    pub font_weight: FontWeight,
    #[serde(default)]
    pub align: TextAlign,
    #[serde(default)]
    pub vertical_align: VerticalAlign,
    #[serde(default)]
    pub line_height: Option<f32>,
    #[serde(default)]
    pub letter_spacing: Option<f32>,
    #[serde(default)]
    pub padding: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CaptionWord {
    pub text: String,
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum CaptionStyle {
    #[default]
    Highlight,
    Karaoke,
    WordByWord,
    /// TikTok-style: only the active word is shown, centered, with a
    /// spring-like scale-in and a rounded pill background.
    WordPop,
    /// Karaoke line (all words visible, wrapped) where the active word
    /// scales up, takes `active_color` and gets a pill background.
    KaraokePop,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GradientBorder {
    pub colors: Vec<String>,
    #[serde(default = "default_gradient_border_width")]
    pub width: f32,
    #[serde(default)]
    pub angle: f32,
}

fn default_gradient_border_width() -> f32 {
    2.0
}

/// Inner shadow configuration (inset shadow).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct InnerShadow {
    pub color: String,
    #[serde(default)]
    pub offset_x: f32,
    #[serde(default)]
    pub offset_y: f32,
    #[serde(default = "default_inner_shadow_blur")]
    pub blur: f32,
}

fn default_inner_shadow_blur() -> f32 {
    10.0
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TextShadow {
    #[serde(default = "default_shadow_color")]
    pub color: String,
    #[serde(default = "default_shadow_offset")]
    pub offset_x: f32,
    #[serde(default = "default_shadow_offset")]
    pub offset_y: f32,
    #[serde(default = "default_shadow_blur")]
    pub blur: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TextBackground {
    pub color: String,
    #[serde(default = "default_text_bg_padding")]
    pub padding: f32,
    #[serde(default)]
    pub corner_radius: f32,
}

// --- Visual Effect Types ---

/// Glow effect (colored luminous halo around the element)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GlowConfig {
    /// Glow color (hex string, e.g. "#5C39EE")
    #[serde(default = "default_glow_color")]
    pub color: String,
    /// Blur radius of the glow
    #[serde(default = "default_glow_radius")]
    pub radius: f32,
    /// Intensity multiplier (higher = brighter glow, default 1.0)
    #[serde(default = "default_glow_intensity")]
    pub intensity: f32,
}

fn default_glow_color() -> String {
    "#FFFFFF80".to_string()
}

fn default_glow_radius() -> f32 {
    10.0
}

fn default_glow_intensity() -> f32 {
    1.0
}

// --- Default functions ---

fn default_font_size() -> f32 {
    48.0
}

fn default_color() -> String {
    "#FFFFFF".to_string()
}

fn default_font_family() -> String {
    "Inter".to_string()
}

fn default_stroke_width() -> f32 {
    2.0
}

fn default_star_points() -> u32 {
    5
}

fn default_polygon_sides() -> u32 {
    6
}

fn default_shadow_color() -> String {
    "#00000080".to_string()
}

fn default_shadow_offset() -> f32 {
    2.0
}

fn default_shadow_blur() -> f32 {
    4.0
}

fn default_text_bg_padding() -> f32 {
    8.0
}

#[cfg(test)]
mod motion_property_tests {
    use super::*;
    use serde_json::json;

    // ---- constat #4: `WiggleConfig.property` / `Animation.property` (via
    // `KeyframesConfig.keyframes`) are free strings the solver silently
    // no-ops on when unrecognised (RED first). ----

    #[test]
    fn wiggle_known_property_still_works() {
        let json = json!({
            "name": "wiggle",
            "property": "translate_x",
            "amplitude": 10.0,
            "frequency": 1.0
        });
        let effect: AnimationEffect = serde_json::from_value(json).unwrap();
        match effect {
            AnimationEffect::Wiggle(cfg) => assert_eq!(cfg.property, "translate_x"),
            other => panic!("expected Wiggle, got {other:?}"),
        }
    }

    #[test]
    fn wiggle_unknown_property_is_a_named_error_not_a_silent_no_op() {
        // A wholly unsupported name — the animation would otherwise play,
        // resolve every frame, and simply never touch any rendered
        // property: no error, no visible effect, no signal at all.
        let json = json!({
            "name": "wiggle",
            "property": "skew",
            "amplitude": 10.0,
            "frequency": 1.0
        });
        let err = serde_json::from_value::<AnimationEffect>(json)
            .expect_err("an unrecognised wiggle property must be rejected, not silently inert");
        assert!(err.to_string().contains("skew"), "got: {err}");
    }

    #[test]
    fn wiggle_kebab_case_property_gets_a_did_you_mean() {
        // The exact trap named in constat #4: this project mixes kebab-case
        // (CSS-style, most of `CssStyle`) and snake_case (these property
        // names) conventions across files, so an author reasoning in
        // kebab-case naturally writes `translate-x` instead of the
        // solver's `translate_x` — silently inert before this fix.
        let json = json!({
            "name": "wiggle",
            "property": "translate-x",
            "amplitude": 10.0,
            "frequency": 1.0
        });
        let err = serde_json::from_value::<AnimationEffect>(json)
            .expect_err("kebab-case must not silently resolve to a snake_case no-op");
        let msg = err.to_string();
        assert!(msg.contains("translate-x"), "got: {msg}");
        assert!(
            msg.contains("translate_x"),
            "expected a did-you-mean nudge toward the correct spelling, got: {msg}"
        );
    }

    #[test]
    fn keyframes_animation_known_property_still_works() {
        let json = json!({
            "name": "keyframes",
            "keyframes": [
                { "property": "opacity", "keyframes": [
                    { "time": 0.0, "value": 0.0 },
                    { "time": 1.0, "value": 1.0 }
                ]}
            ]
        });
        let effect: AnimationEffect = serde_json::from_value(json).unwrap();
        match effect {
            AnimationEffect::Keyframes(cfg) => assert_eq!(cfg.keyframes[0].property, "opacity"),
            other => panic!("expected Keyframes, got {other:?}"),
        }
    }

    #[test]
    fn keyframes_animation_unknown_property_is_a_named_error() {
        let json = json!({
            "name": "keyframes",
            "keyframes": [
                { "property": "positionX", "keyframes": [
                    { "time": 0.0, "value": 0.0 },
                    { "time": 1.0, "value": 1.0 }
                ]}
            ]
        });
        let err = serde_json::from_value::<AnimationEffect>(json).expect_err(
            "an unrecognised keyframe animation property must be rejected, not silently inert",
        );
        assert!(err.to_string().contains("positionX"), "got: {err}");
    }

    #[test]
    fn keyframes_animation_color_property_still_works() {
        // "color" is solver-recognised (special-cased in
        // `resolve_animations`, outside `apply_property`), not a numeric
        // motion property — must not be rejected.
        let json = json!({
            "name": "keyframes",
            "keyframes": [
                { "property": "color", "keyframes": [
                    { "time": 0.0, "value": "#000000" },
                    { "time": 1.0, "value": "#ffffff" }
                ]}
            ]
        });
        let effect: AnimationEffect = serde_json::from_value(json).unwrap();
        assert!(matches!(effect, AnimationEffect::Keyframes(_)));
    }
}
