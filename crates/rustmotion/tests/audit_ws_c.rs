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
