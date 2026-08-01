use rustmotion::error::{Result, RustmotionError};
use std::path::{Path, PathBuf};

use super::geometry::{GeometryViolation, ViolationKind};
use super::validation::{self, ValidationReport, ValidationSource, VarOverrides};

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
        report_out.promote_attr_warnings();
    }

    if let Some(report_path) = report {
        write_report(report_path, &report_out)?;
        eprintln!("Wrote report: {}", report_path.display());
    }

    let mut applied_fixes = 0usize;
    if fix && !report_out.geom_violations.is_empty() {
        let mut json_value = loaded.raw.clone();
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

            // Re-run checks after the fixes so the rest of the function reflects
            // the on-disk state.
            let reloaded = validation::load_with_vars(ValidationSource::File(input), overrides)?;
            report_out = validation::run_checks(&reloaded, strict_anim);
            if strict_attrs {
                report_out.promote_attr_warnings();
            }
        }
    }

    let all_scenes: Vec<_> = loaded.scenario.all_scenes().collect();
    let total_duration: f64 = all_scenes.iter().map(|s| s.duration).sum();

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

fn write_report(path: &Path, report: &ValidationReport) -> Result<()> {
    let json = serde_json::json!({
        "schema_errors": report.schema_errors,
        "geometry_violations": report.geom_violations,
        "unresolved_vars": report.unresolved_vars,
        "warnings": report.warnings,
        "attr_warnings": report.attr_warnings,
    });
    let pretty = serde_json::to_string_pretty(&json)
        .map_err(|e| RustmotionError::Generic(format!("serialize report: {}", e)))?;
    std::fs::write(path, pretty).map_err(|e| RustmotionError::FileRead {
        path: path.display().to_string(),
        source: e,
    })?;
    Ok(())
}

/// Apply safe auto-fixes directly in the raw JSON. Returns the number of
/// successful mutations.
fn apply_fixes(root: &mut serde_json::Value, violations: &[GeometryViolation]) -> usize {
    let mut applied = 0;
    for v in violations {
        let target = match navigate(root, &v.path) {
            Some(t) => t,
            None => continue,
        };
        match v.kind {
            ViolationKind::UnwrappableTextOverflow => {
                // `wrap` is not a `CssStyle` field — `CssStyle` is
                // `deny_unknown_fields`, so writing it silently drops the
                // whole component at the next parse (C1). The real property
                // is `white-space`; removing `nowrap`/`pre` falls back to
                // the schema default (`normal`, i.e. wrapping), which is
                // always a valid, non-destructive mutation.
                if let Some(style_obj) = target.get_mut("style").and_then(|s| s.as_object_mut()) {
                    if style_obj.remove("white-space").is_some() {
                        applied += 1;
                    }
                }
            }
            ViolationKind::AutoScrollDisabledOverflow => {
                if let Some(obj) = target.as_object_mut() {
                    obj.insert("auto_scroll".into(), serde_json::Value::Bool(true));
                    applied += 1;
                }
            }
            ViolationKind::ViewportOverflow
            | ViolationKind::AnimatedTextOverflow
            | ViolationKind::ContentOverflowsBox => {
                // Position/size clamping is too risky to auto-fix without
                // losing intent — leave it for the user. (ContentOverflowsBox
                // specifically: growing the box, shrinking the font, or
                // shortening the copy are all legitimate fixes with very
                // different visual outcomes — not ours to pick.)
            }
        }
    }
    applied
}

/// Walk a path like `views[0].scenes[1].children[2].children[0]` against the
/// raw JSON, transparently handling the legacy `scenes` and `composition` shapes.
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
                    // Implicit single-slide view: stay on root.
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

    /// C1: `apply_fixes` must never write `style.wrap` (not a `CssStyle`
    /// field — writing it drops the whole component at the next parse
    /// because `CssStyle` is `deny_unknown_fields`). It must instead remove
    /// `white-space: nowrap`, and the fixed file must still parse with the
    /// text component intact.
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

        // The fixed file must still parse, and the text must still be
        // present — a `deny_unknown_fields` rejection would have silently
        // dropped it (C1's original failure mode).
        let pretty = serde_json::to_string(&json).unwrap();
        let scenario =
            load_scenario_from_source(None, Some(&pretty)).expect("fixed scenario still parses");
        let top_children = render::deserialize_children(&scenario.views[0].scenes[0]);
        assert_eq!(top_children.len(), 1, "card must survive the fix");
        let text_survived = match &top_children[0].component {
            Component::Card(c) => c.children.len() == 1,
            _ => false,
        };
        assert!(
            text_survived,
            "text child must survive the fix, not be dropped"
        );

        // The fix must also clear the geometry violation it targeted.
        let after = validate_geometry(&scenario);
        assert!(
            after
                .iter()
                .all(|v| v.kind != ViolationKind::UnwrappableTextOverflow),
            "fix must clear the violation: {:?}",
            after
        );
    }

    /// H3: the violation path (produced by geometry.rs's raw-index-preserving
    /// walker) must reference the RAW JSON position of the offending node,
    /// so `navigate()`/`apply_fixes` mutate the right sibling even when an
    /// earlier child never became a `ChildComponent`.
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

        // children[0] (the broken sibling) must be untouched.
        assert_eq!(
            json["scenes"][0]["children"][0]["type"],
            "not_a_real_component_kind"
        );
        // children[1] (the card) is the one that got fixed.
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
}
