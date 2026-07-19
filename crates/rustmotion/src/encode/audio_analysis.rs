use std::sync::Arc;

use rustfft::{num_complex::Complex, FftPlanner};
use rustmotion_core::engine::renderer::audio_analysis::{audio_analysis_cache, AudioAnalysis};
use rustmotion_core::schema::ResolvedScenario;

const FFT_SIZE: usize = 2048;
const NUM_BANDS: usize = 16;

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
/// Already-cached tracks are skipped (idempotent).
pub fn analyze_scenario_audio(scenario: &ResolvedScenario) {
    let tracks = &scenario.audio;
    if tracks.is_empty() {
        return;
    }
    let fps = scenario.video.fps;
    let cache = audio_analysis_cache();
    let band_bounds = band_boundaries();
    let hann = hann_window(FFT_SIZE);
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);

    for track in tracks {
        let src = &track.src;
        if cache.contains_key(src) {
            continue;
        }

        // Decode to PCM f32
        let (samples, sample_rate, channels) = match crate::encode::audio::decode_audio_file(src) {
            Ok(v) => v,
            Err(_) => continue, // graceful degradation: skip undecodable track
        };

        // Downmix to mono
        let mono: Vec<f32> = match channels {
            1 => samples.clone(),
            _ => samples
                .chunks(channels as usize)
                .map(|c| c.iter().sum::<f32>() / channels as f32)
                .collect(),
        };

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
        });
        cache.insert(src.clone(), analysis);
    }
}
