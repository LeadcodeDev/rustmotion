use dashmap::DashMap;
use std::sync::{Arc, OnceLock};

#[derive(Debug, Clone)]
pub struct AudioAnalysis {
    pub frame_rate: u32,
    pub amplitude: Vec<f32>,
    pub bands: Vec<[f32; 16]>,
    pub start: f64,
    pub end: Option<f64>,
}

impl AudioAnalysis {
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

pub fn audio_analysis_cache() -> &'static AudioCacheMap {
    AUDIO_ANALYSIS_CACHE.get_or_init(|| Arc::new(DashMap::new()))
}
