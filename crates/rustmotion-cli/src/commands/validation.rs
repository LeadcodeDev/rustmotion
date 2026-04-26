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
            (s, Some(path.to_path_buf()), IncludeSource::File(path.to_path_buf()))
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
    ValidationReport {
        schema_errors: validate_scenario(&loaded.scenario),
        geom_violations,
        unresolved_vars: variables::find_unresolved(&loaded.raw),
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
            return Err(RustmotionError::UnknownCodec { codec: c.to_string() });
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
