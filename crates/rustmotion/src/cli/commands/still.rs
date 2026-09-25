use rustmotion::encode;
use rustmotion::engine;
use rustmotion::error::{Result, RustmotionError};
use rustmotion::schema::ResolvedScenario;
use std::path::{Path, PathBuf};

use super::validation::{self, LoadedScenario};

const MAX_STILL_RENDER_BUDGET: u64 = 2_000_000_000_000;

fn estimated_total_frames_overestimate(scenario: &ResolvedScenario) -> u64 {
    let fps = scenario.video.fps as f64;
    scenario
        .all_scenes()
        .map(|scene| (scene.duration.max(0.0) * fps).round() as u64)
        .sum()
}

fn reject_oversized_render(scenario: &ResolvedScenario) -> Result<()> {
    let frames = estimated_total_frames_overestimate(scenario);
    let width = scenario.video.width as u64;
    let height = scenario.video.height as u64;
    let budget = width
        .checked_mul(height)
        .and_then(|wh| wh.checked_mul(frames));
    match budget {
        Some(b) if b <= MAX_STILL_RENDER_BUDGET => Ok(()),
        Some(b) => Err(RustmotionError::Generic(format!(
            "scenario needs an estimated {frames} frame(s) at {width}x{height} ({b} \
             pixel-frames total), over this command's render budget of \
             {MAX_STILL_RENDER_BUDGET} — reduce video.width/video.height or scene durations"
        ))),
        None => Err(RustmotionError::Generic(format!(
            "scenario's width ({width}) x height ({height}) x estimated frame count ({frames}) \
             overflows — reduce video.width/video.height or scene durations"
        ))),
    }
}

fn ensure_scenario_is_valid(scenario: ResolvedScenario, lenient: bool) -> Result<ResolvedScenario> {
    let no_raw_json_carried_by_cmd_still = serde_json::Value::Null;
    let loaded = LoadedScenario {
        raw: no_raw_json_carried_by_cmd_still,
        scenario,
        source_path: None,
    };
    let report = validation::run_checks(&loaded, false);
    if !report.is_clean() {
        validation::print_report(&report, "<still>");
    }
    if report.is_blocking(lenient) {
        return Err(report.to_error());
    }
    Ok(loaded.scenario)
}

/// A scratch path in the same directory as `output`, carrying the same
/// extension so extension-sniffing encoders (the `image::save` fallback arm)
/// still resolve the codec they would have resolved for `output` itself.
///
/// Constat #8: `File::create(output)` used to run *before* the encoder that
/// could fail (JPEG always failed on the RGBA buffer), so every failure left
/// a 0-byte file sitting at `output` — indistinguishable from a real, empty
/// render to a downstream script. Encoding into this scratch path first and
/// renaming onto `output` only on success means a failure never touches
/// `output` at all: an old file there is left untouched, and no new
/// truncated file appears.
fn temp_sibling_path(output: &Path) -> PathBuf {
    let ext = output.extension().and_then(|e| e.to_str());
    let stem = output
        .file_stem()
        .and_then(|e| e.to_str())
        .unwrap_or("still");
    let name = match ext {
        Some(ext) => format!(".{stem}.rustmotion-tmp.{ext}"),
        None => format!(".{stem}.rustmotion-tmp"),
    };
    output.with_file_name(name)
}

/// Flatten RGBA onto an opaque background for encoders that cannot
/// represent alpha (JPEG). Compositing onto `video.background` — the color
/// the frame actually renders against — rather than dropping the alpha
/// channel outright (which would implicitly composite onto black).
fn flatten_to_rgb(img: &image::RgbaImage, bg: (u8, u8, u8)) -> Vec<u8> {
    let (bg_r, bg_g, bg_b) = bg;
    let mut rgb = Vec::with_capacity(img.as_raw().len() / 4 * 3);
    for px in img.pixels() {
        let [r, g, b, a] = px.0;
        let a = a as u16;
        let inv_a = 255 - a;
        let blend = |fg: u8, bg: u8| -> u8 { ((fg as u16 * a + bg as u16 * inv_a) / 255) as u8 };
        rgb.push(blend(r, bg_r));
        rgb.push(blend(g, bg_g));
        rgb.push(blend(b, bg_b));
    }
    rgb
}

pub fn cmd_still(
    scenario: ResolvedScenario,
    output: &PathBuf,
    time: f64,
    format: Option<String>,
    quality: u8,
    no_validate: bool,
    lenient: bool,
) -> Result<()> {
    reject_oversized_render(&scenario)?;
    let scenario = if no_validate {
        scenario
    } else {
        ensure_scenario_is_valid(scenario, lenient)?
    };

    let config = &scenario.video;
    let fps = config.fps;

    // Constat #4: pick the frame the same way the encoder does. Summing
    // scene durations linearly (the previous approach) ignores that
    // `build_frame_tasks` truncates the entering scene's tail to make room
    // for a transition's overlap — so `--time` landed on a frame that never
    // appears, composited or otherwise, in the rendered video. Reusing
    // `build_frame_tasks` + `render_frame_task_scaled` also picks up
    // `apply_post_effects` (vignette, grain, ...) for free, which the old
    // per-scene walk never applied at all.
    let tasks = encode::build_frame_tasks(&scenario);
    let total = tasks.len() as u32;
    if total == 0 {
        return Err(RustmotionError::NoFrames);
    }

    // Preserve the previous command's tolerant behavior: negative time
    // clamps to frame 0, time beyond the video's duration clamps to the
    // last frame, instead of erroring.
    let raw_index = (time.max(0.0) * fps as f64).round();
    let frame_index = if raw_index.is_finite() {
        (raw_index as i64).clamp(0, total as i64 - 1) as u32
    } else {
        0
    };

    let task = &tasks[frame_index as usize];
    let rgba = encode::render_frame_task_scaled(config, &scenario, task, 1.0)?;

    // Create parent directories
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    let img = image::RgbaImage::from_raw(config.width, config.height, rgba)
        .ok_or(RustmotionError::PixelImage)?;

    let fmt = format
        .as_deref()
        .unwrap_or_else(|| output.extension().and_then(|e| e.to_str()).unwrap_or("png"));

    let tmp_path = temp_sibling_path(output);
    let encode_result: std::result::Result<(), RustmotionError> = (|| {
        match fmt {
            "jpeg" | "jpg" => {
                use image::ImageEncoder;
                let (bg_r, bg_g, bg_b, _) = engine::parse_hex_color(&config.background);
                let rgb = flatten_to_rgb(&img, (bg_r, bg_g, bg_b));
                let file = std::fs::File::create(&tmp_path)?;
                let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(file, quality);
                encoder.write_image(
                    &rgb,
                    config.width,
                    config.height,
                    image::ExtendedColorType::Rgb8,
                )?;
            }
            "webp" => {
                use image::ImageEncoder;
                let file = std::fs::File::create(&tmp_path)?;
                let encoder = image::codecs::webp::WebPEncoder::new_lossless(file);
                encoder.write_image(
                    img.as_raw(),
                    config.width,
                    config.height,
                    image::ExtendedColorType::Rgba8,
                )?;
            }
            _ => {
                img.save(&tmp_path)?;
            }
        }
        Ok(())
    })();

    match encode_result {
        Ok(()) => {
            std::fs::rename(&tmp_path, output)?;
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(e);
        }
    }

    eprintln!("Still image saved to {}", output.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmotion::loader::load_scenario_from_source;

    fn solid_rect_scenario(width: u32, height: u32, fps: u32, duration: f64, hex: &str) -> String {
        format!(
            r##"{{"video": {{"width": {width}, "height": {height}, "fps": {fps}}},
                 "scenes": [{{"duration": {duration}, "children": [
                    {{"type": "shape", "shape": "rect", "fill": "{hex}",
                      "position": "absolute", "x": 0, "y": 0,
                      "style": {{"width": {width}, "height": {height}}}}}
                 ]}}]}}"##
        )
    }

    fn minimal_scenario(width: u32, height: u32, fps: u32, duration: f64) -> ResolvedScenario {
        let json = solid_rect_scenario(width, height, fps, duration, "#ff0000");
        load_scenario_from_source(None, Some(&json)).expect("load")
    }

    /// `name` (e.g. "still.jpg") must stay the *last* path component so its
    /// extension survives — putting the uniqueness suffix after it (as an
    /// earlier version of this helper did) turned "still.jpg" into
    /// "still.jpg_1234_5678", whose "extension" per `Path::extension()`
    /// becomes "jpg_1234_5678": unrecognized by every format-sniffing
    /// encoder, so every test using it failed on a spurious
    /// `Unsupported(PathExtension(..))` instead of exercising the code
    /// under test at all.
    fn scratch_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "rm_still_test_{}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            name
        ))
    }

    /// Constat #8: JPEG stills used to fail unconditionally (the `image`
    /// crate's JPEG encoder rejects `Rgba8`) and leave a 0-byte file behind.
    #[test]
    fn still_jpeg_encodes_successfully_and_writes_a_nonempty_valid_file() {
        let scenario = minimal_scenario(16, 16, 10, 1.0);
        let out = scratch_path("jpeg_ok.jpg");
        let _ = std::fs::remove_file(&out);

        cmd_still(scenario, &out, 0.0, None, 90, false, false).expect("jpeg still must succeed");

        let meta = std::fs::metadata(&out).expect("output file must exist");
        assert!(meta.len() > 0, "jpeg output must not be empty");
        let img = image::open(&out).expect("must decode as a valid image");
        assert_eq!(img.width(), 16);
        assert_eq!(img.height(), 16);

        let _ = std::fs::remove_file(&out);
    }

    /// Constat #8 (aggravation noted by the verification pass): `--format
    /// jpeg` must succeed regardless of the output path's own extension.
    #[test]
    fn still_format_flag_forces_jpeg_even_with_a_png_extension() {
        let scenario = minimal_scenario(16, 16, 10, 1.0);
        let out = scratch_path("forced.png");
        let _ = std::fs::remove_file(&out);

        cmd_still(
            scenario,
            &out,
            0.0,
            Some("jpeg".to_string()),
            90,
            false,
            false,
        )
        .expect("forced jpeg still must succeed");

        let meta = std::fs::metadata(&out).expect("output file must exist");
        assert!(meta.len() > 0, "forced jpeg output must not be empty");

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn cmd_still_refuses_a_scenario_with_an_unknown_component_attribute() {
        let json = r##"{"video": {"width": 16, "height": 16, "fps": 10},
             "scenes": [{"duration": 1.0, "children": [
                {"type": "shape", "shape": "rect", "fill": "#ff0000",
                 "position": "absolute", "x": 0, "y": 0,
                 "style": {"width": 16, "height": 16},
                 "this_attribute_does_not_exist": true}
             ]}]}"##;
        let scenario = load_scenario_from_source(None, Some(json)).expect("load");
        let out = scratch_path("unknown_attr.png");
        let _ = std::fs::remove_file(&out);

        let result = cmd_still(scenario, &out, 0.0, None, 90, false, false);

        assert!(
            result.is_err(),
            "an unknown component attribute must block `still`, the same way it blocks \
             `render`'s implicit validation pass — got {result:?}"
        );
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn cmd_still_no_validate_skips_the_validation_pass_but_not_the_budget_guard() {
        let json = r##"{"video": {"width": 16, "height": 16, "fps": 10},
             "scenes": [{"duration": 1.0, "children": [
                {"type": "shape", "shape": "rect", "fill": "#ff0000",
                 "position": "absolute", "x": 0, "y": 0,
                 "style": {"width": 16, "height": 16},
                 "this_attribute_does_not_exist": true}
             ]}]}"##;
        let scenario = load_scenario_from_source(None, Some(json)).expect("load");
        let out = scratch_path("no_validate_unknown_attr.png");
        let _ = std::fs::remove_file(&out);

        let result = cmd_still(scenario, &out, 0.0, None, 90, true, false);

        assert!(
            result.is_ok(),
            "--no-validate must skip the validation pass — got {result:?}"
        );
        let _ = std::fs::remove_file(&out);

        let oversized = minimal_scenario(1920, 1080, 30, 40_000.0);
        let out2 = scratch_path("no_validate_oversized.png");
        let _ = std::fs::remove_file(&out2);

        let result2 = cmd_still(oversized, &out2, 0.0, None, 90, true, false);

        assert!(
            result2.is_err(),
            "--no-validate must not open the frame-budget guard, which is a resource ceiling, \
             not a validation check — got {result2:?}"
        );
        let _ = std::fs::remove_file(&out2);
    }

    #[test]
    fn cmd_still_refuses_a_scenario_whose_frame_budget_is_unbounded() {
        let scenario = minimal_scenario(1920, 1080, 30, 40_000.0);
        let out = scratch_path("oversized.png");
        let _ = std::fs::remove_file(&out);

        let result = cmd_still(scenario, &out, 0.0, None, 90, false, false);

        assert!(
            result.is_err(),
            "width x height x estimated-frame-count must be refused before build_frame_tasks \
             allocates a task per frame — got {result:?}"
        );
        let _ = std::fs::remove_file(&out);
    }

    /// `temp_sibling_path` is the mechanism constat #8's "never leave a
    /// truncated file" fix relies on: encode into a scratch path first,
    /// rename onto `output` only on success. Lock in its naming contract —
    /// distinct from `output`, same directory (so the later rename is a
    /// same-filesystem, near-atomic op), extension preserved so
    /// extension-sniffing encoders still resolve the right codec.
    #[test]
    fn temp_sibling_path_is_distinct_same_directory_and_keeps_the_extension() {
        let output = PathBuf::from("/some/dir/still.jpg");
        let tmp = temp_sibling_path(&output);

        assert_ne!(
            tmp, output,
            "scratch path must not collide with the final output path"
        );
        assert_eq!(
            tmp.parent(),
            output.parent(),
            "scratch path must live in the same directory as output (same filesystem for rename)"
        );
        assert_eq!(
            tmp.extension().and_then(|e| e.to_str()),
            Some("jpg"),
            "scratch path must keep output's extension for format-sniffing encoders"
        );
    }

    /// Constat #4: `still --time` must match the frame the encoder actually
    /// emits at that timestamp, not a linear per-scene walk that ignores
    /// transition overlap. Scene A (2s) + scene B (2s, incoming 1s fade):
    /// the rendered stream truncates scene A's tail by 1s, so `--time 2.5`
    /// must resolve to the composited transition frame at global index
    /// round(2.5 * fps), the same index `build_frame_tasks` would hand the
    /// encoder — not scene B's raw, uncomposited frame at local time 0.5s.
    #[test]
    fn still_time_matches_the_encoders_frame_stream_across_a_transition() {
        let fps = 30u32;
        let json = format!(
            r##"{{
            "video": {{"width": 8, "height": 8, "fps": {fps}}},
            "scenes": [
                {{"duration": 2.0, "children": [
                    {{"type": "shape", "shape": "rect", "fill": "#ff0000",
                      "position": "absolute", "x": 0, "y": 0, "style": {{"width": 8, "height": 8}}}}
                ]}},
                {{"duration": 2.0, "transition": {{"type": "fade", "duration": 1.0}}, "children": [
                    {{"type": "shape", "shape": "rect", "fill": "#0000ff",
                      "position": "absolute", "x": 0, "y": 0, "style": {{"width": 8, "height": 8}}}}
                ]}}
            ]
        }}"##
        );
        // `ResolvedScenario` isn't `Clone`, and `cmd_still` takes it by
        // value — load it twice from the same JSON instead.
        let scenario = load_scenario_from_source(None, Some(&json)).expect("load");
        let scenario_for_expected = load_scenario_from_source(None, Some(&json)).expect("load");

        let out = scratch_path("transition.png");
        let _ = std::fs::remove_file(&out);
        cmd_still(scenario, &out, 2.5, None, 90, false, false).expect("still must succeed");
        let still_img = image::open(&out).expect("decode still").to_rgba8();

        let tasks = encode::build_frame_tasks(&scenario_for_expected);
        let frame_index = (2.5_f64 * fps as f64).round() as usize;
        let expected_rgba = encode::render_frame_task_scaled(
            &scenario_for_expected.video,
            &scenario_for_expected,
            &tasks[frame_index],
            1.0,
        )
        .expect("render expected frame");
        let expected_img = image::RgbaImage::from_raw(8, 8, expected_rgba).unwrap();

        assert_eq!(
            still_img.as_raw(),
            expected_img.as_raw(),
            "still --time must match the encoder's frame stream, not a linear scene-boundary walk"
        );

        let _ = std::fs::remove_file(&out);
    }
}
