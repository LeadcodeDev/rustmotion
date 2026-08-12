use dashmap::DashMap;
use std::sync::{Arc, OnceLock};

/// Per-track audio analysis: amplitude (RMS) and frequency bands per video frame.
#[derive(Debug, Clone)]
pub struct AudioAnalysis {
    /// The video frame rate this analysis was computed at.
    pub frame_rate: u32,
    /// RMS amplitude per frame, normalized 0..1 over the whole track.
    pub amplitude: Vec<f32>,
    /// 16 log-spaced frequency bands (20 Hz–16 kHz) per frame, normalized 0..1.
    pub bands: Vec<[f32; 16]>,
    /// Where the track sits on the scenario timeline (`AudioTrack::start`).
    ///
    /// The analysis indexes the *file* from its own sample 0, while every
    /// consumer asks in *scenario* time. Without this the two only line up when
    /// `start == 0`: a track placed at 73 s played from its beginning while the
    /// waveform drew the file's content at 73 s. Held here rather than at the
    /// call sites because the cache is keyed by path and the painters never see
    /// the `AudioTrack`.
    pub start: f64,
    /// `AudioTrack::end`, past which the track is cut and the visualisation
    /// must go flat rather than keep drawing an envelope nobody hears.
    pub end: Option<f64>,
}

impl AudioAnalysis {
    /// Scenario time → offset into the file, or `None` when the track is not
    /// playing at that moment. Every lookup below goes through this, so the
    /// placement is applied in exactly one place.
    fn track_time(&self, time: f64) -> Option<f64> {
        if time < self.start {
            return None;
        }
        if let Some(end) = self.end {
            if time >= end {
                return None;
            }
        }
        Some(time - self.start)
    }

    /// Get the amplitude at a given time (seconds), clamped to valid range.
    pub fn amplitude_at(&self, time: f64) -> f32 {
        let Some(time) = self.track_time(time) else {
            return 0.0;
        };
        let idx = (time * self.frame_rate as f64) as usize;
        self.amplitude
            .get(idx.min(self.amplitude.len().saturating_sub(1)))
            .copied()
            .unwrap_or(0.0)
    }

    /// Get a specific band [0..16) value at a given time.
    pub fn band_at(&self, time: f64, band: u8) -> f32 {
        let Some(time) = self.track_time(time) else {
            return 0.0;
        };
        let idx = (time * self.frame_rate as f64) as usize;
        let idx = idx.min(self.bands.len().saturating_sub(1));
        self.bands
            .get(idx)
            .map(|b| b[band.min(15) as usize])
            .unwrap_or(0.0)
    }

    /// Smoothed amplitude at time: average over the past `smoothing_frames` frames (inclusive of current).
    pub fn amplitude_smoothed(&self, time: f64, smoothing_frames: u32) -> f32 {
        if smoothing_frames == 0 || self.amplitude.is_empty() {
            return self.amplitude_at(time);
        }
        let Some(time) = self.track_time(time) else {
            return 0.0;
        };
        let end_idx = (time * self.frame_rate as f64) as usize;
        let end_idx = end_idx.min(self.amplitude.len().saturating_sub(1));
        let start_idx = end_idx.saturating_sub(smoothing_frames as usize);
        let window = &self.amplitude[start_idx..=end_idx];
        if window.is_empty() {
            return 0.0;
        }
        window.iter().sum::<f32>() / window.len() as f32
    }

    /// Smoothed band value at time over `smoothing_frames` frames.
    pub fn band_smoothed(&self, time: f64, band: u8, smoothing_frames: u32) -> f32 {
        if smoothing_frames == 0 || self.bands.is_empty() {
            return self.band_at(time, band);
        }
        let Some(time) = self.track_time(time) else {
            return 0.0;
        };
        let end_idx = (time * self.frame_rate as f64) as usize;
        let end_idx = end_idx.min(self.bands.len().saturating_sub(1));
        let start_idx = end_idx.saturating_sub(smoothing_frames as usize);
        let window = &self.bands[start_idx..=end_idx];
        if window.is_empty() {
            return 0.0;
        }
        window.iter().map(|b| b[band.min(15) as usize]).sum::<f32>() / window.len() as f32
    }
}

type AudioCacheMap = Arc<DashMap<String, Arc<AudioAnalysis>>>;

static AUDIO_ANALYSIS_CACHE: OnceLock<AudioCacheMap> = OnceLock::new();

/// Global audio analysis cache: keyed by the audio track's `src` path.
pub fn audio_analysis_cache() -> &'static AudioCacheMap {
    AUDIO_ANALYSIS_CACHE.get_or_init(|| Arc::new(DashMap::new()))
}
