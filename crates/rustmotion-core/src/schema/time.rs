use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeCtx {
    pub bpm: Option<f64>,
    pub beat_offset: f64,
    pub scene_start: f64,
}

impl Default for TimeCtx {
    fn default() -> Self {
        TimeCtx {
            bpm: None,
            beat_offset: 0.0,
            scene_start: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum TimeError {
    #[error("beat unit in `{0}` but the scenario declares no bpm")]
    NoBpm(String),
    #[error("cannot parse time `{0}`")]
    Unparseable(String),
}

/// A point in time: a bare JSON number of seconds, or a small string
/// expression anchored to the beat grid.
///
/// # Grammar (for the [`Spec`](TimePoint::Spec) string form)
///
/// An optional leading `@` (forces the value onto the scenario's absolute
/// timeline — see the module doc), followed by one or more terms joined by
/// `+` or `-`. Each term is `<number><unit>`, where `unit` is one of:
///
/// - `s` — seconds, taken as-is.
/// - `ms` — milliseconds, divided by 1000.
/// - `b` — beats: `beat_offset + n * 60 / bpm` (errors with
///   [`TimeError::NoBpm`] when the scenario declares no `bpm`). A term's
///   sign multiplies this *whole* grid position, `beat_offset` included —
///   so `"@2.2s-1b"` is "2.2s before wherever beat 1 lands," not "1 raw
///   beat length before 2.2s."
///
/// Valid examples: `"2.5s"`, `"8b"`, `"120ms"`, `"8b+120ms"`, `"@8b"`,
/// `"@2.2s-1b"`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum TimePoint {
    /// A bare JSON number: seconds, always relative to whatever context
    /// resolves it (see [`TimePoint::resolve_relative`]).
    Seconds(f64),
    /// The string expression form — see the grammar above.
    Spec(String),
}

impl TimePoint {
    pub fn resolve_relative(&self, ctx: &TimeCtx) -> Result<f64, TimeError> {
        self.eval_sum(ctx)
    }

    pub fn resolve_absolute(&self, ctx: &TimeCtx) -> Result<f64, TimeError> {
        let value = self.eval_sum(ctx)?;
        if self.is_absolute() {
            Ok(value)
        } else {
            Ok(ctx.scene_start + value)
        }
    }

    pub fn is_absolute(&self) -> bool {
        match self {
            TimePoint::Seconds(_) => false,
            TimePoint::Spec(spec) => spec.starts_with('@'),
        }
    }

    fn eval_sum(&self, ctx: &TimeCtx) -> Result<f64, TimeError> {
        match self {
            TimePoint::Seconds(seconds) => Ok(*seconds),
            TimePoint::Spec(spec) => eval_spec(spec, ctx),
        }
    }

    pub fn validate_grammar(&self) -> Result<(), TimeError> {
        match self {
            TimePoint::Seconds(_) => Ok(()),
            TimePoint::Spec(_) => {
                let placeholder = TimeCtx {
                    bpm: Some(1.0),
                    beat_offset: 0.0,
                    scene_start: 0.0,
                };
                match self.eval_sum(&placeholder) {
                    Ok(_) => Ok(()),
                    Err(TimeError::NoBpm(_)) => {
                        unreachable!("the placeholder context always supplies a bpm")
                    }
                    Err(e @ TimeError::Unparseable(_)) => Err(e),
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimeUnit {
    Seconds,
    Millis,
    Beats,
}

fn split_signed_terms(body: &str) -> Vec<(f64, String)> {
    let mut terms = Vec::new();
    let mut sign = 1.0;
    let mut current = String::new();
    for (i, ch) in body.char_indices() {
        match ch {
            '+' if i == 0 => {}
            '-' if i == 0 => sign = -1.0,
            '+' | '-' if i != 0 => {
                terms.push((sign, std::mem::take(&mut current)));
                sign = if ch == '-' { -1.0 } else { 1.0 };
            }
            _ => current.push(ch),
        }
    }
    terms.push((sign, current));
    terms
}

fn split_unit(term: &str) -> Option<(&str, TimeUnit)> {
    if let Some(number) = term.strip_suffix("ms") {
        Some((number, TimeUnit::Millis))
    } else if let Some(number) = term.strip_suffix('s') {
        Some((number, TimeUnit::Seconds))
    } else if let Some(number) = term.strip_suffix('b') {
        Some((number, TimeUnit::Beats))
    } else {
        None
    }
}

fn eval_spec(original: &str, ctx: &TimeCtx) -> Result<f64, TimeError> {
    let body = original.strip_prefix('@').unwrap_or(original);
    if body.is_empty() {
        return Err(TimeError::Unparseable(original.to_string()));
    }

    let mut total = 0.0;
    for (sign, term) in split_signed_terms(body) {
        let (number_str, unit) =
            split_unit(&term).ok_or_else(|| TimeError::Unparseable(original.to_string()))?;
        if number_str.is_empty() {
            return Err(TimeError::Unparseable(original.to_string()));
        }
        let number: f64 = number_str
            .parse()
            .map_err(|_| TimeError::Unparseable(original.to_string()))?;

        let term_value = match unit {
            TimeUnit::Seconds => number,
            TimeUnit::Millis => number / 1000.0,
            TimeUnit::Beats => {
                let bpm = ctx
                    .bpm
                    .ok_or_else(|| TimeError::NoBpm(original.to_string()))?;
                ctx.beat_offset + number * 60.0 / bpm
            }
        };
        total += sign * term_value;
    }

    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(bpm: Option<f64>, beat_offset: f64, scene_start: f64) -> TimeCtx {
        TimeCtx {
            bpm,
            beat_offset,
            scene_start,
        }
    }

    #[test]
    fn bare_number_deserializes_as_seconds() {
        let tp: TimePoint = serde_json::from_str("2.5").unwrap();
        assert_eq!(tp, TimePoint::Seconds(2.5));
    }

    #[test]
    fn string_deserializes_as_spec() {
        let tp: TimePoint = serde_json::from_str("\"2.5s\"").unwrap();
        assert_eq!(tp, TimePoint::Spec("2.5s".to_string()));
    }

    #[test]
    fn seconds_spec_resolves_plainly() {
        let tp = TimePoint::Spec("2.5s".to_string());
        let c = ctx(None, 0.0, 10.0);
        assert_eq!(tp.resolve_relative(&c).unwrap(), 2.5);
        assert_eq!(tp.resolve_absolute(&c).unwrap(), 12.5);
    }

    #[test]
    fn millis_spec_resolves() {
        let tp = TimePoint::Spec("120ms".to_string());
        let c = ctx(None, 0.0, 0.0);
        assert_eq!(tp.resolve_relative(&c).unwrap(), 0.12);
    }

    #[test]
    fn beat_spec_resolves_with_beat_offset() {
        let tp = TimePoint::Spec("8b".to_string());
        let c = ctx(Some(120.0), 2.2, 0.0);
        let expected = 2.2 + 8.0 * 60.0 / 120.0;
        assert!((tp.resolve_relative(&c).unwrap() - expected).abs() < 1e-9);
    }

    #[test]
    fn compound_beat_plus_millis_spec_resolves() {
        let tp = TimePoint::Spec("8b+120ms".to_string());
        let c = ctx(Some(120.0), 0.0, 0.0);
        assert!((tp.resolve_relative(&c).unwrap() - 4.12).abs() < 1e-9);
    }

    #[test]
    fn at_prefix_is_absolute_and_ignores_scene_start() {
        let tp = TimePoint::Spec("@8b".to_string());
        assert!(tp.is_absolute());
        let c = ctx(Some(100.0), 2.2, 1000.0);
        let expected = 2.2 + 8.0 * 60.0 / 100.0;
        assert!((tp.resolve_relative(&c).unwrap() - expected).abs() < 1e-9);
        assert!((tp.resolve_absolute(&c).unwrap() - expected).abs() < 1e-9);
    }

    #[test]
    fn at_prefix_with_subtracted_beat_term() {
        let tp = TimePoint::Spec("@2.2s-1b".to_string());
        let c = ctx(Some(120.0), 0.0, 0.0);
        assert!((tp.resolve_absolute(&c).unwrap() - 1.7).abs() < 1e-9);
    }

    #[test]
    fn plain_seconds_variant_is_never_absolute() {
        let tp = TimePoint::Seconds(3.0);
        assert!(!tp.is_absolute());
        let c = ctx(None, 0.0, 5.0);
        assert_eq!(tp.resolve_relative(&c).unwrap(), 3.0);
        assert_eq!(tp.resolve_absolute(&c).unwrap(), 8.0);
    }

    #[test]
    fn beat_unit_without_bpm_errors() {
        let tp = TimePoint::Spec("8b".to_string());
        let c = ctx(None, 0.0, 0.0);
        assert_eq!(
            tp.resolve_relative(&c),
            Err(TimeError::NoBpm("8b".to_string()))
        );
    }

    #[test]
    fn beat_unit_without_bpm_errors_even_nested_in_a_sum() {
        let tp = TimePoint::Spec("120ms+8b".to_string());
        let c = ctx(None, 0.0, 0.0);
        assert_eq!(
            tp.resolve_relative(&c),
            Err(TimeError::NoBpm("120ms+8b".to_string()))
        );
    }

    #[test]
    fn malformed_spec_is_unparseable() {
        for bad in ["", "abc", "5xyz", "s5", "5", "+", "-", "5s+"] {
            let tp = TimePoint::Spec(bad.to_string());
            let c = ctx(Some(120.0), 0.0, 0.0);
            assert_eq!(
                tp.resolve_relative(&c),
                Err(TimeError::Unparseable(bad.to_string())),
                "expected `{bad}` to be unparseable"
            );
        }
    }

    #[test]
    fn error_display_matches_the_frozen_messages() {
        let no_bpm = TimeError::NoBpm("8b".to_string());
        assert_eq!(
            no_bpm.to_string(),
            "beat unit in `8b` but the scenario declares no bpm"
        );
        let unparseable = TimeError::Unparseable("bogus".to_string());
        assert_eq!(unparseable.to_string(), "cannot parse time `bogus`");
    }

    #[test]
    fn validate_grammar_accepts_every_frozen_example_with_no_bpm_in_scope() {
        for good in ["2.5s", "8b", "120ms", "8b+120ms", "@8b", "@2.2s-1b"] {
            let tp = TimePoint::Spec(good.to_string());
            assert_eq!(
                tp.validate_grammar(),
                Ok(()),
                "`{good}` must pass grammar validation without needing bpm"
            );
        }
        assert_eq!(TimePoint::Seconds(3.0).validate_grammar(), Ok(()));
    }

    #[test]
    fn validate_grammar_rejects_the_same_strings_resolution_would() {
        for bad in ["", "abc", "5xyz", "s5", "5", "+", "-", "5s+"] {
            let tp = TimePoint::Spec(bad.to_string());
            assert_eq!(
                tp.validate_grammar(),
                Err(TimeError::Unparseable(bad.to_string())),
                "expected `{bad}` to fail grammar validation"
            );
        }
    }

    #[test]
    fn validate_grammar_never_reports_no_bpm() {
        let tp = TimePoint::Spec("@8b".to_string());
        assert_eq!(tp.validate_grammar(), Ok(()));
    }
}
