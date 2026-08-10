use std::path::{Path, PathBuf};
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

/// How much larger than the *target* (layout) size icons are rasterized, so
/// Skia's downscale keeps edges crisp under sub-pixel positioning and minor
/// scale animations.
///
/// This lives here — not as a local `const` inside the painter — because it
/// must feed the exact same computation [`icon_cache_key`] uses. See that
/// function's doc for why (issue #166).
pub const ICON_OVERSAMPLE: u32 = 2;

/// Single source of truth for both the oversampled rasterization size and
/// the [`asset_cache`] key used for a given icon at a given *target*
/// (layout) size. Returns `(render_width, render_height, cache_key)`.
///
/// # Issue #166
///
/// Before this function existed, `icon.rs`'s painter and `preload.rs`'s
/// prefetcher each built the cache key from their own inlined `format!`.
/// The painter multiplied the target size by [`ICON_OVERSAMPLE`] before
/// building the key; the preloader did not. For a 40×40 icon the painter
/// looked up `"icon:...:80x80"` while the preloader could only ever have
/// written `"icon:...:40x40"` — the two keys could never collide, so
/// `prefetch_icons` never once avoided a duplicate network fetch, and (had
/// its rasterization size matched the key by coincidence) would have cached
/// a bitmap at half the resolution the painter actually samples.
///
/// Both call sites now go through this one function, so they cannot drift
/// apart again — fixing the key without also fixing the raster size (or
/// vice versa) is no longer expressible.
pub fn icon_cache_key(icon: &str, color: &str, target_w: u32, target_h: u32) -> (u32, u32, String) {
    let render_w = target_w.max(1) * ICON_OVERSAMPLE;
    let render_h = target_h.max(1) * ICON_OVERSAMPLE;
    let cache_key = format!("icon:{icon}:{color}:{render_w}x{render_h}");
    (render_w, render_h, cache_key)
}

/// Returns the icon disk-cache directory: `~/.cache/rustmotion/icons`.
///
/// Mirrors [`google_fonts::font_cache_dir`](super::google_fonts::font_cache_dir),
/// which does the same thing for downloaded font files — see that module
/// for the disk-cache-before-network shape this was lifted from.
pub fn icon_cache_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    #[cfg(not(target_os = "windows"))]
    let base = std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".cache"))
        .unwrap_or_else(|| PathBuf::from(".cache"));

    base.join("rustmotion").join("icons")
}

/// Deterministic on-disk file name for a given (icon, color, size). Icon ids
/// contain `:` (`"lucide:home"`); replaced so the id survives as a legible
/// file name instead of being hashed away.
fn icon_cache_file(cache_dir: &Path, icon: &str, color: &str, width: u32, height: u32) -> PathBuf {
    let slug = icon.replace(':', "_");
    let hex_color = color.trim_start_matches('#').to_lowercase();
    cache_dir.join(format!("{slug}-{hex_color}-{width}x{height}.svg"))
}

/// Fetch an icon's SVG bytes, checking the on-disk cache first and falling
/// back to the Iconify API on a miss. Same public signature as before this
/// fix — every existing caller (icon.rs, preload.rs, badge.rs,
/// notification.rs, list.rs, stat.rs) gets the disk cache for free.
pub fn fetch_icon_svg(icon: &str, color: &str, width: u32, height: u32) -> Result<Vec<u8>> {
    fetch_icon_svg_in(icon, color, width, height, &icon_cache_dir())
}

/// Core of [`fetch_icon_svg`], with the cache directory injectable so tests
/// can exercise the cache-hit path without touching `$HOME` or the network —
/// mirrors `google_fonts::resolve_google_font`'s `cache_dir` parameter.
pub fn fetch_icon_svg_in(
    icon: &str,
    color: &str,
    width: u32,
    height: u32,
    cache_dir: &Path,
) -> Result<Vec<u8>> {
    let (prefix, name) =
        icon.split_once(':')
            .ok_or_else(|| RustmotionError::InvalidIconFormat {
                icon: icon.to_string(),
            })?;
    let hex_color = color.trim_start_matches('#');
    let width = width.max(1);
    let height = height.max(1);

    let cache_file = icon_cache_file(cache_dir, icon, color, width, height);
    if let Ok(data) = std::fs::read(&cache_file) {
        if !data.is_empty() {
            return Ok(data);
        }
    }

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

    // Best-effort disk-cache write: failing to persist must not fail a
    // fetch that already succeeded (matches the in-memory `asset_cache`'s
    // existing tolerance for a cache that just doesn't get populated).
    if std::fs::create_dir_all(cache_dir).is_ok() {
        let _ = std::fs::write(&cache_file, &body);
    }

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

/// Returns `true` if `ffmpeg` is on `PATH`.
///
/// Single source of truth for "should we even attempt to shell out to
/// ffmpeg" — `extract_video_frame` below already surfaces a missing binary
/// as `RustmotionError::FfmpegSpawn` on first use, but callers that decode
/// many frames up front (`preload::preextract_video_frames`) want to check
/// once and print one clear warning instead of failing identically once per
/// frame. Mirrors the `ffmpeg_available` helper the `rustmotion` crate's
/// `encode::video_audio` module already uses for the embedded-audio
/// extraction path (PR #151) — same check, same reasoning, different asset
/// kind.
pub fn ffmpeg_available() -> bool {
    std::process::Command::new("ffmpeg")
        .args(["-version"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("rustmotion-test-icons")
            .join(format!(
                "{name}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        std::fs::create_dir_all(&dir).expect("create test cache dir");
        dir
    }

    // ── icon_cache_key: the fix for issue #166 ──────────────────────────────

    /// Regression for issue #166: `icon.rs`'s painter and `preload.rs`'s
    /// prefetcher used to build the cache key independently and disagreed
    /// (painter oversampled, preloader did not — see the RED-phase output
    /// this test replaced: `"icon:lucide:home:#FFFFFF:80x80"` vs
    /// `"icon:lucide:home:#FFFFFF:40x40"`). Both call sites now go through
    /// this one function, so there is only one formula left to test.
    #[test]
    fn oversamples_the_target_size_and_keys_on_the_oversampled_size() {
        let (render_w, render_h, key) = icon_cache_key("lucide:home", "#FFFFFF", 40, 40);
        assert_eq!(render_w, 40 * ICON_OVERSAMPLE);
        assert_eq!(render_h, 40 * ICON_OVERSAMPLE);
        assert_eq!(key, "icon:lucide:home:#FFFFFF:80x80");
    }

    #[test]
    fn zero_target_size_is_clamped_to_at_least_one_before_oversampling() {
        let (render_w, render_h, _key) = icon_cache_key("lucide:home", "#FFFFFF", 0, 0);
        assert_eq!(render_w, ICON_OVERSAMPLE);
        assert_eq!(render_h, ICON_OVERSAMPLE);
    }

    #[test]
    fn distinct_icons_or_colors_never_collide() {
        let (_, _, key_a) = icon_cache_key("lucide:home", "#FFFFFF", 40, 40);
        let (_, _, key_b) = icon_cache_key("lucide:home", "#000000", 40, 40);
        let (_, _, key_c) = icon_cache_key("lucide:settings", "#FFFFFF", 40, 40);
        assert_ne!(key_a, key_b);
        assert_ne!(key_a, key_c);
    }

    // ── fetch_icon_svg_in: disk cache (issue #166 item 2) ────────────────────

    #[test]
    fn disk_cache_hit_returns_bytes_without_touching_the_network() {
        let cache_dir = unique_temp_dir("cache-hit");
        let icon = "test-suite:offline-icon";
        let color = "#ABCDEF";
        let (w, h) = (48, 48);
        let svg_bytes = b"<svg>fake cached icon for the test suite</svg>".to_vec();

        let cache_file = icon_cache_file(&cache_dir, icon, color, w, h);
        std::fs::write(&cache_file, &svg_bytes).unwrap();

        // If this ever fell through to the network, either the test host is
        // offline (fast, deterministic `IconFetch` error — `unwrap` panics
        // clearly) or "test-suite:offline-icon" 404s upstream (same
        // outcome). A silent pass here means the cache was genuinely hit.
        let result = fetch_icon_svg_in(icon, color, w, h, &cache_dir).expect("cache hit");
        assert_eq!(result, svg_bytes);
    }

    #[test]
    fn disk_cache_is_keyed_by_icon_color_and_size() {
        let cache_dir = unique_temp_dir("cache-keying");
        let a = icon_cache_file(&cache_dir, "lucide:home", "#FFFFFF", 80, 80);
        let b = icon_cache_file(&cache_dir, "lucide:home", "#000000", 80, 80);
        let c = icon_cache_file(&cache_dir, "lucide:home", "#FFFFFF", 40, 40);
        assert_ne!(a, b, "different colors must not share a cache file");
        assert_ne!(a, c, "different sizes must not share a cache file");
    }

    #[test]
    fn missing_colon_fails_fast_without_touching_disk_or_network() {
        let cache_dir = unique_temp_dir("invalid-format");
        let result = fetch_icon_svg_in("not-a-valid-icon-id", "#FFFFFF", 40, 40, &cache_dir);
        assert!(matches!(
            result,
            Err(RustmotionError::InvalidIconFormat { .. })
        ));
    }

    // Live network test — mirrors `google_fonts`'s `live_fetch_inter_400`:
    // excluded from normal runs, exercised manually when touching this path.
    #[test]
    #[ignore = "requires network access"]
    fn live_fetch_writes_through_to_the_disk_cache() {
        let cache_dir = unique_temp_dir("live-fetch");
        let icon = "lucide:home";
        let color = "#FFFFFF";
        let (w, h) = (32, 32);

        let first = fetch_icon_svg_in(icon, color, w, h, &cache_dir).expect("live fetch");
        assert!(!first.is_empty());

        let cache_file = icon_cache_file(&cache_dir, icon, color, w, h);
        assert!(
            cache_file.exists(),
            "a successful live fetch must be persisted to disk"
        );

        // A second call must be served from disk. `disk_cache_hit_returns_
        // bytes_without_touching_the_network` already proves the mechanism
        // in isolation; this just confirms the live-written file round-trips.
        let second = fetch_icon_svg_in(icon, color, w, h, &cache_dir).expect("cache hit");
        assert_eq!(first, second);
    }

    // ── ffmpeg_available ──────────────────────────────────────────────────

    #[test]
    fn ffmpeg_available_does_not_panic_either_way() {
        // Not asserting the actual bool: whether ffmpeg is installed depends
        // on the host. This just proves the probe itself cannot panic or
        // hang the preload path that depends on it (item 3).
        let _ = ffmpeg_available();
    }
}
