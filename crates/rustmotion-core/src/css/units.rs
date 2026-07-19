//! CSS length/percentage units.
//!
//! Supports: `px`, `%`, `em`, `rem`, `vw`, `vh`, `fr`, `auto`, plus a bare
//! number (interpreted as `px`).
//!
//! Resolution to absolute pixels happens through [`LengthContext`], which
//! carries the viewport dimensions, parent size (for `%`), and font sizes
//! (for `em` / `rem`).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A pure CSS length, no percentage allowed (e.g. `font-size`, `box-shadow`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Length {
    /// Bare number, treated as pixels.
    Px(f32),
    /// String form: `"24px"`, `"1.5em"`, `"100vw"`, `"0"`.
    String(String),
}

impl Default for Length {
    fn default() -> Self {
        Length::Px(0.0)
    }
}

/// A CSS length OR percentage (e.g. `width`, `padding`, `top`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum LengthPercentage {
    Px(f32),
    String(String),
}

impl Default for LengthPercentage {
    fn default() -> Self {
        LengthPercentage::Px(0.0)
    }
}

/// Internal parsed representation. Once parsed we know which unit it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParsedLength {
    Px(f32),
    Percent(f32),
    Em(f32),
    Rem(f32),
    Vw(f32),
    Vh(f32),
    Fr(f32),
    Auto,
}

#[derive(Debug, Clone, Copy)]
pub struct LengthContext {
    pub viewport_width: f32,
    pub viewport_height: f32,
    pub parent_size: f32,
    pub font_size: f32,
    pub root_font_size: f32,
}

impl Default for LengthContext {
    fn default() -> Self {
        Self {
            viewport_width: 1920.0,
            viewport_height: 1080.0,
            parent_size: 0.0,
            font_size: 16.0,
            root_font_size: 16.0,
        }
    }
}

impl ParsedLength {
    /// Resolve to absolute pixels. Returns `None` for `Auto` / `Fr` (caller decides).
    pub fn resolve(&self, ctx: &LengthContext) -> Option<f32> {
        match self {
            Self::Px(v) => Some(*v),
            Self::Percent(v) => Some(*v / 100.0 * ctx.parent_size),
            Self::Em(v) => Some(*v * ctx.font_size),
            Self::Rem(v) => Some(*v * ctx.root_font_size),
            Self::Vw(v) => Some(*v / 100.0 * ctx.viewport_width),
            Self::Vh(v) => Some(*v / 100.0 * ctx.viewport_height),
            Self::Fr(_) => None,
            Self::Auto => None,
        }
    }
}

/// Parse a CSS length/percentage string that may also contain transform-origin
/// axis keywords (`left`, `center`, `right`, `top`, `bottom`).
///
/// Keywords are normalised to `ParsedLength::Percent` so that the regular
/// `resolve` machinery handles them — the caller must still choose the correct
/// axis dimension (width for x, height for y) in the `LengthContext`.
pub fn parse_origin_component(s: &str) -> Option<ParsedLength> {
    let lower = s.trim().to_ascii_lowercase();
    match lower.as_str() {
        "left" | "top" => return Some(ParsedLength::Percent(0.0)),
        "center" => return Some(ParsedLength::Percent(50.0)),
        "right" | "bottom" => return Some(ParsedLength::Percent(100.0)),
        _ => {}
    }
    parse_length(s)
}

/// Parse a CSS length/percentage string. Whitespace tolerated.
pub fn parse_length(s: &str) -> Option<ParsedLength> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("auto") {
        return Some(ParsedLength::Auto);
    }
    if let Ok(n) = s.parse::<f32>() {
        return Some(ParsedLength::Px(n));
    }
    let suffixes: &[(&str, fn(f32) -> ParsedLength)] = &[
        ("px", ParsedLength::Px),
        ("%", ParsedLength::Percent),
        ("em", ParsedLength::Em),
        ("rem", ParsedLength::Rem),
        ("vw", ParsedLength::Vw),
        ("vh", ParsedLength::Vh),
        ("fr", ParsedLength::Fr),
    ];
    for (suf, ctor) in suffixes {
        if let Some(num_part) = s.strip_suffix(suf) {
            if let Ok(n) = num_part.trim().parse::<f32>() {
                return Some(ctor(n));
            }
        }
    }
    None
}

impl Length {
    pub fn parse(&self) -> ParsedLength {
        match self {
            Length::Px(v) => ParsedLength::Px(*v),
            Length::String(s) => parse_length(s).unwrap_or(ParsedLength::Px(0.0)),
        }
    }

    pub fn resolve(&self, ctx: &LengthContext) -> f32 {
        self.parse().resolve(ctx).unwrap_or(0.0)
    }

    /// Quick px resolution without context (treats em/rem/% as 0). Used by
    /// painters that only need the px value of an explicit length.
    pub fn px(&self) -> f32 {
        match self.parse() {
            ParsedLength::Px(v) => v,
            _ => 0.0,
        }
    }
}

impl LengthPercentage {
    pub fn parse(&self) -> ParsedLength {
        match self {
            LengthPercentage::Px(v) => ParsedLength::Px(*v),
            LengthPercentage::String(s) => parse_length(s).unwrap_or(ParsedLength::Px(0.0)),
        }
    }

    pub fn resolve(&self, ctx: &LengthContext) -> f32 {
        self.parse().resolve(ctx).unwrap_or(0.0)
    }

    /// Quick px resolution without context (treats em/rem/% as 0). Used by
    /// painters that only need the px value of an explicit length.
    pub fn px(&self) -> f32 {
        match self.parse() {
            ParsedLength::Px(v) => v,
            _ => 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_px_with_unit() {
        assert_eq!(parse_length("24px"), Some(ParsedLength::Px(24.0)));
        assert_eq!(parse_length("  24 px"), Some(ParsedLength::Px(24.0)));
    }

    #[test]
    fn parse_percent() {
        assert_eq!(parse_length("50%"), Some(ParsedLength::Percent(50.0)));
    }

    #[test]
    fn parse_em_rem() {
        assert_eq!(parse_length("1.5em"), Some(ParsedLength::Em(1.5)));
        assert_eq!(parse_length("2rem"), Some(ParsedLength::Rem(2.0)));
    }

    #[test]
    fn parse_vw_vh() {
        assert_eq!(parse_length("100vw"), Some(ParsedLength::Vw(100.0)));
        assert_eq!(parse_length("50vh"), Some(ParsedLength::Vh(50.0)));
    }

    #[test]
    fn parse_fr() {
        assert_eq!(parse_length("1fr"), Some(ParsedLength::Fr(1.0)));
        assert_eq!(parse_length("2.5fr"), Some(ParsedLength::Fr(2.5)));
    }

    #[test]
    fn parse_bare_number_is_px() {
        assert_eq!(parse_length("16"), Some(ParsedLength::Px(16.0)));
    }

    #[test]
    fn parse_auto() {
        assert_eq!(parse_length("auto"), Some(ParsedLength::Auto));
        assert_eq!(parse_length("AUTO"), Some(ParsedLength::Auto));
    }

    #[test]
    fn parse_invalid() {
        assert_eq!(parse_length("foo"), None);
        assert_eq!(parse_length(""), None);
    }

    #[test]
    fn resolve_percent_against_parent() {
        let ctx = LengthContext {
            parent_size: 200.0,
            ..Default::default()
        };
        let p = ParsedLength::Percent(50.0);
        assert_eq!(p.resolve(&ctx), Some(100.0));
    }

    #[test]
    fn resolve_em_uses_font_size() {
        let ctx = LengthContext {
            font_size: 20.0,
            ..Default::default()
        };
        assert_eq!(ParsedLength::Em(1.5).resolve(&ctx), Some(30.0));
    }

    #[test]
    fn resolve_vw_uses_viewport() {
        let ctx = LengthContext {
            viewport_width: 1000.0,
            ..Default::default()
        };
        assert_eq!(ParsedLength::Vw(50.0).resolve(&ctx), Some(500.0));
    }

    #[test]
    fn length_struct_resolves_string() {
        let l = Length::String("24px".into());
        assert_eq!(l.resolve(&LengthContext::default()), 24.0);
    }

    #[test]
    fn length_struct_resolves_bare_px() {
        let l = Length::Px(10.0);
        assert_eq!(l.resolve(&LengthContext::default()), 10.0);
    }
}
