//! Regression tests — audit round chantier/audit-2026-09, workstream C
//! (encoding, audio, cancellation).
//!
//! Tests here only exercise the public API surface — `rustmotion::encode::...`
//! — since this is an external integration test crate; regression tests for
//! private helpers live next to them in their own `#[cfg(test)] mod tests`
//! inside the `src/encode/` file that owns them, and are cross-referenced
//! here by the behavior they cover.

use std::path::PathBuf;

/// Minimal RAII scratch file: unique per (label, pid, nanosecond timestamp),
/// removed on drop even if the test panics partway through.
struct ScratchFile(PathBuf);

impl ScratchFile {
    /// `ext` is the extension ffmpeg/the encoder picks its container format
    /// from (`"mp4"`, `"gif"`, ...) — omitting it makes ffmpeg fail at
    /// muxer selection before ever touching the path, which would make a
    /// debris-after-failure test pass for the wrong reason.
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

// ─── A render ffmpeg rejects must leave no debris at output_path ──────────
//
// `encode_with_ffmpeg`/`encode_with_ffmpeg_hw` used to hand ffmpeg the
// user's own `output_path` directly, drop stdin and gracefully wait no
// matter how the frame loop ended. A failure mid-render (a broken pipe, or
// ffmpeg itself exiting non-zero) still let ffmpeg finalize whatever it had
// already received at that exact path, and `-y` meant it would even
// overwrite a previously-good render there. No cleanup ran on any error
// path.
//
// Forcing rustmotion's *own* frame renderer to fail deterministically isn't
// possible from a plain scenario — `Painter::paint_content` cannot return an
// `Err`, so no user-authored content can fail a frame. ffmpeg itself,
// though, refuses an odd width under the crate's default 10-bit H.264
// profile (`yuv420p10le` needs even 4:2:0 chroma dimensions) — confirmed by
// hand: `ffmpeg -f rawvideo ... -video_size 321x240 ... -c:v libx264
// -profile:v high10 -pix_fmt yuv420p10le out.mp4` creates a 0-byte
// `out.mp4` and then fails to open its encoder, closing stdin before
// rustmotion finishes writing frames — exactly the "ffmpeg already has a
// file open at output_path when the failure happens" shape this test cares
// about, without needing an internal render failure at all.
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

// ─── build_atempo_filter(rate) must not loop forever ───────────────────────
//
// `remaining /= 0.5` never reaches the loop's `>= 0.5` exit test starting
// from `0.0`, and diverges away from it starting from any negative rate —
// either way the pre-fix loop pushed a fresh `String` forever. Calling the
// unguarded function directly on the test thread would hang the whole
// suite, so each candidate rate runs on its own thread with a bounded
// `recv_timeout`: a present guard returns well inside the timeout, a
// missing one times out and fails the assertion instead of the process.
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

// ─── A transition's declared easing must actually reshape its progress ────
//
// `transition_progress`/`view_transition_progress` hand a raw *linear*
// fraction to `apply_transition`, which composites pixels at exactly the
// fraction it's given — it has no notion of the scenario's declared
// `transition.easing` on its own. Before this fix, that raw linear fraction
// reached `apply_transition` unmodified for every transition type except
// `camera_pan` (which threads easing through a different function,
// `camera_pan_transition`, that already applies it internally), so
// `"easing": "ease_in"`/`"ease_out"` on a `fade`, wipe, iris, or a
// between-view transition was silently a no-op.
//
// A `fade` between a solid-black and a solid-white full-canvas frame turns
// this into an exact, predictable pixel value: at the frame whose *linear*
// position in the transition is precisely 0.5, an eased blend must land at
// `255 * ease(0.5)`, not at the unequal, easing-blind `255 * 0.5 = 128`.
// `ease_in_cubic(0.5) = 0.125` and `ease_out_cubic(0.5) = 0.875` are both far
// enough from `0.5` that no rendering noise/antialiasing could produce a
// false pass.

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
        let scenario = rustmotion::loader::load_scenario_from_source(None, Some(&json))
            .expect("load");

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

        let rgba =
            rustmotion::encode::video::render_frame_task(&scenario.video, &scenario, target)
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
