use rustmotion::components::{ChildComponent, Component};
use rustmotion::error::Result;
use rustmotion::include;
use rustmotion::schema::{CardDisplay, ResolvedScenario, Scenario};
use rustmotion::variables;
use std::path::{Path, PathBuf};

use super::geometry::{format_violation, validate_geometry, GeometryViolation, ViolationKind};

pub fn cmd_validate(
    input: &PathBuf,
    report: Option<&Path>,
    fix: bool,
    _strict_anim: bool,
    lenient: bool,
) -> Result<()> {
    use rustmotion::error::RustmotionError;

    let json_str = std::fs::read_to_string(input)
        .map_err(|e| RustmotionError::FileRead { path: input.display().to_string(), source: e })?;

    let mut json_value: serde_json::Value = match serde_json::from_str(&json_str) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("JSON parse error: {}", e);
            std::process::exit(1);
        }
    };

    if let Err(e) = variables::apply_defaults(&mut json_value) {
        eprintln!("Variable error: {}", e);
        std::process::exit(1);
    }
    for name in variables::find_unresolved(&json_value) {
        eprintln!("Warning: unresolved variable reference '${}' in '{}'", name, input.display());
    }

    let scenario: Scenario = match serde_json::from_value(json_value.clone()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("JSON parse error: {}", e);
            std::process::exit(1);
        }
    };

    let resolved = match include::resolve_includes(scenario, &include::IncludeSource::File(input.clone())) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Include resolution error: {}", e);
            std::process::exit(1);
        }
    };

    let schema_errors = validate_scenario(&resolved);
    let geom_violations = validate_geometry(&resolved);

    if let Some(report_path) = report {
        write_report(report_path, &schema_errors, &geom_violations)?;
        eprintln!("Wrote report: {}", report_path.display());
    }

    let mut applied_fixes = 0usize;
    if fix && !geom_violations.is_empty() {
        applied_fixes = apply_fixes(&mut json_value, &geom_violations);
        if applied_fixes > 0 {
            let pretty = serde_json::to_string_pretty(&json_value)
                .map_err(|e| RustmotionError::Generic(format!("serialize fixes: {}", e)))?;
            std::fs::write(input, pretty)
                .map_err(|e| RustmotionError::FileRead { path: input.display().to_string(), source: e })?;
            eprintln!("Applied {} auto-fix(es) to {}", applied_fixes, input.display());
        }
    }

    let all_scenes: Vec<_> = resolved.all_scenes().collect();
    let total_duration: f64 = all_scenes.iter().map(|s| s.duration).sum();

    if !schema_errors.is_empty() {
        for err in &schema_errors {
            eprintln!("Error: {}", err);
        }
    }

    if !geom_violations.is_empty() {
        eprintln!();
        eprintln!("Geometry: {} violation(s)", geom_violations.len());
        for v in &geom_violations {
            eprintln!("{}", format_violation(v));
            eprintln!();
        }
    }

    let blocking = !schema_errors.is_empty() || (!geom_violations.is_empty() && !lenient);
    if blocking {
        if applied_fixes > 0 {
            eprintln!("Some fixes applied — re-run validate to confirm.");
        }
        std::process::exit(1);
    }

    eprintln!(
        "Valid scenario: {} scene(s) in {} view(s)",
        all_scenes.len(),
        resolved.views.len()
    );
    eprintln!(
        "  Resolution: {}x{} @ {}fps",
        resolved.video.width, resolved.video.height, resolved.video.fps
    );
    eprintln!("  Duration: {:.1}s", total_duration);
    if !geom_violations.is_empty() {
        eprintln!("  Geometry warnings: {}", geom_violations.len());
    }
    Ok(())
}

fn write_report(
    path: &Path,
    schema_errors: &[String],
    violations: &[GeometryViolation],
) -> Result<()> {
    use rustmotion::error::RustmotionError;
    let report = serde_json::json!({
        "schema_errors": schema_errors,
        "geometry_violations": violations,
    });
    let pretty = serde_json::to_string_pretty(&report)
        .map_err(|e| RustmotionError::Generic(format!("serialize report: {}", e)))?;
    std::fs::write(path, pretty)
        .map_err(|e| RustmotionError::FileRead { path: path.display().to_string(), source: e })?;
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
                    let style = obj.entry("style").or_insert_with(|| serde_json::json!({}));
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
            ViolationKind::ViewportOverflow => {
                // Position/size clamping is too risky to auto-fix without losing
                // intent — leave it for the user.
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

fn validate_children(
    children: &[ChildComponent],
    path: &str,
    errors: &mut Vec<String>,
) {
    for (j, child) in children.iter().enumerate() {
        let p = format!("{}.children[{}]", path, j);

        if let Some(timed) = child.component.as_timed() {
            let (start, end) = timed.timing();
            if let (Some(s), Some(e)) = (start, end) {
                if s >= e {
                    errors.push(format!("{}: start_at ({}) must be < end_at ({})", p, s, e));
                }
            }
        }

        match &child.component {
            Component::Image(img) => {
                if !std::path::Path::new(&img.src).exists() {
                    errors.push(format!("{}.src: file not found '{}'", p, img.src));
                }
            }
            Component::Video(v) => {
                if !std::path::Path::new(&v.src).exists() {
                    errors.push(format!("{}.src: file not found '{}'", p, v.src));
                }
            }
            Component::Gif(g) => {
                if !std::path::Path::new(&g.src).exists() {
                    errors.push(format!("{}.src: file not found '{}'", p, g.src));
                }
            }
            Component::Svg(svg) => {
                if svg.src.is_none() && svg.data.is_none() {
                    errors.push(format!("{}: SVG must have 'src' or 'data'", p));
                }
                if let Some(ref src) = svg.src {
                    if !std::path::Path::new(src).exists() {
                        errors.push(format!("{}.src: file not found '{}'", p, src));
                    }
                }
            }
            Component::Icon(icon) => {
                if let Some((prefix, name)) = icon.icon.split_once(':') {
                    if prefix.is_empty() || name.is_empty() {
                        errors.push(format!(
                            "{}: icon '{}' has empty prefix or name (expected 'prefix:name')",
                            p, icon.icon
                        ));
                    }
                } else {
                    errors.push(format!(
                        "{}: invalid icon format '{}' (expected 'prefix:name', e.g. 'lucide:home')",
                        p, icon.icon
                    ));
                }
            }
            Component::QrCode(qr) => {
                if qr.content.is_empty() {
                    errors.push(format!("{}: QR code content must not be empty", p));
                }
            }
            Component::Mockup(m) => {
                if !std::path::Path::new(&m.src).exists() {
                    errors.push(format!("{}.src: file not found '{}'", p, m.src));
                }
            }
            Component::Card(card) => {
                if matches!(card.style.display, Some(CardDisplay::Grid))
                    && card.style.grid_template_columns.is_none()
                {
                    errors.push(format!("{}: grid display without grid-template-columns", p));
                }
                validate_children(&card.children, &p, errors);
            }
            Component::Flex(flex) => {
                validate_children(&flex.children, &p, errors);
            }
            Component::Grid(grid) => {
                if grid.style.grid_template_columns.is_none() {
                    errors.push(format!("{}: grid without grid-template-columns", p));
                }
                validate_children(&grid.children, &p, errors);
            }
            Component::Positioned(pos) => {
                validate_children(&pos.children, &p, errors);
            }
            Component::Container(container) => {
                validate_children(&container.children, &p, errors);
            }
            _ => {}
        }
    }
}

fn validate_scenario(scenario: &ResolvedScenario) -> Vec<String> {
    let mut errors = Vec::new();

    if scenario.video.width == 0 || scenario.video.height == 0 {
        errors.push("video.width and video.height must be > 0".to_string());
    }
    if scenario.video.width % 2 != 0 || scenario.video.height % 2 != 0 {
        errors.push("video.width and video.height must be even (required for H.264)".to_string());
    }
    if scenario.video.fps == 0 {
        errors.push("video.fps must be > 0".to_string());
    }

    let all_scenes: Vec<_> = scenario.all_scenes().collect();
    if all_scenes.is_empty() {
        errors.push("At least one scene is required".to_string());
    }

    for (vi, view) in scenario.views.iter().enumerate() {
        for (si, scene) in view.scenes.iter().enumerate() {
            if scene.duration <= 0.0 {
                errors.push(format!("views[{}].scenes[{}].duration must be > 0", vi, si));
            }

            let children = rustmotion::engine::render::deserialize_children(scene);
            validate_children(&children, &format!("views[{}].scenes[{}]", vi, si), &mut errors);
        }
    }

    for (i, audio) in scenario.audio.iter().enumerate() {
        if !std::path::Path::new(&audio.src).exists() {
            errors.push(format!(
                "audio[{}].src: file not found '{}'",
                i, audio.src
            ));
        }
    }

    errors
}
