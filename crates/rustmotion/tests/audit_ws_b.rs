use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct ScratchFile(PathBuf);

impl ScratchFile {
    fn new(label: &str) -> Self {
        let unique = format!(
            "rustmotion-audit-ws-b-{label}-{}-{}",
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

struct ScratchDir(PathBuf);

impl ScratchDir {
    fn new(label: &str) -> Self {
        let unique = format!(
            "rustmotion-audit-ws-b-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock before UNIX epoch")
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&path).expect("create scratch dir");
        Self(path)
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run_validate(
    scenario_path: &Path,
    report_path: Option<&Path>,
    fix: bool,
    strict_anim: bool,
) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_rustmotion"));
    cmd.arg("validate").arg("--file").arg(scenario_path);
    if let Some(report_path) = report_path {
        cmd.arg("--report").arg(report_path);
    }
    if fix {
        cmd.arg("--fix");
    }
    if strict_anim {
        cmd.arg("--strict-anim");
    }
    cmd.output().expect("failed to spawn `rustmotion validate`")
}

fn read_report(path: &Path) -> serde_json::Value {
    let text = std::fs::read_to_string(path).expect("read report");
    serde_json::from_str(&text).expect("report is valid JSON")
}

fn violations(report: &serde_json::Value) -> &Vec<serde_json::Value> {
    report["geometry_violations"]
        .as_array()
        .expect("geometry_violations is an array")
}

fn count_kind(report: &serde_json::Value, kind: &str) -> usize {
    violations(report)
        .iter()
        .filter(|v| v["kind"] == kind)
        .count()
}

fn find_kind<'a>(report: &'a serde_json::Value, kind: &str) -> Option<&'a serde_json::Value> {
    violations(report).iter().find(|v| v["kind"] == kind)
}

fn vw_shape_scenario(x: f32) -> String {
    format!(
        r##"{{
            "video": {{ "width": 1080, "height": 1920 }},
            "scenes": [{{
                "duration": 1.0,
                "children": [{{
                    "type": "shape",
                    "shape": "rect",
                    "position": "absolute",
                    "x": {x}, "y": 100,
                    "style": {{ "width": "90vw", "height": "50px" }},
                    "fill": "#ff0000"
                }}]
            }}]
        }}"##
    )
}

#[test]
fn resting_layout_measures_vw_against_the_real_viewport_width() {
    let scenario = ScratchFile::new("rm05-resting-scenario");
    let report = ScratchFile::new("rm05-resting-report");
    std::fs::write(&scenario.0, vw_shape_scenario(200.0)).expect("write scenario");

    let output = run_validate(&scenario.0, Some(&report.0), false, false);
    assert!(
        !output.status.success(),
        "a shape whose right edge is past the viewport at either candidate width must block; \
         stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let report_json = read_report(&report.0);
    let violation = find_kind(&report_json, "viewport_overflow")
        .expect("expected a viewport_overflow violation");
    let width = violation["bbox"]["w"].as_f64().expect("bbox.w is a number");
    assert!(
        (width - 972.0).abs() < 2.0,
        "90vw on a 1080px-wide viewport must resolve to ~972px (the real viewport), \
         not 1728px (0.9 x the hardcoded 1920 default); report: {report_json}"
    );
}

#[test]
fn strict_anim_also_measures_vw_against_the_real_viewport_width() {
    let scenario = ScratchFile::new("rm05-strict-anim-scenario");
    let report = ScratchFile::new("rm05-strict-anim-report");
    std::fs::write(&scenario.0, vw_shape_scenario(50.0)).expect("write scenario");

    let output = run_validate(&scenario.0, Some(&report.0), false, true);
    let report_json = read_report(&report.0);
    assert!(
        output.status.success(),
        "the real 1080px-wide viewport keeps this shape on-screen at every sample; \
         stdout={} stderr={} report={report_json}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        count_kind(&report_json, "viewport_overflow"),
        0,
        "resting pass (line ~180) must be clean: {report_json}"
    );
    assert_eq!(
        count_kind(&report_json, "animated_text_overflow"),
        0,
        "--strict-anim pass (line ~1347) must also be clean: {report_json}"
    );
}

#[test]
fn static_translate_em_resolves_against_the_nodes_own_font_size() {
    let scenario = ScratchFile::new("rm15-em-scenario");
    let report = ScratchFile::new("rm15-em-report");
    let json = r##"{
        "video": { "width": 1920, "height": 1080 },
        "scenes": [{
            "duration": 1.0,
            "children": [{
                "type": "shape",
                "shape": "rect",
                "position": "absolute",
                "x": 200, "y": 400,
                "style": {
                    "width": "100px", "height": "50px",
                    "font-size": "96px",
                    "transform": [{ "fn": "translate-x", "x": "-10em" }]
                },
                "fill": "#ff0000"
            }]
        }]
    }"##;
    std::fs::write(&scenario.0, json).expect("write scenario");

    let output = run_validate(&scenario.0, Some(&report.0), false, false);
    let report_json = read_report(&report.0);
    assert!(
        !output.status.success(),
        "a -10em translate on a 96px-font node must push the shape off-frame; report={report_json}"
    );
    let violation = find_kind(&report_json, "viewport_overflow")
        .expect("expected a viewport_overflow violation");
    let x = violation["bbox"]["x"].as_f64().expect("bbox.x is a number");
    assert!(
        (x - (-760.0)).abs() < 2.0,
        "expected bbox.x ~ -760 (200 - 10*96), got {x}: {report_json}"
    );
}

#[test]
fn static_translate_percent_resolves_per_axis_not_against_max_of_both() {
    let scenario = ScratchFile::new("rm15-percent-scenario");
    let report = ScratchFile::new("rm15-percent-report");
    let json = r##"{
        "video": { "width": 1920, "height": 1080 },
        "scenes": [{
            "duration": 1.0,
            "children": [{
                "type": "shape",
                "shape": "rect",
                "position": "absolute",
                "x": 100, "y": 800,
                "style": {
                    "width": "1000px", "height": "100px",
                    "transform": [{ "fn": "translate-y", "y": "50%" }]
                },
                "fill": "#ff0000"
            }]
        }]
    }"##;
    std::fs::write(&scenario.0, json).expect("write scenario");

    let output = run_validate(&scenario.0, Some(&report.0), false, false);
    let report_json = read_report(&report.0);
    assert!(
        output.status.success(),
        "a 50% translateY on a 1000x100 box must resolve against its own 100px height \
         (50px shift, still on-frame), not max(1000,100); report={report_json}"
    );
    assert_eq!(
        count_kind(&report_json, "viewport_overflow"),
        0,
        "{report_json}"
    );
}

#[test]
fn fix_leaves_relative_asset_paths_untouched() {
    let dir = ScratchDir::new("rm16");
    std::fs::create_dir_all(dir.0.join("assets")).expect("mkdir assets");
    std::fs::write(
        dir.0.join("assets/logo.png"),
        b"not a real png, just needs to exist",
    )
    .expect("write asset");
    let scenario_path = dir.0.join("scenario.json");
    let json = r##"{
        "video": { "width": 1920, "height": 4000 },
        "scenes": [{
            "duration": 1.0,
            "children": [
                {
                    "type": "image",
                    "src": "assets/logo.png",
                    "style": { "width": "200px", "height": "150px" }
                },
                {
                    "type": "text",
                    "content": "This is a fairly long sentence with several short words that will wrap nicely across many lines without any single word being too wide for the box.",
                    "style": {
                        "width": "300px", "height": "2000px",
                        "color": "#ffffff", "font-size": "32px", "white-space": "nowrap"
                    }
                }
            ]
        }]
    }"##;
    std::fs::write(&scenario_path, json).expect("write scenario");

    let output = run_validate(&scenario_path, None, true, false);
    assert!(
        output.status.success(),
        "the only violation (the nowrap text) is fixed in place, so this run should now \
         validate clean; stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let fixed: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&scenario_path).expect("read fixed scenario"),
    )
    .expect("fixed scenario is valid JSON");
    let src = fixed["scenes"][0]["children"][0]["src"]
        .as_str()
        .expect("image src is a string");
    assert_eq!(
        src, "assets/logo.png",
        "the image src must stay exactly as authored, not rewritten to an absolute path: {fixed}"
    );

    let text_style = &fixed["scenes"][0]["children"][1]["style"];
    assert!(
        text_style.get("white-space").is_none(),
        "the actual violation --fix targeted must still be fixed: {fixed}"
    );
}

#[test]
fn unwrappable_text_overflow_is_measured_against_the_content_box() {
    let scenario = ScratchFile::new("rm31-scenario");
    let report = ScratchFile::new("rm31-report");
    let json = r##"{
        "video": { "width": 2400, "height": 1080 },
        "scenes": [{
            "duration": 1.0,
            "children": [{
                "type": "text",
                "content": "Hello World Example",
                "position": "absolute",
                "x": 50, "y": 50,
                "style": {
                    "width": "2000px", "height": "300px",
                    "padding": { "top": "20px", "right": "950px", "bottom": "20px", "left": "950px" },
                    "white-space": "nowrap",
                    "font-size": "48px",
                    "color": "#ffffff"
                }
            }]
        }]
    }"##;
    std::fs::write(&scenario.0, json).expect("write scenario");

    let output = run_validate(&scenario.0, Some(&report.0), false, false);
    let report_json = read_report(&report.0);
    assert!(
        !output.status.success(),
        "the 100px content box (2000px border box minus 1900px of padding) is too narrow \
         for this nowrap line; report={report_json}"
    );
    let violation = find_kind(&report_json, "unwrappable_text_overflow")
        .expect("expected an unwrappable_text_overflow violation");
    let width = violation["bbox"]["w"].as_f64().expect("bbox.w is a number");
    assert!(
        (width - 100.0).abs() < 1.0,
        "violation bbox should be the 100px CONTENT box, not the 2000px border box: {report_json}"
    );
}

#[test]
fn nowrap_text_taller_than_its_box_is_still_flagged() {
    let scenario = ScratchFile::new("rm32-scenario");
    let report = ScratchFile::new("rm32-report");
    let json = r##"{
        "video": { "width": 1920, "height": 1080 },
        "scenes": [{
            "duration": 1.0,
            "children": [{
                "type": "text",
                "content": "Hi",
                "position": "absolute",
                "x": 50, "y": 50,
                "style": {
                    "width": "500px", "height": "40px",
                    "white-space": "nowrap",
                    "font-size": "120px",
                    "color": "#ffffff"
                }
            }]
        }]
    }"##;
    std::fs::write(&scenario.0, json).expect("write scenario");

    let output = run_validate(&scenario.0, Some(&report.0), false, false);
    let report_json = read_report(&report.0);
    assert!(
        !output.status.success(),
        "a 120px-font single line is far taller than a 40px box; report={report_json}"
    );
    let violation = find_kind(&report_json, "content_overflows_box")
        .expect("expected a content_overflows_box violation");
    assert_eq!(violation["axis"], "y", "{report_json}");
}

#[test]
fn geometry_does_not_redefine_component_kind() {
    let source = include_str!("../src/cli/commands/geometry.rs");
    assert!(
        !source.contains("fn component_kind"),
        "geometry.rs must not define its own component_kind — it should call \
         rustmotion::components::box_builder::component_kind instead"
    );
    assert!(
        source.contains("box_builder"),
        "geometry.rs must import component_kind from box_builder"
    );
}

#[test]
fn violation_component_label_still_resolves_after_dedup() {
    let scenario = ScratchFile::new("rm40-scenario");
    let report = ScratchFile::new("rm40-report");
    let json = r##"{
        "video": { "width": 1920, "height": 1080 },
        "scenes": [{
            "duration": 1.0,
            "children": [{
                "type": "shape",
                "shape": "rect",
                "position": "absolute",
                "x": 1900, "y": 100,
                "style": { "width": "100px", "height": "100px" },
                "fill": "#ff0000"
            }]
        }]
    }"##;
    std::fs::write(&scenario.0, json).expect("write scenario");

    let output = run_validate(&scenario.0, Some(&report.0), false, false);
    assert!(!output.status.success());
    let report_json = read_report(&report.0);
    let violation = find_kind(&report_json, "viewport_overflow").expect("violation present");
    assert_eq!(violation["component"], "shape", "{report_json}");
}
