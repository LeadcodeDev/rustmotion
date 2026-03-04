use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Configuration for timed visibility (start_at / end_at).
/// Embedded via `#[serde(flatten)]` in components that support timed visibility.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct TimingConfig {
    #[serde(default)]
    pub start_at: Option<f64>,
    #[serde(default)]
    pub end_at: Option<f64>,
}

/// Trait for components that support timed visibility.
pub trait Timed {
    fn timing(&self) -> (Option<f64>, Option<f64>);
}
