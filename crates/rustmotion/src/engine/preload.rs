use std::sync::Arc;

use crate::components::{ChildComponent, Component};
use crate::schema::Scene;
use rustmotion_core::engine::renderer::{
    asset_cache, fetch_icon_svg, ffmpeg_available, icon_cache_dir, icon_cache_key,
    video_frame_cache,
};
use rustmotion_core::traits::{Styled, Timed};

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
                let opt = usvg::Options::default();
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

            let output = std::process::Command::new("ffmpeg")
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
                Ok(output) => {
                    eprintln!(
                        "rustmotion: video frame preextraction: ffmpeg failed to decode \
                         frames from '{}' (exit status: {}). This video will render blank \
                         for the affected frames.",
                        video.src, output.status
                    );
                }
                Err(e) => {
                    eprintln!(
                        "rustmotion: video frame preextraction: could not spawn ffmpeg for \
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
