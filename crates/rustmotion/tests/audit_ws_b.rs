//! Regression tests — audit round chantier/audit-2026-09, workstream B
//! (geometry validator).
//!
//! Every test here drives the actual compiled `rustmotion` binary rather
//! than calling `commands::geometry`/`commands::validate` in-process:
//! `rustmotion::cli::commands` is a private module (`mod commands;` in
//! `src/cli/mod.rs`), so the CLI subprocess — `validate --report <path>`,
//! whose JSON is `commands::geometry::GeometryViolation`'s public `Serialize`
//! output — is the only externally-observable contract for what the
//! validator decided (mirrors `motion_path_strict_anim.rs`'s reasoning).
//!
//! One section per finding, in briefing order: viewport units, transform
//! lengths, path rewriting, the three box-model checks, and helper reuse.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Minimal RAII scratch file — mirrors `motion_path_strict_anim.rs`'s
/// `ScratchFile`.
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

/// Minimal RAII scratch directory — mirrors `skill_files_match_disk.rs`'s
/// `ScratchDir`. Only the path-rewriting case needs a directory (a file plus a
/// sibling asset file).
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

// ─── vw/vh must resolve against the scenario's real viewport ───────

/// A `width: "90vw"` shape on a 1080×1920 scenario, positioned so its right
/// edge crosses the viewport edge at EITHER candidate width — 972px (90% of
/// the real 1080px-wide viewport) or 1728px (90% of the hardcoded
/// `ConversionContext::default()` 1920px fallback). The violation fires
/// either way; only the reported `bbox.w` distinguishes a correct
/// measurement from the buggy one.
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

/// Same shape, repositioned so it overflows ONLY under the buggy
/// 1920x1080 default (right edge 1778px vs an 1080px-wide viewport) and
/// stays clean at the correct 972px width (right edge 1022px). Isolates
/// the `--strict-anim` call site (`validate_geometry_animated`, geometry.rs
/// ~1347) from the resting one above (~180): each builds its own box tree
/// through `ConversionContext::default()` independently, so fixing only
/// one would still leave this failing.
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

// ─── static transform lengths use the node's own font-size and per-axis size ───

/// `translateX(-10em)` on a 96px-font node must resolve against ITS OWN
/// font-size (960px), not the hardcoded 16px `apply_static_node_transform`
/// used to assume (160px). At x=200 with a 100px-wide box, the correct
/// translate pushes the box to x=-760 (fully off-frame, a real violation);
/// the buggy 160px translate only reaches x=40 (still on-frame, silent).
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

/// `translateY(50%)` on a WIDE, SHORT box (1000×100) must resolve against
/// its OWN height (50px), not `max(width, height)` (500px) — the exact
/// mistake `paint_pass.rs`'s `length_ctx_x`/`length_ctx_y` split exists to
/// avoid. The correct 50px shift keeps the box on-frame; the buggy 500px
/// shift pushes its bottom edge to 1400px, past the 1080px-tall viewport.
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
