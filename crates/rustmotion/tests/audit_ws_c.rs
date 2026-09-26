use std::path::PathBuf;

struct ScratchFile(PathBuf);

impl ScratchFile {
    fn new(label: &str, ext: &str) -> Self {
        let unique = format!(
            "rustmotion-audit-ws-c-{label}-{}-{}.{ext}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock before UNIX epoch")
                .as_nanos()
        );
        Self(std::env::temp_dir().join(unique))
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }

    fn to_str(&self) -> &str {
        self.0.to_str().expect("scratch path must be UTF-8")
    }
}

impl Drop for ScratchFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn ffmpeg_on_path() -> bool {
    std::process::Command::new("ffmpeg")
        .args(["-version"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[test]
fn a_render_ffmpeg_rejects_leaves_no_debris_at_the_output_path() {
    if !ffmpeg_on_path() {
        eprintln!(
            "a_render_ffmpeg_rejects_leaves_no_debris_at_the_output_path: \
             ffmpeg not found — skipping"
        );
        return;
    }

    let json = r#"{"video": {"width": 321, "height": 240, "fps": 10},
                    "scenes": [{"duration": 0.3, "children": []}]}"#;
    let scenario = rustmotion::loader::load_scenario_from_source(None, Some(json)).expect("load");

    let out = ScratchFile::new("odd-width", "mp4");
    let _ = std::fs::remove_file(out.path());

    let result = rustmotion::encode::encode_with_ffmpeg(
        &scenario,
        out.to_str(),
        true,
        "h264",
        None,
        false,
        None,
    );

    assert!(
        result.is_err(),
        "an odd width (321) must make libx264's high10/yuv420p10le encoder \
         init fail — this test's premise depends on ffmpeg actually rejecting it"
    );
    assert!(
        !out.path().exists(),
        "no file — truncated, empty, or otherwise — may be left at output_path \
         after a failed render; got one at {}",
        out.path().display()
    );
}

#[test]
fn atempo_guard_rejects_non_positive_and_non_finite_rates_without_hanging() {
    use std::sync::mpsc;
    use std::time::Duration;

    for rate in [
        0.0_f64,
        -1.0,
        -0.25,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ] {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(rustmotion::encode::video_audio::build_atempo_filter(rate));
        });
        match rx.recv_timeout(Duration::from_secs(2)) {
            Ok(result) => assert_eq!(
                result, None,
                "rate={rate} must return None instead of building an atempo chain"
            ),
            Err(_) => panic!(
                "rate={rate} did not return within 2s — the guard against the infinite loop \
                 in build_atempo_filter is missing or broken"
            ),
        }
    }
}

fn center_pixel_red(rgba: &[u8], width: u32, height: u32) -> u8 {
    let idx = ((height / 2 * width + width / 2) * 4) as usize;
    rgba[idx]
}

#[test]
fn a_fade_transitions_easing_reshapes_its_progress_not_just_camera_pan() {
    let width = 64u32;
    let height = 64u32;
    let fps = 11u32;

    for (easing, expected_u8) in [("ease_in", 32u8), ("ease_out", 224u8)] {
        let json = format!(
            r##"{{"video": {{"width": {width}, "height": {height}, "fps": {fps}}},
            "scenes": [
                {{"duration": 1.0, "children": [
                    {{"type": "shape", "shape": "rect", "fill": "#000000",
                      "position": "absolute", "x": 0, "y": 0,
                      "style": {{"width": {width}, "height": {height}}}}}
                ]}},
                {{"duration": 1.0,
                  "transition": {{"type": "fade", "duration": 1.0, "easing": "{easing}"}},
                  "children": [
                    {{"type": "shape", "shape": "rect", "fill": "#ffffff",
                      "position": "absolute", "x": 0, "y": 0,
                      "style": {{"width": {width}, "height": {height}}}}}
                ]}}
            ]}}"##
        );
        let scenario =
            rustmotion::loader::load_scenario_from_source(None, Some(&json)).expect("load");

        let tasks = rustmotion::encode::video::build_frame_tasks(&scenario);
        let target = tasks
            .iter()
            .find(|t| {
                matches!(
                    t,
                    rustmotion::encode::video::FrameTask::SlideTransition {
                        frame_in_transition: 5,
                        ..
                    }
                )
            })
            .unwrap_or_else(|| panic!("{easing}: expected a mid-transition frame at index 5"));

        let rgba = rustmotion::encode::video::render_frame_task(&scenario.video, &scenario, target)
            .unwrap_or_else(|e| panic!("{easing}: render failed: {e}"));
        let red = center_pixel_red(&rgba, width, height);

        assert!(
            (red as i32 - expected_u8 as i32).abs() <= 4,
            "{easing}: at the transition's linear midpoint, the eased blend must land near \
             {expected_u8}, got {red}"
        );
        assert!(
            (red as i32 - 128).abs() > 20,
            "{easing}: {red} is too close to 128 — the raw, unequal linear-progress blend an \
             easing-blind composite would produce"
        );
    }
}

#[test]
fn a_between_view_fade_transitions_easing_reshapes_its_progress() {
    let width = 64u32;
    let height = 64u32;
    let fps = 11u32;

    let json = format!(
        r##"{{"video": {{"width": {width}, "height": {height}, "fps": {fps}}},
        "composition": [
            {{"type": "slide", "scenes": [
                {{"duration": 1.0, "children": [
                    {{"type": "shape", "shape": "rect", "fill": "#000000",
                      "position": "absolute", "x": 0, "y": 0,
                      "style": {{"width": {width}, "height": {height}}}}}
                ]}}
            ]}},
            {{"type": "slide",
              "transition": {{"type": "fade", "duration": 1.0, "easing": "ease_in"}},
              "scenes": [
                {{"duration": 1.0, "children": [
                    {{"type": "shape", "shape": "rect", "fill": "#ffffff",
                      "position": "absolute", "x": 0, "y": 0,
                      "style": {{"width": {width}, "height": {height}}}}}
                ]}}
            ]}}
        ]}}"##
    );
    let scenario = rustmotion::loader::load_scenario_from_source(None, Some(&json)).expect("load");

    let tasks = rustmotion::encode::video::build_frame_tasks(&scenario);
    let target = tasks
        .iter()
        .find(|t| {
            matches!(
                t,
                rustmotion::encode::video::FrameTask::ViewTransition {
                    frame_in_transition: 5,
                    ..
                }
            )
        })
        .expect("expected a mid-view-transition frame at index 5");

    let rgba = rustmotion::encode::video::render_frame_task(&scenario.video, &scenario, target)
        .expect("render");
    let red = center_pixel_red(&rgba, width, height);

    assert!(
        (red as i32 - 32).abs() <= 4,
        "ease_in at the view transition's linear midpoint must land near 32, got {red}"
    );
    assert!(
        (red as i32 - 128).abs() > 20,
        "{red} is too close to 128 — the raw, unequal linear-progress blend an easing-blind \
         composite would produce"
    );
}
