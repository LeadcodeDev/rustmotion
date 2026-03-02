use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Animation {
    pub property: String,
    pub keyframes: Vec<Keyframe>,
    #[serde(default = "default_easing")]
    pub easing: EasingType,
    #[serde(default)]
    pub spring: Option<SpringConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Keyframe {
    pub time: f64,
    pub value: KeyframeValue,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EasingType {
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
    Spring,
}

fn default_easing() -> EasingType {
    EasingType::EaseOut
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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
}

impl Default for PresetConfig {
    fn default() -> Self {
        Self {
            delay: 0.0,
            duration: 0.8,
            repeat: false,
        }
    }
}

fn default_preset_duration() -> f64 {
    0.8
}
