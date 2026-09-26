use std::collections::HashMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::schema::time::{TimeCtx, TimeError, TimePoint};

use super::voices::Voice;

/// One entry of `score`: trigger `voice` once at `at`, or repeatedly every
/// `every` between `from` (default: the scenario's start) and `to`
/// (default: the scenario's end), phase-shifted by `offset`. Exactly one
/// of `at`/`every` must be given — [`Score::resolve_hits`] rejects an
/// event with neither.
///
/// `at`/`from`/`to` are **instants**: resolved with the scenario's real
/// `bpm`/`beat_offset`, exactly like [`crate::schema::Scene::at`]. `every`/
/// `offset` are **durations**: also [`TimePoint`]s, resolved through the
/// same [`TimeCtx`] machinery (the frozen contract — nothing here reshapes
/// `TimePoint`/`TimeCtx`), but against a context whose `beat_offset` is
/// forced to zero first (see [`resolve_duration`]'s doc) — `beat_offset` is
/// a *phase*, and must not multiply into how long one beat lasts.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScoreEvent {
    /// Key into [`Score::voices`].
    pub voice: String,
    #[serde(default)]
    pub at: Option<TimePoint>,
    #[serde(default)]
    pub every: Option<TimePoint>,
    #[serde(default)]
    pub from: Option<TimePoint>,
    #[serde(default)]
    pub to: Option<TimePoint>,
    #[serde(default)]
    pub offset: Option<TimePoint>,
    /// Per-event gain override, applied on top of the voice's own `gain`.
    /// Linear, not dB (unlike [`CompressorConfig::threshold`] and
    /// [`MasterBus::gain`]) — matching [`Voice::gain`], which this
    /// multiplies against.
    #[serde(default)]
    pub gain: Option<f32>,
}

fn default_compressor_attack_ms() -> f32 {
    5.0
}

fn default_compressor_release_ms() -> f32 {
    50.0
}

/// `master.compressor` — the issue's `{"threshold": -14, "ratio": 4}`.
/// `threshold` is in dB, `ratio` is the `N` of an `N:1` ratio.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompressorConfig {
    pub threshold: f32,
    pub ratio: f32,
    #[serde(default = "default_compressor_attack_ms")]
    pub attack: f32,
    #[serde(default = "default_compressor_release_ms")]
    pub release: f32,
}

fn default_limiter_on() -> bool {
    true
}

/// The score's master bus: an optional gain trim (dB), an optional
/// compressor, and the limiter — deliverable #4/#5, **on by default**
/// (`limiter` defaults to `true`; omitting `master` entirely still applies
/// it). See [`super::dsp::apply_limiter`]'s doc for why it is a hard
/// guarantee, not a best-effort setting.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MasterBus {
    #[serde(default)]
    pub gain: Option<f32>,
    #[serde(default)]
    pub compressor: Option<CompressorConfig>,
    #[serde(default = "default_limiter_on")]
    pub limiter: bool,
}

impl Default for MasterBus {
    fn default() -> Self {
        MasterBus {
            gain: None,
            compressor: None,
            limiter: true,
        }
    }
}

/// The full synth block — `Scenario::audio`'s object form
/// (`schema::scenario::AudioConfig`) carries one of these directly. See
/// the issue's JSON example: `voices` + `score` + `master`, with `bpm`/
/// `beat_offset` living one level up on `AudioConfig` (and, when absent
/// there, on the scenario itself — see `AudioConfig::as_score`'s caller in
/// `rustmotion`'s `encode` crate for exactly how that fallback works).
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Score {
    #[serde(default)]
    pub voices: HashMap<String, Voice>,
    #[serde(default)]
    pub score: Vec<ScoreEvent>,
    #[serde(default)]
    pub master: MasterBus,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ScoreError {
    #[error("score event references unknown voice '{0}' (not declared in `voices`)")]
    UnknownVoice(String),
    #[error("score event for voice '{voice}' has neither 'at' nor 'every' — one is required")]
    MissingTiming { voice: String },
    #[error(
        "score event for voice '{voice}': 'every' resolves to a non-positive interval ({interval}s)"
    )]
    NonPositiveInterval { voice: String, interval: f64 },
    #[error(
        "score event for voice '{voice}' would produce more than {limit} hits — \
         check 'every'/'from'/'to' for a runaway repeat"
    )]
    TooManyHits { voice: String, limit: usize },
    #[error("score event for voice '{voice}': {source}")]
    Time {
        voice: String,
        #[source]
        source: TimeError,
    },
}

const MAX_HITS_PER_EVENT: usize = 100_000;

impl Score {
    pub fn is_empty(&self) -> bool {
        self.voices.is_empty() && self.score.is_empty()
    }

    pub fn resolve_hits(
        &self,
        ctx: &TimeCtx,
        scenario_duration: f64,
    ) -> Result<HashMap<String, Vec<f64>>, ScoreError> {
        let mut hits: HashMap<String, Vec<f64>> = HashMap::new();
        for event in &self.score {
            if !self.voices.contains_key(&event.voice) {
                return Err(ScoreError::UnknownVoice(event.voice.clone()));
            }
            let times = resolve_event_hits(event, ctx, scenario_duration)?;
            hits.entry(event.voice.clone()).or_default().extend(times);
        }
        Ok(hits)
    }
}

fn resolve_instant(tp: &TimePoint, ctx: &TimeCtx, voice: &str) -> Result<f64, ScoreError> {
    tp.resolve_relative(ctx).map_err(|source| ScoreError::Time {
        voice: voice.to_string(),
        source,
    })
}

fn resolve_duration(tp: &TimePoint, ctx: &TimeCtx, voice: &str) -> Result<f64, ScoreError> {
    let duration_ctx = TimeCtx {
        beat_offset: 0.0,
        ..*ctx
    };
    tp.resolve_relative(&duration_ctx)
        .map_err(|source| ScoreError::Time {
            voice: voice.to_string(),
            source,
        })
}

fn resolve_event_hits(
    event: &ScoreEvent,
    ctx: &TimeCtx,
    scenario_duration: f64,
) -> Result<Vec<f64>, ScoreError> {
    if let Some(at) = &event.at {
        let t = resolve_instant(at, ctx, &event.voice)?;
        return Ok(vec![t]);
    }

    let Some(every) = &event.every else {
        return Err(ScoreError::MissingTiming {
            voice: event.voice.clone(),
        });
    };
    let interval = resolve_duration(every, ctx, &event.voice)?;
    if interval <= 0.0 {
        return Err(ScoreError::NonPositiveInterval {
            voice: event.voice.clone(),
            interval,
        });
    }

    let from = match &event.from {
        Some(tp) => resolve_instant(tp, ctx, &event.voice)?,
        None => 0.0,
    };
    let offset = match &event.offset {
        Some(tp) => resolve_duration(tp, ctx, &event.voice)?,
        None => 0.0,
    };
    let to = match &event.to {
        Some(tp) => resolve_instant(tp, ctx, &event.voice)?,
        None => scenario_duration,
    };

    let start = from + offset;
    let mut hits = Vec::new();
    let mut t = start;
    while t <= to + 1e-9 {
        if t >= 0.0 {
            hits.push(t);
        }
        if hits.len() > MAX_HITS_PER_EVENT {
            return Err(ScoreError::TooManyHits {
                voice: event.voice.clone(),
                limit: MAX_HITS_PER_EVENT,
            });
        }
        t += interval;
    }
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(bpm: Option<f64>, beat_offset: f64) -> TimeCtx {
        TimeCtx {
            bpm,
            beat_offset,
            scene_start: 0.0,
        }
    }

    fn tp(s: &str) -> TimePoint {
        TimePoint::Spec(s.to_string())
    }

    #[test]
    fn every_1b_is_one_beat_regardless_of_beat_offset() {
        let c = ctx(Some(120.0), 2.2);
        let d = resolve_duration(&tp("1b"), &c, "kick").unwrap();
        assert!(
            (d - 0.5).abs() < 1e-9,
            "expected exactly one beat (0.5s), got {d}"
        );
    }

    #[test]
    fn issues_two_voice_example_kick_lands_on_every_beat_from_2_2s_to_12_6s() {
        let c = ctx(Some(115.0), 0.0);
        let event = ScoreEvent {
            voice: "kick".to_string(),
            at: None,
            every: Some(tp("1b")),
            from: Some(tp("@2.2s")),
            to: Some(tp("@12.6s")),
            offset: None,
            gain: None,
        };
        let hits = resolve_event_hits(&event, &c, 999.0).unwrap();
        let beat = 60.0 / 115.0;
        assert!((hits[0] - 2.2).abs() < 1e-9);
        assert!((hits[1] - (2.2 + beat)).abs() < 1e-9);
        assert!(*hits.last().unwrap() <= 12.6 + 1e-6);
        assert!(*hits.last().unwrap() > 12.6 - beat);
    }

    #[test]
    fn hat_with_no_to_plays_until_the_given_scenario_duration() {
        let c = ctx(Some(115.0), 0.0);
        let event = ScoreEvent {
            voice: "hat".to_string(),
            at: None,
            every: Some(tp("1b")),
            from: Some(tp("@2.2s")),
            to: None,
            offset: Some(tp("0.5b")),
            gain: None,
        };
        let hits = resolve_event_hits(&event, &c, 10.0).unwrap();
        let beat = 60.0 / 115.0;
        assert!((hits[0] - (2.2 + 0.5 * beat)).abs() < 1e-9);
        assert!(*hits.last().unwrap() <= 10.0 + 1e-6);
    }

    #[test]
    fn event_with_neither_at_nor_every_is_a_named_error() {
        let c = ctx(Some(115.0), 0.0);
        let event = ScoreEvent {
            voice: "kick".to_string(),
            at: None,
            every: None,
            from: None,
            to: None,
            offset: None,
            gain: None,
        };
        assert_eq!(
            resolve_event_hits(&event, &c, 10.0),
            Err(ScoreError::MissingTiming {
                voice: "kick".to_string()
            })
        );
    }

    #[test]
    fn unknown_voice_reference_is_a_named_error() {
        let score = Score {
            voices: HashMap::new(),
            score: vec![ScoreEvent {
                voice: "ghost".to_string(),
                at: Some(tp("1s")),
                every: None,
                from: None,
                to: None,
                offset: None,
                gain: None,
            }],
            master: MasterBus::default(),
        };
        let c = ctx(None, 0.0);
        assert_eq!(
            score.resolve_hits(&c, 10.0),
            Err(ScoreError::UnknownVoice("ghost".to_string()))
        );
    }

    #[test]
    fn runaway_interval_is_capped_not_infinite() {
        let c = ctx(None, 0.0);
        let event = ScoreEvent {
            voice: "kick".to_string(),
            at: None,
            every: Some(TimePoint::Seconds(0.0001)),
            from: None,
            to: None,
            offset: None,
            gain: None,
        };
        let err = resolve_event_hits(&event, &c, 100.0).unwrap_err();
        assert!(matches!(err, ScoreError::TooManyHits { .. }));
    }

    #[test]
    fn master_defaults_to_limiter_on_with_no_compressor() {
        let m = MasterBus::default();
        assert!(m.limiter);
        assert!(m.compressor.is_none());
    }

    #[test]
    fn score_deserializes_from_the_issues_json_shape() {
        let json = serde_json::json!({
            "voices": {
                "kick": { "type": "sine", "freq": [150, 42], "sweep": 0.14, "decay": 0.38 },
                "hat":  { "type": "noise", "filter": {"type":"highpass","freq":7500}, "decay": 0.05 }
            },
            "score":  [
                { "voice": "kick", "every": "1b", "from": "@2.2s", "to": "@12.6s" },
                { "voice": "hat",  "every": "1b", "offset": "0.5b", "from": "@2.2s" }
            ],
            "master": { "compressor": { "threshold": -14, "ratio": 4 }, "limiter": true }
        });
        let score: Score = serde_json::from_value(json).expect("issue's score shape must parse");
        assert_eq!(score.voices.len(), 2);
        assert_eq!(score.score.len(), 2);
        assert!(score.master.limiter);
        assert_eq!(score.master.compressor.unwrap().ratio, 4.0);
    }
}
