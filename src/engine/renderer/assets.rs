use std::sync::{Arc, OnceLock};

use dashmap::DashMap;
use skia_safe::{ColorType, ImageInfo};

use crate::components::{ChildComponent, Component};
use crate::error::{Result, RustmotionError};
use crate::traits::Styled;

/// Global asset cache for decoded images (keyed by file path)
static ASSET_CACHE: OnceLock<Arc<DashMap<String, skia_safe::Image>>> = OnceLock::new();

pub(crate) fn asset_cache() -> &'static Arc<DashMap<String, skia_safe::Image>> {
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
static GIF_CACHE: OnceLock<Arc<DashMap<String, Arc<(Vec<(Vec<u8>, u32, u32)>, Vec<f64>, f64)>>>> = OnceLock::new();

pub(crate) fn gif_cache() -> &'static Arc<DashMap<String, Arc<(Vec<(Vec<u8>, u32, u32)>, Vec<f64>, f64)>>> {
    GIF_CACHE.get_or_init(|| Arc::new(DashMap::new()))
}

// ─── Icon fetching ──────────────────────────────────────────────────────────

pub(crate) fn fetch_icon_svg(icon: &str, color: &str, width: u32, height: u32) -> Result<Vec<u8>> {
    let (prefix, name) = icon
        .split_once(':')
        .ok_or_else(|| RustmotionError::InvalidIconFormat { icon: icon.to_string() })?;
    let hex_color = color.trim_start_matches('#');
    let width = width.max(1);
    let height = height.max(1);
    let url = format!(
        "https://api.iconify.design/{}/{}.svg?color=%23{}&width={}&height={}",
        prefix, name, hex_color, width, height
    );
    let response = ureq::get(&url)
        .call()
        .map_err(|e| RustmotionError::IconFetch { icon: icon.to_string(), reason: e.to_string() })?;
    let body = response
        .into_body()
        .read_to_vec()
        .map_err(|e| RustmotionError::IconFetch { icon: icon.to_string(), reason: e.to_string() })?;
    Ok(body)
}

/// Pre-fetch and cache all icon components before rendering.
/// Call this before the render loop to avoid HTTP requests during parallel rendering.
pub fn prefetch_icons(scenes: &[crate::schema::Scene]) {
    use std::collections::HashSet;

    let mut seen = HashSet::new();

    fn collect_from_component(
        child: &ChildComponent,
        seen: &mut HashSet<(String, String, u32, u32)>,
    ) {
        match &child.component {
            Component::Icon(icon) => {
                let (w, h) = match &icon.size {
                    Some(size) => (size.width as u32, size.height as u32),
                    None => (24, 24),
                };
                seen.insert((icon.icon.clone(), icon.style_config().color_or("#FFFFFF").to_string(), w, h));
            }
            Component::Card(c) => {
                for child in &c.children {
                    collect_from_component(child, seen);
                }
            }
            Component::Flex(c) => {
                for child in &c.children {
                    collect_from_component(child, seen);
                }
            }
            Component::Grid(c) => {
                for child in &c.children {
                    collect_from_component(child, seen);
                }
            }
            Component::Positioned(c) => {
                for child in &c.children {
                    collect_from_component(child, seen);
                }
            }
            Component::Container(c) => {
                for child in &c.children {
                    collect_from_component(child, seen);
                }
            }
            _ => {}
        }
    }

    for scene in scenes {
        for child in &scene.children {
            collect_from_component(child, &mut seen);
        }
    }

    let cache = asset_cache();
    for (icon, color, w, h) in &seen {
        let cache_key = format!("icon:{}:{}:{}x{}", icon, color, w, h);
        if cache.contains_key(&cache_key) {
            continue;
        }
        match fetch_icon_svg(icon, color, *w, *h) {
            Ok(svg_data) => {
                let opt = usvg::Options::default();
                match usvg::Tree::from_data(&svg_data, &opt) {
                    Ok(tree) => {
                        let svg_size = tree.size();
                        let render_w = (*w).max(1);
                        let render_h = (*h).max(1);
                        if let Some(mut pixmap) = tiny_skia::Pixmap::new(render_w, render_h) {
                            let scale_x = render_w as f32 / svg_size.width();
                            let scale_y = render_h as f32 / svg_size.height();
                            let transform = tiny_skia::Transform::from_scale(scale_x, scale_y);
                            resvg::render(&tree, transform, &mut pixmap.as_mut());
                            let img_data = skia_safe::Data::new_copy(pixmap.data());
                            let img_info = ImageInfo::new(
                                (render_w as i32, render_h as i32),
                                ColorType::RGBA8888,
                                skia_safe::AlphaType::Premul,
                                None,
                            );
                            if let Some(decoded) = skia_safe::images::raster_from_data(
                                &img_info,
                                img_data,
                                render_w as usize * 4,
                            ) {
                                cache.insert(cache_key, decoded);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Warning: failed to parse icon '{}': {}", icon, e);
                    }
                }
            }
            Err(e) => {
                eprintln!("Warning: failed to fetch icon '{}': {}", icon, e);
            }
        }
    }
}

// ─── Video frame extraction ─────────────────────────────────────────────────

/// Cache for pre-extracted video frames: key = "src:width:height", value = sorted list of (time, RGBA data, width, height)
static VIDEO_FRAME_CACHE: OnceLock<Arc<DashMap<String, Arc<Vec<(f64, Vec<u8>, u32, u32)>>>>> = OnceLock::new();

pub(crate) fn video_frame_cache() -> &'static Arc<DashMap<String, Arc<Vec<(f64, Vec<u8>, u32, u32)>>>> {
    VIDEO_FRAME_CACHE.get_or_init(|| Arc::new(DashMap::new()))
}

/// Pre-extract all needed frames from video sources in a single ffmpeg pass.
/// Called before the render loop to populate the video frame cache.
pub fn preextract_video_frames(
    scenarios_scenes: &[crate::schema::Scene],
    fps: u32,
) {
    use crate::traits::Timed;

    fn collect_videos(child: &ChildComponent, scene_frames: u32, fps: u32) {
        if let Component::Video(video) = &child.component {
            let rate = video.playback_rate.unwrap_or(1.0);
            let trim_start = video.trim_start.unwrap_or(0.0);
            let width = video.size.width as u32;
            let height = video.size.height as u32;

            let cache_key = format!("{}:{}x{}", video.src, width, height);
            let cache = video_frame_cache();

            if cache.contains_key(&cache_key) {
                return;
            }

            let (start_at, end_at) = video.timing();
            let start_frame = start_at.map(|s| (s * fps as f64).round() as u32).unwrap_or(0);
            let end_frame = end_at.map(|e| (e * fps as f64).round() as u32).unwrap_or(scene_frames);

            let mut times = Vec::new();
            for f in start_frame..end_frame {
                let time = f as f64 / fps as f64;
                let source_time = trim_start + time * rate;
                times.push(source_time);
            }

            if times.is_empty() {
                return;
            }

            let min_time = times.first().copied().unwrap_or(0.0);
            let max_time = times.last().copied().unwrap_or(0.0);
            let duration = max_time - min_time + (1.0 / fps as f64);

            let output = std::process::Command::new("ffmpeg")
                .args([
                    "-ss", &format!("{:.3}", min_time),
                    "-t", &format!("{:.3}", duration),
                    "-i", &video.src,
                    "-vf", &format!("fps={},scale={}:{}", fps, width, height),
                    "-f", "rawvideo",
                    "-pix_fmt", "rgba",
                    "-y", "pipe:1",
                ])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .output();

            match output {
                Ok(output) if output.status.success() => {
                    let frame_size = (width * height * 4) as usize;
                    let data = &output.stdout;
                    let num_frames = data.len() / frame_size;
                    let mut frames: Vec<(f64, Vec<u8>, u32, u32)> = Vec::with_capacity(num_frames);

                    for idx in 0..num_frames {
                        let start = idx * frame_size;
                        let frame_data = data[start..start + frame_size].to_vec();
                        let time = min_time + idx as f64 / fps as f64;
                        frames.push((time, frame_data, width, height));
                    }

                    cache.insert(cache_key, Arc::new(frames));
                }
                _ => {}
            }
        }

        // Recurse into containers
        if let Some(children) = match &child.component {
            Component::Card(c) => Some(&c.children),
            Component::Flex(c) => Some(&c.children),
            Component::Grid(c) => Some(&c.children),
            Component::Positioned(c) => Some(&c.children),
            Component::Container(c) => Some(&c.children),
            _ => None,
        } {
            for c in children {
                collect_videos(c, scene_frames, fps);
            }
        }
    }

    for scene in scenarios_scenes {
        let scene_frames = (scene.duration * fps as f64).round() as u32;
        for child in &scene.children {
            collect_videos(child, scene_frames, fps);
        }
    }
}

pub(crate) fn find_closest_frame(frames: &[(f64, Vec<u8>, u32, u32)], target_time: f64) -> Option<(&[u8], u32, u32)> {
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

pub(crate) fn extract_video_frame(src: &str, time: f64, width: u32, height: u32) -> Result<Vec<u8>> {
    let output = std::process::Command::new("ffmpeg")
        .args([
            "-ss", &format!("{:.3}", time),
            "-i", src,
            "-vframes", "1",
            "-vf", &format!("scale={}:{}", width, height),
            "-f", "image2pipe",
            "-vcodec", "png",
            "-y", "pipe:1",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .map_err(|e| RustmotionError::FfmpegSpawn { reason: e.to_string() })?;

    if !output.status.success() {
        return Err(RustmotionError::FfmpegFrameExtract { src: src.to_string() }.into());
    }

    Ok(output.stdout)
}
