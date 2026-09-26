use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub fn sine(phase: f32) -> f32 {
    (phase * std::f32::consts::TAU).sin()
}

pub fn square(phase: f32) -> f32 {
    if phase < 0.5 {
        1.0
    } else {
        -1.0
    }
}

pub fn saw(phase: f32) -> f32 {
    2.0 * phase - 1.0
}

pub fn triangle(phase: f32) -> f32 {
    if phase < 0.5 {
        4.0 * phase - 1.0
    } else {
        3.0 - 4.0 * phase
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Xorshift32 {
    state: u32,
}

impl Xorshift32 {
    pub fn new(seed: u32) -> Self {
        Xorshift32 {
            state: if seed == 0 { 0x9E37_79B9 } else { seed },
        }
    }

    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        x
    }

    pub fn next_f32(&mut self) -> f32 {
        (self.next_u32() as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// Oscillator waveform a [`super::voices::Voice`] selects — the sine/
/// square/saw/triangle quartet, plus `noise` for a non-tonal voice (hats,
/// snares). Lives here rather than in `voices.rs` because it is exactly
/// the vocabulary [`sine`]/[`square`]/[`saw`]/[`triangle`]/[`Xorshift32`]
/// above already speak.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OscKind {
    Sine,
    Square,
    Saw,
    Triangle,
    Noise,
}

/// The four biquad topologies this synth implements — deliberately just
/// these four (deliverable #2's list), no shelving/peaking EQ.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FilterKind {
    Lowpass,
    Highpass,
    Bandpass,
    Notch,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BiquadState {
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct BiquadCoeffs {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl BiquadCoeffs {
    pub fn design(kind: FilterKind, freq: f32, q: f32, sample_rate: f32) -> Self {
        let freq = freq.clamp(1.0, sample_rate * 0.499);
        let q = q.max(0.01);
        let w0 = std::f32::consts::TAU * freq / sample_rate;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * q);

        let (b0, b1, b2, a0, a1, a2) = match kind {
            FilterKind::Lowpass => {
                let b1 = 1.0 - cos_w0;
                let b0 = b1 / 2.0;
                (b0, b1, b0, 1.0 + alpha, -2.0 * cos_w0, 1.0 - alpha)
            }
            FilterKind::Highpass => {
                let b1 = -(1.0 + cos_w0);
                let b0 = (1.0 + cos_w0) / 2.0;
                (b0, b1, b0, 1.0 + alpha, -2.0 * cos_w0, 1.0 - alpha)
            }
            FilterKind::Bandpass => (alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cos_w0, 1.0 - alpha),
            FilterKind::Notch => (
                1.0,
                -2.0 * cos_w0,
                1.0,
                1.0 + alpha,
                -2.0 * cos_w0,
                1.0 - alpha,
            ),
        };

        BiquadCoeffs {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    pub fn process(&self, state: &mut BiquadState, x: f32) -> f32 {
        let y = self.b0 * x + self.b1 * state.x1 + self.b2 * state.x2
            - self.a1 * state.y1
            - self.a2 * state.y2;
        state.x2 = state.x1;
        state.x1 = x;
        state.y2 = state.y1;
        state.y1 = y;
        y
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Adsr {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub hold: f32,
    pub release: f32,
}

impl Adsr {
    pub fn total_duration(&self) -> f32 {
        self.attack.max(0.0) + self.decay.max(0.0) + self.hold.max(0.0) + self.release.max(0.0)
    }

    pub fn level_at(&self, t: f32) -> f32 {
        if t < 0.0 {
            return 0.0;
        }
        let a = self.attack.max(0.0);
        let d = self.decay.max(0.0);
        let h = self.hold.max(0.0);
        let r = self.release.max(0.0);
        let sustain = self.sustain.clamp(0.0, 1.0);

        if t < a {
            if a <= 0.0 {
                1.0
            } else {
                t / a
            }
        } else if t < a + d {
            let dt = (t - a) / d.max(1e-9);
            1.0 + (sustain - 1.0) * dt
        } else if t < a + d + h {
            sustain
        } else if t < a + d + h + r {
            let dt = (t - (a + d + h)) / r.max(1e-9);
            sustain * (1.0 - dt)
        } else {
            0.0
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CompressorParams {
    pub threshold_db: f32,
    pub ratio: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
}

fn db_to_lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

fn lin_to_db(lin: f32) -> f32 {
    20.0 * lin.max(1e-9).log10()
}

fn time_const_coeff(ms: f32, sample_rate: f32) -> f32 {
    if ms <= 0.0 {
        0.0
    } else {
        (-1.0 / (ms / 1000.0 * sample_rate)).exp()
    }
}

pub fn apply_compressor(buffer: &mut [f32], params: CompressorParams, sample_rate: u32) {
    let sr = sample_rate as f32;
    let attack_coeff = time_const_coeff(params.attack_ms, sr);
    let release_coeff = time_const_coeff(params.release_ms, sr);
    let ratio = params.ratio.max(1.0);
    let mut envelope = 0.0f32;

    for sample in buffer.iter_mut() {
        let input_level = sample.abs();
        let coeff = if input_level > envelope {
            attack_coeff
        } else {
            release_coeff
        };
        envelope = coeff * envelope + (1.0 - coeff) * input_level;

        let gain = if envelope > 1e-9 {
            let envelope_db = lin_to_db(envelope);
            let over_db = envelope_db - params.threshold_db;
            if over_db > 0.0 {
                let gain_reduction_db = over_db * (1.0 - 1.0 / ratio);
                db_to_lin(-gain_reduction_db)
            } else {
                1.0
            }
        } else {
            1.0
        };
        *sample *= gain;
    }
}

pub const LIMITER_CEILING_DB: f32 = -0.3;

pub fn apply_limiter(buffer: &mut [f32], sample_rate: u32) {
    let ceiling = db_to_lin(LIMITER_CEILING_DB);
    let sr = sample_rate as f32;
    let attack_coeff = time_const_coeff(1.0, sr);
    let release_coeff = time_const_coeff(50.0, sr);
    let mut envelope = 0.0f32;

    for sample in buffer.iter_mut() {
        let input_level = sample.abs();
        let coeff = if input_level > envelope {
            attack_coeff
        } else {
            release_coeff
        };
        envelope = coeff * envelope + (1.0 - coeff) * input_level;

        let gain = if envelope > ceiling && envelope > 1e-9 {
            ceiling / envelope
        } else {
            1.0
        };
        *sample = (*sample * gain).clamp(-ceiling, ceiling);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oscillators_are_bounded_and_hit_their_landmarks() {
        assert!((sine(0.0)).abs() < 1e-6);
        assert!((sine(0.25) - 1.0).abs() < 1e-6);
        assert_eq!(square(0.0), 1.0);
        assert_eq!(square(0.75), -1.0);
        assert!((saw(0.0) - -1.0).abs() < 1e-6);
        assert!((saw(1.0) - 1.0).abs() < 1e-6);
        assert!((triangle(0.0) - -1.0).abs() < 1e-6);
        assert!((triangle(0.5) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn xorshift32_is_deterministic_for_a_given_seed() {
        let mut a = Xorshift32::new(42);
        let mut b = Xorshift32::new(42);
        let seq_a: Vec<f32> = (0..64).map(|_| a.next_f32()).collect();
        let seq_b: Vec<f32> = (0..64).map(|_| b.next_f32()).collect();
        assert_eq!(seq_a, seq_b);
        assert!(seq_a.iter().all(|&s| (-1.0..=1.0).contains(&s)));
    }

    #[test]
    fn xorshift32_seed_zero_does_not_lock_up() {
        let mut z = Xorshift32::new(0);
        let vals: Vec<u32> = (0..8).map(|_| z.next_u32()).collect();
        assert!(vals.iter().any(|&v| v != 0));
    }

    #[test]
    fn lowpass_attenuates_a_tone_well_above_cutoff_more_than_one_well_below() {
        let sr = 48_000.0f32;
        let coeffs = BiquadCoeffs::design(FilterKind::Lowpass, 500.0, 0.707, sr);

        let rms_after = |freq: f32| -> f32 {
            let mut state = BiquadState::default();
            let n = 4800usize;
            let mut acc = 0.0f64;
            for i in 0..n {
                let phase = (i as f32 * freq / sr).fract();
                let x = sine(phase);
                let y = coeffs.process(&mut state, x);
                if i > n / 2 {
                    acc += (y as f64) * (y as f64);
                }
            }
            ((acc / (n / 2) as f64).sqrt()) as f32
        };

        let low = rms_after(100.0);
        let high = rms_after(8000.0);
        assert!(
            high < low * 0.5,
            "8kHz through a 500Hz lowpass ({high}) should be much quieter than 100Hz ({low})"
        );
    }

    #[test]
    fn adsr_percussive_default_reaches_silence_after_attack_plus_decay() {
        let env = Adsr {
            attack: 0.01,
            decay: 0.1,
            sustain: 0.0,
            hold: 0.0,
            release: 0.0,
        };
        assert_eq!(env.level_at(-1.0), 0.0);
        assert!(env.level_at(0.0) < env.level_at(0.01));
        assert!((env.level_at(0.01) - 1.0).abs() < 1e-4);
        assert!(env.level_at(0.05) < 1.0);
        assert_eq!(env.level_at(0.11), 0.0);
        assert!((env.total_duration() - 0.11).abs() < 1e-6);
    }

    #[test]
    fn compressor_reduces_gain_above_threshold_and_leaves_quiet_signal_alone() {
        let sr = 48_000;
        let mut loud = vec![0.9f32; 4800];
        let mut quiet = vec![0.05f32; 4800];
        let params = CompressorParams {
            threshold_db: -12.0,
            ratio: 4.0,
            attack_ms: 1.0,
            release_ms: 20.0,
        };
        apply_compressor(&mut loud, params, sr);
        apply_compressor(&mut quiet, params, sr);
        assert!(loud.last().unwrap() < &0.9);
        assert!((quiet.last().unwrap() - 0.05).abs() < 1e-4);
    }

    #[test]
    fn limiter_never_exceeds_its_ceiling_even_on_a_full_scale_step() {
        let sr = 48_000;
        let mut buf = vec![1.0f32; 4800];
        buf[0] = 1.0;
        apply_limiter(&mut buf, sr);
        let ceiling = db_to_lin(LIMITER_CEILING_DB);
        assert!(buf.iter().all(|&s| s.abs() <= ceiling + 1e-6));
    }
}
