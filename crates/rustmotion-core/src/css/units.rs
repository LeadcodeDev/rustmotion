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

    /// True for units whose absolute pixel value depends on a
    /// [`LengthContext`] (`%`, `em`, `rem`, `vw`, `vh`) — i.e. everything
    /// [`Length::px`] / [`LengthPercentage::px`] cannot resolve on their own
    /// and silently (issue #125 §2) or loudly (post-fix) fall back to `0.0`
    /// for. `Px` needs no context; `Auto`/`Fr` are not scalar lengths at all
    /// (the caller decides what they mean) so they are not "relative" in
    /// this sense.
    pub fn is_relative(&self) -> bool {
        matches!(
            self,
            Self::Percent(_) | Self::Em(_) | Self::Rem(_) | Self::Vw(_) | Self::Vh(_)
        )
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
    /// Parse into a resolved unit. Falls back to `Px(0.0)` on unparseable
    /// input, for the many existing call sites across the codebase that
    /// expect an infallible result — but unlike the previous behaviour,
    /// this now logs a warning so the fallback is a detectable signal
    /// instead of a silent, indistinguishable zero. Callers that can
    /// surface the failure themselves (e.g. a validator) should prefer
    /// [`Length::try_parse`], which returns `None` instead of guessing.
    pub fn parse(&self) -> ParsedLength {
        match self {
            Length::Px(v) => ParsedLength::Px(*v),
            Length::String(s) => parse_length_or_warn(s),
        }
    }

    /// Fallible counterpart of [`Length::parse`]: `None` on unparseable
    /// input rather than a silent `Px(0.0)`, and never logs. Prefer this
    /// when the failure can be reported to the caller (e.g. schema
    /// validation) instead of swallowed.
    pub fn try_parse(&self) -> Option<ParsedLength> {
        match self {
            Length::Px(v) => Some(ParsedLength::Px(*v)),
            Length::String(s) => parse_length(s),
        }
    }

    pub fn resolve(&self, ctx: &LengthContext) -> f32 {
        self.parse().resolve(ctx).unwrap_or(0.0)
    }

    /// Quick px resolution without context (treats em/rem/%/vw/vh as 0). Used
    /// by painters that only need the px value of an explicit length and
    /// have no [`LengthContext`] available (see [`CssStyle::font_size_px_or`]
    /// / `letter_spacing_px` / `line_height_for` in `css::style`, all built
    /// on this — issue #125 §2).
    ///
    /// A relative unit here cannot be resolved correctly no matter what:
    /// `em`/`rem` need a font-size base, `vw`/`vh` need the real viewport,
    /// `%` needs a parent box — none of which this context-free accessor
    /// has. Rather than silently returning `0.0` indistinguishably from a
    /// deliberate `0px` (the previous behaviour — the "15.6vw renders a
    /// completely black frame with no signal" bug), this now warns loudly
    /// when the dropped value was a relative unit, same fail-loud contract
    /// as [`parse_length_or_warn`] uses for genuinely unparseable input.
    /// Callers with a real `LengthContext` should call [`Length::resolve`]
    /// instead, which resolves these correctly.
    pub fn px(&self) -> f32 {
        px_or_warn(self.parse())
    }
}

impl LengthPercentage {
    /// See [`Length::parse`] — same infallible-with-a-warning contract.
    pub fn parse(&self) -> ParsedLength {
        match self {
            LengthPercentage::Px(v) => ParsedLength::Px(*v),
            LengthPercentage::String(s) => parse_length_or_warn(s),
        }
    }

    /// See [`Length::try_parse`] — fallible, silent, `None` on failure.
    pub fn try_parse(&self) -> Option<ParsedLength> {
        match self {
            LengthPercentage::Px(v) => Some(ParsedLength::Px(*v)),
            LengthPercentage::String(s) => parse_length(s),
        }
    }

    pub fn resolve(&self, ctx: &LengthContext) -> f32 {
        self.parse().resolve(ctx).unwrap_or(0.0)
    }

    /// Quick px resolution without context (treats em/rem/%/vw/vh as 0). See
    /// [`Length::px`] — same fail-loud contract.
    pub fn px(&self) -> f32 {
        px_or_warn(self.parse())
    }
}

/// Shared by `Length::px` / `LengthPercentage::px`: return the pixel value
/// for `Px`, or `0.0` for anything else — but if the anything-else is a
/// *relative* unit (issue #125 §2: `%`/`em`/`rem`/`vw`/`vh`), warn loudly
/// first, since silently returning `0.0` here is indistinguishable from a
/// deliberate `0px` and was the exact defect reported (`font-size: "15.6vw"`
/// rendering a fully black frame with `render` exiting 0). `Auto`/`Fr`
/// aren't scalar lengths (the caller decides what they mean), so they don't
/// warn — a context-free accessor asking for their "px value" is a caller
/// bug, not a units bug, and predates this fix.
fn px_or_warn(parsed: ParsedLength) -> f32 {
    match parsed {
        ParsedLength::Px(v) => v,
        other => {
            if other.is_relative() {
                eprintln!(
                    "Warning: relative unit {other:?} used where only an absolute px value is \
                     supported (no LengthContext available here) — resolving to 0px instead of \
                     the intended value. Use an absolute px length, or resolve through \
                     LengthContext::resolve() / CssStyle's *_ctx accessors where a context is \
                     available."
                );
            }
            0.0
        }
    }
}

/// Shared fallback used by both `Length::parse` and `LengthPercentage::parse`:
/// parse `s`, or log a warning and return `Px(0.0)` if it doesn't match any
/// known length form. This is what closes the "silently fold into 0px with
/// no signal" gap — the previous `unwrap_or(ParsedLength::Px(0.0))` produced
/// an indistinguishable, unreported zero for a typo'd unit exactly like a
/// deliberate `0px`.
fn parse_length_or_warn(s: &str) -> ParsedLength {
    match parse_length(s) {
        Some(p) => p,
        None => {
            eprintln!(
                "Warning: unparseable length '{s}' — falling back to 0px instead of silently \
                 resolving with no signal"
            );
            ParsedLength::Px(0.0)
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

    // ---- unparseable length: detectable signal, not a silent 0px ----

    #[test]
    fn length_try_parse_returns_none_for_garbage() {
        let l = Length::String("not-a-length".into());
        assert_eq!(l.try_parse(), None);
    }

    #[test]
    fn length_percentage_try_parse_returns_none_for_garbage() {
        let l = LengthPercentage::String("wat".into());
        assert_eq!(l.try_parse(), None);
    }

    #[test]
    fn length_try_parse_agrees_with_parse_on_valid_input() {
        let l = Length::String("24px".into());
        assert_eq!(l.try_parse(), Some(ParsedLength::Px(24.0)));
        assert_eq!(l.parse(), ParsedLength::Px(24.0));
    }

    #[test]
    fn length_parse_still_infallible_fallback_for_garbage() {
        // `.parse()` keeps its infallible signature (existing call sites
        // across the codebase depend on it) — it still resolves to 0px for
        // unparseable input, but `try_parse` above proves the failure is
        // now independently detectable rather than indistinguishable from
        // a deliberate `0px`.
        let l = Length::String("not-a-length".into());
        assert_eq!(l.parse(), ParsedLength::Px(0.0));
        assert_eq!(l.resolve(&LengthContext::default()), 0.0);
    }

    #[test]
    fn length_percentage_parse_still_infallible_fallback_for_garbage() {
        let l = LengthPercentage::String("wat".into());
        assert_eq!(l.parse(), ParsedLength::Px(0.0));
    }

    // ---- issue #125 §2: relative units on the context-free `.px()` path ----

    #[test]
    fn is_relative_classifies_every_variant() {
        assert!(!ParsedLength::Px(1.0).is_relative());
        assert!(ParsedLength::Percent(1.0).is_relative());
        assert!(ParsedLength::Em(1.0).is_relative());
        assert!(ParsedLength::Rem(1.0).is_relative());
        assert!(ParsedLength::Vw(1.0).is_relative());
        assert!(ParsedLength::Vh(1.0).is_relative());
        assert!(!ParsedLength::Fr(1.0).is_relative());
        assert!(!ParsedLength::Auto.is_relative());
    }

    #[test]
    fn px_still_resolves_absolute_px_correctly() {
        assert_eq!(Length::String("24px".into()).px(), 24.0);
        assert_eq!(Length::Px(10.0).px(), 10.0);
        assert_eq!(LengthPercentage::String("24px".into()).px(), 24.0);
    }

    /// `.px()` cannot correctly resolve relative units (no context is
    /// available at this call site) — this locks in that it still falls
    /// back to `0.0` for them post-fix, same numeric behaviour as before.
    /// What changed is that this fallback is now loud (see
    /// `px_or_warn`/`is_relative`): distinguishing "the value really is
    /// relative, and got dropped" from "the value was genuinely `0px`" is
    /// exactly what `is_relative` (tested above) exists to let a caller —
    /// or the `px_or_warn` eprintln — detect, since asserting on stderr
    /// text isn't practical from a unit test.
    #[test]
    fn px_falls_back_to_zero_for_every_relative_unit() {
        for s in ["50%", "1.5em", "2rem", "100vw", "50vh"] {
            assert_eq!(Length::String(s.into()).px(), 0.0, "Length::px() for {s:?}");
            assert_eq!(
                LengthPercentage::String(s.into()).px(),
                0.0,
                "LengthPercentage::px() for {s:?}"
            );
            // And `is_relative` on the same parsed value proves *why*: a
            // caller (or px_or_warn) can tell this apart from a real 0px.
            assert!(parse_length(s).unwrap().is_relative());
        }
    }

    #[test]
    fn px_does_not_warn_for_auto_or_fr_context_free_use() {
        // Auto/Fr aren't scalar lengths — a context-free `.px()` call on
        // them is a caller bug predating this fix, not a relative-unit
        // silent-zero. They fall back to 0.0 too, but `is_relative` is
        // false for both so `px_or_warn` does not treat them as the
        // issue #125 §2 case.
        assert!(!ParsedLength::Auto.is_relative());
        assert!(!ParsedLength::Fr(2.0).is_relative());
        assert_eq!(Length::String("auto".into()).px(), 0.0);
    }
}
