//! [`Voice`]: one instrument definition in a [`super::score::Score`] — the
//! `"kick"`/`"hat"` entries of the issue's `voices` map. A `Voice` is
//! stateless data; [`Voice::render_grain`] is the only place it turns into
//! samples, producing one self-contained "grain" (attack through release,
//! silence before and after) that [`super::synth::render`] then stamps
//! onto the timeline once per resolved hit.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::dsp;

pub use dsp::{FilterKind, OscKind};

/// A voice's frequency: either fixed (a sustained tone), or a two-point
/// sweep — `"freq": [150, 42]` in the issue's `kick` example, read start-to-end
/// against `sweep` (seconds). Linear interpolation, not exponential: simpler,
/// and "small, boring synth" (deliverable #2) does not ask for a
/// perceptually-uniform pitch glide.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum FreqSpec {
    Fixed(f32),
    Sweep([f32; 2]),
}

fn default_q() -> f32 {
    // Butterworth Q — maximally flat passband, the least surprising
    // default when a score gives a filter's `freq` but not its `q` (the
    // issue's `hat` example does exactly this).
    0.707
}

/// `voices.hat.filter` in the issue's example
/// (`{"type":"highpass","freq":7500}`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FilterSpec {
    #[serde(rename = "type")]
    pub kind: FilterKind,
    pub freq: f32,
    #[serde(default = "default_q")]
    pub q: f32,
}

fn default_attack() -> f32 {
    // 2ms: enough to avoid a hard-edge click on a one-shot trigger,
    // short enough to still read as instant next to `decay` (tens/hundreds
    // of ms) on a drum voice.
    0.002
}

fn default_gain() -> f32 {
    1.0
}

/// One instrument: an oscillator or a noise source, an optional filter,
/// and an ADSR envelope (`attack`/`decay`/`sustain`/`hold`/`release` — see
/// [`dsp::Adsr`]'s doc for why `hold` stands in for a note-off this score
/// model never sends). Every field but `type` and `decay` has a
/// drum-machine-flavoured default, so the issue's own two-voice example
/// (`"kick": {"type":"sine","freq":[150,42],"sweep":0.14,"decay":0.38}`)
/// needs nothing else.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Voice {
    #[serde(rename = "type")]
    pub kind: OscKind,
    /// Required for every `type` but `noise`; ignored for `noise`. Falls
    /// back to 440Hz if omitted on a tonal voice rather than erroring —
    /// consistent with this module's general posture (see
    /// [`dsp::BiquadCoeffs::design`]'s doc) of making an underspecified
    /// voice audible rather than rejecting it.
    #[serde(default)]
    pub freq: Option<FreqSpec>,
    /// Seconds the `freq` sweep takes to go from its first to its second
    /// value. Ignored unless `freq` is [`FreqSpec::Sweep`].
    #[serde(default)]
    pub sweep: Option<f32>,
    #[serde(default)]
    pub filter: Option<FilterSpec>,
    #[serde(default = "default_attack")]
    pub attack: f32,
    #[serde(default)]
    pub decay: f32,
    #[serde(default)]
    pub sustain: f32,
    #[serde(default)]
    pub hold: f32,
    #[serde(default)]
    pub release: f32,
    #[serde(default = "default_gain")]
    pub gain: f32,
    /// Seeds this voice's [`dsp::Xorshift32`] noise source. Two `noise`
    /// voices with no explicit seed share the same fixed default and so
    /// sound identical to each other — a minor aesthetic simplification
    /// documented here rather than hidden; give each its own `seed` to
    /// tell them apart.
    #[serde(default)]
    pub seed: Option<u32>,
}

impl Voice {
    fn envelope(&self) -> dsp::Adsr {
        dsp::Adsr {
            attack: self.attack.max(0.0),
            decay: self.decay.max(0.0),
            sustain: self.sustain.clamp(0.0, 1.0),
            hold: self.hold.max(0.0),
            release: self.release.max(0.0),
        }
    }

    /// Renders exactly one trigger of this voice — a self-contained mono
    /// grain, `envelope().total_duration()` seconds long at `sample_rate`,
    /// starting and ending at silence. Identical every time (no dependency
    /// on *when* it is triggered), so [`super::synth::render`] renders it
    /// once per voice and reuses it for every resolved hit.
    pub fn render_grain(&self, sample_rate: u32) -> Vec<f32> {
        let envelope = self.envelope();
        let sr = sample_rate as f32;
        let total = envelope.total_duration().max(1.0 / sr);
        let n = (total * sr).ceil() as usize;

        let mut filter = self.filter.map(|f| {
            (
                dsp::BiquadCoeffs::design(f.kind, f.freq, f.q, sr),
                dsp::BiquadState::default(),
            )
        });
        let mut noise = dsp::Xorshift32::new(self.seed.unwrap_or(0x1234_5678));

        let (f0, f1, sweep) = match self.freq {
            Some(FreqSpec::Fixed(f)) => (f, f, 0.0),
            Some(FreqSpec::Sweep([a, b])) => (a, b, self.sweep.unwrap_or(0.0).max(0.0)),
            None => (440.0, 440.0, 0.0),
        };

        let mut phase = 0.0f32;
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let t = i as f32 / sr;
            let freq = if sweep > 0.0 && t < sweep {
                f0 + (f1 - f0) * (t / sweep)
            } else {
                f1
            };

            let mut sample = match self.kind {
                OscKind::Sine => dsp::sine(phase),
                OscKind::Square => dsp::square(phase),
                OscKind::Saw => dsp::saw(phase),
                OscKind::Triangle => dsp::triangle(phase),
                OscKind::Noise => noise.next_f32(),
            };
            if self.kind != OscKind::Noise {
                phase = (phase + freq / sr).rem_euclid(1.0);
            }

            if let Some((coeffs, state)) = filter.as_mut() {
                sample = coeffs.process(state, sample);
            }

            out.push(sample * envelope.level_at(t) * self.gain);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kick() -> Voice {
        Voice {
            kind: OscKind::Sine,
            freq: Some(FreqSpec::Sweep([150.0, 42.0])),
            sweep: Some(0.14),
            filter: None,
            attack: default_attack(),
            decay: 0.38,
            sustain: 0.0,
            hold: 0.0,
            release: 0.0,
            gain: default_gain(),
            seed: None,
        }
    }

    fn hat() -> Voice {
        Voice {
            kind: OscKind::Noise,
            freq: None,
            sweep: None,
            filter: Some(FilterSpec {
                kind: FilterKind::Highpass,
                freq: 7500.0,
                q: default_q(),
            }),
            attack: default_attack(),
            decay: 0.05,
            sustain: 0.0,
            hold: 0.0,
            release: 0.0,
            gain: default_gain(),
            seed: Some(7),
        }
    }

    #[test]
    fn kick_grain_starts_and_ends_at_silence_and_is_within_range() {
        let grain = kick().render_grain(48_000);
        assert!(!grain.is_empty());
        assert!(
            grain[0].abs() < 1e-3,
            "grain must start near silence (attack ramp)"
        );
        assert!(
            grain.last().unwrap().abs() < 1e-3,
            "grain must decay to silence by the end of its envelope"
        );
        assert!(grain.iter().all(|s| s.abs() <= 1.0 + 1e-3));
    }

    #[test]
    fn hat_grain_is_filtered_noise_not_silence() {
        let grain = hat().render_grain(48_000);
        assert!(!grain.is_empty());
        assert!(
            grain.iter().any(|&s| s.abs() > 0.01),
            "a highpassed noise burst must not be silent"
        );
    }

    #[test]
    fn voice_deserializes_from_the_issues_json_shape() {
        let json = serde_json::json!({
            "type": "sine",
            "freq": [150, 42],
            "sweep": 0.14,
            "decay": 0.38
        });
        let voice: Voice = serde_json::from_value(json).expect("kick voice parses");
        assert_eq!(voice.kind, OscKind::Sine);
        assert_eq!(voice.freq, Some(FreqSpec::Sweep([150.0, 42.0])));
        assert_eq!(voice.decay, 0.38);
        assert_eq!(voice.attack, default_attack());

        let json = serde_json::json!({
            "type": "noise",
            "filter": {"type": "highpass", "freq": 7500},
            "decay": 0.05
        });
        let voice: Voice = serde_json::from_value(json).expect("hat voice parses");
        assert_eq!(voice.kind, OscKind::Noise);
        assert_eq!(voice.filter.unwrap().freq, 7500.0);
    }

    #[test]
    fn rendering_the_same_voice_twice_is_byte_identical() {
        let a = hat().render_grain(48_000);
        let b = hat().render_grain(48_000);
        assert_eq!(a, b);
    }
}
