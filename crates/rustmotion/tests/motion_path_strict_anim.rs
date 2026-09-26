use std::path::PathBuf;
use std::process::{Command, Output};

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

#[test]
fn strict_anim_detects_a_motion_path_that_leaves_the_viewport() {
    let scenario = ScratchFile::new("overflow-scenario");
    let report = ScratchFile::new("overflow-report");
    write_scenario(&scenario, &scenario_json("M0,0 L3000,0", 2.0));

    let output = run_validate(&scenario.0, &report.0, true);
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

#[test]
fn without_strict_anim_the_same_out_of_frame_motion_path_is_not_caught() {
    let scenario = ScratchFile::new("overflow-scenario-no-strict");
    let report = ScratchFile::new("overflow-report-no-strict");
    write_scenario(&scenario, &scenario_json("M0,0 L3000,0", 2.0));

    let output = run_validate(&scenario.0, &report.0, false);
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

#[test]
fn strict_anim_does_not_flag_a_motion_path_that_stays_on_screen() {
    let scenario = ScratchFile::new("safe-scenario");
    let report = ScratchFile::new("safe-report");
    write_scenario(&scenario, &scenario_json("M0,0 L50,0", 2.0));

    let output = run_validate(&scenario.0, &report.0, true);
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
