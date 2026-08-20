//! The decisive proof for the `motion_path` animation effect: a component
//! following a path off the edge of the frame must be caught by
//! `rustmotion validate --strict-anim`.
//!
//! Why this is the test that matters most (see the workstream brief): the
//! validator's `--strict-anim` pass (`commands::geometry::
//! validate_geometry_animated`, folding transforms via
//! `apply_static_node_transform`) only ever looks at `css.transform`. If
//! `motion_path` positioned a component through any other channel — a
//! painter-only field on `AnimatedProperties`, or a bespoke value the CSS
//! bridge (`css::animation::apply_animated_props`) never translates —
//! `--strict-anim` would see nothing, and a component sailing off the
//! viewport while following its curve would pass validation silently. This
//! test is run against the actual compiled `rustmotion` binary (not an
//! internal call into `validate_geometry_animated`) because
//! `rustmotion::cli::commands` is a private module (`mod commands;` in
//! `src/cli/mod.rs`) — the CLI subprocess is the only externally-observable
//! contract for "`--strict-anim` sees this".
//!
//! Three scenarios, each isolating one variable:
//! 1. A `motion_path` that travels far enough to leave the viewport must be
//!    flagged, and only under `--strict-anim` (the resting/frame-0 layout is
//!    fully inside the frame — only sampling the animated transform catches
//!    it, which is the whole point of `--strict-anim`).
//! 2. The same component with a `motion_path` that stays on-screen the
//!    whole time must NOT be flagged — ruling out a validator that treats
//!    any `motion_path` as suspect rather than actually measuring it.

use std::path::PathBuf;
use std::process::{Command, Output};

/// Minimal RAII scratch file — avoids a `tempfile` dev-dependency for three
/// small fixtures (mirrors `skill_files_match_disk.rs`'s `ScratchDir`).
struct ScratchFile(PathBuf);

impl ScratchFile {
    fn new(label: &str) -> Self {
        let unique = format!(
            "rustmotion-motion-path-test-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock before UNIX epoch")
                .as_nanos()
        );
        Self(std::env::temp_dir().join(unique))
    }
}

impl Drop for ScratchFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// A single 100x100 red square, resting well inside a 1920x1080 viewport
/// (x=[200,300], y=[490,590] at rest — both axes comfortably clear of every
/// edge), animated by one `motion_path` effect. `path` is the raw SVG path
/// `d` string; the square starts at its laid-out position (path coordinates
/// are deltas — see `MotionPathConfig`'s doc comment) and slides along
/// `path` over `duration` seconds within a `duration`-second scene (no
/// dead time at the end where sampling would only see the resting-again
/// state).
fn scenario_json(path: &str, duration: f64) -> String {
    format!(
        r##"{{
            "video": {{ "width": 1920, "height": 1080 }},
            "scenes": [{{
                "duration": {duration},
                "children": [{{
                    "type": "shape",
                    "shape": "rect",
                    "position": "absolute",
                    "x": 200, "y": 490,
                    "style": {{
                        "width": "100px", "height": "100px",
                        "animation": [
                            {{ "name": "motion_path", "path": "{path}", "duration": {duration} }}
                        ]
                    }},
                    "fill": "#ff0000"
                }}]
            }}]
        }}"##
    )
}

fn write_scenario(scratch: &ScratchFile, json: &str) {
    std::fs::write(&scratch.0, json).expect("write scenario fixture");
}

fn run_validate(scenario_path: &PathBuf, report_path: &PathBuf, strict_anim: bool) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustmotion"));
    cmd.arg("validate")
        .arg("--file")
        .arg(scenario_path)
        .arg("--report")
        .arg(report_path);
    if strict_anim {
        cmd.arg("--strict-anim");
    }
    cmd.output().expect("failed to spawn `rustmotion validate`")
}

fn animated_text_overflow_count(report_json: &serde_json::Value) -> usize {
    report_json["geometry_violations"]
        .as_array()
        .map(|v| {
            v.iter()
                .filter(|violation| violation["kind"] == "animated_text_overflow")
                .count()
        })
        .unwrap_or(0)
}

/// The decisive test: a `motion_path` sliding a component 2000px to the
/// right over a 2s scene must push it past the 1920px-wide viewport's right
/// edge at some sampled time, and `--strict-anim` must report it as
/// `animated_text_overflow` — the same violation kind the pre-existing
/// `spin`/rotation coverage in `commands::geometry`'s own test suite uses.
#[test]
fn strict_anim_detects_a_motion_path_that_leaves_the_viewport() {
    let scenario = ScratchFile::new("overflow-scenario");
    let report = ScratchFile::new("overflow-report");
    write_scenario(&scenario, &scenario_json("M0,0 L3000,0", 2.0));

    let output = run_validate(&scenario.0, &report.0, /*strict_anim=*/ true);
    assert!(
        !output.status.success(),
        "expected `validate --strict-anim` to fail (block) on an out-of-frame motion_path; \
         stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let report_text = std::fs::read_to_string(&report.0).expect("read report");
    let report_json: serde_json::Value =
        serde_json::from_str(&report_text).expect("report is valid JSON");
    let count = animated_text_overflow_count(&report_json);
    assert!(
        count >= 1,
        "expected at least one animated_text_overflow violation, got report: {report_text}"
    );
}

/// The negative control for the decisive test: the exact same component,
/// same 2000px-off-frame `motion_path`, but validated WITHOUT
/// `--strict-anim`. The component's resting (t=0) box is fully inside the
/// viewport — only sampling the *animated* transform can see the
/// overflow — so this must report clean. If it didn't (i.e. if this also
/// failed), the decisive test above would be meaningless: it would prove
/// only that the static box is flagged, not that `--strict-anim` is doing
/// anything path-specific.
#[test]
fn without_strict_anim_the_same_out_of_frame_motion_path_is_not_caught() {
    let scenario = ScratchFile::new("overflow-scenario-no-strict");
    let report = ScratchFile::new("overflow-report-no-strict");
    write_scenario(&scenario, &scenario_json("M0,0 L3000,0", 2.0));

    let output = run_validate(&scenario.0, &report.0, /*strict_anim=*/ false);
    assert!(
        output.status.success(),
        "the resting layout alone must validate clean (the overflow is animation-only); \
         stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let report_text = std::fs::read_to_string(&report.0).expect("read report");
    let report_json: serde_json::Value =
        serde_json::from_str(&report_text).expect("report is valid JSON");
    assert_eq!(
        animated_text_overflow_count(&report_json),
        0,
        "must not report animated_text_overflow without --strict-anim: {report_text}"
    );
}

/// The false-positive guard: a `motion_path` that only ever moves the
/// component a few pixels — nowhere near any viewport edge — must validate
/// clean even under `--strict-anim`. Without this, the decisive test above
/// would not distinguish "correctly measures the path" from "flags every
/// motion_path indiscriminately".
#[test]
fn strict_anim_does_not_flag_a_motion_path_that_stays_on_screen() {
    let scenario = ScratchFile::new("safe-scenario");
    let report = ScratchFile::new("safe-report");
    // 50px of travel from a resting position 200px clear of the nearest
    // (left) edge and >1500px clear of the right edge — nowhere close to
    // leaving the 1920-wide viewport at any sampled time.
    write_scenario(&scenario, &scenario_json("M0,0 L50,0", 2.0));

    let output = run_validate(&scenario.0, &report.0, /*strict_anim=*/ true);
    assert!(
        output.status.success(),
        "a motion_path that stays on-screen must validate clean under --strict-anim; \
         stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let report_text = std::fs::read_to_string(&report.0).expect("read report");
    let report_json: serde_json::Value =
        serde_json::from_str(&report_text).expect("report is valid JSON");
    assert_eq!(
        animated_text_overflow_count(&report_json),
        0,
        "on-screen travel must not be flagged: {report_text}"
    );
}
