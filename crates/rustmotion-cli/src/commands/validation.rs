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

use rustmotion::engine;
use rustmotion::error::{Result, RustmotionError};
use rustmotion::expand;
use rustmotion::include::{self, IncludeSource};
use rustmotion::schema::{ResolvedScenario, Scenario};
use rustmotion::variables;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::geometry::{
    check_legibility, validate_geometry, validate_geometry_animated, GeometryViolation,
};
use super::validate_schema::validate_scenario;

/// Source of the scenario JSON to validate.
pub enum ValidationSource<'a> {
    File(&'a Path),
    Inline(&'a str),
}

/// Runtime variable overrides from `--props` / `--var` flags.
/// An empty map means "use defaults only".
pub type VarOverrides = HashMap<String, serde_json::Value>;

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
    /// True when there is nothing at all to show the author — no blocking
    /// issue *and* no advisory one. Used to decide whether `render`'s
    /// implicit validation pass prints anything; `warnings`/`attr_warnings`
    /// are included so a non-blocking advisory (e.g. a legibility warning,
    /// or an attr_warning before it is known to block) is never silently
    /// dropped just because nothing else escalated it to an error.
    pub fn is_clean(&self) -> bool {
        self.schema_errors.is_empty()
            && self.geom_violations.is_empty()
            && self.unresolved_vars.is_empty()
            && self.warnings.is_empty()
            && self.attr_warnings.is_empty()
    }

    /// Whether the report contains any issue that should block rendering.
    /// In `lenient` mode geometry violations are downgraded to warnings.
    ///
    /// Unknown component attributes block **unconditionally** — M5 (issue
    /// #110 / #102, "decided at kickoff"), not gated by `lenient` (that
    /// flag's documented scope is geometry violations) and no longer
    /// opt-in via `--strict-attrs`. In practice they already arrive folded
    /// into `schema_errors` (`run_checks` does this unconditionally now, so
    /// every caller blocks on them without extra wiring); the direct
    /// `attr_warnings` check below is defence in depth for a
    /// `ValidationReport` assembled some other way.
    pub fn is_blocking(&self, lenient: bool) -> bool {
        if !self.schema_errors.is_empty() {
            return true;
        }
        if !self.unresolved_vars.is_empty() {
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
            // `attr_warnings` is normally already empty here (`run_checks`
            // folds it into `schema_errors` unconditionally — see there);
            // `+ self.attr_warnings.len()` is defence in depth so this count
            // stays honest even for a `ValidationReport` assembled another
            // way. `RustmotionError::ValidationFailed` has no dedicated
            // attr-warnings field of its own (out of this workstream's
            // scope to add).
            schema_errors: self.schema_errors.len() + self.attr_warnings.len(),
            geometry_violations: self.geom_violations.len(),
            unresolved_vars: self.unresolved_vars.len(),
        }
    }

    /// `--strict-attrs` / CLI compat only: by the time a `ValidationReport`
    /// from `run_checks` reaches this call, `attr_warnings` is already
    /// empty (folded into `schema_errors` there — see M5, issue #110), so
    /// this is a no-op in practice. Kept so `--report` JSON output and any
    /// hand-assembled `ValidationReport` still behave as documented.
    pub fn promote_attr_warnings(&mut self) {
        self.schema_errors.append(&mut self.attr_warnings);
    }
}

/// Load + parse + apply variable defaults + resolve includes. Returns the
/// raw JSON (post-substitution) and the resolved scenario.
pub fn load(source: ValidationSource<'_>) -> Result<LoadedScenario> {
    load_with_vars(source, None)
}

/// Like [`load`] but injects runtime variable overrides before substitution.
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
            // HTML input is transpiled to the scenario JSON first, then validated
            // through the identical JSON pipeline below. Sidecar annotations
            // are merged so validate/render see the same scenario as the
            // studio and `load_input`.
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
    // Expand `for-each`/`use` (and consume `components`) *before* `raw` is
    // captured below, so `LoadedScenario::raw` — what geometry checks walk
    // and what `--fix` would serialise — is already the expanded tree. This
    // is the same reason `include::resolve_includes` runs before this
    // function returns: a validator that reasons about the pre-expansion
    // document would be validating something other than what actually
    // renders.
    expand::expand_directives(&mut json_value, &label)?;

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
///
/// Registers any custom/Google fonts declared on the scenario *before*
/// running geometry checks, so text is measured with the same typeface the
/// render path will use. Without this, geometry checks measure through the
/// Helvetica → Arial → OS fallback chain regardless of what the scenario
/// declares — a systematic false-negative/false-positive source (issue #106).
pub fn run_checks(loaded: &LoadedScenario, strict_anim: bool) -> ValidationReport {
    if !loaded.scenario.fonts.is_empty() {
        engine::renderer::load_custom_fonts(&loaded.scenario.fonts);
    }
    let mut geom_violations = validate_geometry(&loaded.scenario);
    if strict_anim {
        geom_violations.extend(validate_geometry_animated(&loaded.scenario));
    }
    let (mut schema_errors, mut warnings) = validate_scenario(&loaded.scenario);
    warnings.extend(warn_misplaced_animation(&loaded.raw));
    // M4 (issue #110 / #102): legibility floor — always advisory, never
    // blocking (see `check_legibility`'s doc comment for the threshold
    // justification).
    warnings.extend(check_legibility(&loaded.scenario));
    let (attr_errors, mut attr_warnings) =
        super::validate_attrs::check_component_attrs(&loaded.scenario);
    schema_errors.extend(attr_errors);
    // M5 (issue #110 / #102, decided at kickoff): unknown component
    // attributes error by default now, not only under `--strict-attrs`.
    // Folding them into `schema_errors` right here — rather than leaving
    // them in the separate `attr_warnings` bucket for every *caller* of
    // `run_checks` to remember to escalate — means `validate`, `render`,
    // and `watch` all block on them uniformly with zero extra wiring, and
    // none of their existing `!schema_errors.is_empty()` blocking checks
    // needed to change. `attr_warnings` is left empty; `--strict-attrs` /
    // `promote_attr_warnings` are kept as accepted, fully inert flags for
    // CLI-surface stability (see their doc comments).
    schema_errors.append(&mut attr_warnings);
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

/// Validate `--crf` is in the H.264/H.265 valid range, and flag when it is
/// paired with `--hardware-acceleration`. Returns `Ok(Some(warning))` for
/// that combination: hardware encoders (VideoToolbox/NVENC/QSV/AMF) are
/// bitrate/quality-driven, not CRF-driven, and `ffmpeg_args`'s hardware
/// branch does not emit `-crf` at all — passing `--crf` there silently does
/// nothing unless ffmpeg falls back to the software encoder, in which case
/// it applies after all. The caller decides whether/how to print the
/// warning (e.g. respecting `--quiet`); this function stays pure so the
/// combination is testable without capturing stderr.
///
/// Still returns `Err` for an out-of-range value regardless of
/// `hardware_acceleration` — an invalid CRF is invalid on any path.
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

/// Print a report to stderr in the same format as `cmd_validate`.
pub fn print_report(report: &ValidationReport, source_label: &str) {
    use super::geometry::format_violation;

    for w in &report.warnings {
        eprintln!("Warning: {}", w);
    }
    // M5 (issue #110 / #102): `report.attr_warnings` is normally already
    // empty by the time it reaches here — `run_checks` folds unknown
    // component attributes into `schema_errors` unconditionally now, so
    // they print below as `Error:` lines, not `Warning:` ones. Printing
    // "Warning" right before the process exits non-zero for that exact
    // issue was the dishonest-label pattern this workstream exists to
    // remove. This loop stays as a fallback for a `ValidationReport`
    // assembled another way.
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

#[cfg(test)]
mod font_loading_wiring_tests {
    use super::*;
    use serde_json::json;

    /// H5 (issue #106): `run_checks` must register the scenario's declared
    /// fonts before running geometry checks — matching what the render path
    /// already does at `render.rs:47` — so validation and render resolve
    /// fonts through the same call order instead of validation always
    /// measuring through the fallback chain. A missing/unreadable font path
    /// only warns (see `engine::renderer::fonts::register_font_file`); it
    /// must not turn into a blocking validation error.
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

    /// A scenario with no `fonts` entries must not attempt any font I/O
    /// (the `!loaded.scenario.fonts.is_empty()` guard in `run_checks`).
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

/// Notice printed when `--strict-attrs` is passed.
///
/// Unknown component attributes became blocking errors by default, which left
/// this flag with nothing to promote. A flag that silently does nothing is the
/// same failure mode this validator exists to catch — an accepted input with no
/// observable effect — so it announces itself instead of staying mute. It is
/// still accepted so existing scripts and CI pipelines keep working.
pub fn warn_strict_attrs_is_now_default() {
    eprintln!(
        "Notice: --strict-attrs is deprecated and does nothing. Unknown \
         component attributes have been errors by default since the attribute \
         checker was hardened; you can drop the flag."
    );
}

/// Proves `validate` reasons about the *expanded* tree, not the
/// pre-expansion `for-each`/`use` directives — the same requirement the
/// workstream brief states for `include`-produced scenes ("le validateur
/// doit voir l'arbre expansé"). If `load_with_vars` only expanded directives
/// for rendering but validated the raw, unexpanded document, a geometry
/// violation baked into one of several `for-each`-generated items would be
/// invisible: the un-expanded document has no `text`/`card` components at
/// all at that position, only a directive object geometry checks don't know
/// how to measure.
#[cfg(test)]
mod expanded_tree_is_what_gets_validated {
    use super::*;

    #[test]
    fn a_geometry_violation_inside_a_for_each_generated_item_is_detected() {
        // Two iterations: the first is short and fits, the second is a
        // narrow-card/nowrap-text combination guaranteed to overflow — the
        // same violation shape `NARROW_CARD_JSON` uses elsewhere in this
        // crate's tests.
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

        // The raw tree `--fix` would act on must already be expanded: no
        // `for-each` directive marker survives, and there are 2 concrete
        // children where the source only wrote 1 directive.
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
        // The violation's path must point at the *second* expanded card
        // (children[1]), proving the geometry walker is indexing into the
        // expanded array, not some placeholder.
        assert!(
            report.geom_violations[0].path.contains("children[1]"),
            "expected the violation to be attributed to the second expanded card: {}",
            report.geom_violations[0].path
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
