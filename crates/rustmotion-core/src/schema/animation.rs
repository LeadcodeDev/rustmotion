use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// `deny_unknown_fields` (reliquat of wave-A's constat, PR #158): closes the
// last gap in `style.animation[*]` typo detection. Wave A covered the nine
// effect-config structs in `schema/video.rs`; the types *inside* a
// `keyframes[*]` entry (this struct and `Keyframe` below) were left
// uncovered — a typo'd key here (e.g. `duratoin`) used to be silently
// dropped instead of reported, same as every other struct this wave closed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Animation {
    pub property: String,
    pub keyframes: Vec<Keyframe>,
    #[serde(default = "default_easing")]
    pub easing: EasingType,
    #[serde(default)]
    pub spring: Option<SpringConfig>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Keyframe {
    pub time: f64,
    pub value: KeyframeValue,
    /// Optional per-keyframe easing (overrides animation-level easing for the segment starting at this keyframe)
    #[serde(default)]
    pub easing: Option<EasingType>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum KeyframeValue {
    Number(f64),
    Color(String),
}

impl KeyframeValue {
    pub fn as_f64(&self) -> f64 {
        match self {
            KeyframeValue::Number(n) => *n,
            KeyframeValue::Color(_) => 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum EasingType {
    #[default]
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
    EaseInQuad,
    EaseOutQuad,
    EaseInCubic,
    EaseOutCubic,
    EaseInExpo,
    EaseOutExpo,
    EaseInOutQuad,
    EaseInOutExpo,
    EaseInBack,
    EaseOutBack,
    EaseOutElastic,
    Bounce,
    Spring,
    /// Custom cubic-bezier easing curve: cubic_bezier(x1, y1, x2, y2)
    CubicBezier {
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
    },
}

fn default_easing() -> EasingType {
    EasingType::EaseOut
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SpringConfig {
    #[serde(default = "default_damping")]
    pub damping: f64,
    #[serde(default = "default_stiffness")]
    pub stiffness: f64,
    #[serde(default = "default_mass")]
    pub mass: f64,
}

impl Default for SpringConfig {
    fn default() -> Self {
        Self {
            damping: 15.0,
            stiffness: 100.0,
            mass: 1.0,
        }
    }
}

fn default_damping() -> f64 {
    15.0
}
fn default_stiffness() -> f64 {
    100.0
}
fn default_mass() -> f64 {
    1.0
}

/// Preset animation names that expand to keyframes automatically
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AnimationPreset {
    // Entrées
    FadeIn,
    FadeInUp,
    FadeInDown,
    FadeInLeft,
    FadeInRight,
    SlideInLeft,
    SlideInRight,
    SlideInUp,
    SlideInDown,
    ScaleIn,
    BounceIn,
    BlurIn,
    RotateIn,
    ElasticIn,
    // Sorties
    FadeOut,
    FadeOutUp,
    FadeOutDown,
    SlideOutLeft,
    SlideOutRight,
    SlideOutUp,
    SlideOutDown,
    ScaleOut,
    BounceOut,
    BlurOut,
    RotateOut,
    // Effets continus
    Pulse,
    Float,
    Shake,
    Spin,
    // 3D
    FlipInX,
    FlipInY,
    FlipOutX,
    FlipOutY,
    TiltIn,
    // Stroke
    DrawIn,
    StrokeReveal,
    // Floating/orbit
    #[serde(alias = "float_3d")]
    Float3d,
    // Spéciaux
    Typewriter,
    WipeLeft,
    WipeRight,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PresetConfig {
    #[serde(default)]
    pub delay: f64,
    #[serde(default = "default_preset_duration")]
    pub duration: f64,
    /// Loop the animation continuously
    #[serde(default, rename = "loop")]
    pub repeat: bool,
    /// Overshoot/anticipation intensity for scale_in/scale_out (0.0 = none, default 0.08 = 8%).
    #[serde(default)]
    pub overshoot: Option<f64>,
    /// Spring physics applied to the preset's motion keyframes (see
    /// `AnimationTiming::spring`).
    #[serde(default)]
    pub spring: Option<SpringConfig>,
    /// Travel of an oscillating preset, in pixels (`float_3d`; default 12).
    ///
    /// Varying it across elements in one scene is what turns a shared bob into
    /// parallax: things at different depths move by different amounts.
    #[serde(default)]
    pub amplitude: Option<f64>,
}

impl Default for PresetConfig {
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

fn default_preset_duration() -> f64 {
    0.8
}

#[cfg(test)]
mod deny_unknown_fields_tests {
    use super::*;
    use serde_json::json;

    // ---- reliquat of the wave-A fix (PR #158): `deny_unknown_fields` was
    // added to the nine effect-config structs in `schema/video.rs`, but the
    // types *inside* a `keyframes[*]` entry — `Animation` and `Keyframe`,
    // both in this file — were left uncovered. A typo'd key inside one of
    // these (e.g. `duratoin` on an `Animation`, or a per-keyframe field
    // typo) used to be silently ignored instead of reported. This is the
    // one change this workstream is authorized to make in this file. ----

    #[test]
    fn animation_rejects_unknown_fields() {
        let json = json!({
            "property": "opacity",
            "keyframes": [{ "time": 0.0, "value": 1.0 }],
            "easing": "ease_out",
            "duratoin": 5.0
        });
        let err = serde_json::from_value::<Animation>(json)
            .expect_err("a typo'd field on Animation must be rejected, not silently ignored");
        assert!(err.to_string().contains("duratoin"), "got: {err}");
    }

    #[test]
    fn keyframe_rejects_unknown_fields() {
        let json = json!({ "time": 0.0, "value": 1.0, "eaisng": "linear" });
        let err = serde_json::from_value::<Keyframe>(json)
            .expect_err("a typo'd field on Keyframe must be rejected, not silently ignored");
        assert!(err.to_string().contains("eaisng"), "got: {err}");
    }

    #[test]
    fn animation_still_accepts_every_known_field() {
        let json = json!({
            "property": "opacity",
            "keyframes": [
                { "time": 0.0, "value": 0.0, "easing": "linear" },
                { "time": 1.0, "value": 1.0 }
            ],
            "easing": "ease_out",
            "spring": { "damping": 10.0, "stiffness": 100.0, "mass": 1.0 }
        });
        let a: Animation = serde_json::from_value(json).unwrap();
        assert_eq!(a.property, "opacity");
        assert_eq!(a.keyframes.len(), 2);
        assert!(a.spring.is_some());
    }

    #[test]
    fn keyframe_still_accepts_every_known_field() {
        let json = json!({ "time": 0.5, "value": 10.0, "easing": "ease_in" });
        let k: Keyframe = serde_json::from_value(json).unwrap();
        assert_eq!(k.time, 0.5);
        assert!(k.easing.is_some());
    }
}
