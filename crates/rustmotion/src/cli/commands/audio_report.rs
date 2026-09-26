use rustmotion::schema::ResolvedScenario;
use serde::Serialize;

pub const SILENCE_FLOOR_DB: f32 = -120.0;

const CLIP_EPS_DB: f32 = 0.05;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct AudioMeasurement {
    pub peak_db: f32,
    pub rms_db: f32,
    pub true_peak_db: f32,
    pub clipped_samples: usize,
}

impl AudioMeasurement {
    fn silence() -> Self {
        AudioMeasurement {
            peak_db: SILENCE_FLOOR_DB,
            rms_db: SILENCE_FLOOR_DB,
            true_peak_db: SILENCE_FLOOR_DB,
            clipped_samples: 0,
        }
    }

    fn is_clipping(&self) -> bool {
        self.clipped_samples > 0
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BeatAudioMeasurement {
    pub beat_index: usize,
    pub start: f64,
    pub end: f64,
    pub measurement: AudioMeasurement,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AudioViolationKind {
    Clipping,
}

#[derive(Debug, Clone, Serialize)]
pub struct AudioViolation {
    pub kind: AudioViolationKind,
    pub beat_index: Option<usize>,
    pub start: f64,
    pub end: f64,
    pub measurement: AudioMeasurement,
    pub hint: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct AudioReport {
    pub overall: Option<AudioMeasurement>,
    pub beats: Vec<BeatAudioMeasurement>,
    pub violations: Vec<AudioViolation>,
}

pub fn analyze_scenario_audio_levels(scenario: &ResolvedScenario) -> AudioReport {
    if scenario.audio.is_empty() {
        return AudioReport::default();
    }

    let total_duration = rustmotion::encode::video_audio::resolved_scenario_duration(scenario);
    if total_duration <= 0.0 {
        return AudioReport::default();
    }

    let pcm = match rustmotion::encode::audio::mix_audio_tracks(&scenario.audio, total_duration) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return AudioReport::default(),
        Err(_) => return AudioReport::default(),
    };
    const CHANNELS: usize = 2;
    let sample_rate = rustmotion::encode::audio::OUTPUT_SAMPLE_RATE;
    let samples: Vec<i16> = pcm
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect();

    let overall = measure(&samples, CHANNELS);
    let mut violations = Vec::new();
    if overall.is_clipping() {
        violations.push(AudioViolation {
            kind: AudioViolationKind::Clipping,
            beat_index: None,
            start: 0.0,
            end: total_duration,
            measurement: overall,
            hint: format!(
                "the mixed soundtrack clips: {} sample(s) at or within {CLIP_EPS_DB:.2}dB of \
                 0dBFS (peak {:.2}dB, true peak {:.2}dB) across the whole {:.1}s track — lower \
                 `master.gain` or a track's `volume`, or re-enable `master.limiter`.",
                overall.clipped_samples, overall.peak_db, overall.true_peak_db, total_duration,
            ),
        });
    }

    let beats = beat_windows(scenario, total_duration)
        .into_iter()
        .map(|(beat_index, start, end)| {
            let m = measure_range(&samples, sample_rate, CHANNELS, start, end);
            if m.is_clipping() {
                violations.push(AudioViolation {
                    kind: AudioViolationKind::Clipping,
                    beat_index: Some(beat_index),
                    start,
                    end,
                    measurement: m,
                    hint: format!(
                        "beat {beat_index} ({start:.3}s–{end:.3}s) clips: {} sample(s) (peak \
                         {:.2}dB, true peak {:.2}dB).",
                        m.clipped_samples, m.peak_db, m.true_peak_db,
                    ),
                });
            }
            BeatAudioMeasurement {
                beat_index,
                start,
                end,
                measurement: m,
            }
        })
        .collect();

    AudioReport {
        overall: Some(overall),
        beats,
        violations,
    }
}

fn scenario_beat_grid(scenario: &ResolvedScenario) -> Option<(f64, f64)> {
    scenario
        .views
        .iter()
        .flat_map(|v| v.scenes.iter())
        .find_map(|s| {
            s.resolved_time_ctx
                .bpm
                .filter(|b| *b > 0.0)
                .map(|bpm| (bpm, s.resolved_time_ctx.beat_offset))
        })
}

fn beat_windows(scenario: &ResolvedScenario, total_duration: f64) -> Vec<(usize, f64, f64)> {
    let Some((bpm, beat_offset)) = scenario_beat_grid(scenario) else {
        return Vec::new();
    };
    let beat_len = 60.0 / bpm;
    if !beat_len.is_finite() || beat_len <= 0.0 {
        return Vec::new();
    }

    let mut windows = Vec::new();
    let mut n = 0usize;
    loop {
        let start = beat_offset + n as f64 * beat_len;
        if start >= total_duration {
            break;
        }
        let end = (start + beat_len).min(total_duration);
        let start = start.max(0.0);
        if end > start {
            windows.push((n, start, end));
        }
        n += 1;
        if n > 100_000 {
            break;
        }
    }
    windows
}

fn lin_to_db(x: f32) -> f32 {
    if x <= 1e-6 {
        SILENCE_FLOOR_DB
    } else {
        (20.0 * x.log10()).max(SILENCE_FLOOR_DB)
    }
}

fn db_to_lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

fn measure(samples_i16: &[i16], channels: usize) -> AudioMeasurement {
    measure_slice(samples_i16, channels)
}

fn measure_range(
    samples_i16: &[i16],
    sample_rate: u32,
    channels: usize,
    start: f64,
    end: f64,
) -> AudioMeasurement {
    let frame_count = samples_i16.len() / channels.max(1);
    let start_frame = ((start * sample_rate as f64).round() as i64)
        .max(0)
        .min(frame_count as i64) as usize;
    let end_frame = ((end * sample_rate as f64).round() as i64)
        .max(0)
        .min(frame_count as i64) as usize;
    if end_frame <= start_frame {
        return AudioMeasurement::silence();
    }
    measure_slice(
        &samples_i16[start_frame * channels..end_frame * channels],
        channels,
    )
}

fn measure_slice(samples_i16: &[i16], channels: usize) -> AudioMeasurement {
    if samples_i16.is_empty() || channels == 0 {
        return AudioMeasurement::silence();
    }
    let norm: Vec<f32> = samples_i16.iter().map(|&s| s as f32 / 32768.0).collect();
    let clip_threshold = db_to_lin(-CLIP_EPS_DB);

    let mut peak = 0.0f32;
    let mut sum_sq = 0.0f64;
    let mut clipped = 0usize;
    for &s in &norm {
        let a = s.abs();
        peak = peak.max(a);
        sum_sq += (s as f64) * (s as f64);
        if a >= clip_threshold {
            clipped += 1;
        }
    }
    let rms = (sum_sq / norm.len() as f64).sqrt() as f32;

    let mut true_peak = peak;
    for ch in 0..channels {
        let channel_samples: Vec<f32> = norm.iter().skip(ch).step_by(channels).copied().collect();
        true_peak = true_peak.max(true_peak_channel(&channel_samples));
    }

    AudioMeasurement {
        peak_db: lin_to_db(peak),
        rms_db: lin_to_db(rms),
        true_peak_db: lin_to_db(true_peak),
        clipped_samples: clipped,
    }
}

fn true_peak_channel(samples: &[f32]) -> f32 {
    let n = samples.len();
    if n < 2 {
        return samples.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
    }
    const OVERSAMPLE: usize = 4;
    let mut peak = samples.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
    for i in 0..n - 1 {
        let p0 = if i == 0 { samples[0] } else { samples[i - 1] };
        let p1 = samples[i];
        let p2 = samples[i + 1];
        let p3 = if i + 2 < n {
            samples[i + 2]
        } else {
            samples[n - 1]
        };
        for k in 1..OVERSAMPLE {
            let t = k as f32 / OVERSAMPLE as f32;
            peak = peak.max(catmull_rom(p0, p1, p2, p3, t).abs());
        }
    }
    peak
}

fn catmull_rom(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    let t2 = t * t;
    let t3 = t2 * t;
    0.5 * ((2.0 * p1)
        + (-p0 + p2) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmotion::loader::load_scenario_from_source;

    fn parse(json: &str) -> ResolvedScenario {
        load_scenario_from_source(None, Some(json)).expect("scenario parses")
    }

    #[test]
    fn no_audio_produces_an_empty_report() {
        let json = r##"{
            "video": { "width": 320, "height": 180 },
            "scenes": [{ "duration": 1.0, "children": [] }]
        }"##;
        let report = analyze_scenario_audio_levels(&parse(json));
        assert!(report.overall.is_none());
        assert!(report.violations.is_empty());
    }

    #[test]
    fn catmull_rom_reproduces_the_unclamped_linear_segment_at_its_endpoints() {
        assert_eq!(catmull_rom(0.0, 1.0, 2.0, 3.0, 0.0), 1.0);
        assert_eq!(catmull_rom(0.0, 1.0, 2.0, 3.0, 1.0), 2.0);
    }

    #[test]
    fn true_peak_channel_never_reports_less_than_the_sample_peak() {
        let samples = vec![0.0, 0.5, -0.9, 0.2, -0.3, 0.95, 0.0];
        let sample_peak = samples.iter().fold(0.0f32, |m, s: &f32| m.max(s.abs()));
        assert!(true_peak_channel(&samples) >= sample_peak - 1e-6);
    }

    #[test]
    fn measure_slice_reports_silence_floor_for_all_zero_samples() {
        let m = measure_slice(&[0i16; 100], 2);
        assert_eq!(m.peak_db, SILENCE_FLOOR_DB);
        assert_eq!(m.rms_db, SILENCE_FLOOR_DB);
        assert_eq!(m.clipped_samples, 0);
    }

    #[test]
    fn measure_slice_flags_full_scale_samples_as_clipped() {
        let m = measure_slice(&[i16::MAX, i16::MIN, 0, 0], 2);
        assert!(m.is_clipping());
        assert_eq!(m.clipped_samples, 2);
        assert!(
            m.peak_db > -0.1,
            "expected near-0dBFS peak, got {}",
            m.peak_db
        );
    }

    const SYNTH_CLIPPING_JSON: &str = r##"{
        "video": { "width": 320, "height": 180, "fps": 30 },
        "bpm": 120,
        "audio": {
            "voices": {
                "tone": {
                    "type": "sine", "freq": 440,
                    "attack": 0.01, "decay": 0.05, "sustain": 1.0,
                    "hold": 0.3, "release": 0.05, "gain": 1.0
                }
            },
            "score": [
                { "voice": "tone", "every": 0.5, "from": 0, "to": 2.0 }
            ],
            "master": { "gain": 12.0, "limiter": false }
        },
        "scenes": [{ "duration": 2.0, "children": [] }]
    }"##;

    const SYNTH_CLEAN_JSON: &str = r##"{
        "video": { "width": 320, "height": 180, "fps": 30 },
        "bpm": 120,
        "audio": {
            "voices": {
                "tone": {
                    "type": "sine", "freq": 440,
                    "attack": 0.01, "decay": 0.05, "sustain": 1.0,
                    "hold": 0.3, "release": 0.05, "gain": 0.3
                }
            },
            "score": [
                { "voice": "tone", "every": 0.5, "from": 0, "to": 2.0 }
            ]
        },
        "scenes": [{ "duration": 2.0, "children": [] }]
    }"##;

    #[test]
    fn a_synthesised_score_with_the_limiter_disabled_and_gain_cranked_clips() {
        let scenario = parse(SYNTH_CLIPPING_JSON);
        assert!(
            !scenario.audio.is_empty(),
            "the synthesised score must have been rendered and appended as an AudioTrack"
        );
        let report = analyze_scenario_audio_levels(&scenario);
        let overall = report.overall.expect("audio present");
        assert!(
            overall.is_clipping(),
            "expected the +12dB, limiter-disabled mix to clip, got {overall:?}"
        );
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.kind == AudioViolationKind::Clipping && v.beat_index.is_none()),
            "expected a whole-track clipping violation: {:?}",
            report.violations
        );
        assert!(
            report
                .violations
                .iter()
                .any(|v| v.kind == AudioViolationKind::Clipping && v.beat_index.is_some()),
            "expected at least one beat-scoped clipping violation: {:?}",
            report.violations
        );
        assert!(
            !report.beats.is_empty(),
            "bpm is declared, so the per-beat breakdown must be populated"
        );
    }

    #[test]
    fn the_same_score_at_a_sane_gain_with_the_limiter_on_does_not_clip() {
        let scenario = parse(SYNTH_CLEAN_JSON);
        let report = analyze_scenario_audio_levels(&scenario);
        let overall = report.overall.expect("audio present");
        assert!(
            !overall.is_clipping(),
            "expected a modest, limiter-protected mix not to clip: {overall:?}"
        );
        assert!(
            report.violations.is_empty(),
            "expected no violations for a clean mix: {:?}",
            report.violations
        );
    }
}
