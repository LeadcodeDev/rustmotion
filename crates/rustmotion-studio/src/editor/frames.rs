use rustmotion::schema::ResolvedScenario;

/// Render one frame and encode it to JPEG bytes (preview-only; the final video
/// render path does not use this). JPEG keeps the encode cost low enough for
/// the webview transport to keep up with playback.
pub fn render_frame(
    scenario: &ResolvedScenario,
    tasks: &[rustmotion::encode::video::FrameTask],
    frame: u32,
    scale: f32,
) -> Vec<u8> {
    if tasks.is_empty() {
        return Vec::new();
    }
    let idx = (frame as usize).min(tasks.len() - 1);
    let task = &tasks[idx];
    let rgba = rustmotion::encode::render_frame_task_scaled(&scenario.video, scenario, task, scale)
        .expect("render frame");
    let w = (scenario.video.width as f32 * scale) as u32;
    let h = (scenario.video.height as f32 * scale) as u32;
    let rgba_img = image::RgbaImage::from_raw(w, h, rgba).expect("rgba matches dimensions");
    // JPEG has no alpha; drop it (preview frames are opaque).
    let rgb = image::DynamicImage::ImageRgba8(rgba_img).to_rgb8();
    let mut jpeg = Vec::new();
    image::DynamicImage::ImageRgb8(rgb)
        .write_to(&mut std::io::Cursor::new(&mut jpeg), image::ImageFormat::Jpeg)
        .expect("encode jpeg");
    jpeg
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
    /// Full JSON Pointer into the source scenario, e.g. "/scenes/0/children/2".
    pub pointer: Option<String>,
}

/// Compute the current frame's clickable element boxes in percentage coords
/// (render only — no JPEG encode, so this is cheap per frame). `scene_prefix`
/// is the JSON-Pointer prefix of the current scene (see [`scene_prefix`]).
pub fn frame_hits(
    scenario: &ResolvedScenario,
    tasks: &[rustmotion::encode::video::FrameTask],
    frame: u32,
    scene_prefix: &str,
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
            pointer: h.pointer.map(|rel| format!("{scene_prefix}{rel}")),
        })
        .collect()
}

/// JSON-Pointer prefix to the scene of the given frame, derived from the raw
/// scenario JSON (handles both top-level `scenes` and `composition`).
pub fn scene_prefix(
    raw: &serde_json::Value,
    tasks: &[rustmotion::encode::video::FrameTask],
    frame: u32,
) -> String {
    use rustmotion::encode::video::FrameTask;
    if tasks.is_empty() {
        return String::new();
    }
    let idx = (frame as usize).min(tasks.len() - 1);
    if let FrameTask::Normal { view_idx, scene_idx, .. } = &tasks[idx] {
        if raw.get("composition").is_some() {
            format!("/composition/{view_idx}/scenes/{scene_idx}")
        } else {
            format!("/scenes/{scene_idx}")
        }
    } else {
        String::new()
    }
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
        let jpeg = render_frame(&scenario, &tasks, 0, 1.0);
        assert!(jpeg.len() > 2);
        // JPEG SOI marker.
        assert_eq!(&jpeg[0..2], &[0xFF, 0xD8], "must be a JPEG");
    }

    #[test]
    fn frame_hits_are_in_percent_and_have_kind() {
        let json = r##"{ "video": { "width": 800, "height": 600, "background": "#101418" }, "scenes": [ { "duration": 1.0, "children": [ { "type": "text", "content": "Hi", "style": { "font-size": 40 } } ] } ] }"##;
        let scenario = rustmotion::loader::load_scenario_from_source(None, Some(json)).unwrap();
        let tasks = rustmotion::encode::build_frame_tasks(&scenario);
        let hits = frame_hits(&scenario, &tasks, 0, "/scenes/0");
        let text = hits.iter().find(|h| h.kind == "text").expect("text hit");
        assert!(hits.iter().all(|h| h.x >= 0.0 && h.x <= 100.0 && h.w <= 100.0));
        assert!(
            text.pointer.as_deref().unwrap().starts_with("/scenes/0/children/"),
            "pointer = {:?}",
            text.pointer
        );
    }

    #[test]
    fn scene_prefix_handles_top_level_scenes() {
        let json = r##"{ "video": { "width": 1, "height": 1 }, "scenes": [ { "duration": 1.0 } ] }"##;
        let scenario = rustmotion::loader::load_scenario_from_source(None, Some(json)).unwrap();
        let raw: serde_json::Value = serde_json::from_str(json).unwrap();
        let tasks = rustmotion::encode::build_frame_tasks(&scenario);
        assert_eq!(scene_prefix(&raw, &tasks, 0), "/scenes/0");
    }
}
