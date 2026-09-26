use std::collections::BTreeMap;

use crate::engine::animator::ease;
use crate::schema::animation::EasingType;
use crate::schema::time::{TimeCtx, TimeError};

use super::schema::{VarDef, VarSet};

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum VarsError {
    #[error("variable `{name}`: {source}")]
    Time {
        name: String,
        #[source]
        source: TimeError,
    },
    #[error(
        "variable `{name}`: keyframe {index} starts at {at}s, which is not after keyframe {prev_index}'s start ({prev_at}s) — keyframes must be strictly increasing"
    )]
    KeyframesNotIncreasing {
        name: String,
        index: usize,
        at: f64,
        prev_index: usize,
        prev_at: f64,
    },
    #[error("variable `{name}`: keyframe {index} has a negative duration ({duration}s)")]
    NegativeDuration {
        name: String,
        index: usize,
        duration: f64,
    },
}

#[derive(Debug, Clone, PartialEq)]
struct Segment {
    at: f64,
    duration: f64,
    start: f64,
    to: f64,
    easing: EasingType,
}

impl Segment {
    fn value_at(&self, t: f64) -> f64 {
        if self.duration <= 0.0 {
            return self.to;
        }
        let raw = ((t - self.at) / self.duration).clamp(0.0, 1.0);
        let eased = ease(raw, &self.easing);
        self.start + (self.to - self.start) * eased
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct VarTrack {
    default: f64,
    segments: Vec<Segment>,
}

impl VarTrack {
    pub fn compile(name: &str, def: &VarDef, ctx: &TimeCtx) -> Result<Self, VarsError> {
        let mut segments = Vec::with_capacity(def.animation.len());
        let mut running = def.default;
        let mut prev: Option<(usize, f64)> = None;

        for (index, kf) in def.animation.iter().enumerate() {
            let at_ctx = TimeCtx {
                scene_start: 0.0,
                ..*ctx
            };
            let at = kf
                .at
                .resolve_absolute(&at_ctx)
                .map_err(|source| VarsError::Time {
                    name: name.to_string(),
                    source,
                })?;

            if let Some((prev_index, prev_at)) = prev {
                if at <= prev_at {
                    return Err(VarsError::KeyframesNotIncreasing {
                        name: name.to_string(),
                        index,
                        at,
                        prev_index,
                        prev_at,
                    });
                }
            }
            prev = Some((index, at));

            let duration_ctx = TimeCtx {
                beat_offset: 0.0,
                scene_start: 0.0,
                ..*ctx
            };
            let duration = kf
                .duration
                .resolve_relative(&duration_ctx)
                .map_err(|source| VarsError::Time {
                    name: name.to_string(),
                    source,
                })?;
            if duration < 0.0 {
                return Err(VarsError::NegativeDuration {
                    name: name.to_string(),
                    index,
                    duration,
                });
            }

            segments.push(Segment {
                at,
                duration,
                start: running,
                to: kf.to,
                easing: kf.easing.clone(),
            });
            running = kf.to;
        }

        Ok(VarTrack {
            default: def.default,
            segments,
        })
    }

    pub fn value_at(&self, t: f64) -> f64 {
        let mut value = self.default;
        for seg in &self.segments {
            if t < seg.at {
                return value;
            }
            let end = seg.at + seg.duration;
            if t <= end {
                return seg.value_at(t);
            }
            value = seg.to;
        }
        value
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct VarTable {
    tracks: BTreeMap<String, VarTrack>,
}

impl VarTable {
    pub fn compile(vars: &VarSet, ctx: &TimeCtx) -> Result<Self, VarsError> {
        let mut tracks = BTreeMap::new();
        for (name, def) in vars {
            tracks.insert(name.clone(), VarTrack::compile(name, def, ctx)?);
        }
        Ok(VarTable { tracks })
    }

    pub fn value_at(&self, name: &str, t: f64) -> Option<f64> {
        self.tracks.get(name).map(|track| track.value_at(t))
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.tracks.keys().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::time::TimePoint;
    use crate::vars::schema::VarKeyframe;

    fn ctx(bpm: Option<f64>, beat_offset: f64) -> TimeCtx {
        TimeCtx {
            bpm,
            beat_offset,
            scene_start: 0.0,
        }
    }

    fn linear_0_to_1_over_3_beats() -> VarDef {
        VarDef {
            default: 0.0,
            animation: vec![VarKeyframe {
                at: TimePoint::Spec("@4.85s".to_string()),
                to: 1.0,
                duration: TimePoint::Spec("3b".to_string()),
                easing: EasingType::Linear,
            }],
        }
    }

    #[test]
    fn value_before_first_keyframe_is_default() {
        let def = linear_0_to_1_over_3_beats();
        let track = VarTrack::compile("keyDraw", &def, &ctx(Some(120.0), 0.0)).unwrap();
        assert_eq!(track.value_at(0.0), 0.0);
        assert_eq!(track.value_at(4.84), 0.0);
    }

    #[test]
    fn value_at_several_sampled_instants_across_the_tween() {
        let def = linear_0_to_1_over_3_beats();
        let track = VarTrack::compile("keyDraw", &def, &ctx(Some(120.0), 0.0)).unwrap();

        assert_eq!(track.value_at(4.85), 0.0);
        assert!((track.value_at(5.6) - 0.5).abs() < 1e-9);
        assert_eq!(track.value_at(6.35), 1.0);
    }

    #[test]
    fn value_after_last_keyframe_holds_at_to() {
        let def = linear_0_to_1_over_3_beats();
        let track = VarTrack::compile("keyDraw", &def, &ctx(Some(120.0), 0.0)).unwrap();
        assert_eq!(track.value_at(6.36), 1.0);
        assert_eq!(track.value_at(1000.0), 1.0);
    }

    #[test]
    fn beat_offset_does_not_leak_into_duration() {
        let def = linear_0_to_1_over_3_beats();
        let track = VarTrack::compile("keyDraw", &def, &ctx(Some(120.0), 2.2)).unwrap();
        assert_eq!(track.value_at(4.85), 0.0);
        assert_eq!(track.value_at(6.35), 1.0);
    }

    #[test]
    fn multiple_keyframes_chain_start_values() {
        let def = VarDef {
            default: 0.0,
            animation: vec![
                VarKeyframe {
                    at: TimePoint::Seconds(1.0),
                    to: 10.0,
                    duration: TimePoint::Seconds(1.0),
                    easing: EasingType::Linear,
                },
                VarKeyframe {
                    at: TimePoint::Seconds(3.0),
                    to: 0.0,
                    duration: TimePoint::Seconds(1.0),
                    easing: EasingType::Linear,
                },
            ],
        };
        let track = VarTrack::compile("v", &def, &ctx(None, 0.0)).unwrap();
        assert_eq!(track.value_at(0.0), 0.0);
        assert_eq!(track.value_at(1.0), 0.0);
        assert_eq!(track.value_at(1.5), 5.0);
        assert_eq!(track.value_at(2.0), 10.0);
        assert_eq!(track.value_at(2.5), 10.0);
        assert_eq!(track.value_at(3.0), 10.0);
        assert_eq!(track.value_at(3.5), 5.0);
        assert_eq!(track.value_at(4.0), 0.0);
        assert_eq!(track.value_at(100.0), 0.0);
    }

    #[test]
    fn zero_duration_keyframe_snaps() {
        let def = VarDef {
            default: 0.0,
            animation: vec![VarKeyframe {
                at: TimePoint::Seconds(1.0),
                to: 1.0,
                duration: TimePoint::Seconds(0.0),
                easing: EasingType::Linear,
            }],
        };
        let track = VarTrack::compile("v", &def, &ctx(None, 0.0)).unwrap();
        assert_eq!(track.value_at(0.999), 0.0);
        assert_eq!(track.value_at(1.0), 1.0);
        assert_eq!(track.value_at(2.0), 1.0);
    }

    #[test]
    fn ease_in_out_is_not_linear_at_the_midpoint() {
        let def = VarDef {
            default: 0.0,
            animation: vec![VarKeyframe {
                at: TimePoint::Seconds(0.0),
                to: 1.0,
                duration: TimePoint::Seconds(1.0),
                easing: EasingType::EaseInOut,
            }],
        };
        let track = VarTrack::compile("v", &def, &ctx(None, 0.0)).unwrap();
        let quarter = track.value_at(0.25);
        assert!(
            quarter < 0.25,
            "expected eased value below linear, got {quarter}"
        );
        assert!((track.value_at(0.5) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn beat_unit_without_bpm_errors_with_the_variable_name() {
        let def = linear_0_to_1_over_3_beats();
        let err = VarTrack::compile("keyDraw", &def, &ctx(None, 0.0)).unwrap_err();
        assert!(matches!(err, VarsError::Time { name, .. } if name == "keyDraw"));
    }

    #[test]
    fn non_increasing_keyframes_are_rejected() {
        let def = VarDef {
            default: 0.0,
            animation: vec![
                VarKeyframe {
                    at: TimePoint::Seconds(2.0),
                    to: 1.0,
                    duration: TimePoint::Seconds(1.0),
                    easing: EasingType::Linear,
                },
                VarKeyframe {
                    at: TimePoint::Seconds(2.0),
                    to: 0.0,
                    duration: TimePoint::Seconds(1.0),
                    easing: EasingType::Linear,
                },
            ],
        };
        let err = VarTrack::compile("v", &def, &ctx(None, 0.0)).unwrap_err();
        assert!(matches!(
            err,
            VarsError::KeyframesNotIncreasing { index: 1, .. }
        ));
    }

    #[test]
    fn negative_duration_is_rejected() {
        let def = VarDef {
            default: 0.0,
            animation: vec![VarKeyframe {
                at: TimePoint::Spec("@2s-3s".to_string()),
                to: 1.0,
                duration: TimePoint::Spec("1s-3s".to_string()),
                easing: EasingType::Linear,
            }],
        };
        let err = VarTrack::compile("v", &def, &ctx(None, 0.0)).unwrap_err();
        assert!(matches!(err, VarsError::NegativeDuration { .. }));
    }

    #[test]
    fn var_table_compiles_and_resolves_by_name() {
        let mut vars = VarSet::new();
        vars.insert("keyDraw".to_string(), linear_0_to_1_over_3_beats());
        vars.insert(
            "constant".to_string(),
            VarDef {
                default: 42.0,
                animation: Vec::new(),
            },
        );

        let table = VarTable::compile(&vars, &ctx(Some(120.0), 0.0)).unwrap();
        assert_eq!(table.value_at("constant", 0.0), Some(42.0));
        assert_eq!(table.value_at("constant", 999.0), Some(42.0));
        assert_eq!(table.value_at("keyDraw", 0.0), Some(0.0));
        assert_eq!(table.value_at("keyDraw", 1000.0), Some(1.0));
        assert_eq!(table.value_at("nope", 0.0), None);

        let mut names: Vec<&str> = table.names().collect();
        names.sort_unstable();
        assert_eq!(names, vec!["constant", "keyDraw"]);
    }
}
