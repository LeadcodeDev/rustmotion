use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use dashmap::DashMap;

use crate::error::{Result, RustmotionError};

type GifFrame = (Vec<u8>, u32, u32);
type GifData = Arc<(Vec<GifFrame>, Vec<f64>, f64)>;
type GifCacheMap = Arc<DashMap<String, GifData>>;

type VideoFrame = (f64, Vec<u8>, u32, u32);
type VideoFrameList = Arc<Vec<VideoFrame>>;
type VideoFrameCacheMap = Arc<DashMap<String, VideoFrameList>>;

static ASSET_CACHE: OnceLock<Arc<DashMap<String, skia_safe::Image>>> = OnceLock::new();

pub fn asset_cache() -> &'static Arc<DashMap<String, skia_safe::Image>> {
    ASSET_CACHE.get_or_init(|| Arc::new(DashMap::new()))
}

pub fn clear_asset_cache() {
    if let Some(cache) = ASSET_CACHE.get() {
        cache.clear();
    }
}

static GIF_CACHE: OnceLock<GifCacheMap> = OnceLock::new();

pub fn gif_cache() -> &'static GifCacheMap {
    GIF_CACHE.get_or_init(|| Arc::new(DashMap::new()))
}

static HTTP_AGENT: OnceLock<ureq::Agent> = OnceLock::new();

pub fn http_agent() -> &'static ureq::Agent {
    HTTP_AGENT.get_or_init(|| {
        let config = ureq::config::Config::builder()
            .timeout_global(Some(Duration::from_secs(20)))
            .timeout_connect(Some(Duration::from_secs(5)))
            .build();
        ureq::Agent::new_with_config(config)
    })
}

pub const ICON_OVERSAMPLE: u32 = 2;

pub fn icon_cache_key(icon: &str, color: &str, target_w: u32, target_h: u32) -> (u32, u32, String) {
    let render_w = target_w.max(1) * ICON_OVERSAMPLE;
    let render_h = target_h.max(1) * ICON_OVERSAMPLE;
    let cache_key = format!("icon:{icon}:{color}:{render_w}x{render_h}");
    (render_w, render_h, cache_key)
}

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

fn icon_cache_file(cache_dir: &Path, icon: &str, color: &str, width: u32, height: u32) -> PathBuf {
    let slug = icon.replace(':', "_");
    let hex_color = color.trim_start_matches('#').to_lowercase();
    cache_dir.join(format!("{slug}-{hex_color}-{width}x{height}.svg"))
}

pub fn fetch_icon_svg(icon: &str, color: &str, width: u32, height: u32) -> Result<Vec<u8>> {
    fetch_icon_svg_in(icon, color, width, height, &icon_cache_dir())
}

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
    let response = http_agent()
        .get(&url)
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

    if std::fs::create_dir_all(cache_dir).is_ok() {
        let _ = std::fs::write(&cache_file, &body);
    }

    Ok(body)
}

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

pub fn ffmpeg_available() -> bool {
    std::process::Command::new("ffmpeg")
        .args(["-version"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn reject_remote_video_src(src: &str) -> Result<()> {
    let Some(scheme_end) = src.find("://") else {
        return Ok(());
    };
    let scheme = &src[..scheme_end];
    let looks_like_scheme = !scheme.is_empty()
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
    if looks_like_scheme {
        return Err(RustmotionError::Generic(format!(
            "video src '{src}' names a '{scheme}://' URL — rustmotion does not fetch video \
             over the network, only local file paths are accepted (RM-42)"
        )));
    }
    Ok(())
}

pub fn extract_video_frame(src: &str, time: f64, width: u32, height: u32) -> Result<Vec<u8>> {
    reject_remote_video_src(src)?;
    let output = std::process::Command::new("ffmpeg")
        .args([
            "-protocol_whitelist",
            "file",
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

pub fn probe_image_dimensions(path: &str) -> Result<(u32, u32)> {
    let reader = image::ImageReader::open(path)
        .map_err(|e| RustmotionError::ImageLoad {
            path: path.to_string(),
            reason: e.to_string(),
        })?
        .with_guessed_format()
        .map_err(|e| RustmotionError::ImageLoad {
            path: path.to_string(),
            reason: e.to_string(),
        })?;
    reader
        .into_dimensions()
        .map_err(|e| RustmotionError::ImageLoad {
            path: path.to_string(),
            reason: e.to_string(),
        })
}

pub fn ffprobe_available() -> bool {
    std::process::Command::new("ffprobe")
        .args(["-version"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[derive(Debug, Clone, PartialEq)]
pub struct VideoProbe {
    pub width: u32,
    pub height: u32,
    pub duration_secs: f64,
    pub fps: Option<f64>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct FfprobeOutput {
    #[serde(default)]
    streams: Vec<FfprobeStream>,
    #[serde(default)]
    format: Option<FfprobeFormat>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct FfprobeStream {
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
    #[serde(default)]
    r_frame_rate: Option<String>,
    #[serde(default)]
    duration: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct FfprobeFormat {
    #[serde(default)]
    duration: Option<String>,
}

fn parse_frame_rate(s: &str) -> Option<f64> {
    let (num, den) = s.split_once('/')?;
    let num: f64 = num.trim().parse().ok()?;
    let den: f64 = den.trim().parse().ok()?;
    if den == 0.0 {
        return None;
    }
    Some(num / den)
}

pub fn probe_video_metadata(src: &str) -> Result<VideoProbe> {
    if !ffprobe_available() {
        return Err(RustmotionError::Generic(format!(
            "Cannot read metadata for '{src}': ffprobe not found on PATH. ffprobe ships with \
             ffmpeg — install it with `brew install ffmpeg` (macOS) or see \
             https://ffmpeg.org/download.html."
        )));
    }

    let output = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_streams",
            "-show_format",
            "-of",
            "json",
            src,
        ])
        .output()
        .map_err(|e| RustmotionError::Generic(format!("Failed to run ffprobe on '{src}': {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(RustmotionError::Generic(format!(
            "ffprobe could not read '{src}': {}",
            stderr.trim()
        )));
    }

    let parsed: FfprobeOutput = serde_json::from_slice(&output.stdout).map_err(|e| {
        RustmotionError::Generic(format!(
            "ffprobe produced output that could not be parsed for '{src}': {e}"
        ))
    })?;

    let stream = parsed.streams.first().ok_or_else(|| {
        RustmotionError::Generic(format!("'{src}' has no video stream ffprobe could find"))
    })?;

    let width = stream
        .width
        .ok_or_else(|| RustmotionError::Generic(format!("'{src}': ffprobe reported no width")))?;
    let height = stream
        .height
        .ok_or_else(|| RustmotionError::Generic(format!("'{src}': ffprobe reported no height")))?;

    let duration_secs = stream
        .duration
        .as_deref()
        .and_then(|d| d.parse::<f64>().ok())
        .or_else(|| {
            parsed
                .format
                .as_ref()
                .and_then(|f| f.duration.as_deref())
                .and_then(|d| d.parse::<f64>().ok())
        })
        .ok_or_else(|| {
            RustmotionError::Generic(format!("'{src}': ffprobe reported no duration"))
        })?;

    let fps = stream.r_frame_rate.as_deref().and_then(parse_frame_rate);

    Ok(VideoProbe {
        width,
        height,
        duration_secs,
        fps,
    })
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

    #[test]
    fn disk_cache_hit_returns_bytes_without_touching_the_network() {
        let cache_dir = unique_temp_dir("cache-hit");
        let icon = "test-suite:offline-icon";
        let color = "#ABCDEF";
        let (w, h) = (48, 48);
        let svg_bytes = b"<svg>fake cached icon for the test suite</svg>".to_vec();

        let cache_file = icon_cache_file(&cache_dir, icon, color, w, h);
        std::fs::write(&cache_file, &svg_bytes).unwrap();

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

        let second = fetch_icon_svg_in(icon, color, w, h, &cache_dir).expect("cache hit");
        assert_eq!(first, second);
    }

    #[test]
    fn ffmpeg_available_does_not_panic_either_way() {
        let _ = ffmpeg_available();
    }

    fn scratch_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "rm_assets_probe_test_{}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            name
        ))
    }

    fn write_test_png(path: &Path, w: u32, h: u32) {
        let img = image::RgbImage::from_pixel(w, h, image::Rgb([10, 20, 30]));
        img.save(path).expect("write PNG fixture");
    }

    fn write_test_gif(path: &Path, w: u32, h: u32) {
        use image::codecs::gif::GifEncoder;
        let file = std::fs::File::create(path).expect("create GIF fixture");
        let mut encoder = GifEncoder::new(file);
        let frame = image::Frame::new(image::RgbaImage::from_pixel(
            w,
            h,
            image::Rgba([200, 50, 10, 255]),
        ));
        encoder.encode_frame(frame).expect("encode GIF fixture");
    }

    #[test]
    fn probe_image_dimensions_reads_a_png_header() {
        let path = scratch_path("dims.png");
        write_test_png(&path, 37, 21);
        let (w, h) = probe_image_dimensions(path.to_str().unwrap()).expect("must read PNG dims");
        assert_eq!((w, h), (37, 21));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn probe_image_dimensions_reads_a_gif_header() {
        let path = scratch_path("dims.gif");
        write_test_gif(&path, 12, 9);
        let (w, h) = probe_image_dimensions(path.to_str().unwrap()).expect("must read GIF dims");
        assert_eq!((w, h), (12, 9));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn probe_image_dimensions_on_a_missing_file_is_an_error_not_a_panic() {
        let path = scratch_path("does-not-exist.png");
        let result = probe_image_dimensions(path.to_str().unwrap());
        assert!(
            result.is_err(),
            "missing file must be an error, not a panic"
        );
    }

    #[test]
    fn probe_image_dimensions_on_garbage_bytes_is_an_error_not_a_panic() {
        let path = scratch_path("garbage.png");
        std::fs::write(&path, b"this is not an image").unwrap();
        let result = probe_image_dimensions(path.to_str().unwrap());
        assert!(
            result.is_err(),
            "unreadable content must be an error, not a panic"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn parse_frame_rate_reads_integer_and_ntsc_fractions() {
        assert_eq!(parse_frame_rate("30/1"), Some(30.0));
        assert!((parse_frame_rate("30000/1001").unwrap() - 29.97).abs() < 0.01);
    }

    #[test]
    fn parse_frame_rate_rejects_zero_denominator_and_garbage() {
        assert_eq!(parse_frame_rate("30/0"), None);
        assert_eq!(parse_frame_rate("not-a-rate"), None);
    }

    fn make_test_video(path: &Path, width: u32, height: u32, fps: u32, duration_s: u32) -> bool {
        std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                &format!("testsrc=size={width}x{height}:rate={fps}:duration={duration_s}"),
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(path)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[test]
    fn probe_video_metadata_reads_dimensions_duration_and_fps() {
        if !ffmpeg_available() || !ffprobe_available() {
            eprintln!(
                "probe_video_metadata_reads_dimensions_duration_and_fps: ffmpeg/ffprobe not \
                 found on PATH — skipping"
            );
            return;
        }
        let path = scratch_path("probe.mp4");
        assert!(
            make_test_video(&path, 64, 36, 25, 2),
            "fixture video must encode"
        );

        let probe =
            probe_video_metadata(path.to_str().unwrap()).expect("must probe video metadata");
        assert_eq!(probe.width, 64);
        assert_eq!(probe.height, 36);
        assert!(
            (probe.duration_secs - 2.0).abs() < 0.2,
            "duration: {}",
            probe.duration_secs
        );
        assert!(probe.fps.is_some(), "expected a frame rate");
        assert!(
            (probe.fps.unwrap() - 25.0).abs() < 0.1,
            "fps: {:?}",
            probe.fps
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn probe_video_metadata_on_a_missing_file_is_an_error_not_a_panic() {
        if !ffprobe_available() {
            eprintln!(
                "probe_video_metadata_on_a_missing_file_is_an_error_not_a_panic: ffprobe not \
                 found on PATH — skipping"
            );
            return;
        }
        let path = scratch_path("does-not-exist.mp4");
        let result = probe_video_metadata(path.to_str().unwrap());
        assert!(
            result.is_err(),
            "missing file must be an error, not a panic"
        );
    }

    #[test]
    fn probe_video_metadata_on_garbage_bytes_is_an_error_not_a_panic() {
        if !ffprobe_available() {
            eprintln!(
                "probe_video_metadata_on_garbage_bytes_is_an_error_not_a_panic: ffprobe not \
                 found on PATH — skipping"
            );
            return;
        }
        let path = scratch_path("garbage.mp4");
        std::fs::write(&path, b"not a real video file").unwrap();
        let result = probe_video_metadata(path.to_str().unwrap());
        assert!(
            result.is_err(),
            "unreadable content must be an error, not a panic"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn ffprobe_available_does_not_panic_either_way() {
        let _ = ffprobe_available();
    }
}
