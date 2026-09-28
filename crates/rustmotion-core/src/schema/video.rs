use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::Path as SkiaPath;

use super::animation::{
    Animation, AnimationPreset, ChromaticAberrationConfig, EasingType, PresetConfig, SpringConfig,
};
use super::style::{FontWeight, TextAlign, VerticalAlign};

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
    /// Scale up from nothing with a back-out overshoot, then a short elastic
    /// pulse before settling — the notification-badge arrival, where the
    /// second beat is what draws the eye after the first has placed the
    /// element. `AnimationTiming.overshoot` sets the pulse amplitude
    /// (default 0.18 = 118%); 0 reduces it to a plain back-out scale-in.
    PopIn(AnimationTiming),
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
    Pulse(AnimationTiming),
    Float(AnimationTiming),
    Shake(AnimationTiming),
    Spin(AnimationTiming),
    FlipInX(AnimationTiming),
    FlipInY(AnimationTiming),
    FlipOutX(AnimationTiming),
    FlipOutY(AnimationTiming),
    TiltIn(TiltInConfig),
    DrawIn(AnimationTiming),
    StrokeReveal(AnimationTiming),
    Typewriter(AnimationTiming),
    WipeLeft(AnimationTiming),
    WipeRight(AnimationTiming),
    #[serde(alias = "float_3d")]
    Float3d(AnimationTiming),
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
    /// starting blur sigma in px (default
    /// `engine::animator::DEFAULT_CHAR_BLUR_SIGMA`, tuned for 100px+ display
    /// type).
    ///
    /// Resolved through `engine::animator::extract_effects` →
    /// `ResolvedCharAnimation` like its five siblings, so it picks up
    /// container-level stagger shifting and `timeline`-embedded copies. (It
    /// used to be read directly off `style.animation` inside
    /// `rustmotion_components::text::Text::paint` and therefore missed both.)
    CharBlurIn(CharAnimationTiming),
    Glow(GlowConfig),
    /// A band of light sweeping across the element's own painted pixels.
    /// See [`ShimmerConfig`].
    Shimmer(ShimmerConfig),
    Wiggle(WiggleConfig),
    Orbit(OrbitConfig),
    Keyframes(KeyframesConfig),
    MotionBlur(MotionBlurConfig),
    /// Temporal trail effect: paints copies of the component at prior times
    /// with decaying opacity, creating a persistence-of-vision ghost trail.
    Trail(TrailConfig),
    /// Move the component along an SVG path, and — when `orient` is set —
    /// rotate it to face the path's tangent direction. See
    /// [`MotionPathConfig`]'s doc comment for the path syntax, the
    /// coordinate space path points are interpreted in, and how degenerate
    /// paths (empty, single-point, zero-length) are handled.
    MotionPath(MotionPathConfig),
    /// Per-element channel-split: the node's own rendered content splits into
    /// red/cyan fringes that converge back to zero separation by the end.
    /// See [`ChromaticAberrationConfig`]'s doc comment.
    ChromaticAberration(ChromaticAberrationConfig),
    /// Cuts the node's own rendered pixels into a deterministic Voronoi
    /// partition and flies the pieces apart (or together). See
    /// [`ShatterConfig`]'s doc comment.
    Shatter(ShatterConfig),
    /// Throws a ring of short strokes outward from just off the node's own
    /// box edge, then retracts them. See [`BurstConfig`]'s doc comment.
    Burst(BurstConfig),
}

impl AnimationEffect {
    pub fn shift_delay(&mut self, by: f64) {
        use AnimationEffect::*;
        match self {
            FadeIn(t) | FadeInUp(t) | FadeInDown(t) | FadeInLeft(t) | FadeInRight(t)
            | SlideInLeft(t) | SlideInRight(t) | SlideInUp(t) | SlideInDown(t) | ScaleIn(t)
            | BounceIn(t) | BlurIn(t) | RotateIn(t) | ElasticIn(t) | PopIn(t) | FadeOut(t)
            | FadeOutUp(t) | FadeOutDown(t) | SlideOutLeft(t) | SlideOutRight(t)
            | SlideOutUp(t) | SlideOutDown(t) | ScaleOut(t) | BounceOut(t) | BlurOut(t)
            | RotateOut(t) | Pulse(t) | Float(t) | Shake(t) | Spin(t) | FlipInX(t) | FlipInY(t)
            | FlipOutX(t) | FlipOutY(t) | DrawIn(t) | StrokeReveal(t) | Typewriter(t)
            | WipeLeft(t) | WipeRight(t) | Float3d(t) => t.delay += by,
            TiltIn(c) => c.delay += by,
            CharScaleIn(c) | CharFadeIn(c) | CharWave(c) | CharBounce(c) | CharRotateIn(c)
            | CharSlideUp(c) | CharBlurIn(c) => c.delay += by,
            Keyframes(c) => c.delay += by,
            MotionPath(c) => c.delay += by,
            Shimmer(c) => c.delay += by,
            ChromaticAberration(c) => c.delay += by,
            Shatter(c) => c.delay += by,
            Burst(c) => c.delay += by,
            Glow(_) | Wiggle(_) | Orbit(_) | MotionBlur(_) | Trail(_) => {}
        }
    }

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
            Self::PopIn(t) => Some((AnimationPreset::PopIn, t)),
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
#[derive(Debug, Clone, JsonSchema, PartialEq)]
pub struct AnimationTiming {
    /// Delay before animation starts (seconds).
    #[serde(default)]
    pub delay: f64,
    /// Animation duration (seconds).
    #[serde(default = "default_animation_duration")]
    pub duration: f64,
    /// Loop the animation continuously. Kept as a plain bool for source
    /// compatibility with every reader that only ever checked this flag
    /// (several live outside this workstream's owned files) — `true`
    /// covers both "loop forever" (`repeat_count: None`) and "loop a known
    /// number of times" (`repeat_count: Some(n)`), so a finite count is
    /// never mistaken for a one-shot animation by code that only reads
    /// this field. The JSON `"loop"` key this deserializes from accepts
    /// either shape (see [`AnimationTimingWire`]); `repeat` alone can't
    /// tell you which one was written — check `repeat_count` for that.
    #[serde(rename = "loop")]
    pub repeat: bool,
    /// How many times the animation plays, when the JSON `"loop"` value
    /// was a positive integer rather than a bare bool (issue #330) — e.g.
    /// `"loop": 12` for GSAP's `repeat: 11` (11 *re*plays, 12 plays
    /// total — this field counts total plays, not replays). `None`
    /// defers entirely to `repeat`: `true` loops forever, `false` plays
    /// once — today's behaviour, unchanged. `0` and `1` both fold back
    /// into `repeat: false, repeat_count: None` at parse time (see
    /// [`RepeatSpec::into_parts`]): there is no visible difference
    /// between "play once" and "loop zero times", so there's no reason to
    /// carry a count that never changes anything downstream.
    #[serde(default)]
    pub repeat_count: Option<u32>,
    /// Reverse direction on every other play (ping-pong) instead of
    /// snapping back to the start each cycle — GSAP calls this `yoyo`.
    /// Only meaningful when the animation actually repeats (`repeat` or
    /// `repeat_count`); a no-op otherwise. Works on any animation this
    /// timing drives, not a fixed set of presets — see
    /// `engine::animator::cycle_time`.
    #[serde(default)]
    pub yoyo: bool,
    /// Pause between plays, in seconds, held at the resting value of the
    /// play that just finished before the next one starts. `0.0` (default)
    /// is a seamless loop.
    #[serde(default)]
    pub repeat_delay: f64,
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

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
enum RepeatSpec {
    Loop(bool),
    Count(u32),
}

impl Default for RepeatSpec {
    fn default() -> Self {
        RepeatSpec::Loop(false)
    }
}

impl RepeatSpec {
    fn into_parts(self) -> (bool, Option<u32>) {
        match self {
            RepeatSpec::Loop(b) => (b, None),
            RepeatSpec::Count(0) | RepeatSpec::Count(1) => (false, None),
            RepeatSpec::Count(n) => (true, Some(n)),
        }
    }

    fn from_parts(repeat: bool, repeat_count: Option<u32>) -> Self {
        match repeat_count {
            Some(n) => RepeatSpec::Count(n),
            None => RepeatSpec::Loop(repeat),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AnimationTimingWire {
    #[serde(default)]
    delay: f64,
    #[serde(default = "default_animation_duration")]
    duration: f64,
    #[serde(default, rename = "loop")]
    repeat: RepeatSpec,
    #[serde(default)]
    yoyo: bool,
    #[serde(default)]
    repeat_delay: f64,
    #[serde(default)]
    overshoot: Option<f64>,
    #[serde(default)]
    spring: Option<SpringConfig>,
    #[serde(default)]
    amplitude: Option<f64>,
}

impl From<AnimationTimingWire> for AnimationTiming {
    fn from(wire: AnimationTimingWire) -> Self {
        let (repeat, repeat_count) = wire.repeat.into_parts();
        AnimationTiming {
            delay: wire.delay,
            duration: wire.duration,
            repeat,
            repeat_count,
            yoyo: wire.yoyo,
            repeat_delay: wire.repeat_delay,
            overshoot: wire.overshoot,
            spring: wire.spring,
            amplitude: wire.amplitude,
        }
    }
}

impl From<&AnimationTiming> for AnimationTimingWire {
    fn from(t: &AnimationTiming) -> Self {
        AnimationTimingWire {
            delay: t.delay,
            duration: t.duration,
            repeat: RepeatSpec::from_parts(t.repeat, t.repeat_count),
            yoyo: t.yoyo,
            repeat_delay: t.repeat_delay,
            overshoot: t.overshoot,
            spring: t.spring.clone(),
            amplitude: t.amplitude,
        }
    }
}

impl<'de> Deserialize<'de> for AnimationTiming {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        AnimationTimingWire::deserialize(deserializer).map(AnimationTiming::from)
    }
}

impl Serialize for AnimationTiming {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        AnimationTimingWire::from(self).serialize(serializer)
    }
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
            repeat_count: None,
            yoyo: false,
            repeat_delay: 0.0,
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
    /// Which way each unit travels in from. Only `char_slide_up` and
    /// `char_blur_in` have a travel axis to redirect; the other presets
    /// ignore it. Default `up` — the historical behaviour.
    #[serde(default)]
    pub direction: TextAnimDirection,
    /// Multiplier on how far each unit travels, `1.0` being each preset's
    /// own tuned distance (0.8em for `slide_up`, 0.12em for `blur_in`).
    /// Use ~0.5 for a tighter arrival, ~1.85 for a pronounced staircase.
    #[serde(default)]
    pub distance: Option<f64>,
    /// Scale each unit starts at, growing to 1.0 over its animation —
    /// combined with, not instead of, the preset's own motion. 0.82 gives
    /// the punchy "number pops in" arrival, 0.92 a barely-perceptible
    /// settle. Unset means no scaling (the historical behaviour); the
    /// scale-based presets (`char_scale_in`, `char_bounce`) own their scale
    /// curve outright and ignore this.
    #[serde(default)]
    pub scale_from: Option<f64>,
    /// Randomises each unit's start time by up to ±`jitter × stagger`, so
    /// the units arrive in uneven bursts instead of a metronomic march.
    /// This is what separates a streaming-token look from a typewriter:
    /// language models don't emit words on a clock. 0 (default) keeps the
    /// exact even spacing.
    ///
    /// The offsets are derived from `seed` and the unit's index, never from
    /// a live RNG — a frame must render identically no matter which
    /// process, thread or `--frames` segment computes it.
    #[serde(default)]
    pub jitter: Option<f64>,
    /// Seed for `jitter`. Changing it reshuffles the arrival rhythm without
    /// changing its statistics.
    #[serde(default)]
    pub seed: Option<u32>,
    /// Colour each unit starts at before settling to the text's own colour
    /// over its animation. A dim grey here reproduces the way freshly
    /// streamed tokens read as unsettled before the eye accepts them.
    #[serde(default)]
    pub ink_from: Option<String>,
}

impl Default for CharAnimationTiming {
    fn default() -> Self {
        Self {
            delay: 0.0,
            duration: default_char_duration_f64(),
            stagger: default_char_stagger_f64(),
            granularity: TextAnimGranularity::default(),
            easing: EasingType::default(),
            overshoot: None,
            blur: None,
            direction: TextAnimDirection::default(),
            distance: None,
            scale_from: None,
            jitter: None,
            seed: None,
            ink_from: None,
        }
    }
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

/// Which way a per-unit entrance travels from before it settles.
///
/// The unit starts offset in the *opposite* direction of travel and moves
/// towards its laid-out position: `Up` (the default, and the historical
/// behaviour) starts below the baseline and rises; `Down` starts above and
/// falls, which is the "letters cascading from the top" look.
///
/// Read by the `slide_up` and `blur_in` char presets — the ones whose motion
/// is a translate. `scale_in`, `bounce`, `rotate_in`, `fade_in` and `wave`
/// have no travel axis to redirect and ignore it.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TextAnimDirection {
    /// Starts below its line and rises into place (default).
    #[default]
    Up,
    /// Starts above its line and falls into place.
    Down,
    /// Starts to the right and slides left into place.
    Left,
    /// Starts to the left and slides right into place.
    Right,
}

impl TextAnimDirection {
    pub fn offset(self, travel: f32) -> (f32, f32) {
        match self {
            Self::Up => (0.0, travel),
            Self::Down => (0.0, -travel),
            Self::Left => (travel, 0.0),
            Self::Right => (-travel, 0.0),
        }
    }
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
    pub fn to_preset_config(&self) -> PresetConfig {
        PresetConfig {
            amplitude: self.amplitude,
            delay: self.delay,
            duration: self.duration,
            repeat: self.repeat,
            repeat_count: self.repeat_count,
            yoyo: self.yoyo,
            repeat_delay: self.repeat_delay,
            overshoot: self.overshoot,
            spring: self.spring.clone(),
        }
    }
}

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
    "blur_x",
    "blur_y",
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
    "clip_path_progress",
    "letter_spacing",
    "draw_start",
];

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
///
/// `mode: "smear"` skips the ghost sampler entirely: the node's displacement
/// over the `shutter / fps` window that precedes `t` is turned into a
/// `{ "fn": "blur", "radius-x": …, "radius-y": … }` filter on the principal
/// itself — one streaked copy instead of up to sixteen stacked ones, and no
/// risk of a ghost taking a flex slot since none are created.
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
    /// Ignored when `mode` is `"smear"`.
    #[serde(default = "default_motion_blur_samples")]
    pub samples: u32,
    /// Fraction of one frame duration used as the shutter window (default 0.5).
    /// The temporal spread equals `shutter / fps` seconds. Also the window
    /// `mode: "smear"` samples velocity over.
    #[serde(default = "default_motion_blur_shutter")]
    pub shutter: f64,
    /// `"stack"` (default) paints ghost copies at sampled instants. `"smear"`
    /// derives a directional blur from instantaneous velocity instead.
    #[serde(default)]
    pub mode: MotionBlurMode,
}

/// How `motion_blur` renders its trail. See `MotionBlurConfig`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MotionBlurMode {
    /// Ghost copies painted at sampled instants (the historical behaviour).
    #[default]
    Stack,
    /// A directional `blur` derived from instantaneous velocity, applied to
    /// the principal itself. Creates no ghost nodes; `samples` is ignored.
    Smear,
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

/// Configuration for the `motion_path` animation effect: moves — and,
/// optionally, orients — a component along an SVG path.
///
/// # Path syntax
/// `path` is the SVG path `d`-attribute mini-language (`M`/`L`/`H`/`V`/`C`/
/// `S`/`Q`/`T`/`A`/`Z`, absolute or relative) — the exact syntax `shape`'s
/// `ShapeType::Path { data }` already accepts and parses with
/// `skia_safe::Path::from_svg` (`engine/renderer/shapes.rs`). This effect
/// reuses that same call rather than inventing a second path grammar, and
/// measures the parsed path with `skia_safe::PathMeasure` — the exact
/// primitive `svg.rs` (dash-reveal of an SVG document's paths) and
/// `arrow.rs` (dash-reveal of a bezier curve, plus its own tangent-based
/// arrowhead orientation) already use for `draw_progress`. `orient`'s
/// tangent-to-degrees math is the same `atan2(tangent.y, tangent.x)` idiom
/// `arrow.rs::draw_arrowhead` already computes for its arrowhead.
///
/// # Coordinate space
/// Path coordinates are pixel **deltas relative to wherever CSS layout
/// placed the component absent this effect** — the same convention `orbit`
/// already uses for its circular motion (see `OrbitConfig`/`apply_orbits`),
/// and the only one implementable here: the resolver this effect plugs into
/// (`engine::animator::resolve_props_for_effects`) receives only
/// `(effects, time, scene_duration)` — never the component's resolved
/// layout box or the viewport, which are computed downstream in
/// `paint_pass.rs`/`box_builder.rs`. So `"M0,0 L200,0"` slides the
/// component 200px right of its laid-out position (and back, if the effect
/// loops); `"M100,0 L300,0"` starts the component already displaced 100px
/// right of its laid-out position — a direct, intentional consequence of
/// treating the whole path as a translate delta, not a bug to normalize
/// away.
///
/// # Degenerate paths
/// - **Empty or syntactically invalid** `path` (e.g. `""`, garbage text) is
///   rejected at JSON-parse time by [`deserialize_motion_path_data`] — a
///   named error, never a silent no-op, mirroring
///   [`deserialize_motion_property`]'s treatment of an unrecognised
///   `wiggle`/`keyframes` property name.
/// - **Zero measured length** (a single point, e.g. `"M50,50"`, or every
///   segment collapsing onto one point) is syntactically valid SVG and is
///   *not* rejected at parse time — it has a well-defined render-time
///   meaning: the component holds at that single point for the whole
///   timeline, and `orient` (if set) contributes no rotation (a tangent is
///   undefined at zero length) instead of propagating a NaN. See
///   `engine::animator::motion_path_sample`. `validate_schema.rs` flags it
///   as a (non-blocking) warning, since it is very likely — but not
///   certainly — an authoring mistake (e.g. duplicated coordinates).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct MotionPathConfig {
    /// SVG path data (the `d`-attribute mini-language). Must contain at
    /// least one drawable point — an empty or unparsable path is rejected
    /// at deserialize time (see [`deserialize_motion_path_data`]).
    #[serde(deserialize_with = "deserialize_motion_path_data")]
    pub path: String,
    /// Delay before travel starts (seconds). Before this, the component
    /// sits at the path's start point (progress 0) — the same "hold at the
    /// entrance state" behaviour every other preset/effect with a `delay`
    /// already has.
    #[serde(default)]
    pub delay: f64,
    /// Time to travel the whole path once (seconds).
    #[serde(default = "default_animation_duration")]
    pub duration: f64,
    /// Loop the traversal continuously instead of holding at the path's end
    /// once `delay + duration` has elapsed.
    #[serde(default, rename = "loop")]
    pub repeat: bool,
    /// Rotate the component to face the path's tangent direction (default
    /// false — position without orientation, e.g. for content that should
    /// stay upright while it moves).
    #[serde(default)]
    pub orient: bool,
    /// Degrees added on top of the tangent-derived rotation, for assets
    /// whose drawn "forward" direction is not +X (default 0.0). E.g. an
    /// icon drawn pointing up needs `orient_offset: 90`.
    #[serde(default)]
    pub orient_offset: f64,
    /// Easing applied to progress along the path (default linear — constant
    /// speed along the curve, the expected default for a hand-authored
    /// trajectory; `ease_in`/`ease_out` bunches travel toward one end).
    #[serde(default)]
    pub easing: EasingType,
}

fn deserialize_motion_path_data<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    match SkiaPath::from_svg(&s) {
        Some(path) if path.count_points() > 0 => Ok(s),
        Some(_) => Err(serde::de::Error::custom(format!(
            "motion_path.path '{s}' has no drawable point (an empty path has nothing to travel \
             to) — provide at least one command, e.g. \"M0,0 L100,0\""
        ))),
        None => Err(serde::de::Error::custom(format!(
            "motion_path.path '{s}' is not valid SVG path data (the same 'd'-attribute \
             mini-language shape's type: \"path\" accepts), e.g. \"M0,0 C50,-100 150,-100 200,0\""
        ))),
    }
}

/// Configuration for the `shatter` animation effect: the node's own rendered
/// subtree — background, border, content and children, exactly as it would
/// have painted — is rasterised once, cut into `pieces` convex cells by a
/// deterministic Voronoi partition seeded by `seed`, and each cell is flown
/// away from `origin`, spun and faded independently.
///
/// `seed` and the sampled instant fully determine every shard's cell,
/// direction, spin and depth — two renders of the same file at the same time
/// are byte-identical.
///
/// `mode` decides which end of the timeline is the intact node and which is
/// the fully dispersed one, and what happens once `delay + duration` has
/// elapsed:
/// - `"out"` (default): assembled at `delay`, fully dispersed at
///   `delay + duration`. Outside `[delay, delay + duration)` the effect
///   contributes nothing at all — the node renders byte-identically to one
///   with no `shatter` effect in its `animation` list, the same hard
///   short-circuit `chromatic_aberration` and `zoom_blur` use rather than a
///   fade that only gets close to zero.
/// - `"in"` is the mirror: dispersed at `delay`, assembled at
///   `delay + duration`, and — like `"out"` — outside its window the node
///   renders as if the effect were absent.
/// - `"hold"` plays the same dispersal as `"out"` but freezes at the fully
///   dispersed state once `delay + duration` is reached instead of
///   short-circuiting back to the plain node — it legitimately never
///   converges.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ShatterConfig {
    /// Delay before the shards start moving (seconds).
    #[serde(default)]
    pub delay: f64,
    /// How long the dispersal (or, in `"in"` mode, the assembly) takes
    /// (seconds).
    #[serde(default = "default_shatter_duration")]
    pub duration: f64,
    /// Which end of the timeline is intact and what happens after
    /// `delay + duration`. See the type-level doc comment.
    #[serde(default)]
    pub mode: ShatterMode,
    /// Number of Voronoi shards (default 24, clamped 1..=64).
    #[serde(default = "default_shatter_pieces")]
    pub pieces: u32,
    /// Seed for the deterministic Voronoi partition and every shard's
    /// direction/spin/depth jitter.
    #[serde(default)]
    pub seed: u32,
    /// Point shards fly away from (or, in `"in"` mode, converge toward), as
    /// a fraction of the node's own box (`{ "x": 0.5, "y": 0.5 }` is the
    /// centre, the default).
    #[serde(default)]
    pub origin: ShatterOrigin,
    /// Radial travel multiplier at full dispersal, relative to the node's
    /// own diagonal (default 1.0). `0` pins shards in place — only spin,
    /// depth and fade remain visible.
    #[serde(default = "default_shatter_spread")]
    pub spread: f32,
    /// Maximum rotation in degrees a shard reaches at full dispersal
    /// (default 90); each shard's own sign and magnitude are jittered from
    /// `seed`.
    #[serde(default = "default_shatter_spin")]
    pub spin: f32,
    /// Per-shard scale modulation at full dispersal, the same "0.0 = none"
    /// semantics as `OrbitConfig::depth` (default 0.4): each shard's own
    /// signed depth is jittered from `seed`, so some shards appear to come
    /// toward the camera (scale > 1) while others recede (scale < 1).
    #[serde(default = "default_shatter_depth")]
    pub depth: f32,
    /// Fade each shard's opacity to zero as it reaches full dispersal
    /// (default true). With `mode: "in"` this is a mirror: shards start
    /// transparent and reach full opacity as they assemble.
    #[serde(default = "default_true")]
    pub fade: bool,
}

fn default_shatter_duration() -> f64 {
    0.6
}
fn default_shatter_pieces() -> u32 {
    24
}
fn default_shatter_spread() -> f32 {
    1.0
}
fn default_shatter_spin() -> f32 {
    90.0
}
fn default_shatter_depth() -> f32 {
    0.4
}
fn default_true() -> bool {
    true
}

/// Which end of `shatter`'s timeline is the intact node. See
/// [`ShatterConfig`]'s doc comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ShatterMode {
    /// Assembled at `delay`, dispersed at `delay + duration`, then
    /// short-circuits back to the plain node.
    #[default]
    Out,
    /// Dispersed at `delay`, assembled at `delay + duration`, then
    /// short-circuits back to the plain node (the same "no effect" state
    /// its own end converges to).
    In,
    /// Same dispersal as `"out"`, but freezes at full dispersal instead of
    /// returning to the plain node.
    Hold,
}

/// Fractional point within `shatter`'s own box — `{ "x": 0.0, "y": 0.0 }` is
/// the top-left corner, `{ "x": 1.0, "y": 1.0 }` the bottom-right. See
/// [`ShatterConfig::origin`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShatterOrigin {
    #[serde(default = "default_shatter_origin_component")]
    pub x: f32,
    #[serde(default = "default_shatter_origin_component")]
    pub y: f32,
}

impl Default for ShatterOrigin {
    fn default() -> Self {
        Self {
            x: default_shatter_origin_component(),
            y: default_shatter_origin_component(),
        }
    }
}

fn default_shatter_origin_component() -> f32 {
    0.5
}

/// Configuration for the `burst` animation effect: a ring of short strokes
/// that shoot outward from just off the node's own box edge and then retract,
/// the splash of ink a pill or a badge throws when it pops in.
///
/// Nothing about the node itself is touched — the strokes are painted over it,
/// outside its box, and take no layout space. Outside
/// `[delay, delay + duration)` the effect contributes nothing at all.
///
/// The stroke is drawn by a head and a tail that each cross the stroke's track
/// once: the head runs out over the first half of the window, the tail follows
/// over the second. Both ends of the window therefore have a zero-length
/// stroke — the burst is invisible at `delay` and at `delay + duration`
/// without ever being a fade that merely gets small.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BurstConfig {
    /// Delay before the strokes start shooting out (seconds).
    #[serde(default)]
    pub delay: f64,
    /// Full out-and-back duration (seconds). The head reaches the far end of
    /// its track at the halfway point.
    #[serde(default = "default_burst_duration")]
    pub duration: f64,
    /// Number of strokes in the ring (default 8, clamped 1..=64).
    #[serde(default = "default_burst_count")]
    pub count: u32,
    /// Length of each stroke's track, in px, measured outward from `gap`
    /// (default 40).
    #[serde(default = "default_burst_length")]
    pub length: f32,
    /// Distance in px between the node's box edge and the near end of every
    /// stroke's track (default 12). The box is never overdrawn.
    #[serde(default = "default_burst_gap")]
    pub gap: f32,
    /// Stroke width in px (default 4).
    #[serde(default = "default_burst_width")]
    pub width: f32,
    /// Stroke colour (hex string, default "#FFB020").
    #[serde(default = "default_burst_color")]
    pub color: String,
    /// Seed for the per-stroke angle, length and phase jitter.
    #[serde(default)]
    pub seed: u32,
    /// How far each stroke may stray from its even share of the ring, as a
    /// fraction of the spacing between two strokes (default 0.2). `0` gives a
    /// perfectly regular ring; `1` lets a stroke reach its neighbour's slot.
    /// It also scales the per-stroke length and phase jitter.
    #[serde(default = "default_burst_jitter")]
    pub jitter: f32,
}

fn default_burst_duration() -> f64 {
    0.4
}
fn default_burst_count() -> u32 {
    8
}
fn default_burst_length() -> f32 {
    40.0
}
fn default_burst_gap() -> f32 {
    12.0
}
fn default_burst_width() -> f32 {
    4.0
}
fn default_burst_color() -> String {
    "#FFB020".to_string()
}
fn default_burst_jitter() -> f32 {
    0.2
}

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
    /// `skia_safe::PathEffect::dash` interval list (on-length, off-length,
    /// repeating) — the same spelling and shape `arrow`/`connector`/`line`
    /// already use for their own `dashed` field. `None` (the default)
    /// strokes solid, exactly as every `Stroke` did before this field
    /// existed.
    #[serde(default)]
    pub dashed: Option<Vec<f32>>,
    /// Phase offset (px) into `dashed`'s pattern — `skia_safe::PathEffect::
    /// dash`'s second argument, hardcoded to `0.0` on `arrow`/`connector`/
    /// `line` today. A literal number is a constant phase; an `"= ..."`
    /// expression (see `crate::expr`) is re-evaluated every frame against
    /// the node's `crate::css::FrameClock` (`$t`/`$T`/`$duration`/`$W`/
    /// `$H`/`$fps`), the same per-frame mechanism `CssStyle`'s `opacity`/
    /// `width`/`height` already use — this is what makes a dashed stroke's
    /// "draw-on" (dash length == path length, offset animating from the
    /// full length down to `0`) expressible without the engine's classic
    /// keyframe/easing `AnimatedProperties` pipeline.
    #[serde(default)]
    pub dash_offset: Option<crate::expr::Computed<f32>>,
    /// `stroke-linecap`. Default `LineCap::Butt` — skia's own default, so
    /// a stroke that never set this renders byte-identical to before this
    /// field existed.
    #[serde(default)]
    pub line_cap: LineCap,
    /// `stroke-linejoin`. Default `LineJoin::Miter` — skia's own default,
    /// same backward-compatibility guarantee as `line_cap` above.
    #[serde(default)]
    pub line_join: LineJoin,
}

/// `stroke-linecap` — how an open subpath's two ends are drawn. See
/// `Stroke::line_cap`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum LineCap {
    #[default]
    Butt,
    Round,
    Square,
}

/// `stroke-linejoin` — how two stroked segments meet at a vertex. See
/// `Stroke::line_join`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum LineJoin {
    #[default]
    Miter,
    Round,
    Bevel,
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

/// A later label a `text` swaps to, and when.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TextState {
    /// Time (seconds, scene-local) at which this label takes over.
    pub at: f64,
    /// The label from `at` onwards.
    pub content: String,
}

/// How a `text` crosses from one of its `states` to the next.
///
/// Both labels are on screen at once during the window: the outgoing one
/// leaves upwards while blurring out, the incoming one arrives from below
/// while sharpening. Cutting between them instead reads as a glitch, and
/// fading alone reads as two unrelated labels rather than one value changing.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TextSwapConfig {
    /// How long the crossing takes (seconds).
    #[serde(default = "default_swap_duration")]
    pub duration: f64,
    /// Vertical travel of each label, in px.
    #[serde(default = "default_swap_distance")]
    pub distance: f32,
    /// Peak blur sigma (px) reached by a label at the far end of its travel.
    /// `0` gives a pure slide-and-fade.
    #[serde(default = "default_swap_blur")]
    pub blur: f32,
}

impl Default for TextSwapConfig {
    fn default() -> Self {
        Self {
            duration: default_swap_duration(),
            distance: default_swap_distance(),
            blur: default_swap_blur(),
        }
    }
}

fn default_swap_duration() -> f64 {
    0.45
}

fn default_swap_distance() -> f32 {
    18.0
}

fn default_swap_blur() -> f32 {
    8.0
}

/// Shape of a text caret.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CaretShape {
    /// A thin vertical rule, like a text editor's insertion point.
    #[default]
    Line,
    /// A filled block covering a character cell, like a terminal.
    Block,
}

/// A caret pinned to a `text`'s reveal head.
///
/// Only meaningful alongside a `typewriter` animation: the caret follows the
/// last revealed character and stops at the end of the line once the reveal
/// finishes. Composing a standalone `cursor` component next to the text gets
/// you a caret that stays where you put it while the text grows out from
/// under it, which is the thing this exists to avoid.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CaretConfig {
    /// Caret shape.
    #[serde(default)]
    pub shape: CaretShape,
    /// Caret colour (hex). Defaults to the text's own colour, so a caret
    /// inherits a theme change without being restated.
    #[serde(default)]
    pub color: Option<String>,
    /// Blink period in seconds — one full off/on cycle. `0` disables the
    /// blink and leaves the caret solid.
    #[serde(default = "default_caret_blink")]
    pub blink: f32,
    /// Hide the caret once the reveal has finished, instead of leaving it
    /// parked at the end of the text.
    #[serde(default)]
    pub hide_when_done: bool,
}

impl Default for CaretConfig {
    fn default() -> Self {
        Self {
            shape: CaretShape::default(),
            color: None,
            blink: default_caret_blink(),
            hide_when_done: false,
        }
    }
}

fn default_caret_blink() -> f32 {
    1.0
}

/// A band of light sweeping across the element, restricted to the pixels the
/// element actually painted.
///
/// That restriction is the whole point: painted over the element's *box*, a
/// sheen reads as a rectangle sliding past. Painted over the element's own
/// alpha, it reads as light catching the glyphs (or the icon, or the chart
/// bars) themselves. See `engine::paint_pass`, which composites it with
/// `BlendMode::SrcATop` inside the node's own layer.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ShimmerConfig {
    /// Delay before the sweep starts (seconds).
    #[serde(default)]
    pub delay: f64,
    /// How long one sweep takes (seconds).
    #[serde(default = "default_shimmer_duration")]
    pub duration: f64,
    /// Colour of the band (hex).
    #[serde(default = "default_shimmer_color")]
    pub color: String,
    /// Band width as a fraction of the distance it sweeps (0.35 = a band
    /// covering roughly a third of the element at any instant). Wider reads
    /// as a soft wash, narrower as a hard glint.
    #[serde(default = "default_shimmer_width")]
    pub width: f32,
    /// Peak opacity of the band (0..1).
    #[serde(default = "default_shimmer_intensity")]
    pub intensity: f32,
    /// Lean of the band in degrees. `0` is an upright band sweeping
    /// left-to-right; the default 20 tilts it, which is what makes it read
    /// as a reflection rather than a wipe. The band always travels
    /// perpendicular to itself, so this angles the travel too.
    #[serde(default = "default_shimmer_angle")]
    pub angle: f32,
    /// Repeat the sweep for the rest of the scene.
    #[serde(default, rename = "loop")]
    pub repeat: bool,
}

fn default_shimmer_duration() -> f64 {
    0.9
}

fn default_shimmer_color() -> String {
    "#FFFFFF".to_string()
}

fn default_shimmer_width() -> f32 {
    0.35
}

fn default_shimmer_intensity() -> f32 {
    0.75
}

fn default_shimmer_angle() -> f32 {
    20.0
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

#[cfg(test)]
mod motion_path_schema_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn motion_path_deserializes_with_known_fields() {
        let json = json!({
            "name": "motion_path",
            "path": "M0,0 L100,0 L100,100",
            "delay": 0.2,
            "duration": 1.5,
            "loop": true,
            "orient": true,
            "orient_offset": 90.0,
            "easing": "ease_out"
        });
        let effect: AnimationEffect = serde_json::from_value(json).unwrap();
        match effect {
            AnimationEffect::MotionPath(cfg) => {
                assert_eq!(cfg.path, "M0,0 L100,0 L100,100");
                assert_eq!(cfg.delay, 0.2);
                assert_eq!(cfg.duration, 1.5);
                assert!(cfg.repeat);
                assert!(cfg.orient);
                assert_eq!(cfg.orient_offset, 90.0);
                assert_eq!(cfg.easing, EasingType::EaseOut);
            }
            other => panic!("expected MotionPath, got {other:?}"),
        }
    }

    #[test]
    fn motion_path_defaults_match_documented_values() {
        let json = json!({ "name": "motion_path", "path": "M0,0 L10,0" });
        let effect: AnimationEffect = serde_json::from_value(json).unwrap();
        match effect {
            AnimationEffect::MotionPath(cfg) => {
                assert_eq!(cfg.delay, 0.0);
                assert_eq!(cfg.duration, default_animation_duration());
                assert!(!cfg.repeat);
                assert!(!cfg.orient);
                assert_eq!(cfg.orient_offset, 0.0);
                assert_eq!(cfg.easing, EasingType::Linear);
            }
            other => panic!("expected MotionPath, got {other:?}"),
        }
    }

    #[test]
    fn motion_path_rejects_an_empty_path_string() {
        let json = json!({ "name": "motion_path", "path": "" });
        let err = serde_json::from_value::<AnimationEffect>(json)
            .expect_err("an empty motion_path.path must be rejected, not silently accepted");
        assert!(err.to_string().contains("no drawable point"), "got: {err}");
    }

    #[test]
    fn motion_path_rejects_unparsable_svg_path_data() {
        let json = json!({ "name": "motion_path", "path": "this is not svg path data !!" });
        let err = serde_json::from_value::<AnimationEffect>(json)
            .expect_err("garbage path data must be rejected, not silently accepted");
        assert!(
            err.to_string().contains("not valid SVG path data"),
            "got: {err}"
        );
    }

    #[test]
    fn motion_path_accepts_a_single_point_path() {
        let json = json!({ "name": "motion_path", "path": "M50,50" });
        let effect: AnimationEffect = serde_json::from_value(json)
            .expect("a syntactically valid single-point path must be accepted");
        assert!(matches!(effect, AnimationEffect::MotionPath(_)));
    }

    #[test]
    fn motion_path_rejects_unknown_fields() {
        let json = json!({ "name": "motion_path", "path": "M0,0 L10,0", "detla": 0.2 });
        let err = serde_json::from_value::<AnimationEffect>(json)
            .expect_err("a typo'd field on motion_path must be rejected, not silently ignored");
        assert!(err.to_string().contains("detla"), "got: {err}");
    }

    #[test]
    fn motion_path_shift_delay_shifts_the_configs_own_delay() {
        let mut effect = AnimationEffect::MotionPath(MotionPathConfig {
            path: "M0,0 L10,0".to_string(),
            delay: 0.5,
            duration: 1.0,
            repeat: false,
            orient: false,
            orient_offset: 0.0,
            easing: EasingType::Linear,
        });
        effect.shift_delay(0.25);
        match effect {
            AnimationEffect::MotionPath(cfg) => assert!((cfg.delay - 0.75).abs() < 1e-9),
            other => panic!("expected MotionPath, got {other:?}"),
        }
    }
}

#[cfg(test)]
mod animation_timing_repeat_widening_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn loop_true_is_unchanged_infinite_repeat() {
        let json = json!({ "name": "pulse", "loop": true });
        let effect: AnimationEffect = serde_json::from_value(json).unwrap();
        match effect {
            AnimationEffect::Pulse(t) => {
                assert!(t.repeat, "\"loop\": true must still set repeat = true");
                assert_eq!(
                    t.repeat_count, None,
                    "a bare `true` carries no count — infinite, exactly as before"
                );
                assert!(!t.yoyo);
                assert_eq!(t.repeat_delay, 0.0);
            }
            other => panic!("expected Pulse, got {other:?}"),
        }
    }

    #[test]
    fn loop_false_and_omitted_are_unchanged_and_identical() {
        let explicit: AnimationTiming = serde_json::from_value(json!({ "loop": false })).unwrap();
        let omitted: AnimationTiming = serde_json::from_value(json!({})).unwrap();
        assert_eq!(
            explicit, omitted,
            "an explicit `false` and an omitted `loop` must resolve identically"
        );
        assert!(!explicit.repeat);
        assert_eq!(explicit.repeat_count, None);
        assert_eq!(explicit, AnimationTiming::default());
    }

    #[test]
    fn loop_as_a_positive_integer_sets_repeat_and_the_count() {
        let t: AnimationTiming = serde_json::from_value(json!({ "loop": 12 })).unwrap();
        assert!(t.repeat, "a finite count still loops — see the field doc");
        assert_eq!(t.repeat_count, Some(12));
    }

    #[test]
    fn loop_as_zero_or_one_folds_back_to_the_boolean_form() {
        for n in [0, 1] {
            let t: AnimationTiming = serde_json::from_value(json!({ "loop": n })).unwrap();
            assert!(!t.repeat, "loop: {n} must not set repeat");
            assert_eq!(t.repeat_count, None, "loop: {n} must not carry a count");
        }
    }

    #[test]
    fn yoyo_and_repeat_delay_default_to_off_and_zero() {
        let t: AnimationTiming = serde_json::from_value(json!({})).unwrap();
        assert!(!t.yoyo);
        assert_eq!(t.repeat_delay, 0.0);
    }

    #[test]
    fn yoyo_and_repeat_delay_are_accepted_on_any_animation_timing() {
        let json = json!({
            "name": "shake",
            "duration": 0.035,
            "loop": 12,
            "yoyo": true,
            "repeat_delay": 0.01
        });
        let effect: AnimationEffect = serde_json::from_value(json).unwrap();
        match effect {
            AnimationEffect::Shake(t) => {
                assert!(t.yoyo);
                assert_eq!(t.repeat_delay, 0.01);
                assert_eq!(t.repeat_count, Some(12));
            }
            other => panic!("expected Shake, got {other:?}"),
        }
    }

    #[test]
    fn a_typo_is_still_rejected_through_the_wire_type() {
        let json = json!({ "name": "pulse", "duratoin": 1.0 });
        let err = serde_json::from_value::<AnimationEffect>(json)
            .expect_err("a typo'd field must still be rejected, not silently ignored");
        assert!(err.to_string().contains("duratoin"), "got: {err}");
    }

    #[test]
    fn loop_as_a_negative_number_is_rejected_not_silently_coerced() {
        let json = json!({ "name": "pulse", "loop": -1 });
        assert!(
            serde_json::from_value::<AnimationEffect>(json).is_err(),
            "a negative \"loop\" is neither a bool nor a valid play count"
        );
    }

    #[test]
    fn a_finite_repeat_count_round_trips_through_a_single_loop_key() {
        let t: AnimationTiming = serde_json::from_value(json!({ "loop": 12 })).unwrap();
        let json = serde_json::to_value(&t).unwrap();
        assert!(
            json.get("repeat_count").is_none(),
            "must not emit a separate repeat_count key: {json}"
        );
        assert_eq!(json["loop"], serde_json::json!(12));
        let back: AnimationTiming = serde_json::from_value(json).unwrap();
        assert_eq!(t, back);
    }

    #[test]
    fn easing_steps_round_trips_and_is_distinct_from_cubic_bezier() {
        let json = json!({ "steps": 4 });
        let easing: EasingType = serde_json::from_value(json).unwrap();
        assert_eq!(easing, EasingType::Steps(4));
        let back = serde_json::to_value(&easing).unwrap();
        assert_eq!(back, json!({ "steps": 4 }));
    }
}
