//! Shared validation pipeline used by both `validate` and `render`.
//!
//! The pipeline is the source of truth for "is this scenario safe to render?".
//! It runs the same checks regardless of which command invokes it:
//!   1. Parse JSON
//!   2. Apply variable defaults
//!   3. Detect unresolved `$variable` references
//!   4. Deserialize into `Scenario`
//!   5. Resolve includes → `ResolvedScenario`
//!   6. Schema-level checks (file existence, dimensions, durations, etc.)
//!   7. Geometry checks (viewport overflow, wrap, auto_scroll)

use rustmotion::error::{Result, RustmotionError};
use rustmotion::include::{self, IncludeSource};
use rustmotion::schema::{ResolvedScenario, Scenario};
use rustmotion::variables;
use std::path::{Path, PathBuf};

use super::geometry::{validate_geometry, validate_geometry_animated, GeometryViolation};
use super::validate_schema::validate_scenario;

/// Source of the scenario JSON to validate.
pub enum ValidationSource<'a> {
    File(&'a Path),
    Inline(&'a str),
}

/// A scenario after parsing, variable resolution, and include resolution.
/// Keeps the raw JSON around so it can be inspected by validators (e.g. for
/// path-based auto-fixes).
pub struct LoadedScenario {
    pub raw: serde_json::Value,
    pub scenario: ResolvedScenario,
    pub source_path: Option<PathBuf>,
}

/// Result of running the validation pipeline.
#[derive(Default)]
pub struct ValidationReport {
    pub schema_errors: Vec<String>,
    pub geom_violations: Vec<GeometryViolation>,
    pub unresolved_vars: Vec<String>,
    /// Non-blocking advisory messages (do not prevent rendering).
    pub warnings: Vec<String>,
    /// Unknown component attributes (silently ignored at load). Advisory by
    /// default; promoted to blocking errors by `--strict-attrs`.
    pub attr_warnings: Vec<String>,
}

impl ValidationReport {
    pub fn is_clean(&self) -> bool {
        self.schema_errors.is_empty()
            && self.geom_violations.is_empty()
            && self.unresolved_vars.is_empty()
    }

    /// Whether the report contains any issue that should block rendering.
    /// In `lenient` mode geometry violations are downgraded to warnings.
    pub fn is_blocking(&self, lenient: bool) -> bool {
        if !self.schema_errors.is_empty() {
            return true;
        }
        if !self.unresolved_vars.is_empty() {
            return true;
        }
        if !lenient && !self.geom_violations.is_empty() {
            return true;
        }
        false
    }

    pub fn to_error(&self) -> RustmotionError {
        RustmotionError::ValidationFailed {
            schema_errors: self.schema_errors.len(),
            geometry_violations: self.geom_violations.len(),
            unresolved_vars: self.unresolved_vars.len(),
        }
    }

    /// `--strict-attrs`: turn unknown-attribute warnings into blocking schema
    /// errors.
    pub fn promote_attr_warnings(&mut self) {
        self.schema_errors.append(&mut self.attr_warnings);
    }
}

/// Load + parse + apply variable defaults + resolve includes. Returns the
/// raw JSON (post-substitution) and the resolved scenario.
pub fn load(source: ValidationSource<'_>) -> Result<LoadedScenario> {
    let (json_str, source_path, include_source) = match source {
        ValidationSource::File(path) => {
            let s = std::fs::read_to_string(path).map_err(|e| RustmotionError::FileRead {
                path: path.display().to_string(),
                source: e,
            })?;
            // HTML input is transpiled to the scenario JSON first, then validated
            // through the identical JSON pipeline below.
            let s = if rustmotion::loader::is_html_path(path) {
                let value = rustmotion::loader::html_to_scenario_json(&s)?;
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

    variables::apply_defaults(&mut json_value)?;

    let scenario: Scenario = serde_json::from_value(json_value.clone())?;
    let resolved = include::resolve_includes(scenario, &include_source)?;

    Ok(LoadedScenario {
        raw: json_value,
        scenario: resolved,
        source_path,
    })
}

/// Run all validation checks against a loaded scenario. When `strict_anim`
/// is true, also sample animation frames and check that no widget's
/// transformed bbox leaves the viewport.
pub fn run_checks(loaded: &LoadedScenario, strict_anim: bool) -> ValidationReport {
    let mut geom_violations = validate_geometry(&loaded.scenario);
    if strict_anim {
        geom_violations.extend(validate_geometry_animated(&loaded.scenario));
    }
    let (mut schema_errors, mut warnings) = validate_scenario(&loaded.scenario);
    warnings.extend(warn_misplaced_animation(&loaded.raw));
    let (attr_errors, attr_warnings) =
        super::validate_attrs::check_component_attrs(&loaded.scenario);
    schema_errors.extend(attr_errors);
    ValidationReport {
        schema_errors,
        geom_violations,
        unresolved_vars: variables::find_unresolved(&loaded.raw),
        warnings,
        attr_warnings,
    }
}

/// Detect `animation` placed at a component's top level (a sibling of `style`).
/// The engine only reads `style.animation`, so a top-level `animation` is
/// silently ignored — a common, hard-to-spot mistake. Returns one warning per
/// offending component.
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

/// Inspect the raw JSON to surface defaults that were silently applied. We
/// only warn when the field is *missing*, not when it equals the default — an
/// explicit `"fps": 30` is intentional, an absent `fps` is something the user
/// likely forgot.
pub fn warn_on_silent_defaults(loaded: &LoadedScenario) {
    if let Some(video) = loaded.raw.get("video") {
        if video.get("fps").is_none() {
            eprintln!("Warning: video.fps not specified, using default 30");
        }
        if video.get("background").is_none() {
            eprintln!("Warning: video.background not specified, using default #000000");
        }
    }
    // Legacy `scenes` at top-level: still works but the new format is `composition: [{type:"slide",scenes:[...]}]`.
    if loaded.raw.get("composition").is_none() && loaded.raw.get("scenes").is_some() {
        eprintln!(
            "Warning: top-level `scenes` is legacy. Migrate to `composition: [{{ type: \"slide\", scenes: [...] }}]` for clarity."
        );
    }
}

/// Validate `--codec` against the list ffmpeg can drive. Defaults to OK if None.
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

/// Validate `--crf` is in the H.264/H.265 valid range. Defaults to OK if None.
pub fn check_crf(crf: Option<u8>) -> Result<()> {
    if let Some(v) = crf {
        if v > 51 {
            return Err(RustmotionError::InvalidCrf { value: v });
        }
    }
    Ok(())
}

/// Print a report to stderr in the same format as `cmd_validate`.
pub fn print_report(report: &ValidationReport, source_label: &str) {
    use super::geometry::format_violation;

    for w in &report.warnings {
        eprintln!("Warning: {}", w);
    }
    for w in &report.attr_warnings {
        eprintln!("Warning: {}", w);
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
        // CssStyle is deny_unknown_fields: a typo'd CSS property must surface
        // as a validation error naming the property, not silently drop the
        // child at render time.
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
    fn unknown_animation_preset_from_html_is_a_blocking_error() {
        // Unknown preset names are not validated in the transpiler (no schema
        // dependency there); the typed deserialization of `style.animation`
        // (tagged AnimationEffect enum) must reject them here, as a blocking
        // and readable error.
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
