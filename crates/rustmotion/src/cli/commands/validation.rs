use rustmotion::engine;
use rustmotion::error::{Result, RustmotionError};
use rustmotion::expand;
use rustmotion::include::IncludeSource;
use rustmotion::schema::{ResolvedScenario, Scenario};
use rustmotion::variables;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::geometry::{
    check_legibility, check_off_grid_cuts, validate_geometry, validate_geometry_animated,
    validate_geometry_transitions, GeometryViolation,
};
use super::validate_schema::validate_scenario;

pub enum ValidationSource<'a> {
    File(&'a Path),
    Inline(&'a str),
}

pub type VarOverrides = HashMap<String, serde_json::Value>;

pub struct LoadedScenario {
    pub raw: serde_json::Value,
    pub scenario: ResolvedScenario,
    pub source_path: Option<PathBuf>,
}

#[derive(Default)]
pub struct ValidationReport {
    pub schema_errors: Vec<String>,
    pub geom_violations: Vec<GeometryViolation>,
    pub unresolved_vars: Vec<String>,
    pub warnings: Vec<String>,
    pub attr_warnings: Vec<String>,
}

impl ValidationReport {
    pub fn is_clean(&self) -> bool {
        self.schema_errors.is_empty()
            && self.geom_violations.is_empty()
            && self.unresolved_vars.is_empty()
            && self.warnings.is_empty()
            && self.attr_warnings.is_empty()
    }

    pub fn is_blocking(&self, lenient: bool) -> bool {
        if !self.schema_errors.is_empty() {
            return true;
        }
        if !self.attr_warnings.is_empty() {
            return true;
        }
        if !lenient && !self.geom_violations.is_empty() {
            return true;
        }
        false
    }

    pub fn to_error(&self) -> RustmotionError {
        RustmotionError::ValidationFailed {
            schema_errors: self.schema_errors.len() + self.attr_warnings.len(),
            geometry_violations: self.geom_violations.len(),
            unresolved_vars: self.unresolved_vars.len(),
        }
    }

    pub fn promote_attr_warnings(&mut self) {
        self.schema_errors.append(&mut self.attr_warnings);
    }
}

pub fn load(source: ValidationSource<'_>) -> Result<LoadedScenario> {
    load_with_vars(source, None)
}

pub fn load_with_vars(
    source: ValidationSource<'_>,
    overrides: Option<&VarOverrides>,
) -> Result<LoadedScenario> {
    let (json_str, source_path, include_source) = match source {
        ValidationSource::File(path) => {
            let s = std::fs::read_to_string(path).map_err(|e| RustmotionError::FileRead {
                path: path.display().to_string(),
                source: e,
            })?;
            let s = if rustmotion::loader::is_html_path(path) {
                let mut value = rustmotion::loader::html_to_scenario_json(&s)?;
                let annotations = rustmotion::loader::load_html_annotations_sidecar(path)?;
                if !annotations.is_empty() {
                    value["annotations"] = serde_json::Value::Array(annotations);
                }
                serde_json::to_string(&value).map_err(RustmotionError::from)?
            } else {
                s
            };
            (
                s,
                Some(path.to_path_buf()),
                IncludeSource::File(path.to_path_buf()),
            )
        }
        ValidationSource::Inline(json) => (json.to_string(), None, IncludeSource::Inline),
    };

    let mut json_value: serde_json::Value =
        serde_json::from_str(&json_str).map_err(RustmotionError::from)?;

    let label = source_path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<inline>".to_string());
    variables::apply_variables(&mut json_value, overrides, &label)?;
    expand::expand_directives(&mut json_value, &label)?;
    rustmotion::loader::fold_static_expressions(&mut json_value, &label)?;

    if let Some(dir) = source_path.as_ref().and_then(|p| p.parent()) {
        rustmotion::assets::rebase_relative_paths(&mut json_value, dir);
    }

    let scenario: Scenario = serde_json::from_value(json_value.clone())?;
    let resolved =
        rustmotion::loader::resolve_includes_and_synthesize_audio(scenario, &include_source)?;

    Ok(LoadedScenario {
        raw: json_value,
        scenario: resolved,
        source_path,
    })
}

pub fn run_checks(loaded: &LoadedScenario, strict_anim: bool) -> ValidationReport {
    if !loaded.scenario.fonts.is_empty() {
        engine::renderer::load_custom_fonts(&loaded.scenario.fonts);
    }
    let mut geom_violations = validate_geometry(&loaded.scenario);
    if strict_anim {
        geom_violations.extend(validate_geometry_animated(&loaded.scenario));
        geom_violations.extend(validate_geometry_transitions(&loaded.scenario));
    }
    let (mut schema_errors, mut warnings) = validate_scenario(&loaded.scenario);
    warnings.extend(warn_misplaced_animation(&loaded.raw));
    warnings.extend(check_legibility(&loaded.scenario));
    warnings.extend(check_off_grid_cuts(&loaded.scenario));
    schema_errors.extend(check_node_references(&loaded.scenario));
    let (attr_errors, mut attr_warnings) =
        super::validate_attrs::check_component_attrs(&loaded.scenario);
    schema_errors.extend(attr_errors);
    schema_errors.append(&mut attr_warnings);
    ValidationReport {
        schema_errors,
        geom_violations,
        unresolved_vars: variables::find_unresolved(&loaded.raw),
        warnings,
        attr_warnings,
    }
}

fn check_node_references(scenario: &ResolvedScenario) -> Vec<String> {
    use rustmotion::components::box_builder::collect_node_refs;
    use rustmotion::engine::deps::DepGraph;
    use std::collections::HashSet;

    let per_scene: Vec<(
        (usize, usize),
        Vec<(String, Vec<rustmotion::engine::deps::NodeRef>)>,
    )> = scenario
        .views
        .iter()
        .enumerate()
        .flat_map(|(vi, view)| {
            view.scenes
                .iter()
                .enumerate()
                .map(move |(si, scene)| (vi, si, scene))
        })
        .map(|(vi, si, scene)| {
            let children = engine::render::deserialize_children(scene);
            ((vi, si), collect_node_refs(&children))
        })
        .collect();

    let mut errors = Vec::new();
    for (i, (loc, nodes)) in per_scene.iter().enumerate() {
        let other_scene_ids: HashSet<String> = per_scene
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .flat_map(|(_, (_, n))| n.iter().map(|(id, _)| id.clone()))
            .collect();
        if let Err(e) = DepGraph::build(nodes, &other_scene_ids) {
            let (vi, si) = loc;
            errors.push(format!("views[{vi}].scenes[{si}]: {e}"));
        }
    }
    errors
}

pub fn warn_misplaced_animation(raw: &serde_json::Value) -> Vec<String> {
    let mut out = Vec::new();
    walk_misplaced_animation(raw, String::new(), &mut out);
    out
}

fn walk_misplaced_animation(v: &serde_json::Value, path: String, out: &mut Vec<String>) {
    match v {
        serde_json::Value::Object(map) => {
            if map.contains_key("type") && map.contains_key("animation") {
                let kind = map.get("type").and_then(|t| t.as_str()).unwrap_or("?");
                let label = map
                    .get("content")
                    .or_else(|| map.get("text"))
                    .and_then(|t| t.as_str())
                    .map(|s| format!(" (\"{}\")", s.chars().take(24).collect::<String>()))
                    .unwrap_or_default();
                let where_ = if path.is_empty() { "<root>" } else { &path };
                out.push(format!(
                    "`animation` on `{kind}`{label} at {where_} is at the component top level and is IGNORED — move it inside `style` (style.animation)."
                ));
            }
            for (k, child) in map {
                walk_misplaced_animation(child, format!("{path}.{k}"), out);
            }
        }
        serde_json::Value::Array(arr) => {
            for (i, child) in arr.iter().enumerate() {
                walk_misplaced_animation(child, format!("{path}[{i}]"), out);
            }
        }
        _ => {}
    }
}

pub fn warn_on_silent_defaults(loaded: &LoadedScenario) {
    if let Some(video) = loaded.raw.get("video") {
        if video.get("fps").is_none() {
            eprintln!("Warning: video.fps not specified, using default 30");
        }
        if video.get("background").is_none() {
            eprintln!("Warning: video.background not specified, using default #000000");
        }
    }
    if loaded.raw.get("composition").is_none() && loaded.raw.get("scenes").is_some() {
        eprintln!(
            "Warning: top-level `scenes` is legacy. Migrate to `composition: [{{ type: \"slide\", scenes: [...] }}]` for clarity."
        );
    }
    if loaded.raw.get("timing").is_none() {
        eprintln!(
            "Warning: `timing` not specified, using legacy v1 (transition durations are \
             subtracted from the total). Set `\"timing\": \"v2\"` for absolute scene \
             placement with a beat grid."
        );
    }
}

pub fn check_codec(codec: Option<&str>) -> Result<()> {
    if let Some(c) = codec {
        let allowed = ["h264", "h265", "vp9", "prores"];
        if !allowed.contains(&c) {
            return Err(RustmotionError::UnknownCodec {
                codec: c.to_string(),
            });
        }
    }
    Ok(())
}

pub fn check_crf(crf: Option<u8>, hardware_acceleration: bool) -> Result<Option<String>> {
    if let Some(v) = crf {
        if v > 51 {
            return Err(RustmotionError::InvalidCrf { value: v });
        }
        if hardware_acceleration {
            return Ok(Some(format!(
                "--crf {v} has no effect if a hardware encoder ends up being used under \
                 --hardware-acceleration (VideoToolbox/NVENC/QSV/AMF are bitrate/quality-driven, \
                 not CRF-driven); it still applies if ffmpeg falls back to the software encoder."
            )));
        }
    }
    Ok(None)
}

pub fn print_report(report: &ValidationReport, source_label: &str) {
    use super::geometry::format_violation;

    for w in &report.warnings {
        eprintln!("Warning: {}", w);
    }
    for w in &report.attr_warnings {
        eprintln!("Error: {}", w);
    }
    for name in &report.unresolved_vars {
        eprintln!(
            "Warning: unresolved variable reference '${}' in '{}'",
            name, source_label
        );
    }
    for err in &report.schema_errors {
        eprintln!("Error: {}", err);
    }
    if !report.geom_violations.is_empty() {
        eprintln!();
        eprintln!("Geometry: {} violation(s)", report.geom_violations.len());
        for v in &report.geom_violations {
            eprintln!("{}", format_violation(v));
            eprintln!();
        }
    }
}

#[cfg(test)]
mod html_css_error_tests {
    use super::*;

    #[test]
    fn unknown_css_property_from_html_is_a_readable_validate_error() {
        let html = r##"<rustmotion width="100" height="100"><scene duration="2"><h1 style="font-siez:96">Hi</h1></scene></rustmotion>"##;
        let value = rustmotion::loader::html_to_scenario_json(html).expect("transpiles");
        let json = serde_json::to_string(&value).unwrap();
        let loaded = load(ValidationSource::Inline(&json)).expect("loads");
        let report = run_checks(&loaded, false);
        assert!(
            report.schema_errors.iter().any(|e| e.contains("font-siez")),
            "expected a schema error naming the unknown CSS property: {:?}",
            report.schema_errors
        );
        assert!(report.is_blocking(false), "must block rendering");
    }

    #[test]
    fn an_unresolved_variable_warns_without_blocking() {
        let json = r##"{
            "version": "1.0",
            "video": {"width": 320, "height": 180, "fps": 30, "background": "#000"},
            "scenes": [{"duration": 1.0, "children": [
                {"type": "text", "content": "$9.99 a month",
                 "style": {"font-size": 20, "color": "#FFF"}}]}]
        }"##;
        let loaded = load(ValidationSource::Inline(json)).expect("loads");
        let report = run_checks(&loaded, false);

        assert!(
            !report.unresolved_vars.is_empty(),
            "a literal `$` must still be reported: {:?}",
            report.unresolved_vars
        );
        assert!(
            !report.is_blocking(false),
            "a literal `$` in content must not stop the render"
        );
        assert!(
            !report.is_clean(),
            "it is still an advisory — is_clean must stay false so it gets printed"
        );
    }

    #[test]
    fn unknown_animation_preset_from_html_is_a_blocking_error() {
        let html = r##"<rustmotion width="100" height="100"><scene duration="2"><h1 anim="not-a-preset">Hi</h1></scene></rustmotion>"##;
        let value = rustmotion::loader::html_to_scenario_json(html).expect("transpiles");
        let json = serde_json::to_string(&value).unwrap();
        let loaded = load(ValidationSource::Inline(&json)).expect("loads");
        let report = run_checks(&loaded, false);
        assert!(
            report
                .schema_errors
                .iter()
                .any(|e| e.contains("not_a_preset")),
            "expected a schema error naming the unknown preset: {:?}",
            report.schema_errors
        );
        assert!(report.is_blocking(false), "must block rendering");
    }
}

#[cfg(test)]
mod misplaced_animation_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn flags_only_top_level_animation() {
        let raw = json!({
            "scenes": [{ "children": [
                { "type": "text", "style": { "color": "#fff" }, "animation": [{ "name": "fade_in" }] },
                { "type": "text", "style": { "color": "#fff", "animation": [{ "name": "fade_in" }] } }
            ]}]
        });
        let w = warn_misplaced_animation(&raw);
        assert_eq!(
            w.len(),
            1,
            "only the top-level animation should warn: {w:?}"
        );
        assert!(w[0].contains("style.animation"));
    }
}

#[cfg(test)]
mod font_loading_wiring_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn run_checks_does_not_block_on_a_declared_font() {
        let json = json!({
            "video": { "width": 200, "height": 200 },
            "fonts": [
                { "family": "DoesNotExist", "path": "does/not/exist.ttf" }
            ],
            "scenes": [{
                "duration": 1.0,
                "children": [
                    { "type": "text", "content": "hi", "style": { "color": "#fff" } }
                ]
            }]
        })
        .to_string();

        let loaded = load(ValidationSource::Inline(&json)).expect("scenario loads");
        let report = run_checks(&loaded, false);
        assert!(
            report.schema_errors.is_empty(),
            "declared fonts must not block validation: {:?}",
            report.schema_errors
        );
        assert!(!report.is_blocking(false));
    }

    #[test]
    fn run_checks_skips_font_loading_when_no_fonts_declared() {
        let json = json!({
            "video": { "width": 200, "height": 200 },
            "scenes": [{
                "duration": 1.0,
                "children": [
                    { "type": "text", "content": "hi", "style": { "color": "#fff" } }
                ]
            }]
        })
        .to_string();

        let loaded = load(ValidationSource::Inline(&json)).expect("scenario loads");
        assert!(loaded.scenario.fonts.is_empty());
        let report = run_checks(&loaded, false);
        assert!(
            report.schema_errors.is_empty(),
            "{:?}",
            report.schema_errors
        );
    }
}

pub fn warn_strict_attrs_is_now_default() {
    eprintln!(
        "Notice: --strict-attrs is deprecated and does nothing. Unknown \
         component attributes have been errors by default since the attribute \
         checker was hardened; you can drop the flag."
    );
}

#[cfg(test)]
mod expanded_tree_is_what_gets_validated {
    use super::*;

    #[test]
    fn a_geometry_violation_inside_a_for_each_generated_item_is_detected() {
        let json = serde_json::json!({
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "for-each": [
                        { "label": "ok" },
                        { "label": "this string is far too long to fit in this narrow card" }
                    ],
                    "template": {
                        "type": "card",
                        "x": 100, "y": 100,
                        "style": { "width": "200px", "height": "200px", "background": "#222244" },
                        "children": [{
                            "type": "text",
                            "content": "$label",
                            "style": { "color": "#ffffff", "font-size": "96px", "white-space": "nowrap" }
                        }]
                    }
                }]
            }]
        })
        .to_string();

        let loaded = load(ValidationSource::Inline(&json)).expect("scenario loads");

        let raw_children = loaded.raw["scenes"][0]["children"].as_array().unwrap();
        assert_eq!(
            raw_children.len(),
            2,
            "loaded.raw must hold the 2 expanded cards, not the 1 for-each directive"
        );
        assert!(
            raw_children.iter().all(|c| c.get("for-each").is_none()),
            "no for-each directive marker must survive into loaded.raw: {raw_children:?}"
        );

        let report = run_checks(&loaded, false);
        assert_eq!(
            report.geom_violations.len(),
            1,
            "exactly one of the two for-each-generated cards overflows; a validator that only \
             saw the pre-expansion directive could not have found this at all: {:?}",
            report.geom_violations
        );
        assert!(
            report.geom_violations[0].path.contains("children[1]"),
            "expected the violation to be attributed to the second expanded card: {}",
            report.geom_violations[0].path
        );
    }
}

#[cfg(test)]
mod expr_fold_through_validation_pipeline {
    use super::*;

    #[test]
    fn for_each_circle_of_expressions_validates_clean_through_this_pipeline() {
        let json = serde_json::json!({
            "video": { "width": 1080, "height": 1920, "fps": 30 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "for-each": [1,2,3,4,5,6,7,8],
                    "template": {
                        "type": "text",
                        "content": "badge",
                        "position": "absolute",
                        "style": { "color": "#fff", "font-size": "40px", "white-space": "nowrap" },
                        "x": "= $W/2 + cos($i / $count * TAU - PI/2) * 400 - 40",
                        "y": "= $H/2 + sin($i / $count * TAU - PI/2) * 400 - 20"
                    }
                }]
            }]
        })
        .to_string();

        let loaded = load(ValidationSource::Inline(&json)).expect("scenario loads and folds");
        let children = &loaded.raw["scenes"][0]["children"];
        let children = children.as_array().expect("8 expanded children");
        assert_eq!(children.len(), 8);
        for (i, child) in children.iter().enumerate() {
            assert!(
                child["x"].is_number() && child["y"].is_number(),
                "child {i}'s x/y must be folded to plain numbers, got {child}"
            );
        }

        let report = run_checks(&loaded, false);
        assert!(
            report.schema_errors.is_empty(),
            "an expression-driven component must deserialize, not be dropped: {:?}",
            report.schema_errors
        );
        assert!(
            report.unresolved_vars.is_empty(),
            "$W/$H/$i/$count must not be reported as unresolved variables: {:?}",
            report.unresolved_vars
        );
        assert!(!report.is_blocking(false));
    }

    #[test]
    fn dollar_free_static_expression_folds_and_deserializes() {
        let json = serde_json::json!({
            "video": { "width": 200, "height": 200 },
            "scenes": [{
                "duration": 1.0,
                "children": [
                    { "type": "text", "content": "c", "position": "absolute",
                      "x": "= 960 + cos(5.0 / 8.0 * TAU - PI/2) * 600 - 60" }
                ]
            }]
        })
        .to_string();

        let loaded = load(ValidationSource::Inline(&json)).expect("scenario loads and folds");
        assert!(loaded.raw["scenes"][0]["children"][0]["x"].is_number());
        let report = run_checks(&loaded, false);
        assert!(
            report.schema_errors.is_empty(),
            "must not be silently dropped at render: {:?}",
            report.schema_errors
        );
    }
}

#[cfg(test)]
mod node_reference_checks {
    use super::*;

    fn scene_with_id_and_transform_ref(id: &str, target_id: &str) -> serde_json::Value {
        serde_json::json!({
            "duration": 1.0,
            "children": [{
                "type": "shape", "shape": "circle", "id": id,
                "position": "absolute", "x": 0, "y": 0,
                "style": {
                    "width": "20px", "height": "20px",
                    "transform": [
                        { "fn": "translate", "x": format!("= node(\"{target_id}\", \"tx\")"), "y": 0 }
                    ]
                }
            }]
        })
    }

    #[test]
    fn a_reference_to_an_id_in_another_scene_is_reported_as_reference_cross_scene() {
        let json = serde_json::json!({
            "video": { "width": 320, "height": 240, "fps": 30 },
            "scenes": [
                { "duration": 1.0, "children": [
                    { "type": "shape", "shape": "circle", "id": "anchor",
                      "position": "absolute", "x": 0, "y": 0,
                      "style": { "width": "20px", "height": "20px" } }
                ] },
                scene_with_id_and_transform_ref("follower", "anchor")
            ]
        })
        .to_string();

        let loaded = load(ValidationSource::Inline(&json)).expect("scenario loads");
        let report = run_checks(&loaded, false);
        assert!(
            report
                .schema_errors
                .iter()
                .any(|e| e.contains("reference_cross_scene")),
            "expected a reference_cross_scene schema error: {:?}",
            report.schema_errors
        );
        assert!(report.is_blocking(false), "must block, lenient or not");
        assert!(
            report.is_blocking(true),
            "a structural reference error is not a geometry violation — --lenient must not \
             downgrade it"
        );
    }

    #[test]
    fn a_reference_to_an_id_in_the_same_scene_validates_clean() {
        let json = serde_json::json!({
            "video": { "width": 320, "height": 240, "fps": 30 },
            "scenes": [{
                "duration": 1.0,
                "children": [
                    { "type": "shape", "shape": "circle", "id": "anchor",
                      "position": "absolute", "x": 0, "y": 0,
                      "style": { "width": "20px", "height": "20px" } },
                    scene_with_id_and_transform_ref("follower", "anchor")["children"][0].clone()
                ]
            }]
        })
        .to_string();

        let loaded = load(ValidationSource::Inline(&json)).expect("scenario loads");
        let report = run_checks(&loaded, false);
        assert!(
            report
                .schema_errors
                .iter()
                .all(|e| !e.contains("reference_cross_scene") && !e.contains("node(")),
            "a same-scene reference must not be flagged: {:?}",
            report.schema_errors
        );
    }

    #[test]
    fn a_reference_to_a_genuinely_unknown_id_is_reported() {
        let json = serde_json::json!({
            "video": { "width": 320, "height": 240, "fps": 30 },
            "scenes": [
                scene_with_id_and_transform_ref("follower", "does_not_exist_anywhere")
            ]
        })
        .to_string();

        let loaded = load(ValidationSource::Inline(&json)).expect("scenario loads");
        let report = run_checks(&loaded, false);
        assert!(
            report
                .schema_errors
                .iter()
                .any(|e| e.contains("unknown id")),
            "expected an unknown-id schema error: {:?}",
            report.schema_errors
        );
    }
}

#[cfg(test)]
mod check_crf_tests {
    use super::check_crf;

    #[test]
    fn no_crf_is_always_fine() {
        assert_eq!(check_crf(None, false).unwrap(), None);
        assert_eq!(check_crf(None, true).unwrap(), None);
    }

    #[test]
    fn crf_without_hardware_acceleration_warns_about_nothing() {
        assert_eq!(check_crf(Some(23), false).unwrap(), None);
    }

    #[test]
    fn crf_with_hardware_acceleration_returns_a_warning_naming_the_value() {
        let warning = check_crf(Some(23), true).unwrap().expect("must warn");
        assert!(
            warning.contains("23"),
            "warning should name the ignored value: {warning}"
        );
        assert!(
            warning.contains("--hardware-acceleration"),
            "warning should name the flag responsible: {warning}"
        );
    }

    #[test]
    fn out_of_range_crf_still_errors_even_with_hardware_acceleration() {
        assert!(check_crf(Some(52), true).is_err());
        assert!(check_crf(Some(52), false).is_err());
    }
}
