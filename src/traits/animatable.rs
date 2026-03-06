use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::schema::{Animation, AnimationPreset, PresetConfig, WiggleConfig};

/// Configuration for animation, presets, wiggle, and motion blur.
/// Embedded via `#[serde(flatten)]` in components that support animation.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AnimationConfig {
    #[serde(default)]
    pub animations: Vec<Animation>,
    #[serde(default)]
    pub preset: Option<AnimationPreset>,
    #[serde(default)]
    pub preset_config: Option<PresetConfig>,
    #[serde(default)]
    pub wiggle: Option<Vec<WiggleConfig>>,
    #[serde(default)]
    pub motion_blur: Option<f32>,
}

impl Default for AnimationConfig {
    fn default() -> Self {
        Self {
            animations: Vec::new(),
            preset: None,
            preset_config: None,
            wiggle: None,
            motion_blur: None,
        }
    }
}

/// Trait for components that support animation.
pub trait Animatable {
    fn animation_config(&self) -> &AnimationConfig;
}
