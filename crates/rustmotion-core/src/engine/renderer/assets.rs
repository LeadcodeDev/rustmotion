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

pub fn icon_source_cache_file(cache_dir: &Path, icon: &str) -> PathBuf {
    cache_dir.join(format!("{}.svg", icon.replace(':', "_")))
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RemoteIconPolicy {
    #[default]
    Deny,
    Allow,
}

static REMOTE_ICON_POLICY: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub fn set_remote_icon_policy(policy: RemoteIconPolicy) {
    REMOTE_ICON_POLICY.store(
        policy == RemoteIconPolicy::Allow,
        std::sync::atomic::Ordering::Release,
    );
}

pub fn remote_icon_policy() -> RemoteIconPolicy {
    if REMOTE_ICON_POLICY.load(std::sync::atomic::Ordering::Acquire) {
        RemoteIconPolicy::Allow
    } else {
        RemoteIconPolicy::Deny
    }
}

pub fn icon_source_url(prefix: &str, name: &str) -> String {
    format!("https://api.iconify.design/{prefix}/{name}.svg")
}

fn set_root_attribute(svg: &str, attribute: &str, value: &str) -> String {
    let Some(tag_start) = svg.find("<svg") else {
        return svg.to_string();
    };
    let Some(tag_len) = svg[tag_start..].find('>') else {
        return svg.to_string();
    };
    let tag = &svg[tag_start..tag_start + tag_len];

    let needle = format!(" {attribute}=\"");
    let replaced = match tag.find(&needle) {
        Some(at) => {
            let value_start = at + needle.len();
            match tag[value_start..].find('"') {
                Some(value_len) => format!(
                    "{}{}\"{}",
                    &tag[..value_start],
                    value,
                    &tag[value_start + value_len + 1..]
                ),
                None => tag.to_string(),
            }
        }
        None => format!("<svg {attribute}=\"{value}\"{}", &tag["<svg".len()..]),
    };

    format!(
        "{}{}{}",
        &svg[..tag_start],
        replaced,
        &svg[tag_start + tag_len..]
    )
}

fn recolour_and_resize(source: &[u8], hex_color: &str, width: u32, height: u32) -> Vec<u8> {
    let coloured =
        String::from_utf8_lossy(source).replace("currentColor", &format!("#{hex_color}"));
    let sized = set_root_attribute(&coloured, "width", &width.to_string());
    set_root_attribute(&sized, "height", &height.to_string()).into_bytes()
}

fn legacy_icon_source(cache_dir: &Path, icon: &str) -> Option<Vec<u8>> {
    let slug = icon.replace(':', "_");
    let prefix = format!("{slug}-");
    let entries = std::fs::read_dir(cache_dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_str()?;
        let Some(rest) = name.strip_prefix(&prefix) else {
            continue;
        };
        let Some(rest) = rest.strip_suffix(".svg") else {
            continue;
        };
        let Some((colour, size)) = rest.split_once('-') else {
            continue;
        };
        if colour.len() != 6 || !colour.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        let Some((w, h)) = size.split_once('x') else {
            continue;
        };
        if w.parse::<u32>().is_err() || h.parse::<u32>().is_err() {
            continue;
        }
        let baked = std::fs::read_to_string(entry.path()).ok()?;
        let restored = baked
            .replace(&format!("#{}", colour.to_lowercase()), "currentColor")
            .replace(&format!("#{}", colour.to_uppercase()), "currentColor");
        if restored.contains("currentColor") {
            return Some(restored.into_bytes());
        }
    }
    None
}

const ICON_FETCH_ATTEMPTS: u32 = 4;

fn status_is_worth_retrying(error: &ureq::Error) -> bool {
    matches!(error, ureq::Error::StatusCode(code) if *code == 429 || *code >= 500)
}

fn fetch_icon_source(icon: &str, prefix: &str, name: &str) -> Result<Vec<u8>> {
    let url = icon_source_url(prefix, name);
    let mut backoff = Duration::from_millis(250);
    let mut last_reason = String::new();

    for attempt in 0..ICON_FETCH_ATTEMPTS {
        match http_agent().get(&url).call() {
            Ok(response) => {
                return response
                    .into_body()
                    .read_to_vec()
                    .map_err(|e| RustmotionError::IconFetch {
                        icon: icon.to_string(),
                        reason: e.to_string(),
                    })
            }
            Err(e) => {
                last_reason = e.to_string();
                let is_last_attempt = attempt + 1 == ICON_FETCH_ATTEMPTS;
                if !status_is_worth_retrying(&e) || is_last_attempt {
                    break;
                }
                std::thread::sleep(backoff);
                backoff *= 2;
            }
        }
    }

    Err(RustmotionError::IconFetch {
        icon: icon.to_string(),
        reason: last_reason,
    })
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

    let hex_color = hex_color.to_lowercase();
    let source_file = icon_source_cache_file(cache_dir, icon);

    let source = match std::fs::read(&source_file) {
        Ok(data) if !data.is_empty() => data,
        _ => match legacy_icon_source(cache_dir, icon) {
            Some(migrated) => {
                let _ = std::fs::write(&source_file, &migrated);
                migrated
            }
            None => {
                if remote_icon_policy() != RemoteIconPolicy::Allow {
                    return Err(RustmotionError::RemoteIconDenied {
                        icon: icon.to_string(),
                        url: icon_source_url(prefix, name),
                        cache_hint: source_file.display().to_string(),
                    });
                }
                let fetched = fetch_icon_source(icon, prefix, name)?;
                if std::fs::create_dir_all(cache_dir).is_ok() {
                    let _ = std::fs::write(&source_file, &fetched);
                }
                fetched
            }
        },
    };

    Ok(recolour_and_resize(&source, &hex_color, width, height))
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

        let cache_file = icon_source_cache_file(&cache_dir, icon);
        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::write(&cache_file, &svg_bytes).unwrap();

        let result = fetch_icon_svg_in(icon, color, w, h, &cache_dir).expect("cache hit");
        let result = String::from_utf8(result).unwrap();
        assert!(
            result.contains("fake cached icon for the test suite"),
            "the cached source must be what is served — the network was never reached: {result}"
        );
        assert!(
            result.contains(r#"width="48""#),
            "and it is resized on the way out, which is what lets one cached source serve \
             every colour and size: {result}"
        );
    }

    #[test]
    fn disk_cache_is_keyed_by_the_icon_alone() {
        let cache_dir = unique_temp_dir("cache-keying");
        assert_eq!(
            icon_source_cache_file(&cache_dir, "lucide:home"),
            icon_source_cache_file(&cache_dir, "lucide:home"),
        );
        assert_ne!(
            icon_source_cache_file(&cache_dir, "lucide:home"),
            icon_source_cache_file(&cache_dir, "lucide:check"),
        );
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

        let cache_file = icon_source_cache_file(&cache_dir, icon);
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

#[cfg(test)]
mod icon_source_cache_tests {
    use super::*;

    const LUCIDE_CHECK: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M20 6 9 17l-5-5"/></svg>"#;

    #[test]
    fn the_disk_entry_is_named_by_the_icon_alone() {
        let dir = Path::new("/tmp/icons");
        assert_eq!(
            icon_source_cache_file(dir, "lucide:check"),
            dir.join("lucide_check.svg"),
            "one entry per icon: naming it by colour and size is what filled a cache with 20 \
             copies of the same glyph and made a colour change hit the network"
        );
    }

    fn a_cache_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rm_legacy_icons_{}_{}_{tag}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("cache dir");
        dir
    }

    #[test]
    fn a_file_left_by_the_old_naming_is_migrated_instead_of_refetched() {
        let dir = a_cache_dir("migrate");
        let baked = LUCIDE_CHECK.replace("currentColor", "#3B6FD4");
        std::fs::write(dir.join("lucide_check-3b6fd4-26x26.svg"), &baked).expect("seed");

        let migrated = legacy_icon_source(&dir, "lucide:check")
            .expect("674 of these sit in a real cache; an offline render must be able to use one");
        let text = String::from_utf8(migrated).unwrap();
        assert!(
            text.contains("currentColor"),
            "the baked colour has to come back out, or the icon can never be recoloured: {text}"
        );
        assert!(!text.contains("3B6FD4"), "got {text}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn migration_survives_an_icon_whose_own_name_contains_a_dash() {
        let dir = a_cache_dir("dashed");
        let baked = LUCIDE_CHECK.replace("currentColor", "#8b5cf6");
        std::fs::write(dir.join("lucide_arrow-down-8b5cf6-176x176.svg"), &baked).expect("seed");
        assert!(
            legacy_icon_source(&dir, "lucide:arrow-down").is_some(),
            "the colour and size have to be parsed from the right, not from the first dash"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_unrelated_file_is_not_mistaken_for_a_legacy_entry() {
        let dir = a_cache_dir("unrelated");
        std::fs::write(dir.join("lucide_check-notes.svg"), LUCIDE_CHECK).expect("seed");
        std::fs::write(dir.join("lucide_checkmark.svg"), LUCIDE_CHECK).expect("seed");
        assert!(
            legacy_icon_source(&dir, "lucide:check").is_none(),
            "only a name ending in -{{6 hex}}-{{w}}x{{h}} is a legacy entry"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_rewritten_icon_still_parses_as_svg() {
        let out = recolour_and_resize(LUCIDE_CHECK.as_bytes(), "2563eb", 84, 84);
        let text = String::from_utf8(out.clone()).unwrap();
        assert!(
            !text.contains("\"\""),
            "the rewrite must not leave the previous value's closing quote behind: {text}"
        );
        usvg::Tree::from_data(&out, &usvg::Options::default()).unwrap_or_else(|e| {
            panic!(
                "a rewritten icon has to parse, or nothing renders and the only sign is a \
                 warning on stderr: {e}\n{text}"
            )
        });
    }

    #[test]
    fn rewriting_an_attribute_that_already_has_a_value_replaces_it_exactly_once() {
        let source = r#"<svg xmlns="http://www.w3.org/2000/svg" width="1em" height="1em" viewBox="0 0 24 24"><path d="M4 12h16"/></svg>"#;
        let out = String::from_utf8(recolour_and_resize(source.as_bytes(), "000000", 84, 96))
            .expect("utf8");
        assert_eq!(
            out.matches("width=").count(),
            1,
            "one width attribute, not two: {out}"
        );
        assert!(out.contains(r#"width="84""#), "got {out}");
        assert!(out.contains(r#"height="96""#), "got {out}");
        assert!(
            !out.contains("1em"),
            "the 1em Iconify ships must be gone: {out}"
        );
    }

    #[test]
    fn recolouring_substitutes_current_color_everywhere() {
        let out = recolour_and_resize(LUCIDE_CHECK.as_bytes(), "2a2f6b", 84, 84);
        let out = String::from_utf8(out).unwrap();
        assert!(!out.contains("currentColor"));
        assert!(out.contains(r##"stroke="#2a2f6b""##), "got {out}");
    }

    #[test]
    fn resizing_rewrites_the_root_size_and_leaves_the_viewbox_alone() {
        let out = recolour_and_resize(LUCIDE_CHECK.as_bytes(), "ffffff", 84, 96);
        let out = String::from_utf8(out).unwrap();
        assert!(out.contains(r#"width="84""#), "got {out}");
        assert!(out.contains(r#"height="96""#), "got {out}");
        assert!(
            out.contains(r#"viewBox="0 0 24 24""#),
            "the viewBox is the glyph's own coordinate space and must survive a resize: {out}"
        );
    }

    #[test]
    fn a_root_without_a_size_gains_one() {
        let bare = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"></svg>"#;
        let out =
            String::from_utf8(recolour_and_resize(bare.as_bytes(), "000000", 32, 32)).unwrap();
        assert!(
            out.contains(r#"width="32""#) && out.contains(r#"height="32""#),
            "got {out}"
        );
    }

    #[test]
    fn two_colours_of_one_icon_share_a_single_source_entry() {
        let dir = Path::new("/tmp/icons");
        assert_eq!(
            icon_source_cache_file(dir, "lucide:sparkles"),
            icon_source_cache_file(dir, "lucide:sparkles")
        );
        let red = recolour_and_resize(LUCIDE_CHECK.as_bytes(), "ff0000", 24, 24);
        let blue = recolour_and_resize(LUCIDE_CHECK.as_bytes(), "0000ff", 24, 24);
        assert_ne!(
            red, blue,
            "the same source still produces different renders"
        );
    }

    #[test]
    fn only_rate_limits_and_server_faults_are_retried() {
        assert!(status_is_worth_retrying(&ureq::Error::StatusCode(429)));
        assert!(status_is_worth_retrying(&ureq::Error::StatusCode(503)));
        assert!(
            !status_is_worth_retrying(&ureq::Error::StatusCode(404)),
            "a missing icon is not going to appear on the fourth try — retrying it would just \
             make a typo take four times as long to report"
        );
    }
}

#[cfg(test)]
mod remote_icon_policy_tests {
    use super::*;

    struct Restore(RemoteIconPolicy);
    impl Drop for Restore {
        fn drop(&mut self) {
            set_remote_icon_policy(self.0);
        }
    }

    fn a_cache_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rm_icon_policy_{}_{}_{name}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("cache dir");
        dir
    }

    const A_SQUARE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="24" height="24">
             <rect x="2" y="2" width="20" height="20" fill="currentColor"/>
           </svg>"#;

    #[test]
    fn the_default_is_deny() {
        assert_eq!(
            RemoteIconPolicy::default(),
            RemoteIconPolicy::Deny,
            "a scenario chooses which icon is fetched, so it chooses the target of the \
             request — the same reason fonts are denied by default"
        );
    }

    #[test]
    fn an_icon_that_is_not_cached_is_refused_by_name_without_touching_the_network() {
        let _restore = Restore(remote_icon_policy());
        set_remote_icon_policy(RemoteIconPolicy::Deny);
        let dir = a_cache_dir("denied");

        let message = fetch_icon_svg_in("lucide:sparkles", "#FFFFFF", 24, 24, &dir)
            .expect_err("an icon absent from the cache would have to be fetched")
            .to_string();

        assert!(
            message.contains("lucide:sparkles"),
            "the refusal has to name the icon: {message}"
        );
        assert!(
            message.contains("api.iconify.design"),
            "and the target it declined to reach: {message}"
        );
        assert!(
            message.contains("rustmotion icons prefetch")
                && message.contains("--allow-remote-icons"),
            "and both ways out — fill the cache once, or opt in for this run: {message}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_icon_already_in_the_cache_needs_no_flag() {
        let _restore = Restore(remote_icon_policy());
        set_remote_icon_policy(RemoteIconPolicy::Deny);
        let dir = a_cache_dir("cached");
        std::fs::write(icon_source_cache_file(&dir, "lucide:check"), A_SQUARE).expect("seed");

        let svg = fetch_icon_svg_in("lucide:check", "#FF3366", 24, 24, &dir)
            .expect("nothing has to be fetched, so nothing is denied");
        assert!(
            String::from_utf8_lossy(&svg).contains("ff3366"),
            "the cached source must still be recoloured and returned under a deny policy"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_cache_left_by_the_old_naming_is_migrated_without_the_flag() {
        let _restore = Restore(remote_icon_policy());
        set_remote_icon_policy(RemoteIconPolicy::Deny);
        let dir = a_cache_dir("legacy");
        let baked = A_SQUARE.replace("currentColor", "#ffffff");
        std::fs::write(dir.join("lucide_home-ffffff-24x24.svg"), baked).expect("seed");

        let svg = fetch_icon_svg_in("lucide:home", "#33CCFF", 24, 24, &dir)
            .expect("the legacy entry is on disk, so the deny gate must never be reached");
        assert!(
            String::from_utf8_lossy(&svg).contains("33ccff"),
            "an offline render that already has its icons under the pre-#425 naming must keep \
             working: the gate belongs after the migration, not before it"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
