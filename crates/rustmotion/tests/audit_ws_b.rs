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

// ─── --fix must not bake in machine-absolute asset paths ───────────

/// `--fix` used to serialise `LoadedScenario::raw`, captured AFTER
/// `rustmotion::assets::rebase_relative_paths` rewrites every existing-file
/// `src`/`track` into a canonicalised ABSOLUTE path — so fixing an
/// unrelated violation (here, a too-wide nowrap text) silently replaced
/// `"assets/logo.png"` with this machine's own absolute path. The asset
/// file only needs to EXIST (rebasing is gated on `Path::is_file()`); its
/// content is irrelevant here since geometry validation never decodes it
/// (`Image` uses a fixed 400×300 default intrinsic size, not real pixel
/// dimensions).
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

    let output = run_validate(&scenario_path, None, /*fix=*/ true, false);
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

// ─── unwrappable_text_overflow must measure the CONTENT box ────────

/// A nowrap text's own painter draws inside its CONTENT box
/// (`LegacyPaintDispatcher` hands it `layout.content_box()`, not the raw
/// layout box, for every component except `codeblock`) — so the geometry
/// check must compare the natural line width against the content box too.
/// Content box width here is 2000 - 1900 = 100px (950px of padding on each
/// side); the border box is 2000px. Any real natural width for this
/// string/font-size sits comfortably in between, so the violation fires if
/// and only if the content box is used.
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

// ─── white-space: nowrap must not silence the height check ─────────

/// A single unwrapped 120px-font line is ~144px tall, well past a 40px-tall
/// box — the exact case `content_overflows_box` already catches for
/// wrapping text. `white-space: nowrap` used to return before measuring
/// height at all, so this validated clean.
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

// ─── auto_scroll_disabled_overflow must use the terminal's CONTENT box height ───

/// A terminal's own painter is NOT self-padding
/// (`LegacyPaintDispatcher::is_self_padding` matches only `Codeblock`) — it
/// paints inside its content box, so `auto_scroll: false` must compare
/// natural height against that, not the border box. 10 lines ≈ 288px
/// natural height (36px chrome + 32px internal terminal padding + 10×22px
/// lines at the default 14px font); border box height is 400px, content box
/// height is 400 - 300 (150px top+bottom CSS padding) = 100px.
#[test]
fn terminal_auto_scroll_disabled_overflow_is_measured_against_the_content_box() {
    let scenario = ScratchFile::new("rm33-scenario");
    let report = ScratchFile::new("rm33-report");
    let lines: String = (1..=10)
        .map(|i| format!(r##"{{ "text": "line {i}" }}"##))
        .collect::<Vec<_>>()
        .join(",");
    let json = format!(
        r##"{{
            "video": {{ "width": 1920, "height": 1080 }},
            "scenes": [{{
                "duration": 1.0,
                "children": [{{
                    "type": "terminal",
                    "lines": [{lines}],
                    "auto_scroll": false,
                    "position": "absolute",
                    "x": 50, "y": 50,
                    "style": {{
                        "width": "800px", "height": "400px",
                        "padding": {{ "top": "150px", "bottom": "150px" }}
                    }}
                }}]
            }}]
        }}"##
    );
    std::fs::write(&scenario.0, json).expect("write scenario");

    let output = run_validate(&scenario.0, Some(&report.0), false, false);
    let report_json = read_report(&report.0);
    assert!(
        !output.status.success(),
        "~288px of natural content is far past a 100px content box (400px border box minus \
         300px of padding); report={report_json}"
    );
    let violation = find_kind(&report_json, "auto_scroll_disabled_overflow")
        .expect("expected an auto_scroll_disabled_overflow violation");
    let height = violation["bbox"]["h"].as_f64().expect("bbox.h is a number");
    assert!(
        (height - 100.0).abs() < 1.0,
        "violation bbox should be the 100px CONTENT box, not the 400px border box: {report_json}"
    );
}

/// Negative control: a codeblock genuinely IS self-padding
/// (`LegacyPaintDispatcher::is_self_padding`), so its own natural-height
/// formula already bakes its padding in — the codeblock arm must keep
/// comparing against the BORDER box, unaffected by this fix. Same 60px
/// padding fixture as the pre-existing
/// `codeblock_auto_scroll_check_honours_explicit_padding_not_a_hardcoded_16px`
/// internal test, driven through the CLI instead.
#[test]
fn codeblock_auto_scroll_disabled_overflow_still_uses_the_border_box() {
    let scenario = ScratchFile::new("rm33-codeblock-scenario");
    let report = ScratchFile::new("rm33-codeblock-report");
    let code_lines: String = (1..=10)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join("\\n");
    let json = format!(
        r##"{{
            "video": {{ "width": 1920, "height": 1080 }},
            "scenes": [{{
                "duration": 1.0,
                "children": [{{
                    "type": "codeblock",
                    "code": "{code_lines}",
                    "auto_scroll": false,
                    "style": {{ "width": "600px", "height": "250px", "padding": "60px" }}
                }}]
            }}]
        }}"##
    );
    std::fs::write(&scenario.0, json).expect("write scenario");

    let output = run_validate(&scenario.0, Some(&report.0), false, false);
    let report_json = read_report(&report.0);
    assert!(!output.status.success(), "report={report_json}");
    let violation = find_kind(&report_json, "auto_scroll_disabled_overflow")
        .expect("expected an auto_scroll_disabled_overflow violation");
    let height = violation["bbox"]["h"].as_f64().expect("bbox.h is a number");
    assert!(
        (height - 250.0).abs() < 1.0,
        "codeblock is self-padding: the reported bbox must stay the 250px BORDER box, \
         not a content box: {report_json}"
    );
}

// ─── geometry.rs must not duplicate box_builder's component_kind ───

/// `rustmotion_components::box_builder::component_kind` is already `pub`
/// and already imported into this same binary crate elsewhere
/// (`engine/render/scene.rs:719`) — geometry.rs must reuse it instead of
/// carrying its own private 60-arm copy that can silently drift from it on
/// a rename. `rustmotion::cli::commands` is a private module, so this
/// checks the SOURCE FILE directly rather than calling the (unreachable)
/// function itself — see this file's own top-of-file doc comment for why
/// every other test here goes through the CLI subprocess instead.
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

/// Smoke test: violations must still carry a sensible `component` label
/// after the switch to the shared helper — proves the dedup didn't silently
/// break the import wiring.
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
