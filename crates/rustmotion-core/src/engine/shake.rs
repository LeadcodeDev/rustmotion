//! Evaluates [`crate::schema::shake::SceneShake`] at a given scene-local
//! time (issue #330). See that type's own doc for the damped-oscillation
//! formula this module implements verbatim; nothing here should ever
//! diverge from it — if the two disagree, the schema doc is the spec and
//! this file has a bug.
//!
//! [`shake_offset`] is a pure function: it knows nothing about
//! [`crate::schema::scenario::Camera`] or how a resolved camera transform
//! gets built. The offset it returns is meant to be added to whatever the
//! camera already resolved to independently — see [`SceneShake`]'s own doc
//! for why "additive" rather than "instead of".

use crate::schema::shake::SceneShake;
use crate::schema::time::{TimeCtx, TimeError};

/// The camera's shake contribution at a scene-local `time`, in the same
/// units [`crate::schema::scenario::Camera`]'s own `x`/`y` (pixels) and
/// `rotation` (degrees) use. Additive: a caller adds this to the camera's
/// independently-resolved values rather than replacing them.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ShakeOffset {
    pub x: f64,
    pub y: f64,
    pub rotation: f64,
}

impl ShakeOffset {
    fn plus(self, other: ShakeOffset) -> ShakeOffset {
        ShakeOffset {
            x: self.x + other.x,
            y: self.y + other.y,
            rotation: self.rotation + other.rotation,
        }
    }
}

/// Evaluates every impact in `shake.impacts` at `time` (scene-local
/// seconds — the same clock `PaintCtx::time` uses, not
/// `PaintCtx::scenario_time`) and sums their contributions. Each impact's
/// own `at` is a [`crate::schema::time::TimePoint`], resolved against `ctx`
/// via [`crate::schema::time::TimePoint::resolve_relative`] — the same
/// "relative to the scene's own start unless `@`-prefixed" rule every
/// other in-scene time follows.
///
/// See [`SceneShake`]'s doc for the formula. Returns [`TimeError`] only
/// when an impact's `at` fails to resolve (e.g. a `b`-unit term with no
/// `bpm` declared) — a grammatically malformed `at` is caught earlier, at
/// scenario deserialization, by the same mechanism every other
/// [`crate::schema::time::TimePoint`] uses.
pub fn shake_offset(
    shake: &SceneShake,
    ctx: &TimeCtx,
    time: f64,
) -> Result<ShakeOffset, TimeError> {
    let mut total = ShakeOffset::default();
    for impact in &shake.impacts {
        let at = impact.at.resolve_relative(ctx)?;
        let dt = time - at;
        if dt < 0.0 {
            continue;
        }
        let envelope = impact.amplitude * (-shake.decay * dt).exp();
        let phase = std::f64::consts::TAU * shake.frequency * dt;
        let x = envelope * phase.cos();
        let y = envelope * phase.sin();
        total = total.plus(ShakeOffset {
            x,
            y,
            rotation: shake.rotation * x,
        });
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::shake::ShakeImpact;
    use crate::schema::time::TimePoint;

    fn ctx() -> TimeCtx {
        TimeCtx {
            bpm: Some(120.0),
            beat_offset: 0.0,
            scene_start: 0.0,
        }
    }

    fn single_impact(
        at: f64,
        amplitude: f64,
        decay: f64,
        frequency: f64,
        rotation: f64,
    ) -> SceneShake {
        SceneShake {
            impacts: vec![ShakeImpact {
                at: TimePoint::Seconds(at),
                amplitude,
            }],
            decay,
            frequency,
            rotation,
        }
    }

    #[test]
    fn an_impact_contributes_nothing_before_it_lands() {
        let shake = single_impact(1.0, 20.0, 10.0, 20.0, 0.0);
        for t in [0.0, 0.5, 0.999] {
            let offset = shake_offset(&shake, &ctx(), t).unwrap();
            assert_eq!(offset, ShakeOffset::default(), "t={t} is before at=1.0");
        }
    }

    #[test]
    fn an_impact_peaks_at_its_own_at_along_x_with_zero_y() {
        let shake = single_impact(1.0, 20.0, 10.0, 20.0, 0.0);
        let offset = shake_offset(&shake, &ctx(), 1.0).unwrap();
        assert!((offset.x - 20.0).abs() < 1e-9, "x={}", offset.x);
        assert!(offset.y.abs() < 1e-9, "y={}", offset.y);
    }

    #[test]
    fn the_envelope_decays_to_roughly_one_over_e_after_one_over_decay_seconds() {
        let decay = 10.0;
        let shake = single_impact(0.0, 20.0, decay, 0.0, 0.0); // frequency=0 -> x is the raw envelope
        let offset = shake_offset(&shake, &ctx(), 1.0 / decay).unwrap();
        let expected = 20.0 / std::f64::consts::E;
        assert!(
            (offset.x - expected).abs() < 1e-6,
            "x={}, expected {expected}",
            offset.x
        );
    }

    #[test]
    fn multiple_impacts_sum_linearly() {
        let a = single_impact(0.0, 10.0, 8.0, 15.0, 0.0);
        let b = single_impact(0.3, 6.0, 8.0, 15.0, 0.0);
        let combined = SceneShake {
            impacts: [a.impacts.clone(), b.impacts.clone()].concat(),
            decay: 8.0,
            frequency: 15.0,
            rotation: 0.0,
        };
        for t in [0.0, 0.15, 0.3, 0.5, 1.0] {
            let oa = shake_offset(&a, &ctx(), t).unwrap();
            let ob = shake_offset(&b, &ctx(), t).unwrap();
            let oc = shake_offset(&combined, &ctx(), t).unwrap();
            assert!((oc.x - (oa.x + ob.x)).abs() < 1e-9, "t={t}: x mismatch");
            assert!((oc.y - (oa.y + ob.y)).abs() < 1e-9, "t={t}: y mismatch");
        }
    }

    #[test]
    fn rotation_is_proportional_to_x_via_the_documented_coefficient() {
        let coeff = 0.5;
        let shake = single_impact(0.0, 20.0, 8.0, 15.0, coeff);
        for t in [0.0, 0.05, 0.2, 0.5] {
            let offset = shake_offset(&shake, &ctx(), t).unwrap();
            assert!(
                (offset.rotation - coeff * offset.x).abs() < 1e-9,
                "t={t}: rotation={}, x={}",
                offset.rotation,
                offset.x
            );
        }
    }

    #[test]
    fn beat_grid_impacts_resolve_through_bpm_and_beat_offset() {
        // bpm=120 -> 0.5s/beat. Impact at beat 2 lands at t=1.0s.
        let shake = SceneShake {
            impacts: vec![ShakeImpact {
                at: TimePoint::Spec("2b".to_string()),
                amplitude: 15.0,
            }],
            decay: 10.0,
            frequency: 20.0,
            rotation: 0.0,
        };
        let before = shake_offset(&shake, &ctx(), 0.99).unwrap();
        assert_eq!(before, ShakeOffset::default());
        let at_impact = shake_offset(&shake, &ctx(), 1.0).unwrap();
        assert!((at_impact.x - 15.0).abs() < 1e-6);
    }

    #[test]
    fn a_beat_unit_impact_with_no_bpm_errors_instead_of_silently_resolving() {
        let shake = SceneShake {
            impacts: vec![ShakeImpact {
                at: TimePoint::Spec("2b".to_string()),
                amplitude: 15.0,
            }],
            decay: 10.0,
            frequency: 20.0,
            rotation: 0.0,
        };
        let no_bpm_ctx = TimeCtx {
            bpm: None,
            beat_offset: 0.0,
            scene_start: 0.0,
        };
        assert!(shake_offset(&shake, &no_bpm_ctx, 1.0).is_err());
    }

    /// Six declarative impacts (as the acceptance criterion describes)
    /// reproduce the same damped-sine shape the old workflow got from
    /// sampling this exact formula 155 times into a hand-pasted camera
    /// keyframe track. Since the original reel's Python loop isn't
    /// available to diff against, this test is the strongest available
    /// proof: it independently re-derives the formula from `SceneShake`'s
    /// own doc comment (not by calling into this module's
    /// implementation) and checks `shake_offset` agrees at 155 sampled
    /// points across the shake's span — i.e. that a hand-rolled,
    /// per-frame keyframe track built the way the old workflow built one
    /// would be indistinguishable from what six `{at, amplitude}` pairs
    /// now produce directly.
    #[test]
    fn six_impacts_reproduce_a_hand_sampled_155_keyframe_track() {
        let decay = 12.0;
        let frequency = 18.0;
        let rotation = 0.4;
        let impacts_at_amp = [
            (0.0, 24.0),
            (0.5, 20.0),
            (1.0, 20.0),
            (1.5, 16.0),
            (2.0, 16.0),
            (2.5, 12.0),
        ];
        let shake = SceneShake {
            impacts: impacts_at_amp
                .iter()
                .map(|(at, amp)| ShakeImpact {
                    at: TimePoint::Seconds(*at),
                    amplitude: *amp,
                })
                .collect(),
            decay,
            frequency,
            rotation,
        };

        // Independent re-derivation of the doc's formula — a hand-rolled
        // sampler, exactly as a Python loop pasting keyframes would build.
        let hand_sampled = |t: f64| -> (f64, f64, f64) {
            let mut x = 0.0;
            let mut y = 0.0;
            for (at, amp) in impacts_at_amp {
                let dt = t - at;
                if dt < 0.0 {
                    continue;
                }
                let envelope = amp * (-decay * dt).exp();
                let phase = std::f64::consts::TAU * frequency * dt;
                x += envelope * phase.cos();
                y += envelope * phase.sin();
            }
            (x, y, rotation * x)
        };

        let span_end = 3.0; // last impact at 2.5s, well decayed by 3.0s.
        let samples = 155;
        for i in 0..samples {
            let t = span_end * i as f64 / (samples - 1) as f64;
            let offset = shake_offset(&shake, &ctx(), t).unwrap();
            let (ex, ey, erot) = hand_sampled(t);
            assert!(
                (offset.x - ex).abs() < 1e-9,
                "t={t}: x={}, expected {ex}",
                offset.x
            );
            assert!(
                (offset.y - ey).abs() < 1e-9,
                "t={t}: y={}, expected {ey}",
                offset.y
            );
            assert!(
                (offset.rotation - erot).abs() < 1e-9,
                "t={t}: rotation={}, expected {erot}",
                offset.rotation
            );
        }
    }
}
