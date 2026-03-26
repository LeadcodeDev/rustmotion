use std::sync::Arc;

use rustmotion_core::engine::renderer::{asset_cache, fetch_icon_svg, video_frame_cache};
use rustmotion_core::traits::{Styled, Timed};
use crate::components::{ChildComponent, Component};
use crate::schema::Scene;

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
        let children: Vec<ChildComponent> = scene.children.iter()
            .filter_map(|v| serde_json::from_value(v.clone()).ok())
            .collect();
        for child in &children {
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
                eprintln!("Warning: failed to fetch icon '{}': {}", icon, e);
            }
        }
    }
}

/// Pre-extract all needed frames from video sources in a single ffmpeg pass.
/// Called before the render loop to populate the video frame cache.
pub fn preextract_video_frames(
    scenes: &[Scene],
    fps: u32,
) {
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

    for scene in scenes {
        let scene_frames = (scene.duration * fps as f64).round() as u32;
        let children: Vec<ChildComponent> = scene.children.iter()
            .filter_map(|v| serde_json::from_value(v.clone()).ok())
            .collect();
        for child in &children {
            collect_videos(child, scene_frames, fps);
        }
    }
}
