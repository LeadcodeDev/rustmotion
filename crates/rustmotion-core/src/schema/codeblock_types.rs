use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::animation::EasingType;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CodeblockChrome {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CodeblockHighlight {
    pub lines: Vec<u32>,
    #[serde(default = "default_highlight_color")]
    pub color: String,
    #[serde(default)]
    pub start: Option<f64>,
    #[serde(default)]
    pub end: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CodeblockReveal {
    pub mode: RevealMode,
    #[serde(default)]
    pub start: f64,
    #[serde(default = "default_reveal_duration")]
    pub duration: f64,
    #[serde(default)]
    pub easing: EasingType,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RevealMode {
    Typewriter,
    LineByLine,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CodeblockState {
    pub code: String,
    pub at: f64,
    #[serde(default = "default_state_duration")]
    pub duration: f64,
    #[serde(default = "default_state_easing")]
    pub easing: EasingType,
    #[serde(default)]
    pub cursor: Option<CodeblockCursor>,
    #[serde(default)]
    pub highlights: Option<Vec<CodeblockHighlight>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CodeblockCursor {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_cursor_color")]
    pub color: String,
    #[serde(default = "default_cursor_width")]
    pub width: f32,
    #[serde(default = "default_true")]
    pub blink: bool,
}

fn default_true() -> bool {
    true
}

fn default_highlight_color() -> String {
    "#FFFF0033".to_string()
}

fn default_reveal_duration() -> f64 {
    1.0
}

fn default_state_duration() -> f64 {
    0.6
}

fn default_state_easing() -> EasingType {
    EasingType::EaseInOut
}

fn default_cursor_color() -> String {
    "#FFFFFF".to_string()
}

fn default_cursor_width() -> f32 {
    2.0
}

#[cfg(test)]
mod unknown_field_tests {
    use super::*;

    #[test]
    fn chrome_typo_d_key_is_a_named_error() {
        let err = serde_json::from_value::<CodeblockChrome>(serde_json::json!({
            "titel": "main.rs"
        }))
        .expect_err("a typo'd `titel` must not silently default `title` to None");
        assert!(err.to_string().contains("titel"), "got: {err}");
    }

    #[test]
    fn reveal_typo_d_key_is_a_named_error() {
        let err = serde_json::from_value::<CodeblockReveal>(serde_json::json!({
            "mode": "typewriter",
            "duratoin": 2.0
        }))
        .expect_err("a typo'd `duratoin` must not silently keep the default duration");
        assert!(err.to_string().contains("duratoin"), "got: {err}");
    }

    #[test]
    fn highlight_typo_d_key_is_a_named_error() {
        let err = serde_json::from_value::<CodeblockHighlight>(serde_json::json!({
            "lines": [1, 2],
            "colour": "#ff0000"
        }))
        .expect_err("a typo'd `colour` must not silently keep the default highlight color");
        assert!(err.to_string().contains("colour"), "got: {err}");
    }

    #[test]
    fn state_typo_d_key_is_a_named_error() {
        let err = serde_json::from_value::<CodeblockState>(serde_json::json!({
            "code": "fn main() {}",
            "at": 1.0,
            "duraton": 0.6
        }))
        .expect_err("a typo'd `duraton` must not silently keep the default state duration");
        assert!(err.to_string().contains("duraton"), "got: {err}");
    }

    #[test]
    fn cursor_typo_d_key_is_a_named_error() {
        let err = serde_json::from_value::<CodeblockCursor>(serde_json::json!({
            "colour": "#ffffff"
        }))
        .expect_err("a typo'd `colour` must not silently keep the default cursor color");
        assert!(err.to_string().contains("colour"), "got: {err}");
    }

    #[test]
    fn well_formed_configs_still_parse() {
        let chrome: CodeblockChrome =
            serde_json::from_value(serde_json::json!({ "title": "main.rs" })).unwrap();
        assert_eq!(chrome.title.as_deref(), Some("main.rs"));

        let reveal: CodeblockReveal = serde_json::from_value(serde_json::json!({
            "mode": "typewriter",
            "duration": 2.0
        }))
        .unwrap();
        assert_eq!(reveal.duration, 2.0);
    }
}
