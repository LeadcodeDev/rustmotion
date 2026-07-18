use std::sync::{Arc, OnceLock};

use dashmap::DashMap;

use crate::error::{Result, RustmotionError};

type GifFrame = (Vec<u8>, u32, u32);
type GifData = Arc<(Vec<GifFrame>, Vec<f64>, f64)>;
type GifCacheMap = Arc<DashMap<String, GifData>>;

type VideoFrame = (f64, Vec<u8>, u32, u32);
type VideoFrameList = Arc<Vec<VideoFrame>>;
type VideoFrameCacheMap = Arc<DashMap<String, VideoFrameList>>;

/// Global asset cache for decoded images (keyed by file path)
static ASSET_CACHE: OnceLock<Arc<DashMap<String, skia_safe::Image>>> = OnceLock::new();

pub fn asset_cache() -> &'static Arc<DashMap<String, skia_safe::Image>> {
    ASSET_CACHE.get_or_init(|| Arc::new(DashMap::new()))
}

/// Clear the asset cache (call between renders if needed)
pub fn clear_asset_cache() {
    if let Some(cache) = ASSET_CACHE.get() {
        cache.clear();
    }
}

/// GIF frame data cache: stores decoded frames with pre-computed cumulative timestamps
/// (frames_rgba, cumulative_times, total_duration) keyed by file path
static GIF_CACHE: OnceLock<GifCacheMap> = OnceLock::new();

pub fn gif_cache() -> &'static GifCacheMap {
    GIF_CACHE.get_or_init(|| Arc::new(DashMap::new()))
}

// ─── Icon fetching ──────────────────────────────────────────────────────────

pub fn fetch_icon_svg(icon: &str, color: &str, width: u32, height: u32) -> Result<Vec<u8>> {
    let (prefix, name) =
        icon.split_once(':')
            .ok_or_else(|| RustmotionError::InvalidIconFormat {
                icon: icon.to_string(),
            })?;
    let hex_color = color.trim_start_matches('#');
    let width = width.max(1);
    let height = height.max(1);
    let url = format!(
        "https://api.iconify.design/{}/{}.svg?color=%23{}&width={}&height={}",
        prefix, name, hex_color, width, height
    );
    let response = ureq::get(&url)
        .call()
        .map_err(|e| RustmotionError::IconFetch {
            icon: icon.to_string(),
            reason: e.to_string(),
        })?;
    let body = response
        .into_body()
        .read_to_vec()
        .map_err(|e| RustmotionError::IconFetch {
            icon: icon.to_string(),
            reason: e.to_string(),
        })?;
    Ok(body)
}

// ─── Video frame extraction ─────────────────────────────────────────────────

/// Cache for pre-extracted video frames: key = "src:width:height", value = sorted list of (time, RGBA data, width, height)
static VIDEO_FRAME_CACHE: OnceLock<VideoFrameCacheMap> = OnceLock::new();

pub fn video_frame_cache() -> &'static VideoFrameCacheMap {
    VIDEO_FRAME_CACHE.get_or_init(|| Arc::new(DashMap::new()))
}

pub fn find_closest_frame(
    frames: &[(f64, Vec<u8>, u32, u32)],
    target_time: f64,
) -> Option<(&[u8], u32, u32)> {
    if frames.is_empty() {
        return None;
    }
    let idx = frames.partition_point(|(t, _, _, _)| *t < target_time);
    let best = if idx == 0 {
        0
    } else if idx >= frames.len() {
        frames.len() - 1
    } else {
        if (frames[idx].0 - target_time).abs() < (frames[idx - 1].0 - target_time).abs() {
            idx
        } else {
            idx - 1
        }
    };
    let (_, ref rgba, w, h) = frames[best];
    Some((rgba, w, h))
}

pub fn extract_video_frame(src: &str, time: f64, width: u32, height: u32) -> Result<Vec<u8>> {
    let output = std::process::Command::new("ffmpeg")
        .args([
            "-ss",
            &format!("{:.3}", time),
            "-i",
            src,
            "-vframes",
            "1",
            "-vf",
            &format!("scale={}:{}", width, height),
            "-f",
            "image2pipe",
            "-vcodec",
            "png",
            "-y",
            "pipe:1",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .map_err(|e| RustmotionError::FfmpegSpawn {
            reason: e.to_string(),
        })?;

    if !output.status.success() {
        return Err(RustmotionError::FfmpegFrameExtract {
            src: src.to_string(),
        });
    }

    Ok(output.stdout)
}
