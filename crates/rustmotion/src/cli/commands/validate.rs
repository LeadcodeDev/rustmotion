use rustmotion::error::{Result, RustmotionError};
use rustmotion::schema::ResolvedScenario;
use std::path::{Path, PathBuf};

use super::audio_report::analyze_scenario_audio_levels;
use super::geometry::{GeometryViolation, ViolationKind};
use super::validation::{self, ValidationReport, ValidationSource, VarOverrides};

fn announced_duration(scenario: &ResolvedScenario) -> f64 {
    let fps = scenario.video.fps as f64;
    if fps <= 0.0 {
        return 0.0;
    }
    rustmotion::encode::build_frame_tasks(scenario).len() as f64 / fps
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum FixRefusal {
    HtmlSource,
    Templated,
    UsesInclude,
    UsesTemplateDirectives,
    UsesExpression,
}

impl FixRefusal {
    pub(crate) fn explain(&self, path: &Path) -> String {
        let p = path.display();
        match self {
            Self::HtmlSource => format!(
                "--fix cannot rewrite {p}: it is an HTML source, and the fixer only knows how to \
                 emit JSON — applying it would replace your markup with the transpiled scenario. \
                 Apply the fix to the HTML by hand, or transpile first and fix the JSON."
            ),
            Self::Templated => format!(
                "--fix cannot rewrite {p}: it declares `config` or uses `$variables`, and the \
                 fixer would write back the substituted scenario — dropping the template and \
                 making `--var` a silent no-op. Fix the template by hand."
            ),
            Self::UsesInclude => format!(
                "--fix cannot rewrite {p}: it uses `include`, and the fixer would write back the \
                 resolved tree — inlining the included files into the parent and patching by a \
                 path that no longer means the same node. Fix the included file directly."
            ),
            Self::UsesTemplateDirectives => format!(
                "--fix cannot rewrite {p}: it uses `for-each`/`use` (or declares `components`), \
                 and the fixer would write back the expanded tree — inlining every repeated \
                 instance and patching by a path that no longer means the same source node, \
                 exactly like `include`. Fix the `components` definition or the `for-each` \
                 template directly."
            ),
            Self::UsesExpression => format!(
                "--fix cannot rewrite {p}: it uses an `= ...` expression (see \
                 `rustmotion_core::expr`), and the fixer would write back the *evaluated* tree — \
                 a static expression folds to its literal number at load, and writing that \
                 number back would silently replace the formula with the one value it happened \
                 to produce, making the expression unrecoverable. Fix the expression by hand."
            ),
        }
    }

    pub(crate) fn explain_for_migrate(&self, path: &Path) -> String {
        let p = path.display();
        match self {
            Self::HtmlSource => format!(
                "migrate cannot rewrite {p}: it is an HTML source, and the migrator only knows \
                 how to emit JSON — applying it would replace your markup with the transpiled \
                 scenario. Transpile to JSON first, then migrate that."
            ),
            Self::Templated => format!(
                "migrate cannot rewrite {p}: it declares `config` or uses `$variables`, and the \
                 migrator would write back the substituted scenario — dropping the template and \
                 making `--var` a silent no-op. Migrate the template by hand."
            ),
            Self::UsesInclude => format!(
                "migrate cannot rewrite {p}: it uses `include`, and the migrator would write back \
                 the resolved tree — inlining the included files into the parent and patching by \
                 a path that no longer means the same node. Migrate the included file directly."
            ),
            Self::UsesTemplateDirectives => format!(
                "migrate cannot rewrite {p}: it uses `for-each`/`use` (or declares `components`), \
                 and the migrator would write back the expanded tree — inlining every repeated \
                 instance and patching by a path that no longer means the same source node, \
                 exactly like `include`. Migrate the `components` definition or the `for-each` \
                 template directly."
            ),
            Self::UsesExpression => format!(
                "migrate cannot rewrite {p}: it uses an `= ...` expression (see \
                 `rustmotion_core::expr`), and the migrator would write back the *evaluated* tree \
                 — a static expression folds to its literal number at load, and writing that \
                 number back would silently replace the formula with the one value it happened \
                 to produce, making the expression unrecoverable. Migrate the expression by hand."
            ),
        }
    }
}

pub(crate) fn refuse_fix(input: &Path, raw_source: &str) -> Option<FixRefusal> {
    if rustmotion::loader::is_html_path(input) {
        return Some(FixRefusal::HtmlSource);
    }
    let source: serde_json::Value = match serde_json::from_str(raw_source) {
        Ok(v) => v,
        Err(_) => return Some(FixRefusal::Templated),
    };
    if source.get("config").is_some() || raw_source.contains("$") {
        return Some(FixRefusal::Templated);
    }
    if raw_source.contains("\"include\"") {
        return Some(FixRefusal::UsesInclude);
    }
    if source.get("components").is_some()
        || raw_source.contains("\"for-each\"")
        || raw_source.contains("\"use\"")
    {
        return Some(FixRefusal::UsesTemplateDirectives);
    }
    if rustmotion::loader::source_uses_expression(raw_source) {
        return Some(FixRefusal::UsesExpression);
    }
    None
}

pub(crate) fn fixable_source(raw_source: &str) -> Result<serde_json::Value> {
    serde_json::from_str(raw_source).map_err(|e| {
        RustmotionError::Generic(format!("re-parse source for --fix/--migrate: {}", e))
    })
}

pub fn cmd_validate(
    input: &PathBuf,
    report: Option<&Path>,
    fix: bool,
    strict_anim: bool,
    strict_attrs: bool,
    lenient: bool,
    overrides: Option<&VarOverrides>,
) -> Result<()> {
    let loaded = match validation::load_with_vars(ValidationSource::File(input), overrides) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    let mut report_out = validation::run_checks(&loaded, strict_anim);
    if strict_attrs {
        validation::warn_strict_attrs_is_now_default();
        report_out.promote_attr_warnings();
    }

    if let Some(report_path) = report {
        write_report(report_path, &report_out, &loaded.scenario)?;
        eprintln!("Wrote report: {}", report_path.display());
    }

    let mut applied_fixes = 0usize;
    if fix && !report_out.geom_violations.is_empty() {
        let raw_source = std::fs::read_to_string(input).unwrap_or_default();
        if let Some(refusal) = refuse_fix(input, &raw_source) {
            return Err(RustmotionError::Generic(refusal.explain(input)));
        }
        let mut json_value = fixable_source(&raw_source)?;
        applied_fixes = apply_fixes(&mut json_value, &report_out.geom_violations);
        if applied_fixes > 0 {
            let pretty = serde_json::to_string_pretty(&json_value)
                .map_err(|e| RustmotionError::Generic(format!("serialize fixes: {}", e)))?;
            std::fs::write(input, pretty).map_err(|e| RustmotionError::FileRead {
                path: input.display().to_string(),
                source: e,
            })?;
            eprintln!(
                "Applied {} auto-fix(es) to {}",
                applied_fixes,
                input.display()
            );

            let reloaded = validation::load_with_vars(ValidationSource::File(input), overrides)?;
            report_out = validation::run_checks(&reloaded, strict_anim);
            if strict_attrs {
                report_out.promote_attr_warnings();
            }
        }
    }

    let all_scenes: Vec<_> = loaded.scenario.all_scenes().collect();
    let total_duration = announced_duration(&loaded.scenario);

    validation::print_report(&report_out, &input.display().to_string());

    let blocking = !report_out.schema_errors.is_empty()
        || (!report_out.geom_violations.is_empty() && !lenient);
    if blocking {
        if applied_fixes > 0 {
            eprintln!("Some fixes applied — re-run validate to confirm.");
        }
        std::process::exit(1);
    }

    eprintln!(
        "Valid scenario: {} scene(s) in {} view(s)",
        all_scenes.len(),
        loaded.scenario.views.len()
    );
    eprintln!(
        "  Resolution: {}x{} @ {}fps",
        loaded.scenario.video.width, loaded.scenario.video.height, loaded.scenario.video.fps
    );
    eprintln!("  Duration: {:.1}s", total_duration);
    if !report_out.geom_violations.is_empty() {
        eprintln!("  Geometry warnings: {}", report_out.geom_violations.len());
    }
    Ok(())
}

fn write_report(path: &Path, report: &ValidationReport, scenario: &ResolvedScenario) -> Result<()> {
    let audio = analyze_scenario_audio_levels(scenario);
    let json = serde_json::json!({
        "schema_errors": report.schema_errors,
        "geometry_violations": report.geom_violations,
        "unresolved_vars": report.unresolved_vars,
        "warnings": report.warnings,
        "attr_warnings": report.attr_warnings,
        "audio": audio,
    });
    let pretty = serde_json::to_string_pretty(&json)
        .map_err(|e| RustmotionError::Generic(format!("serialize report: {}", e)))?;
    std::fs::write(path, pretty).map_err(|e| RustmotionError::FileRead {
        path: path.display().to_string(),
        source: e,
    })?;
    Ok(())
}

fn apply_fixes(root: &mut serde_json::Value, violations: &[GeometryViolation]) -> usize {
    let mut applied = 0;
    for v in violations {
        let target = match navigate(root, &v.path) {
            Some(t) => t,
            None => continue,
        };
        match v.kind {
            ViolationKind::UnwrappableTextOverflow => {
                if let Some(style_obj) = target.get_mut("style").and_then(|s| s.as_object_mut()) {
                    if style_obj.remove("white-space").is_some() {
                        applied += 1;
                    }
                }
            }
            ViolationKind::ContentOverflowsBox => {
                let kind = target.get("type").and_then(|t| t.as_str());
                if matches!(kind, Some("text") | Some("gradient_text")) {
                    if let Some(style) = target
                        .as_object_mut()
                        .and_then(|o| o.get_mut("style"))
                        .and_then(|s| s.as_object_mut())
                    {
                        if !style.contains_key("text-autofit") {
                            style.insert("text-autofit".into(), serde_json::Value::Bool(true));
                            applied += 1;
                        }
                    }
                }
            }
            ViolationKind::ViewportOverflow
            | ViolationKind::AnimatedTextOverflow
            | ViolationKind::ContentOverflowsCard => {}
        }
    }
    applied
}

fn navigate<'a>(root: &'a mut serde_json::Value, path: &str) -> Option<&'a mut serde_json::Value> {
    let segments = parse_segments(path);
    let mut cursor: &mut serde_json::Value = root;
    let mut idx = 0;
    while idx < segments.len() {
        let (name, n) = &segments[idx];
        cursor = match name.as_str() {
            "views" => {
                if cursor.get("views").is_some() {
                    cursor.get_mut("views")?.get_mut(*n)?
                } else if cursor.get("composition").is_some() {
                    cursor.get_mut("composition")?.get_mut(*n)?
                } else if *n == 0 {
                    cursor
                } else {
                    return None;
                }
            }
            "scenes" => cursor.get_mut("scenes")?.get_mut(*n)?,
            "children" => cursor.get_mut("children")?.get_mut(*n)?,
            _ => return None,
        };
        idx += 1;
    }
    Some(cursor)
}

fn parse_segments(path: &str) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    for part in path.split('.') {
        if let Some(open) = part.find('[') {
            let close = part.find(']').unwrap_or(part.len());
            let name = &part[..open];
            if let Ok(n) = part[open + 1..close].parse::<usize>() {
                out.push((name.to_string(), n));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::geometry::{validate_geometry, Axis, BBox};
    use super::*;
    use rustmotion::components::Component;
    use rustmotion::engine::render;
    use rustmotion::loader::load_scenario_from_source;

    const NARROW_CARD_JSON: &str = r##"{
        "video": { "width": 1920, "height": 1080 },
        "scenes": [{
            "duration": 1.0,
            "children": [{
                "type": "card",
                "x": 100, "y": 100,
                "style": { "width": "200px", "height": "200px", "background": "#222244" },
                "children": [{
                    "type": "text",
                    "content": "this string is too long to fit",
                    "style": { "color": "#ffffff", "font-size": "96px", "white-space": "nowrap" }
                }]
            }]
        }]
    }"##;

    fn unwrappable_violation(path: &str) -> GeometryViolation {
        GeometryViolation {
            view_index: 0,
            scene_index: 0,
            path: path.to_string(),
            component: "text".to_string(),
            axis: Axis::X,
            kind: ViolationKind::UnwrappableTextOverflow,
            bbox: BBox {
                x: 0.0,
                y: 0.0,
                w: 200.0,
                h: 40.0,
            },
            viewport: (1920, 1080),
            hint: String::new(),
        }
    }

    fn overflow_box_violation(path: &str) -> GeometryViolation {
        GeometryViolation {
            kind: ViolationKind::ContentOverflowsBox,
            ..unwrappable_violation(path)
        }
    }

    #[test]
    fn fix_declares_text_autofit_on_an_overflowing_text() {
        let mut json: serde_json::Value = serde_json::from_str(NARROW_CARD_JSON).unwrap();
        let path = "views[0].scenes[0].children[0].children[0]";
        let applied = apply_fixes(&mut json, &[overflow_box_violation(path)]);
        assert_eq!(applied, 1, "expected exactly one fix applied");

        let target = navigate(&mut json, path).expect("path resolves");
        assert_eq!(
            target.get("style").and_then(|s| s.get("text-autofit")),
            Some(&serde_json::Value::Bool(true))
        );
        assert!(
            target.get("content").is_some(),
            "content must be untouched: {target}"
        );
    }

    #[test]
    fn fix_leaves_overflowing_components_that_cannot_autofit_alone() {
        let json_src = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{ "duration": 1.0, "children": [
                { "type": "table", "headers": ["a"], "rows": [["b"]],
                  "style": { "width": "40px", "font-size": 40 } }
            ]}]
        }"##;
        let mut json: serde_json::Value = serde_json::from_str(json_src).unwrap();
        let path = "views[0].scenes[0].children[0]";
        assert_eq!(
            apply_fixes(&mut json, &[overflow_box_violation(path)]),
            0,
            "a table cannot autofit, so nothing should be claimed as fixed"
        );
        let target = navigate(&mut json, path).expect("path resolves");
        assert!(
            target
                .get("style")
                .and_then(|s| s.get("text-autofit"))
                .is_none(),
            "must not write a field this painter ignores: {target}"
        );
    }

    #[test]
    fn fix_removes_white_space_and_never_writes_the_nonexistent_wrap_field() {
        let mut json: serde_json::Value = serde_json::from_str(NARROW_CARD_JSON).unwrap();
        let violations = vec![unwrappable_violation(
            "views[0].scenes[0].children[0].children[0]",
        )];
        let applied = apply_fixes(&mut json, &violations);
        assert_eq!(applied, 1);

        let style = &json["scenes"][0]["children"][0]["children"][0]["style"];
        assert!(
            style.get("wrap").is_none(),
            "must never write the nonexistent CssStyle::wrap field: {}",
            style
        );
        assert!(
            style.get("white-space").is_none(),
            "white-space: nowrap must be removed, not left in place: {}",
            style
        );

        let pretty = serde_json::to_string(&json).unwrap();
        let scenario =
            load_scenario_from_source(None, Some(&pretty)).expect("fixed scenario still parses");
        let top_children = render::deserialize_children(&scenario.views[0].scenes[0]);
        assert_eq!(top_children.len(), 1, "card must survive the fix");
        let text_survived = match &top_children[0].component {
            Component::Container(c) => c.children.len() == 1,
            _ => false,
        };
        assert!(
            text_survived,
            "text child must survive the fix, not be dropped"
        );

        let after = validate_geometry(&scenario);
        assert!(
            after
                .iter()
                .all(|v| v.kind != ViolationKind::UnwrappableTextOverflow),
            "fix must clear the violation: {:?}",
            after
        );
    }

    #[test]
    fn fix_patches_the_raw_json_sibling_even_when_an_earlier_child_failed_to_deserialize() {
        let raw = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [
                    { "type": "not_a_real_component_kind" },
                    {
                        "type": "card",
                        "x": 100, "y": 100,
                        "style": { "width": "200px", "height": "200px", "background": "#222244" },
                        "children": [{
                            "type": "text",
                            "content": "this string is too long to fit",
                            "style": { "color": "#ffffff", "font-size": "96px", "white-space": "nowrap" }
                        }]
                    }
                ]
            }]
        }"##;
        let mut json: serde_json::Value = serde_json::from_str(raw).unwrap();
        let violations = vec![unwrappable_violation(
            "views[0].scenes[0].children[1].children[0]",
        )];
        let applied = apply_fixes(&mut json, &violations);
        assert_eq!(applied, 1);

        assert_eq!(
            json["scenes"][0]["children"][0]["type"],
            "not_a_real_component_kind"
        );
        let style = &json["scenes"][0]["children"][1]["children"][0]["style"];
        assert!(style.get("white-space").is_none());
        assert!(style.get("wrap").is_none());
    }

    #[test]
    fn navigate_resolves_implicit_single_slide_view_at_index_zero() {
        let mut json: serde_json::Value = serde_json::from_str(NARROW_CARD_JSON).unwrap();
        let target = navigate(&mut json, "views[0].scenes[0].children[0].children[0]");
        assert!(target.is_some(), "must resolve into the implicit view 0");
        assert_eq!(target.unwrap()["type"], "text");
    }

    #[test]
    fn announced_duration_subtracts_the_overlapping_transition_instead_of_summing_scene_durations()
    {
        let json = r##"{
            "video": { "width": 640, "height": 360, "fps": 30 },
            "scenes": [
                { "duration": 2.0, "children": [] },
                {
                    "duration": 2.0,
                    "transition": { "type": "fade", "duration": 0.5 },
                    "children": []
                }
            ]
        }"##;
        let scenario = load_scenario_from_source(None, Some(json)).expect("scenario parses");

        let naive_sum: f64 = scenario.all_scenes().map(|s| s.duration).sum();
        assert_eq!(
            naive_sum, 4.0,
            "sanity check: naive summing must reproduce the old 4.0s (over-)estimate"
        );

        let duration = announced_duration(&scenario);
        assert!(
            (duration - 3.5).abs() < 1e-9,
            "expected the transition-overlap-corrected 3.5s, got {duration}"
        );

        let expected_from_frame_count =
            rustmotion::encode::build_frame_tasks(&scenario).len() as f64 / 30.0;
        assert_eq!(duration, expected_from_frame_count);
    }

    #[test]
    fn announced_duration_matches_scene_duration_sum_when_there_are_no_transitions() {
        let json = r##"{
            "video": { "width": 640, "height": 360, "fps": 30 },
            "scenes": [
                { "duration": 1.0, "children": [] },
                { "duration": 2.0, "children": [] }
            ]
        }"##;
        let scenario = load_scenario_from_source(None, Some(json)).expect("scenario parses");
        let duration = announced_duration(&scenario);
        assert!(
            (duration - 3.0).abs() < 1e-6,
            "expected 3.0s with no transitions, got {duration}"
        );
    }

    mod fix_refusals {
        use super::super::{refuse_fix, FixRefusal};
        use std::path::Path;

        const PLAIN: &str = r#"{"video":{"width":320,"height":240,"fps":30},
            "scenes":[{"duration":1.0,"children":[]}]}"#;

        #[test]
        fn a_plain_json_scenario_is_writable() {
            assert_eq!(refuse_fix(Path::new("s.json"), PLAIN), None);
        }

        #[test]
        fn an_html_source_is_refused() {
            assert_eq!(
                refuse_fix(Path::new("s.html"), "<rustmotion></rustmotion>"),
                Some(FixRefusal::HtmlSource)
            );
        }

        #[test]
        fn a_templated_scenario_is_refused() {
            let with_config = r#"{"config":{"title":"hi"},"video":{"width":320,"height":240,
                "fps":30},"scenes":[{"duration":1.0,"children":[]}]}"#;
            assert_eq!(
                refuse_fix(Path::new("s.json"), with_config),
                Some(FixRefusal::Templated)
            );

            let with_var = r#"{"video":{"width":320,"height":240,"fps":30},
                "scenes":[{"duration":1.0,"children":[
                {"type":"text","content":"$title"}]}]}"#;
            assert_eq!(
                refuse_fix(Path::new("s.json"), with_var),
                Some(FixRefusal::Templated)
            );
        }

        #[test]
        fn a_scenario_using_include_is_refused() {
            let with_include = r#"{"video":{"width":320,"height":240,"fps":30},
                "scenes":[{"include":"part.json"}]}"#;
            assert_eq!(
                refuse_fix(Path::new("s.json"), with_include),
                Some(FixRefusal::UsesInclude)
            );
        }

        #[test]
        fn a_scenario_using_for_each_is_refused() {
            let with_for_each = r##"{"video":{"width":320,"height":240,"fps":30},
                "scenes":[{"duration":1.0,"children":[
                {"for-each":[1,2],"template":{"type":"text","content":"static"}}
                ]}]}"##;
            assert_eq!(
                refuse_fix(Path::new("s.json"), with_for_each),
                Some(FixRefusal::UsesTemplateDirectives)
            );
        }

        #[test]
        fn a_scenario_declaring_components_is_refused_even_with_no_use_site_yet() {
            let with_components = r##"{"video":{"width":320,"height":240,"fps":30},
                "components":{"card":{"params":{},"template":{"type":"text","content":"hi"}}},
                "scenes":[{"duration":1.0,"children":[]}]}"##;
            assert_eq!(
                refuse_fix(Path::new("s.json"), with_components),
                Some(FixRefusal::UsesTemplateDirectives)
            );
        }

        #[test]
        fn a_scenario_using_a_dollar_free_static_expression_is_refused() {
            let with_expression = r##"{"video":{"width":320,"height":240,"fps":30},
                "scenes":[{"duration":1.0,"children":[
                {"type":"text","content":"hi","x":"= cos(PI/4) * 100"}
                ]}]}"##;
            assert_eq!(
                refuse_fix(Path::new("s.json"), with_expression),
                Some(FixRefusal::UsesExpression)
            );
        }

        #[test]
        fn a_scenario_using_a_dollar_expression_is_refused_as_templated_not_expression() {
            let with_var_expression = r##"{"video":{"width":320,"height":240,"fps":30},
                "scenes":[{"duration":1.0,"children":[
                {"type":"text","content":"hi","x":"= $W/2"}
                ]}]}"##;
            assert_eq!(
                refuse_fix(Path::new("s.json"), with_var_expression),
                Some(FixRefusal::Templated)
            );
        }

        #[test]
        fn every_refusal_names_the_file_and_says_what_to_do_instead() {
            let p = Path::new("scenes/hero.json");
            for r in [
                FixRefusal::HtmlSource,
                FixRefusal::Templated,
                FixRefusal::UsesInclude,
                FixRefusal::UsesTemplateDirectives,
                FixRefusal::UsesExpression,
            ] {
                let msg = r.explain(p);
                assert!(msg.contains("scenes/hero.json"), "{msg}");
                assert!(msg.contains("by hand") || msg.contains("directly"), "{msg}");
            }
        }
    }

    mod fix_refusals_end_to_end {
        use super::super::cmd_validate;

        #[test]
        fn cmd_validate_fix_refuses_to_overwrite_a_templated_scenario_and_leaves_the_file_untouched(
        ) {
            let path = std::env::temp_dir().join(format!(
                "rm_validate_fix_templated_{}.json",
                std::process::id()
            ));
            let original = r##"{
                "config": { "title": { "type": "string", "default": "hi" } },
                "video": { "width": 1920, "height": 1080 },
                "scenes": [{
                    "duration": 1.0,
                    "children": [{
                        "type": "card",
                        "x": 100, "y": 100,
                        "style": { "width": "200px", "height": "200px", "background": "#222244" },
                        "children": [{
                            "type": "text",
                            "content": "$title but also this string is too long to fit",
                            "style": { "color": "#ffffff", "font-size": "96px", "white-space": "nowrap" }
                        }]
                    }]
                }]
            }"##;
            std::fs::write(&path, original).expect("write fixture");

            let result = cmd_validate(&path, None, true, false, false, false, None);

            let after = std::fs::read_to_string(&path).expect("read back fixture");
            std::fs::remove_file(&path).ok();

            assert!(
                result.is_err(),
                "--fix on a templated scenario with a real violation must be refused, \
                 not silently applied"
            );
            assert_eq!(
                after, original,
                "the file must be byte-identical after a refused --fix — writing \
                 loaded.raw here would have dropped `config` and baked in the \
                 substituted $title"
            );
        }

        #[test]
        fn cmd_validate_fix_refuses_to_overwrite_a_scenario_using_include_and_leaves_files_untouched(
        ) {
            let dir = std::env::temp_dir()
                .join(format!("rm_validate_fix_include_{}", std::process::id()));
            std::fs::create_dir_all(&dir).expect("mkdir");
            let part_path = dir.join("part.json");
            let parent_path = dir.join("parent.json");

            let part = r##"{
                "video": { "width": 1920, "height": 1080 },
                "scenes": [
                    { "duration": 1.0, "children": [] },
                    {
                        "duration": 1.0,
                        "children": [{
                            "type": "card",
                            "x": 100, "y": 100,
                            "style": { "width": "200px", "height": "200px", "background": "#222244" },
                            "children": [{
                                "type": "text",
                                "content": "this string is too long to fit",
                                "style": { "color": "#ffffff", "font-size": "96px", "white-space": "nowrap" }
                            }]
                        }]
                    }
                ]
            }"##;
            let parent = r##"{
                "video": { "width": 1920, "height": 1080 },
                "scenes": [{ "include": "part.json" }]
            }"##;
            std::fs::write(&part_path, part).expect("write part fixture");
            std::fs::write(&parent_path, parent).expect("write parent fixture");

            let result = cmd_validate(&parent_path, None, true, false, false, false, None);

            let parent_after = std::fs::read_to_string(&parent_path).expect("read back parent");
            let part_after = std::fs::read_to_string(&part_path).expect("read back part");
            std::fs::remove_dir_all(&dir).ok();

            assert!(
                result.is_err(),
                "--fix on an include-using scenario with a real violation must be refused"
            );
            assert_eq!(
                parent_after, parent,
                "parent file must be byte-identical after a refused --fix"
            );
            assert_eq!(part_after, part, "included file must be untouched too");
        }

        #[test]
        fn cmd_validate_fix_refuses_to_overwrite_a_scenario_using_for_each_and_leaves_the_file_untouched(
        ) {
            let path = std::env::temp_dir().join(format!(
                "rm_validate_fix_for_each_{}.json",
                std::process::id()
            ));
            let original = r##"{
                "video": { "width": 1920, "height": 1080 },
                "scenes": [{
                    "duration": 1.0,
                    "children": [{
                        "for-each": [
                            { "label": "short" },
                            { "label": "this string is too long to fit in its card" }
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
            }"##;
            std::fs::write(&path, original).expect("write fixture");

            let result = cmd_validate(&path, None, true, false, false, false, None);

            let after = std::fs::read_to_string(&path).expect("read back fixture");
            std::fs::remove_file(&path).ok();

            assert!(
                result.is_err(),
                "--fix on a for-each-using scenario with a real violation must be refused"
            );
            assert_eq!(
                after, original,
                "the file must be byte-identical after a refused --fix — the two `for-each` \
                 iterations expand into two card siblings, so a path-based patch would not even \
                 land on the right one"
            );
        }
    }
}
