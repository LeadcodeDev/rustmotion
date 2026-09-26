//! [`render`]: the single entry point that turns a [`Score`] into a
//! finished, mixed, mastered stereo buffer — the "small, boring synth"
//! deliverable's top-level orchestration. Everything else in this module
//! (`dsp`, `voices`, `score`) is a building block this function assembles;
//! nothing outside `rustmotion-core::audio` needs to call anything but
//! this.

use std::collections::HashMap;

use crate::schema::time::TimeCtx;

use super::dsp;
use super::score::{Score, ScoreError};

/// Sample rate the synth renders at, per deliverable #2 ("rendered offline
/// to an f32 buffer at 48 kHz"). The file-based [`crate::schema::AudioTrack`]
/// mixer downstream (`rustmotion`'s `encode` crate) declares a different,
/// fixed rate for its own muxed PCM — the exact same resampling path an
/// ordinary 48kHz source file already goes through there carries this
/// buffer down to that rate, so nothing in this crate needs to duplicate
/// it.
pub const SYNTH_SAMPLE_RATE: u32 = 48_000;

/// Renders `score` into an interleaved stereo `f32` buffer, `duration_secs`
/// long at [`SYNTH_SAMPLE_RATE`] (mono voices, duplicated to both
/// channels — this synth has no panning model). `ctx` is the scenario's own
/// [`TimeCtx`] (real `bpm`/`beat_offset`, `scene_start: 0.0` — a score is
/// never nested inside a scene): every `TimePoint` in `score.score` resolves
/// against it, which is what puts a kick on the same instant as a beat-grid
/// scene cut.
///
/// Deterministic: same `score` + `ctx` + `duration_secs` always produces
/// the same bytes (no wall-clock or thread-seeded randomness anywhere in
/// this crate's synth — see [`dsp::Xorshift32`]'s doc). That is the
/// property `rustmotion`'s `--frames a-b` slicing and the "two renders are
/// byte-identical" acceptance criterion both depend on.
pub fn render(score: &Score, ctx: TimeCtx, duration_secs: f64) -> Result<Vec<f32>, ScoreError> {
    let duration_secs = duration_secs.max(0.0);
    let num_samples = (duration_secs * SYNTH_SAMPLE_RATE as f64).ceil() as usize;
    let mut mono = vec![0.0f32; num_samples];

    let hits = score.resolve_hits(&ctx, duration_secs)?;

    // Iterated in a fixed (sorted) order, not `HashMap`'s own — std's
    // hasher is randomized per process, and mixing three or more voices'
    // grains into the *same* sample index is a floating-point sum whose
    // bit pattern can depend on accumulation order. Without this sort,
    // "two renders are byte-identical" (this module's whole determinism
    // promise, and an explicit acceptance criterion) would hold almost
    // always and occasionally not, which is worse than never.
    let mut voice_names: Vec<&String> = hits.keys().collect();
    voice_names.sort();

    // Every trigger of a given voice renders to the identical grain (see
    // `Voice::render_grain`'s doc) — render each voice once and stamp it
    // onto the timeline per hit, rather than re-synthesizing per hit.
    let mut grains: HashMap<&str, Vec<f32>> = HashMap::new();
    for name in &voice_names {
        if let Some(voice) = score.voices.get(*name) {
            grains.insert(name.as_str(), voice.render_grain(SYNTH_SAMPLE_RATE));
        }
    }

    for name in &voice_names {
        let times = &hits[*name];
        let Some(grain) = grains.get(name.as_str()) else {
            continue;
        };
        for &t in times {
            if t < 0.0 {
                continue;
            }
            let start = (t * SYNTH_SAMPLE_RATE as f64).round() as i64;
            for (i, &s) in grain.iter().enumerate() {
                let idx = start + i as i64;
                if idx < 0 {
                    continue;
                }
                let idx = idx as usize;
                if idx >= mono.len() {
                    break;
                }
                mono[idx] += s;
            }
        }
    }

    if let Some(gain_db) = score.master.gain {
        let gain = 10f32.powf(gain_db / 20.0);
        for sample in mono.iter_mut() {
            *sample *= gain;
        }
    }
    if let Some(comp) = &score.master.compressor {
        dsp::apply_compressor(
            &mut mono,
            dsp::CompressorParams {
                threshold_db: comp.threshold,
                ratio: comp.ratio,
                attack_ms: comp.attack,
                release_ms: comp.release,
            },
            SYNTH_SAMPLE_RATE,
        );
    }
    if score.master.limiter {
        dsp::apply_limiter(&mut mono, SYNTH_SAMPLE_RATE);
    }

    let mut stereo = Vec::with_capacity(mono.len() * 2);
    for sample in mono {
        stereo.push(sample);
        stereo.push(sample);
    }
    Ok(stereo)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::score::{CompressorConfig, MasterBus, ScoreEvent};
    use crate::audio::voices::{FilterKind, FilterSpec, FreqSpec, OscKind, Voice};
    use crate::schema::time::TimePoint;

    fn issue_example_score() -> Score {
        let mut voices = HashMap::new();
        voices.insert(
            "kick".to_string(),
            Voice {
                kind: OscKind::Sine,
                freq: Some(FreqSpec::Sweep([150.0, 42.0])),
                sweep: Some(0.14),
                filter: None,
                attack: 0.002,
                decay: 0.38,
                sustain: 0.0,
                hold: 0.0,
                release: 0.0,
                gain: 1.0,
                seed: None,
            },
        );
        voices.insert(
            "hat".to_string(),
            Voice {
                kind: OscKind::Noise,
                freq: None,
                sweep: None,
                filter: Some(FilterSpec {
                    kind: FilterKind::Highpass,
                    freq: 7500.0,
                    q: 0.707,
                }),
                attack: 0.002,
                decay: 0.05,
                sustain: 0.0,
                hold: 0.0,
                release: 0.0,
                gain: 1.0,
                seed: Some(11),
            },
        );
        Score {
            voices,
            score: vec![
                ScoreEvent {
                    voice: "kick".to_string(),
                    at: None,
                    every: Some(TimePoint::Spec("1b".to_string())),
                    from: Some(TimePoint::Spec("@2.2s".to_string())),
                    to: Some(TimePoint::Spec("@12.6s".to_string())),
                    offset: None,
                    gain: None,
                },
                ScoreEvent {
                    voice: "hat".to_string(),
                    at: None,
                    every: Some(TimePoint::Spec("1b".to_string())),
                    from: Some(TimePoint::Spec("@2.2s".to_string())),
                    to: None,
                    offset: Some(TimePoint::Spec("0.5b".to_string())),
                    gain: None,
                },
            ],
            master: MasterBus {
                gain: None,
                compressor: Some(CompressorConfig {
                    threshold: -14.0,
                    ratio: 4.0,
                    attack: 5.0,
                    release: 50.0,
                }),
                limiter: true,
            },
        }
    }

    fn ctx() -> TimeCtx {
        TimeCtx {
            bpm: Some(115.0),
            beat_offset: 0.0,
            scene_start: 0.0,
        }
    }

    #[test]
    fn render_produces_a_nonsilent_stereo_buffer_of_the_requested_length() {
        let buf = render(&issue_example_score(), ctx(), 13.0).expect("render must succeed");
        assert_eq!(
            buf.len(),
            (13.0 * SYNTH_SAMPLE_RATE as f64).ceil() as usize * 2
        );
        assert!(buf.iter().any(|&s| s.abs() > 0.01), "must not be silent");
    }

    #[test]
    fn render_is_deterministic_across_two_calls() {
        let a = render(&issue_example_score(), ctx(), 13.0).unwrap();
        let b = render(&issue_example_score(), ctx(), 13.0).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn render_with_limiter_on_never_exceeds_the_limiter_ceiling() {
        let buf = render(&issue_example_score(), ctx(), 13.0).unwrap();
        let ceiling = 10f32.powf(dsp::LIMITER_CEILING_DB / 20.0);
        assert!(buf.iter().all(|&s| s.abs() <= ceiling + 1e-6));
    }

    #[test]
    fn stereo_channels_are_identical_mono_duplicated() {
        let buf = render(&issue_example_score(), ctx(), 13.0).unwrap();
        for pair in buf.as_chunks::<2>().0 {
            assert_eq!(pair[0], pair[1]);
        }
    }

    #[test]
    fn unknown_voice_propagates_as_a_score_error() {
        let mut score = issue_example_score();
        score.score.push(ScoreEvent {
            voice: "cowbell".to_string(),
            at: Some(TimePoint::Seconds(1.0)),
            every: None,
            from: None,
            to: None,
            offset: None,
            gain: None,
        });
        let err = render(&score, ctx(), 13.0).unwrap_err();
        assert!(matches!(err, ScoreError::UnknownVoice(v) if v == "cowbell"));
    }
}
