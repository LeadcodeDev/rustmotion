use std::io::Read;
use std::sync::Arc;

use crate::components::{ChildComponent, Component};
use crate::schema::Scene;
use rustmotion_core::engine::renderer::{
    asset_cache, fetch_icon_svg, ffmpeg_available, icon_cache_dir, icon_cache_key,
    sandboxed_svg_options, video_frame_cache,
};
use rustmotion_core::traits::{Styled, Timed};

/// Total bytes `VIDEO_FRAME_CACHE` may hold across every distinct
/// `(src, width, height)` entry combined. `preextract_video_frames` refuses
/// to add an entry that would push the cache past this ceiling rather than
/// caching it anyway — a video past the budget renders blank for the
/// affected frames, the same degraded outcome an ffmpeg failure already
/// produces on this path, instead of the process exhausting memory (a single
/// 1080p 30s embed alone reaches ~7.5 GB of raw decoded RGBA held in memory
/// forever, with no eviction).
pub const VIDEO_FRAME_CACHE_BUDGET_BYTES: u64 = 1024 * 1024 * 1024;

/// Bytes one raw RGBA frame at `width`×`height` occupies, computed in `u64`
/// and saturating rather than the plain `u32` multiplication this used to be:
/// `width * height * 4` in `u32` wraps for a large-enough declared size
/// (65536×16384 wraps to 0), which downstream turned into a division by
/// zero. Saturating instead of panicking means an absurd declared size still
/// fails the budget check below rather than crashing the preload pass.
pub fn video_frame_byte_size(width: u32, height: u32) -> u64 {
    u64::from(width)
        .saturating_mul(u64::from(height))
        .saturating_mul(4)
}

/// Whether caching `additional_bytes` more on top of `already_cached_bytes`
/// would cross [`VIDEO_FRAME_CACHE_BUDGET_BYTES`]. Saturating so a caller
/// that already (somehow) exceeds the budget, or an `additional_bytes` at
/// `u64::MAX` from a saturated [`video_frame_byte_size`], still reports
/// "over budget" instead of wrapping back under it.
pub fn would_exceed_cache_budget(already_cached_bytes: u64, additional_bytes: u64) -> bool {
    already_cached_bytes.saturating_add(additional_bytes) > VIDEO_FRAME_CACHE_BUDGET_BYTES
}

/// Bytes currently held across every entry of the process-global video-frame
/// cache. `VIDEO_FRAME_CACHE` has no eviction (see `assets.rs`), so this is a
/// running total the caller checks before adding to it, not a size taken
/// from any single-entry accounting the map itself keeps.
fn video_frame_cache_bytes() -> u64 {
    video_frame_cache()
        .iter()
        .map(|entry| {
            entry
                .value()
                .iter()
                .map(|(_, data, _, _)| data.len() as u64)
                .sum::<u64>()
        })
        .sum()
}

/// Pre-fetch and cache all icon components before rendering.
/// Call this before the render loop to avoid HTTP requests during parallel rendering.
pub fn prefetch_icons(scenes: &[Scene]) {
    use std::collections::HashSet;

    let mut seen = HashSet::new();

    fn collect_from_component(
        child: &ChildComponent,
        seen: &mut HashSet<(String, String, u32, u32)>,
    ) {
        match &child.component {
            Component::Icon(icon) => {
                // Size now comes from CSS style; at preload time we use a reasonable default.
                use rustmotion_core::css::style::Size as CSize;
                use rustmotion_core::css::units::LengthPercentage;
                let w = match &icon.style.width {
                    Some(CSize::Length(LengthPercentage::Px(v))) => (*v as u32).max(1),
                    _ => 24,
                };
                let h = match &icon.style.height {
                    Some(CSize::Length(LengthPercentage::Px(v))) => (*v as u32).max(1),
                    _ => 24,
                };
                seen.insert((
                    icon.icon.clone(),
                    icon.style_config().color_str_or("#FFFFFF").to_string(),
                    w,
                    h,
                ));
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
        let children: Vec<ChildComponent> = scene
            .children
            .iter()
            .filter_map(|v| serde_json::from_value(v.clone()).ok())
            .collect();
        for child in &children {
            collect_from_component(child, &mut seen);
        }
    }

    let cache = asset_cache();
    // Issue #166: icons that genuinely cannot be resolved (checked both the
    // disk cache and the network, inside `fetch_icon_svg`) are collected
    // instead of merely logged — a scene that silently renders without an
    // icon is exactly the "valid but wrong" outcome this project treats as
    // worse than a hard failure. Parse/rasterize errors (a malformed SVG
    // response, not a missing icon) stay warnings: they are not what "icon
    // remains unresolvable" means here, and are rare enough downstream
    // provider bugs that they don't warrant aborting the whole render.
    let mut unresolved: Vec<String> = Vec::new();
    for (icon, color, w, h) in &seen {
        // Same formula the painter (`icon.rs`) uses at paint time — see
        // `icon_cache_key`'s doc for why these used to disagree (issue #166).
        let (render_w, render_h, cache_key) = icon_cache_key(icon, color, *w, *h);
        if cache.contains_key(&cache_key) {
            continue;
        }
        match fetch_icon_svg(icon, color, render_w, render_h) {
            Ok(svg_data) => {
                let opt = sandboxed_svg_options();
                match usvg::Tree::from_data(&svg_data, &opt) {
                    Ok(tree) => {
                        let svg_size = tree.size();
                        if let Some(mut pixmap) = tiny_skia::Pixmap::new(render_w, render_h) {
                            let scale_x = render_w as f32 / svg_size.width();
                            let scale_y = render_h as f32 / svg_size.height();
                            let transform = tiny_skia::Transform::from_scale(scale_x, scale_y);
                            resvg::render(&tree, transform, &mut pixmap.as_mut());
                            let img_data = skia_safe::Data::new_copy(pixmap.data());
                            let img_info = skia_safe::ImageInfo::new(
                                (render_w as i32, render_h as i32),
                                skia_safe::ColorType::RGBA8888,
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
                unresolved.push(format!("'{icon}' (color {color}, target {w}x{h}px): {e}"));
            }
        }
    }

    if !unresolved.is_empty() {
        panic!(
            "rustmotion: {} icon(s) could not be preloaded — checked the disk cache at \
             {} and the network, both failed:\n  - {}\n\
             A render must not silently omit an icon: fix the identifier(s), or connect to \
             the network so they can be downloaded once and cached for offline use.",
            unresolved.len(),
            icon_cache_dir().display(),
            unresolved.join("\n  - ")
        );
    }
}

/// Pre-extract all needed frames from video sources in a single ffmpeg pass.
/// Called before the render loop to populate the video frame cache.
///
/// Item 3 (issue #167): this used to fail in total silence — `ffmpeg`
/// missing, or a single extraction failing, both fell into `_ => {}` with no
/// trace anywhere, leaving affected `video` components entirely blank.
/// Replicates the `ffmpeg_available()` + one-time-warning discipline PR #151
/// already established for embedded-video *audio* extraction
/// (`encode::video_audio::collect_video_audio_tracks`), which this frame
/// path never inherited.
///
/// ffmpeg's rawvideo stdout is read directly off the pipe in
/// `frame_byte_size` chunks (`Read::read_exact`) rather than buffered whole
/// via `Command::output` and then copied frame-by-frame out of that buffer —
/// the old shape held the full decode in memory twice at its peak. A byte
/// budget (`would_exceed_cache_budget`) is checked before ffmpeg is even
/// spawned, and the read loop itself stops at `expected_frames` regardless,
/// so a source that would blow the budget is refused up front and one that
/// somehow outputs more frames than the requested time range implies cannot
/// grow the cache past what was budgeted for it.
pub fn preextract_video_frames(scenes: &[Scene], fps: u32) {
    if !ffmpeg_available() {
        eprintln!(
            "rustmotion: ffmpeg not found — video components will render blank frames. \
             Install ffmpeg to decode embedded video sources."
        );
        return;
    }

    fn collect_videos(child: &ChildComponent, scene_frames: u32, fps: u32) {
        if let Component::Video(video) = &child.component {
            use rustmotion_core::css::style::Size as CSize;
            use rustmotion_core::css::units::LengthPercentage;
            // Size now comes from CSS style; skip preload if not set as fixed px.
            let width = match &video.style.width {
                Some(CSize::Length(LengthPercentage::Px(v))) => (*v as u32).max(1),
                _ => return,
            };
            let height = match &video.style.height {
                Some(CSize::Length(LengthPercentage::Px(v))) => (*v as u32).max(1),
                _ => return,
            };
            let rate = video.playback_rate.unwrap_or(1.0);
            let trim_start = video.trim_start.unwrap_or(0.0);

            let cache_key = format!("{}:{}x{}", video.src, width, height);
            let cache = video_frame_cache();

            if cache.contains_key(&cache_key) {
                return;
            }

            let (start_at, end_at) = video.timing();
            let start_frame = start_at
                .map(|s| (s * fps as f64).round() as u32)
                .unwrap_or(0);
            let end_frame = end_at
                .map(|e| (e * fps as f64).round() as u32)
                .unwrap_or(scene_frames);

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

            let frame_byte_size = video_frame_byte_size(width, height);
            if frame_byte_size == 0 {
                eprintln!(
                    "rustmotion: video frame preextraction: '{}' resolved to a zero-byte \
                     frame size ({width}x{height}) — refusing to preextract. This video will \
                     render blank for the affected frames.",
                    video.src
                );
                return;
            }
            let expected_frames = (times.len() as u64).saturating_add(1);
            let expected_bytes = frame_byte_size.saturating_mul(expected_frames);
            let already_cached = video_frame_cache_bytes();
            if would_exceed_cache_budget(already_cached, expected_bytes) {
                eprintln!(
                    "rustmotion: video frame preextraction: caching '{}' at {width}x{height} \
                     would need ~{} MiB on top of the {} MiB already cached, over the {} MiB \
                     budget — refusing to preextract. This video will render blank for the \
                     affected frames.",
                    video.src,
                    expected_bytes / (1024 * 1024),
                    already_cached / (1024 * 1024),
                    VIDEO_FRAME_CACHE_BUDGET_BYTES / (1024 * 1024),
                );
                return;
            }

            let mut child = match std::process::Command::new("ffmpeg")
                .args([
                    "-ss",
                    &format!("{:.3}", min_time),
                    "-t",
                    &format!("{:.3}", duration),
                    "-i",
                    &video.src,
                    "-vf",
                    &format!("fps={},scale={}:{}", fps, width, height),
                    "-f",
                    "rawvideo",
                    "-pix_fmt",
                    "rgba",
                    "-y",
                    "pipe:1",
                ])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                Ok(child) => child,
                Err(e) => {
                    eprintln!(
                        "rustmotion: video frame preextraction: could not spawn ffmpeg for \
                         '{}': {}. This video will render blank for the affected frames.",
                        video.src, e
                    );
                    return;
                }
            };

            let Some(mut stdout) = child.stdout.take() else {
                eprintln!(
                    "rustmotion: video frame preextraction: ffmpeg for '{}' produced no \
                     stdout pipe. This video will render blank for the affected frames.",
                    video.src
                );
                let _ = child.wait();
                return;
            };

            let frame_size = frame_byte_size as usize;
            let max_frames = expected_frames as usize;
            let mut frames: Vec<(f64, Vec<u8>, u32, u32)> = Vec::with_capacity(times.len());
            loop {
                if frames.len() >= max_frames {
                    break;
                }
                let mut buf = vec![0u8; frame_size];
                match stdout.read_exact(&mut buf) {
                    Ok(()) => {
                        let time = min_time + frames.len() as f64 / fps as f64;
                        frames.push((time, buf, width, height));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
                    Err(e) => {
                        eprintln!(
                            "rustmotion: video frame preextraction: reading ffmpeg output for \
                             '{}' failed: {}. Keeping the {} frame(s) decoded so far.",
                            video.src,
                            e,
                            frames.len()
                        );
                        break;
                    }
                }
            }
            drop(stdout);

            match child.wait() {
                Ok(status) if status.success() => {
                    if frames.is_empty() {
                        eprintln!(
                            "rustmotion: video frame preextraction: ffmpeg produced no frames \
                             for '{}'. This video will render blank for the affected frames.",
                            video.src
                        );
                        return;
                    }
                    cache.insert(cache_key, Arc::new(frames));
                }
                Ok(status) => {
                    eprintln!(
                        "rustmotion: video frame preextraction: ffmpeg failed to decode \
                         frames from '{}' (exit status: {}). This video will render blank \
                         for the affected frames.",
                        video.src, status
                    );
                }
                Err(e) => {
                    eprintln!(
                        "rustmotion: video frame preextraction: could not wait on ffmpeg for \
                         '{}': {}. This video will render blank for the affected frames.",
                        video.src, e
                    );
                }
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

    for scene in scenes {
        let scene_frames = (scene.duration * fps as f64).round() as u32;
        let children: Vec<ChildComponent> = scene
            .children
            .iter()
            .filter_map(|v| serde_json::from_value(v.clone()).ok())
            .collect();
        for child in &children {
            collect_videos(child, scene_frames, fps);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unresolvable_icon_must_fail_the_preload_not_be_swallowed() {
        // `fetch_icon_svg` fails deterministically (no network needed) for
        // an icon id with no ':' — `InvalidIconFormat`. Pre-fix,
        // `prefetch_icons` catches this in its `Err(e) => eprintln!(...)`
        // arm and returns normally: the render proceeds as if nothing were
        // wrong, and the icon silently never paints.
        let scene: Scene = serde_json::from_value(serde_json::json!({
            "duration": 1.0,
            "children": [
                {"type": "icon", "icon": "not-a-valid-icon-id-no-colon"}
            ]
        }))
        .expect("scene must deserialize");

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            prefetch_icons(std::slice::from_ref(&scene));
        }));

        assert!(
            result.is_err(),
            "prefetch_icons must panic (or otherwise hard-fail) when an icon cannot be \
             resolved via disk cache or network, instead of silently continuing"
        );
    }
}
