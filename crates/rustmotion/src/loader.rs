use crate::error::{Result, RustmotionError};
use crate::schema::{ResolvedScenario, Scenario};
use crate::{expand, include, variables};
use std::path::PathBuf;

pub fn resolve_includes_and_synthesize_audio(
    scenario: Scenario,
    source: &include::IncludeSource,
) -> Result<ResolvedScenario> {
    let scenario_bpm = scenario.bpm;
    let scenario_beat_offset = scenario.beat_offset;
    let synth_config = scenario.audio.config().cloned();

    let mut resolved = include::resolve_includes(scenario, source)?;

    if let Some(cfg) = synth_config {
        crate::encode::audio::synthesize_score_into_track(
            &mut resolved,
            &cfg,
            scenario_bpm,
            scenario_beat_offset,
        )?;
    }

    Ok(resolved)
}

pub fn load_scenario(input: &PathBuf) -> Result<ResolvedScenario> {
    load_scenario_with_vars(input, None)
}

pub fn load_scenario_with_vars(
    input: &PathBuf,
    overrides: Option<&std::collections::HashMap<String, serde_json::Value>>,
) -> Result<ResolvedScenario> {
    let json_str = std::fs::read_to_string(input).map_err(|e| RustmotionError::FileRead {
        path: input.display().to_string(),
        source: e,
    })?;
    let mut json_value: serde_json::Value =
        serde_json::from_str(&json_str).map_err(RustmotionError::from)?;

    let label = input.display().to_string();
    variables::apply_variables(&mut json_value, overrides, &label)?;
    expand::expand_directives(&mut json_value, &label)?;
    fold_static_expressions(&mut json_value, &label)?;
    if let Some(dir) = input.parent() {
        {
            crate::assets::rebase_relative_paths(&mut json_value, dir);
        }
    }

    let scenario: Scenario = serde_json::from_value(json_value).map_err(RustmotionError::from)?;
    resolve_includes_and_synthesize_audio(scenario, &include::IncludeSource::File(input.clone()))
}

pub(crate) fn fold_static_expressions(value: &mut serde_json::Value, label: &str) -> Result<()> {
    let scope = LoadScope::from_document(value);
    let unfoldable_vars = collect_unfoldable_var_names(value, &scope);
    fold_value(value, &scope, &unfoldable_vars, label)
}

struct LoadScope {
    width: Option<f64>,
    height: Option<f64>,
    fps: Option<f64>,
    constant_scenario_vars: std::collections::HashMap<String, f64>,
}

impl LoadScope {
    fn from_document(value: &serde_json::Value) -> Self {
        let video = value.get("video");
        let constant_scenario_vars = value
            .get("vars")
            .and_then(|v| serde_json::from_value::<rustmotion_core::vars::VarSet>(v.clone()).ok())
            .map(|vars| {
                vars.iter()
                    .filter(|(_, def)| def.is_static())
                    .map(|(name, def)| (name.clone(), def.default))
                    .collect()
            })
            .unwrap_or_default();
        LoadScope {
            width: video.and_then(|v| v.get("width")).and_then(|v| v.as_f64()),
            height: video.and_then(|v| v.get("height")).and_then(|v| v.as_f64()),
            fps: video.and_then(|v| v.get("fps")).and_then(|v| v.as_f64()),
            constant_scenario_vars,
        }
    }
}

impl rustmotion_core::expr::Scope for LoadScope {
    fn var(&self, name: &str) -> Option<f64> {
        match name {
            "W" => self.width,
            "H" => self.height,
            "fps" => self.fps,
            _ => self.constant_scenario_vars.get(name).copied(),
        }
    }
}

fn collect_unfoldable_var_names(
    value: &serde_json::Value,
    scope: &LoadScope,
) -> std::collections::HashSet<String> {
    let mut names = collect_declared_var_names(value);
    for resolvable in scope.constant_scenario_vars.keys() {
        names.remove(resolvable);
    }
    names
}

fn collect_declared_var_names(value: &serde_json::Value) -> std::collections::HashSet<String> {
    let mut names = std::collections::HashSet::new();
    collect_declared_var_names_into(value, &mut names);
    names
}

fn collect_declared_var_names_into(
    value: &serde_json::Value,
    names: &mut std::collections::HashSet<String>,
) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(vars) = map.get("vars") {
                if let Ok(set) =
                    serde_json::from_value::<rustmotion_core::vars::VarSet>(vars.clone())
                {
                    names.extend(set.keys().cloned());
                }
            }
            for (key, v) in map {
                if key == "config" {
                    continue;
                }
                collect_declared_var_names_into(v, names);
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                collect_declared_var_names_into(item, names);
            }
        }
        _ => {}
    }
}

fn fold_value(
    value: &mut serde_json::Value,
    scope: &LoadScope,
    unfoldable_vars: &std::collections::HashSet<String>,
    label: &str,
) -> Result<()> {
    match value {
        serde_json::Value::String(s) => {
            if let Some(src) = s.strip_prefix('=') {
                let expr = rustmotion_core::expr::Expr::parse(src).map_err(|e| {
                    RustmotionError::Generic(format!("expression error in '{label}': {e}"))
                })?;
                let reads_an_unfoldable_var = expr
                    .free_vars()
                    .iter()
                    .any(|v| unfoldable_vars.contains(v.as_str()));
                if expr.is_static() && !reads_an_unfoldable_var {
                    let n = expr.eval(scope).map_err(|e| {
                        RustmotionError::Generic(format!("expression error in '{label}': {e}"))
                    })?;
                    *value = expr_result_to_json(n, s, label)?;
                }
            }
        }
        serde_json::Value::Object(map) => {
            for (key, v) in map.iter_mut() {
                if key == "config" {
                    continue;
                }
                fold_value(v, scope, unfoldable_vars, label)?;
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr.iter_mut() {
                fold_value(item, scope, unfoldable_vars, label)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn expr_result_to_json(n: f64, src: &str, label: &str) -> Result<serde_json::Value> {
    serde_json::Number::from_f64(n)
        .map(serde_json::Value::Number)
        .ok_or_else(|| {
            RustmotionError::Generic(format!(
                "expression `{src}` in '{label}' evaluated to {n}, which has no JSON \
                 representation (NaN/Infinity) — check for a division by zero or an \
                 out-of-domain call"
            ))
        })
}

pub fn source_uses_expression(raw_source: &str) -> bool {
    raw_source.contains("\"=")
}

pub fn load_scenario_from_source(
    input: Option<&PathBuf>,
    json: Option<&str>,
) -> Result<ResolvedScenario> {
    load_scenario_from_source_with_vars(input, json, None)
}

pub fn load_scenario_from_source_with_vars(
    input: Option<&PathBuf>,
    json: Option<&str>,
    overrides: Option<&std::collections::HashMap<String, serde_json::Value>>,
) -> Result<ResolvedScenario> {
    match (input, json) {
        (Some(_), Some(_)) => Err(RustmotionError::ConflictingInput),
        (Some(path), None) => load_scenario_with_vars(path, overrides),
        (None, Some(json_str)) => {
            let mut json_value: serde_json::Value =
                serde_json::from_str(json_str).map_err(RustmotionError::from)?;
            variables::apply_variables(&mut json_value, overrides, "<inline>")?;
            expand::expand_directives(&mut json_value, "<inline>")?;
            fold_static_expressions(&mut json_value, "<inline>")?;
            let scenario: Scenario =
                serde_json::from_value(json_value).map_err(RustmotionError::from)?;
            resolve_includes_and_synthesize_audio(scenario, &include::IncludeSource::Inline)
        }
        (None, None) => Err(RustmotionError::MissingInput),
    }
}

pub fn load_scenario_from_html(input: &PathBuf) -> Result<ResolvedScenario> {
    load_scenario_from_html_with_vars(input, None)
}

pub fn load_scenario_from_html_with_vars(
    input: &PathBuf,
    overrides: Option<&std::collections::HashMap<String, serde_json::Value>>,
) -> Result<ResolvedScenario> {
    let html = std::fs::read_to_string(input).map_err(|e| RustmotionError::FileRead {
        path: input.display().to_string(),
        source: e,
    })?;
    let mut value = rustmotion_html::html_to_scenario_value(&html)
        .map_err(|e| RustmotionError::HtmlParse(e.to_string()))?;
    let annotations = load_html_annotations_sidecar(input)?;
    if !annotations.is_empty() {
        if let Some(obj) = value.as_object_mut() {
            let arr = obj
                .entry("annotations")
                .or_insert_with(|| serde_json::Value::Array(vec![]));
            if let serde_json::Value::Array(a) = arr {
                a.extend(annotations);
            }
        }
    }
    let label = input.display().to_string();
    variables::apply_variables(&mut value, overrides, &label)?;
    expand::expand_directives(&mut value, &label)?;
    fold_static_expressions(&mut value, &label)?;
    if let Some(dir) = input.parent() {
        crate::assets::rebase_relative_paths(&mut value, dir);
    }
    let scenario: Scenario = serde_json::from_value(value).map_err(RustmotionError::from)?;
    resolve_includes_and_synthesize_audio(scenario, &include::IncludeSource::File(input.clone()))
}

pub fn load_html_annotations_sidecar(input: &std::path::Path) -> Result<Vec<serde_json::Value>> {
    let sidecar = input.with_extension("annotations.json");
    let text = match std::fs::read_to_string(&sidecar) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => {
            return Err(RustmotionError::FileRead {
                path: sidecar.display().to_string(),
                source: e,
            })
        }
    };
    let doc: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
        RustmotionError::from(format!("annotations sidecar {}: {e}", sidecar.display()))
    })?;
    doc.get("annotations")
        .and_then(|a| a.as_array())
        .cloned()
        .ok_or_else(|| {
            RustmotionError::from(format!(
                "annotations sidecar {}: missing \"annotations\" array",
                sidecar.display()
            ))
        })
}

pub fn load_input(input: &PathBuf) -> Result<ResolvedScenario> {
    load_input_with_vars(input, None)
}

pub fn load_input_with_vars(
    input: &PathBuf,
    overrides: Option<&std::collections::HashMap<String, serde_json::Value>>,
) -> Result<ResolvedScenario> {
    match input.extension().and_then(|e| e.to_str()) {
        Some("html") | Some("htm") => load_scenario_from_html_with_vars(input, overrides),
        _ => load_scenario_with_vars(input, overrides),
    }
}

pub fn html_to_scenario_json(html: &str) -> Result<serde_json::Value> {
    rustmotion_html::html_to_scenario_value(html)
        .map_err(|e| RustmotionError::HtmlParse(e.to_string()))
}

pub fn is_html_path(path: &std::path::Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("html") | Some("htm")
    )
}

pub fn set_html_inline_style(html: &str, pointer: &str, prop: &str, value: &str) -> Option<String> {
    rustmotion_html::set_inline_style(html, pointer, prop, value)
}

pub fn set_html_text_content(html: &str, pointer: &str, text: &str) -> Option<String> {
    rustmotion_html::set_text_content(html, pointer, text)
}

pub fn set_html_attribute(html: &str, pointer: &str, name: &str, value: &str) -> Option<String> {
    rustmotion_html::set_attribute(html, pointer, name, value)
}

pub fn remove_html_inline_style(html: &str, pointer: &str, prop: &str) -> Option<String> {
    rustmotion_html::remove_inline_style(html, pointer, prop)
}

#[cfg(test)]
mod html_tests {
    use super::*;
    use std::io::Write;

    const HTML: &str = r##"<rustmotion width="1920" height="1080" fps="30" background="#0f172a">
            <scene duration="4"><h1 style="font-size:96; color:#ffffff">Hi</h1></scene>
        </rustmotion>"##;

    fn write_html(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("{name}_{}.html", std::process::id()));
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(HTML.as_bytes()).unwrap();
        path
    }

    #[test]
    fn loads_html_scenario_into_resolved() {
        let path = write_html("rm_html_loader_test");
        let resolved = load_input(&path).expect("html loads");
        assert_eq!(resolved.video.width, 1920);
        assert_eq!(resolved.views.len(), 1);
        assert_eq!(resolved.views[0].scenes.len(), 1);
        assert_eq!(resolved.views[0].scenes[0].duration, 4.0);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn html_load_merges_annotations_sidecar() {
        let path = write_html("rm_html_sidecar_merge");
        let sidecar = path.with_extension("annotations.json");
        std::fs::write(
            &sidecar,
            r##"{ "annotations": [ { "id": "an_1", "note": "smaller", "status": "open",
                 "frame": 3, "target": { "pointer": "/scenes/0/children/0", "kind": "text" } } ] }"##,
        )
        .unwrap();

        let annotations = load_html_annotations_sidecar(&path).expect("sidecar loads");
        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0]["id"], "an_1");
        load_input(&path).expect("html with sidecar loads");

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&sidecar);
    }

    #[test]
    fn corrupt_annotations_sidecar_fails_the_load() {
        let path = write_html("rm_html_sidecar_corrupt");
        let sidecar = path.with_extension("annotations.json");
        std::fs::write(&sidecar, "{ not json").unwrap();

        assert!(load_html_annotations_sidecar(&path).is_err());
        let err = load_input(&path).expect_err("corrupt sidecar must fail the load");
        assert!(
            err.to_string().contains("annotations sidecar"),
            "error should name the sidecar, got: {err}"
        );

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&sidecar);
    }

    #[test]
    fn missing_annotations_sidecar_is_empty() {
        let path = write_html("rm_html_sidecar_missing");
        assert!(load_html_annotations_sidecar(&path).unwrap().is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn html_var_in_text_substituted_via_load_input_with_vars() {
        let html = r##"<rustmotion width="320" height="240" fps="30">
            <scene duration="1"><h1 style="font-size:24; color:#ffffff">Hello $greeting</h1></scene>
        </rustmotion>"##;
        let path = {
            let p =
                std::env::temp_dir().join(format!("rm_html_var_subst_{}.html", std::process::id()));
            let mut f = std::fs::File::create(&p).unwrap();
            std::io::Write::write_all(&mut f, html.as_bytes()).unwrap();
            p
        };

        let mut overrides = std::collections::HashMap::new();
        overrides.insert("greeting".to_string(), serde_json::json!("World"));

        let resolved =
            load_input_with_vars(&path, Some(&overrides)).expect("html var substitution");
        let content = &resolved.views[0].scenes[0].children[0];
        let _ = content;
        assert_eq!(resolved.video.width, 320);
        let _ = std::fs::remove_file(&path);
    }
}

#[cfg(test)]
mod vars_tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn json_override_number_preserves_type() {
        let json_str = serde_json::json!({
            "config": {
                "width_px": { "type": "number", "default": 100 }
            },
            "video": { "width": "$width_px", "height": 100, "fps": 30 },
            "scenes": [{ "duration": 0.1, "children": [] }]
        })
        .to_string();

        let path = {
            let p = std::env::temp_dir().join(format!("rm_var_num_{}.json", std::process::id()));
            let mut f = std::fs::File::create(&p).unwrap();
            f.write_all(json_str.as_bytes()).unwrap();
            p
        };

        let mut overrides = std::collections::HashMap::new();
        overrides.insert("width_px".to_string(), serde_json::json!(200));

        let resolved = load_input_with_vars(&path, Some(&overrides)).expect("loads with override");
        assert_eq!(
            resolved.video.width, 200,
            "override number must be applied as number"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn unknown_override_with_config_errors_actionably() {
        let json_str = serde_json::json!({
            "config": {
                "color": { "type": "string", "default": "#000" }
            },
            "video": { "width": 100, "height": 100, "fps": 30 },
            "scenes": [{ "duration": 0.1, "children": [] }]
        })
        .to_string();

        let path = {
            let p =
                std::env::temp_dir().join(format!("rm_var_unknown_{}.json", std::process::id()));
            let mut f = std::fs::File::create(&p).unwrap();
            f.write_all(json_str.as_bytes()).unwrap();
            p
        };

        let mut overrides = std::collections::HashMap::new();
        overrides.insert("not_declared".to_string(), serde_json::json!("x"));

        let err =
            load_input_with_vars(&path, Some(&overrides)).expect_err("unknown var must error");
        assert!(
            err.to_string().contains("not_declared"),
            "error must name the unknown variable, got: {err}"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn override_wins_over_default() {
        let json_str = serde_json::json!({
            "config": {
                "title": { "type": "string", "default": "Default Title" }
            },
            "video": { "width": 100, "height": 100, "fps": 30 },
            "scenes": [{ "duration": 0.1, "children": [
                { "type": "text", "content": "$title" }
            ]}]
        })
        .to_string();

        let path = {
            let p = std::env::temp_dir().join(format!("rm_var_prec_{}.json", std::process::id()));
            let mut f = std::fs::File::create(&p).unwrap();
            f.write_all(json_str.as_bytes()).unwrap();
            p
        };

        let mut overrides = std::collections::HashMap::new();
        overrides.insert("title".to_string(), serde_json::json!("Override Title"));

        let resolved = load_input_with_vars(&path, Some(&overrides)).expect("loads");
        assert_eq!(resolved.views[0].scenes[0].duration, 0.1);
        let _ = std::fs::remove_file(&path);
    }
}

#[cfg(test)]
mod expr_fold_tests {
    use super::*;

    fn load(json: &serde_json::Value) -> ResolvedScenario {
        load_scenario_from_source(None, Some(&json.to_string())).expect("scenario loads")
    }

    #[test]
    fn for_each_over_eight_items_folds_to_literals_matching_real_cosines() {
        let items: Vec<serde_json::Value> = (0..8).map(|_| serde_json::json!({})).collect();
        let json = serde_json::json!({
            "video": { "width": 1080, "height": 1920, "fps": 30 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "for-each": items,
                    "template": {
                        "type": "text",
                        "content": "badge",
                        "x": "= $W/2 + cos($i / $count * TAU - PI/2) * 700"
                    }
                }]
            }]
        });
        let resolved = load(&json);
        let children = &resolved.views[0].scenes[0].children;
        assert_eq!(children.len(), 8);
        for (i, child) in children.iter().enumerate() {
            let got = child["x"].as_f64().unwrap_or_else(|| {
                panic!("child {i}'s x did not fold to a number: {:?}", child["x"])
            });
            let want = 1080.0 / 2.0
                + (i as f64 / 8.0 * std::f64::consts::TAU - std::f64::consts::PI / 2.0).cos()
                    * 700.0;
            assert!(
                (got - want).abs() < 1e-9,
                "badge {i}: folded x = {got}, expected {want}"
            );
        }
    }

    #[test]
    fn plain_expression_folds_w_h_fps_from_video_block() {
        let json = serde_json::json!({
            "video": { "width": 800, "height": 600, "fps": 24 },
            "scenes": [{
                "duration": 1.0,
                "children": [
                    { "type": "text", "content": "c", "x": "= $W/2", "y": "= $H/2", "opacity": "= $fps / 24" }
                ]
            }]
        });
        let resolved = load(&json);
        let child = &resolved.views[0].scenes[0].children[0];
        assert_eq!(child["x"], serde_json::json!(400.0));
        assert_eq!(child["y"], serde_json::json!(300.0));
        assert_eq!(child["opacity"], serde_json::json!(1.0));
    }

    #[test]
    fn hostile_expression_is_rejected_at_load_not_hung() {
        let hostile = format!("={}1{}", "(".repeat(500), ")".repeat(500));
        let json = serde_json::json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{
                "duration": 1.0,
                "children": [
                    { "type": "text", "content": "c", "x": hostile }
                ]
            }]
        });
        let err = load_scenario_from_source(None, Some(&json.to_string()))
            .expect_err("a hostile expression must fail to load");
        assert!(
            err.to_string().contains("nests deeper"),
            "error should name the nesting problem, got: {err}"
        );
    }

    #[test]
    fn expression_naming_scene_time_is_left_unfolded() {
        let json = serde_json::json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{
                "duration": 1.0,
                "children": [
                    { "type": "text", "content": "c", "x": "= $t * 10" }
                ]
            }]
        });
        let resolved = load(&json);
        let x = &resolved.views[0].scenes[0].children[0]["x"];
        assert_eq!(x, &serde_json::json!("= $t * 10"));
    }

    #[test]
    fn unresolvable_identifier_is_a_named_load_error() {
        let json = serde_json::json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{
                "duration": 1.0,
                "children": [
                    { "type": "text", "content": "c", "x": "= $totallyUndeclared + 1" }
                ]
            }]
        });
        let err = load_scenario_from_source(None, Some(&json.to_string()))
            .expect_err("an unresolvable identifier must fail to load");
        assert!(
            err.to_string().contains("totallyUndeclared"),
            "error should name the identifier, got: {err}"
        );
    }

    #[test]
    fn rand_folds_deterministically_across_separate_loads() {
        let json = serde_json::json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{
                "duration": 1.0,
                "children": [
                    { "type": "text", "content": "c", "x": "= rand(42)" }
                ]
            }]
        });
        let a = load(&json).views[0].scenes[0].children[0]["x"].clone();
        let b = load(&json).views[0].scenes[0].children[0]["x"].clone();
        assert_eq!(a, b);
        assert!(a.is_f64());
    }

    #[test]
    fn source_uses_expression_detects_dollar_free_expressions() {
        assert!(source_uses_expression(r#"{"x": "= cos(PI/4) * 100"}"#));
        assert!(source_uses_expression(r#"{"x": "= $W/2"}"#));
        assert!(!source_uses_expression(r#"{"x": "plain literal"}"#));
    }

    #[test]
    fn scenario_level_constant_var_folds_like_w_h_fps() {
        let json = serde_json::json!({
            "video": { "width": 100, "height": 100 },
            "vars": { "badgeCount": { "default": 8 } },
            "scenes": [{
                "duration": 1.0,
                "children": [
                    { "type": "text", "content": "c", "opacity": "= $badgeCount / 8" }
                ]
            }]
        });
        let resolved = load(&json);
        let child = &resolved.views[0].scenes[0].children[0];
        assert_eq!(child["opacity"], serde_json::json!(1.0));
    }

    #[test]
    fn scenario_level_animated_var_is_left_unfolded_not_treated_as_unknown() {
        let json = serde_json::json!({
            "video": { "width": 100, "height": 100 },
            "vars": {
                "keyDraw": {
                    "default": 0,
                    "animation": [{ "at": "1s", "to": 1, "duration": "1s" }]
                }
            },
            "scenes": [{
                "duration": 2.0,
                "children": [
                    { "type": "text", "content": "c", "opacity": "= $keyDraw" }
                ]
            }]
        });
        let resolved = load(&json);
        let opacity = &resolved.views[0].scenes[0].children[0]["opacity"];
        assert_eq!(opacity, &serde_json::json!("= $keyDraw"));
    }

    #[test]
    fn scene_level_var_is_left_unfolded_even_when_constant() {
        let json = serde_json::json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{
                "duration": 1.0,
                "vars": { "localOnly": { "default": 42 } },
                "children": [
                    { "type": "text", "content": "c", "opacity": "= $localOnly" }
                ]
            }]
        });
        let resolved = load(&json);
        let opacity = &resolved.views[0].scenes[0].children[0]["opacity"];
        assert_eq!(opacity, &serde_json::json!("= $localOnly"));
    }

    #[test]
    fn unresolvable_identifier_still_errors_alongside_declared_vars() {
        let json = serde_json::json!({
            "video": { "width": 100, "height": 100 },
            "vars": { "keyDraw": { "default": 0 } },
            "scenes": [{
                "duration": 1.0,
                "children": [
                    { "type": "text", "content": "c", "x": "= $totallyUndeclared + 1" }
                ]
            }]
        });
        let err = load_scenario_from_source(None, Some(&json.to_string()))
            .expect_err("an unresolvable identifier must still fail to load");
        assert!(
            err.to_string().contains("totallyUndeclared"),
            "error should name the identifier, got: {err}"
        );
    }

    #[test]
    fn scenario_and_scene_vars_survive_the_real_loader_into_resolved_scene_fields() {
        let json = serde_json::json!({
            "video": { "width": 100, "height": 100 },
            "vars": { "keyDraw": { "default": 0,
                "animation": [{ "at": "1s", "to": 1, "duration": "1s" }] } },
            "scenes": [{
                "duration": 2.0,
                "vars": { "localOnly": { "default": 42 } },
                "children": []
            }]
        });
        let resolved = load(&json);
        let scene = &resolved.views[0].scenes[0];
        assert_eq!(scene.vars.len(), 1);
        assert!(scene.vars.contains_key("localOnly"));
        assert_eq!(scene.resolved_scenario_vars.len(), 1);
        assert!(scene.resolved_scenario_vars.contains_key("keyDraw"));
    }
}
