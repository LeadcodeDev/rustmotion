use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::schema::animation::EasingType;
use crate::schema::time::TimePoint;

pub type VarSet = BTreeMap<String, VarDef>;

/// A single declared variable: the value it holds before any keyframe
/// fires (and its whole value, forever, if `animation` is empty — see
/// [`VarDef::is_static`]), plus an ordered list of keyframes that tween it
/// across the scenario's absolute timeline.
///
/// ```json
/// "keyDraw": { "default": 0,
///   "animation": [{ "at": "@4.85s", "to": 1, "duration": "3b", "easing": "ease_in_out" }] }
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VarDef {
    /// This variable's value before its first keyframe fires, and its only
    /// value if `animation` is empty.
    pub default: f64,
    /// Ordered tweens on the scenario's absolute timeline. Must be
    /// strictly increasing by resolved `at` — see
    /// [`super::track::VarTrack::compile`].
    #[serde(default)]
    pub animation: Vec<VarKeyframe>,
}

impl VarDef {
    pub fn is_static(&self) -> bool {
        self.animation.is_empty()
    }
}

/// One tween: hold at the running value until `at`, then ease to `to` over
/// `duration`, then hold at `to` until the next keyframe fires (or
/// forever, for the last one). See [`super::track`]'s module doc, "Value
/// outside the keyframes", for the exact before-first/after-last rule.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct VarKeyframe {
    /// Where this segment starts, on the scenario's **absolute** timeline.
    /// Resolved via [`TimePoint::resolve_absolute`] with `scene_start`
    /// pinned to `0.0` — a variable has no enclosing scene of its own, so
    /// the leading `@` this grammar otherwise requires for an absolute
    /// reading is accepted but never changes anything here: `"4b"` and
    /// `"@4b"` resolve identically for a [`VarKeyframe`].
    pub at: TimePoint,
    /// The value this segment eases towards.
    pub to: f64,
    /// How long the ease from the running value to `to` takes.
    ///
    /// **Not resolved the way you'd expect from reading [`TimePoint`]'s own
    /// contract.** `TimePoint`'s `"b"` unit is defined as a position on the
    /// beat grid — `beat_offset + n * 60 / bpm`, `beat_offset` included, by
    /// design, because every other `TimePoint` field in this schema (`at`,
    /// `Scene::at`, a shake impact's `at`, …) names a *point in time*, where
    /// picking up the grid's anchor is exactly right. `duration` is the one
    /// exception: it names a *span* — "3 beats long" — and a span must not
    /// shift just because the grid happens to start somewhere other than
    /// zero. Resolving `"3b"` through [`TimePoint::resolve_relative`]
    /// unchanged would compute `beat_offset + 3 * 60 / bpm`, silently
    /// stretching every tween by the scenario's own `beat_offset` on top of
    /// its declared length. [`super::track::VarTrack::compile`] resolves
    /// this field against a copy of the scenario's `TimeCtx` with
    /// `beat_offset` zeroed instead, so `"3b"` always means exactly
    /// `3 * 60 / bpm` seconds — see that function's doc, and
    /// [`super::track`]'s module doc, for the full reasoning and a test
    /// (`beat_offset_does_not_leak_into_duration`) pinning it down.
    ///
    /// Defaults to an instant snap (`0` seconds) when omitted.
    #[serde(default = "default_duration")]
    pub duration: TimePoint,
    /// Defaults to [`EasingType::Linear`] — the same default
    /// [`EasingType`]'s own `#[default]` picks, deliberately *not*
    /// `schema::animation::Animation`'s node-animation default
    /// (`EaseOut`): a variable has no established "usual feel" the way an
    /// entrance animation does, so an un-set easing should be the
    /// arithmetically neutral choice.
    #[serde(default)]
    pub easing: EasingType,
}

fn default_duration() -> TimePoint {
    TimePoint::Seconds(0.0)
}

pub fn dynamic_names(vars: &VarSet) -> impl Iterator<Item = &str> {
    vars.iter()
        .filter(|(_, def)| !def.is_static())
        .map(|(name, _)| name.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn var_def_without_animation_is_static() {
        let def = VarDef {
            default: 42.0,
            animation: Vec::new(),
        };
        assert!(def.is_static());
    }

    #[test]
    fn var_def_with_animation_is_not_static() {
        let def = VarDef {
            default: 0.0,
            animation: vec![VarKeyframe {
                at: TimePoint::Spec("@1s".to_string()),
                to: 1.0,
                duration: TimePoint::Seconds(1.0),
                easing: EasingType::Linear,
            }],
        };
        assert!(!def.is_static());
    }

    #[test]
    fn dynamic_names_only_reports_animated_variables() {
        let mut vars = VarSet::new();
        vars.insert(
            "constant".to_string(),
            VarDef {
                default: 1.0,
                animation: Vec::new(),
            },
        );
        vars.insert(
            "moving".to_string(),
            VarDef {
                default: 0.0,
                animation: vec![VarKeyframe {
                    at: TimePoint::Seconds(0.0),
                    to: 1.0,
                    duration: TimePoint::Seconds(1.0),
                    easing: EasingType::Linear,
                }],
            },
        );

        let names: Vec<&str> = dynamic_names(&vars).collect();
        assert_eq!(names, vec!["moving"]);
    }

    #[test]
    fn keyframe_deserializes_from_the_issue_example() {
        let json = r#"{
            "default": 0,
            "animation": [{ "at": "@4.85s", "to": 1, "duration": "3b", "easing": "ease_in_out" }]
        }"#;
        let def: VarDef = serde_json::from_str(json).unwrap();
        assert_eq!(def.default, 0.0);
        assert_eq!(def.animation.len(), 1);
        assert_eq!(def.animation[0].at, TimePoint::Spec("@4.85s".to_string()));
        assert_eq!(def.animation[0].to, 1.0);
        assert_eq!(def.animation[0].duration, TimePoint::Spec("3b".to_string()));
        assert_eq!(def.animation[0].easing, EasingType::EaseInOut);
    }

    #[test]
    fn keyframe_duration_defaults_to_an_instant_snap() {
        let json = r#"{ "default": 0, "animation": [{ "at": "1s", "to": 1 }] }"#;
        let def: VarDef = serde_json::from_str(json).unwrap();
        assert_eq!(def.animation[0].duration, TimePoint::Seconds(0.0));
        assert_eq!(def.animation[0].easing, EasingType::Linear);
    }

    #[test]
    fn unknown_field_is_rejected() {
        let json = r#"{ "default": 0, "typo": 1 }"#;
        assert!(serde_json::from_str::<VarDef>(json).is_err());
    }
}
