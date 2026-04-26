use rustmotion::error::{Result, RustmotionError};
use std::path::{Path, PathBuf};

use super::geometry::{GeometryViolation, ViolationKind};
use super::validation::{self, ValidationReport, ValidationSource};

pub fn cmd_validate(
    input: &PathBuf,
    report: Option<&Path>,
    fix: bool,
    strict_anim: bool,
    lenient: bool,
) -> Result<()> {
    let loaded = match validation::load(ValidationSource::File(input)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    let mut report_out = validation::run_checks(&loaded, strict_anim);

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
            let reloaded = validation::load(ValidationSource::File(input))?;
            report_out = validation::run_checks(&reloaded, strict_anim);
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
                if let Some(obj) = target.as_object_mut() {
                    let style = obj
                        .entry("style")
                        .or_insert_with(|| serde_json::json!({}));
                    if let Some(style_obj) = style.as_object_mut() {
                        style_obj.insert("wrap".into(), serde_json::Value::Bool(true));
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
            ViolationKind::ViewportOverflow | ViolationKind::AnimatedTextOverflow => {
                // Position/size clamping is too risky to auto-fix without
                // losing intent — leave it for the user.
            }
        }
    }
    applied
}

/// Walk a path like `views[0].scenes[1].children[2].children[0]` against the
/// raw JSON, transparently handling the legacy `scenes` and `composition` shapes.
fn navigate<'a>(
    root: &'a mut serde_json::Value,
    path: &str,
) -> Option<&'a mut serde_json::Value> {
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
