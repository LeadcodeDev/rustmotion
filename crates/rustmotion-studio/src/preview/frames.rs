use std::time::{Duration, Instant};

use rustmotion::schema::ResolvedScenario;

/// Render one frame to PNG bytes. Returns (png, width, height, render_time, encode_time).
pub fn render_png(
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
    let img = image::RgbaImage::from_raw(w, h, rgba).expect("rgba matches dimensions");
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .expect("encode png");
    let encode_time = t1.elapsed();

    (png, w, h, render_time, encode_time)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCENARIO: &str =
        r##"{ "video": { "width": 1280, "height": 720, "background": "#101418" }, "scenes": [ { "duration": 1.0 } ] }"##;

    #[test]
    fn renders_frame_to_nonempty_png() {
        let scenario = rustmotion::loader::load_scenario_from_source(None, Some(SCENARIO)).unwrap();
        let tasks = rustmotion::encode::build_frame_tasks(&scenario);
        assert!(!tasks.is_empty());
        let (png, w, h, _r, _e) = render_png(&scenario, &tasks, 0, 1.0);
        assert_eq!((w, h), (1280, 720));
        assert!(png.len() > 8);
        assert_eq!(&png[0..4], &[0x89, b'P', b'N', b'G'], "must be a PNG");
    }
}
