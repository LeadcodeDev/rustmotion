//! Declarative camera shake (issue #330): a list of beat-synced impacts
//! instead of a hand-generated camera keyframe track — the reel this
//! feature was built for spent 155 sampled keyframes (a Python loop
//! evaluating a damped sine, pasted into the JSON) on what six `{at,
//! amplitude}` pairs now express directly.
//!
//! See [`SceneShake`] for the damped-oscillation formula and
//! `crate::engine::shake::shake_offset` for the function that evaluates it
//! at a given time.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::time::TimePoint;

/// One beat-synced hit that kicks the camera. `at` resolves the same way
/// as any other in-scene [`TimePoint`] — relative to the scene's own start
/// unless it carries the `@` prefix (see the anchoring rule documented
/// once on [`TimePoint`]) — so an impact can land exactly on a cut:
/// `{"at": "4b", "amplitude": 24.0}`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ShakeImpact {
    /// When this impact lands, on the scene's own timeline.
    pub at: TimePoint,
    /// Peak displacement at `t = at`, in pixels, before decay.
    pub amplitude: f64,
}

/// Declarative camera shake: every impact in [`SceneShake::impacts`] is an
/// independent damped harmonic oscillator, summed together. **Additive**
/// over whatever [`super::scenario::Scene::camera`] already resolves to —
/// `camera.keyframes` is a single value-over-time series per property,
/// with no way to layer a second signal onto it without replacing the
/// first, so a shake needs its own field to coexist with a pan instead of
/// fighting over one keyframe track.
///
/// # Formula
///
/// For an impact of `amplitude` `A` landing at `t0`, its contribution at
/// `t >= t0` (exactly zero before `t0` — an impact never affects time
/// before it lands) is:
///
/// ```text
/// Δt        = t - t0
/// envelope  = A * exp(-decay * Δt)
/// phase     = 2π * frequency * Δt
/// x         = envelope * cos(phase)
/// y         = envelope * sin(phase)
/// rotation  = shake.rotation * x            (degrees)
/// ```
///
/// `x` and `y` are 90° out of phase, so the combined offset traces a
/// decaying spiral rather than a straight line back and forth — this is
/// what reads as a *shake* instead of a *bounce*. `decay` (1/seconds) is
/// the exponential rate: the envelope reaches `1/e` (~37%) of its peak
/// after `1/decay` seconds. `frequency` (Hz) is how many oscillations per
/// second it makes while decaying. `rotation` is a degrees-per-pixel
/// coefficient applied to the already-computed `x` offset — not an
/// independent oscillator — so the twist always stays in phase with the
/// translation instead of drifting against it; `0.0` (the default)
/// disables rotational shake entirely.
///
/// Multiple impacts are summed — not replaced — at every instant, so a
/// fast retrigger before the previous impact has decayed accumulates
/// rather than resetting: the same physical intuition as striking a bell
/// twice in quick succession.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct SceneShake {
    /// Every hit contributing to this scene's shake. Impacts are summed,
    /// not last-wins — see [`SceneShake`]'s formula.
    pub impacts: Vec<ShakeImpact>,
    /// Exponential decay rate, in 1/seconds. Higher decays faster: the
    /// envelope reaches `1/e` of its peak after `1/decay` seconds.
    /// Default: 10.0 (~0.1s to 1/e, fully read as settled well within half
    /// a second).
    #[serde(default = "default_shake_decay")]
    pub decay: f64,
    /// Oscillations per second while the envelope decays. Default: 20.0.
    #[serde(default = "default_shake_frequency")]
    pub frequency: f64,
    /// Degrees of rotational shake per pixel of the (already-computed) `x`
    /// offset — see [`SceneShake`]'s formula. `0.0` (default) disables
    /// rotational shake.
    #[serde(default)]
    pub rotation: f64,
}

fn default_shake_decay() -> f64 {
    10.0
}

fn default_shake_frequency() -> f64 {
    20.0
}

#[cfg(test)]
mod schema_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserializes_with_every_known_field() {
        let json = json!({
            "impacts": [
                { "at": "4b", "amplitude": 24.0 },
                { "at": 1.5, "amplitude": 12.0 }
            ],
            "decay": 8.0,
            "frequency": 18.0,
            "rotation": 0.5
        });
        let shake: SceneShake = serde_json::from_value(json).unwrap();
        assert_eq!(shake.impacts.len(), 2);
        assert_eq!(shake.impacts[0].at, TimePoint::Spec("4b".to_string()));
        assert_eq!(shake.impacts[0].amplitude, 24.0);
        assert_eq!(shake.impacts[1].at, TimePoint::Seconds(1.5));
        assert_eq!(shake.decay, 8.0);
        assert_eq!(shake.frequency, 18.0);
        assert_eq!(shake.rotation, 0.5);
    }

    #[test]
    fn decay_frequency_and_rotation_default() {
        let json = json!({ "impacts": [{ "at": 0.0, "amplitude": 10.0 }] });
        let shake: SceneShake = serde_json::from_value(json).unwrap();
        assert_eq!(shake.decay, default_shake_decay());
        assert_eq!(shake.frequency, default_shake_frequency());
        assert_eq!(shake.rotation, 0.0);
    }

    #[test]
    fn a_typo_on_scene_shake_is_rejected() {
        let json = json!({ "impacts": [], "decy": 8.0 });
        let err = serde_json::from_value::<SceneShake>(json)
            .expect_err("a typo'd field on SceneShake must be rejected, not silently ignored");
        assert!(err.to_string().contains("decy"), "got: {err}");
    }

    #[test]
    fn a_typo_on_a_shake_impact_is_rejected() {
        let json = json!({ "impacts": [{ "at": 0.0, "amplitud": 10.0 }] });
        let err = serde_json::from_value::<SceneShake>(json)
            .expect_err("a typo'd field on ShakeImpact must be rejected, not silently ignored");
        assert!(err.to_string().contains("amplitud"), "got: {err}");
    }

    #[test]
    fn amplitude_is_required_on_every_impact() {
        let json = json!({ "impacts": [{ "at": 0.0 }] });
        assert!(serde_json::from_value::<SceneShake>(json).is_err());
    }

    #[test]
    fn six_impacts_round_trip_through_json() {
        let impacts: Vec<_> = (0..6)
            .map(|i| json!({ "at": format!("{}b", i * 4), "amplitude": 20.0 - i as f64 * 2.0 }))
            .collect();
        let json = json!({ "impacts": impacts });
        let shake: SceneShake = serde_json::from_value(json).unwrap();
        assert_eq!(shake.impacts.len(), 6);
        let back = serde_json::to_value(&shake).unwrap();
        let round_tripped: SceneShake = serde_json::from_value(back).unwrap();
        assert_eq!(shake, round_tripped);
    }
}
