use std::time::{Duration, Instant};

use rustmotion::schema::ResolvedScenario;

/// Render one frame and encode it to JPEG bytes (preview-only; the final video
/// render path does not use this). JPEG keeps the encode cost low enough for
/// the webview transport to keep up with playback. Returns
/// (jpeg, width, height, render_time, encode_time).
pub fn render_frame(
    scenario: &ResolvedScenario,
    tasks: &[rustmotion::encode::video::FrameTask],
    frame: u32,
    scale: f32,
) -> (Vec<u8>, u32, u32, Duration, Duration) {
    let idx = (frame as usize).min(tasks.len().saturating_sub(1));
    let task = &tasks[idx];

    let t0 = Instant::now();
    let rgba = rustmotion::encode::render_frame_task_scaled(&scenario.video, scenario, task, scale)
        .expect("render frame");
    let render_time = t0.elapsed();

    let w = (scenario.video.width as f32 * scale) as u32;
    let h = (scenario.video.height as f32 * scale) as u32;

    let t1 = Instant::now();
    let rgba_img = image::RgbaImage::from_raw(w, h, rgba).expect("rgba matches dimensions");
    // JPEG has no alpha; drop it (preview frames are opaque).
    let rgb = image::DynamicImage::ImageRgba8(rgba_img).to_rgb8();
    let mut jpeg = Vec::new();
    image::DynamicImage::ImageRgb8(rgb)
        .write_to(&mut std::io::Cursor::new(&mut jpeg), image::ImageFormat::Jpeg)
        .expect("encode jpeg");
    let encode_time = t1.elapsed();

    (jpeg, w, h, render_time, encode_time)
}

/// A clickable element box in percentage-of-frame coords, with its kind.
#[derive(Debug, Clone, PartialEq)]
pub struct HitPct {
    pub node_id: u32,
    pub kind: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Compute the current frame's clickable element boxes in percentage coords
/// (render only — no JPEG encode, so this is cheap per frame).
pub fn frame_hits(
    scenario: &ResolvedScenario,
    tasks: &[rustmotion::encode::video::FrameTask],
    frame: u32,
) -> Vec<HitPct> {
    if tasks.is_empty() {
        return Vec::new();
    }
    let idx = (frame as usize).min(tasks.len() - 1);
    let task = &tasks[idx];
    let vw = scenario.video.width as f32;
    let vh = scenario.video.height as f32;
    rustmotion::encode::render_frame_task_hits(scenario, task)
        .into_iter()
        .map(|h| HitPct {
            node_id: h.node_id,
            kind: h.kind,
            x: (h.rect.x / vw) * 100.0,
            y: (h.rect.y / vh) * 100.0,
            w: (h.rect.w / vw) * 100.0,
            h: (h.rect.h / vh) * 100.0,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCENARIO: &str =
        r##"{ "video": { "width": 1280, "height": 720, "background": "#101418" }, "scenes": [ { "duration": 1.0 } ] }"##;

    #[test]
    fn renders_frame_to_nonempty_jpeg() {
        let scenario = rustmotion::loader::load_scenario_from_source(None, Some(SCENARIO)).unwrap();
        let tasks = rustmotion::encode::build_frame_tasks(&scenario);
        assert!(!tasks.is_empty());
        let (jpeg, w, h, _r, _e) = render_frame(&scenario, &tasks, 0, 1.0);
        assert_eq!((w, h), (1280, 720));
        assert!(jpeg.len() > 2);
        // JPEG SOI marker.
        assert_eq!(&jpeg[0..2], &[0xFF, 0xD8], "must be a JPEG");
    }

    #[test]
    fn frame_hits_are_in_percent_and_have_kind() {
        let json = r##"{ "video": { "width": 800, "height": 600, "background": "#101418" }, "scenes": [ { "duration": 1.0, "children": [ { "type": "text", "content": "Hi", "style": { "font-size": 40 } } ] } ] }"##;
        let scenario = rustmotion::loader::load_scenario_from_source(None, Some(json)).unwrap();
        let tasks = rustmotion::encode::build_frame_tasks(&scenario);
        let hits = frame_hits(&scenario, &tasks, 0);
        assert!(hits.iter().any(|h| h.kind == "text"), "got {hits:?}");
        assert!(hits.iter().all(|h| h.x >= 0.0 && h.x <= 100.0 && h.w <= 100.0));
    }
}
