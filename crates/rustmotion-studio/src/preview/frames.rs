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
}
