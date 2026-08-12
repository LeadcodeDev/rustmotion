use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use rustfft::{num_complex::Complex, FftPlanner};
use rustmotion_core::engine::renderer::audio_analysis::{audio_analysis_cache, AudioAnalysis};
use rustmotion_core::schema::ResolvedScenario;

const FFT_SIZE: usize = 2048;
const NUM_BANDS: usize = 16;

/// A track that could not be analysed, and why.
///
/// Returned rather than logged so each caller decides how loud to be: an
/// encode prints a warning and carries on, the studio shows it in the topbar.
/// Swallowing it leaves `waveform`/`audio_spectrum` drawing their flat
/// fallback with nothing anywhere saying why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioAnalysisFailure {
    pub src: String,
    pub reason: String,
}

impl std::fmt::Display for AudioAnalysisFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.src, self.reason)
    }
}

/// What a cached analysis was computed from: file size, mtime, and the fps it
/// was bucketed at. The cache is keyed by path alone (the painters look tracks
/// up that way), so without this a track whose *content* changed under a stable
/// path — the normal case when someone re-exports a mix while the studio is
/// open — would keep serving the old envelope forever.
/// File identity (length, mtime), the fps it was bucketed at, and a hash of
/// everything about the *track* that changes the result: `start`/`end` move the
/// lookup, and `volume`/`volume_keyframes`/the fades are baked into the
/// amplitudes. An entry computed for one of those must never be served for
/// another — two scenarios can name the same file with different mixes.
type SourceFingerprint = (u64, u128, u32, u64);

static FINGERPRINTS: OnceLock<Mutex<HashMap<String, SourceFingerprint>>> = OnceLock::new();

fn fingerprints() -> &'static Mutex<HashMap<String, SourceFingerprint>> {
    FINGERPRINTS.get_or_init(Default::default)
}

/// `None` when the file cannot be stat'ed — treated as "changed", so the next
/// analysis attempt runs and reports a real decode error instead of silently
/// reusing a stale entry.
fn source_fingerprint(
    src: &str,
    fps: u32,
    track: &rustmotion_core::schema::AudioTrack,
) -> Option<SourceFingerprint> {
    let meta = std::fs::metadata(src).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some((meta.len(), mtime, fps, track_hash(track)))
}

/// Hash the track's placement and volume envelope. Serialised rather than
/// hashed field by field so adding a field to `AudioTrack` cannot silently
/// leave it out of the key.
fn track_hash(track: &rustmotion_core::schema::AudioTrack) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(track)
        .unwrap_or_default()
        .hash(&mut hasher);
    hasher.finish()
}

/// Build the 16 log-spaced band frequency boundaries (Hz) from 20..16000.
fn band_boundaries() -> [(f32, f32); NUM_BANDS] {
    let mut bounds = [(0.0f32, 0.0f32); NUM_BANDS];
    let low = 20.0f32.log2();
    let high = 16000.0f32.log2();
    for (i, bound) in bounds.iter_mut().enumerate() {
        let lo = 2.0f32.powf(low + (high - low) * i as f32 / NUM_BANDS as f32);
        let hi = 2.0f32.powf(low + (high - low) * (i + 1) as f32 / NUM_BANDS as f32);
        *bound = (lo, hi);
    }
    bounds
}

/// Compute a Hann window of length `n`.
fn hann_window(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (n - 1) as f32).cos()))
        .collect()
}

/// Analyze all audio tracks in the scenario and populate the global cache.
/// Idempotent: a track is re-analysed only when its file changed on disk or
/// the fps did. Returns the tracks that could not be analysed — an empty vec
/// means every track in the scenario now has an entry in the cache.
pub fn analyze_scenario_audio(scenario: &ResolvedScenario) -> Vec<AudioAnalysisFailure> {
    let mut failures = Vec::new();
    let tracks = &scenario.audio;
    if tracks.is_empty() {
        return failures;
    }
    let fps = scenario.video.fps;
    let cache = audio_analysis_cache();
    let fps_of = fingerprints();
    let band_bounds = band_boundaries();
    let hann = hann_window(FFT_SIZE);
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);

    for track in tracks {
        let src = &track.src;
        let fingerprint = source_fingerprint(src, fps, track);
        let cached_and_current = cache.contains_key(src)
            && fingerprint.is_some()
            && fps_of
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(src)
                .copied()
                == fingerprint;
        if cached_and_current {
            continue;
        }

        // Decode to PCM f32
        let (samples, sample_rate, channels) = match crate::encode::audio::decode_audio_file(src) {
            Ok(v) => v,
            Err(e) => {
                failures.push(AudioAnalysisFailure {
                    src: src.clone(),
                    reason: e.to_string(),
                });
                continue;
            }
        };

        // Downmix to mono
        let mono: Vec<f32> = match channels {
            1 => samples.clone(),
            _ => samples
                .chunks(channels as usize)
                .map(|c| c.iter().sum::<f32>() / channels as f32)
                .collect(),
        };

        // Follow the *mix*, not the source. `volume`, `volume_keyframes` and the
        // fades are what comes out of the speakers, and since #182 the studio
        // plays exactly that — a waveform drawing the raw file's envelope while
        // a fade takes the sound down contradicts what the viewer hears. The
        // gain comes from the encoder's own `track_gain_at`, so the picture
        // cannot drift from the audio.
        let audible = {
            let file_seconds = mono.len() as f64 / sample_rate as f64;
            match track.end {
                Some(end) => file_seconds.min((end - track.start).max(0.0)),
                None => file_seconds,
            }
        };
        let mono: Vec<f32> = mono
            .into_iter()
            .enumerate()
            .map(|(i, s)| {
                let t = i as f64 / sample_rate as f64;
                s * crate::encode::audio::track_gain_at(track, t, audible)
            })
            .collect();

        let samples_per_frame = (sample_rate as f64 / fps as f64).ceil() as usize;
        let num_frames = (mono.len() as f64 / samples_per_frame as f64).ceil() as usize;

        let mut amplitude = Vec::with_capacity(num_frames);
        let mut bands_all: Vec<[f32; NUM_BANDS]> = Vec::with_capacity(num_frames);

        for frame_idx in 0..num_frames {
            let start = frame_idx * samples_per_frame;
            let end = (start + samples_per_frame).min(mono.len());
            let frame_samples = &mono[start..end];

            // RMS amplitude
            let rms = if frame_samples.is_empty() {
                0.0f32
            } else {
                (frame_samples.iter().map(|s| s * s).sum::<f32>() / frame_samples.len() as f32)
                    .sqrt()
            };
            amplitude.push(rms);

            // FFT: take FFT_SIZE samples from this frame (with zero-padding)
            let mut buf: Vec<Complex<f32>> = (0..FFT_SIZE)
                .map(|i| {
                    let s = if i < frame_samples.len() {
                        frame_samples[i]
                    } else {
                        0.0
                    };
                    Complex {
                        re: s * hann[i],
                        im: 0.0,
                    }
                })
                .collect();
            fft.process(&mut buf);

            // Map FFT bins to bands
            let bin_hz = sample_rate as f32 / FFT_SIZE as f32;
            let half = FFT_SIZE / 2;
            let mut frame_bands = [0.0f32; NUM_BANDS];
            for (b, &(lo, hi)) in band_bounds.iter().enumerate() {
                let lo_bin = (lo / bin_hz) as usize;
                let hi_bin = ((hi / bin_hz) as usize + 1).min(half);
                let lo_bin = lo_bin.min(half);
                if lo_bin >= hi_bin {
                    frame_bands[b] = 0.0;
                    continue;
                }
                let energy: f32 = buf[lo_bin..hi_bin].iter().map(|c| c.norm()).sum::<f32>()
                    / (hi_bin - lo_bin) as f32;
                frame_bands[b] = energy;
            }
            bands_all.push(frame_bands);
        }

        // Normalize amplitude to 0..1
        let amp_max = amplitude.iter().cloned().fold(0.0f32, f32::max);
        if amp_max > 1e-8 {
            for a in &mut amplitude {
                *a /= amp_max;
            }
        }

        // Normalize bands with a single global max so cross-band energy
        // ratios stay meaningful (a quiet band stays quiet on screen).
        let bands_max = bands_all
            .iter()
            .flat_map(|fr| fr.iter().copied())
            .fold(0.0f32, f32::max);
        if bands_max > 1e-8 {
            for fr in &mut bands_all {
                for v in fr.iter_mut() {
                    *v /= bands_max;
                }
            }
        }

        let analysis = Arc::new(AudioAnalysis {
            frame_rate: fps,
            amplitude,
            bands: bands_all,
            start: track.start,
            end: track.end,
        });
        cache.insert(src.clone(), analysis);
        let mut fps_of = fps_of.lock().unwrap_or_else(|e| e.into_inner());
        match fingerprint {
            Some(fp) => {
                fps_of.insert(src.clone(), fp);
            }
            // Un-stat'able but decodable: don't record a fingerprint, so the
            // next call re-analyses rather than trusting an entry it cannot
            // check.
            None => {
                fps_of.remove(src);
            }
        }
    }
    failures
}
