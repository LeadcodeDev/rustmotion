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
//!   7. Geometry checks (viewport overflow, wrap)

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
    ///
    /// `unresolved_vars` is deliberately **not** blocking. `variables.rs`
    /// (constat #7) settled that a leftover `$word` cannot be told apart from
    /// legitimate literal `$` content — a price tag, a terminal `$PATH` — so it
    /// is reported as a loud warning and the document renders. Blocking here
    /// contradicted that: `validate` printed "Valid scenario" and exited 0 on a
    /// file `render` then refused, while the refusal told the user to run
    /// `validate` for details it would never print. A declared variable can
    /// never be left unresolved (every name in `defs` is present in
    /// defaults ∪ overrides), and a `for-each`/`use` mistake is caught by name
    /// in `expand.rs` — so nothing that reaches here is a diagnosable typo.
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
    // Same ordering rule, same reason, for `= ...` expressions: this is a
    // second, independent load pipeline from `rustmotion::loader`'s (this
    // crate's `validate`/`render` both go through *this* one, not that one —
    // see `rustmotion_core::expr`'s module doc and `loader::fold_static_expressions`'s
    // doc for why the fold must run right here, immediately after expansion
    // and before `Scenario` deserialization: a `for-each`-authored template
    // has its `$i`/`$index`/`$item`/`$count` already substituted to literal
    // text by the expansion step just above, which is what lets a purely
    // arithmetic expression like `cos($i / $count * TAU) * 600` fold to a
    // plain number here rather than reach `Scenario` deserialization as a
    // string where an `f32` is expected (which used to fail with a
    // misleading "invalid type: string, expected f32" instead of the
    // scenario simply working).
    rustmotion::loader::fold_static_expressions(&mut json_value, &label)?;

    // Assets are relative to the scenario file, like `include` — and this must
    // happen before `raw` is captured, so the existence check below and the
    // renderer look at the same, already-resolved paths.
    if let Some(dir) = source_path.as_ref().and_then(|p| p.parent()) {
        rustmotion::assets::rebase_relative_paths(&mut json_value, dir);
    }

    let scenario: Scenario = serde_json::from_value(json_value.clone())?;
    // `resolve_includes_and_synthesize_audio` is `include::resolve_includes`
    // plus, when this scenario's own `audio` declares a synthesised score
    // (issue #331), rendering it and appending it to the resolved
    // scenario's `audio` as an ordinary `AudioTrack` — both `validate` and
    // `render` go through this one shared function, not a bespoke call to
    // `include::resolve_includes` that would silently skip that step.
    let resolved =
        rustmotion::loader::resolve_includes_and_synthesize_audio(scenario, &include_source)?;

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
        // Issue #334's second blind spot, closed: a `SlideTransition`/
        // `ViewTransition` frame is a real, on-screen frame like any other —
        // sampling only `[0, scene_duration]` (above) never looked at it, so
        // text that only leaves the viewport mid-transition passed clean.
        // Same `--strict-anim` gate, same cost trade, same `ViolationKind`.
        geom_violations.extend(validate_geometry_transitions(&loaded.scenario));
    }
    let (mut schema_errors, mut warnings) = validate_scenario(&loaded.scenario);
    warnings.extend(warn_misplaced_animation(&loaded.raw));
    // M4 (issue #110 / #102): legibility floor — always advisory, never
    // blocking (see `check_legibility`'s doc comment for the threshold
    // justification).
    warnings.extend(check_legibility(&loaded.scenario));
    // Issue #336: off-grid cuts — always advisory, never blocking (see
    // `check_off_grid_cuts`'s doc comment).
    warnings.extend(check_off_grid_cuts(&loaded.scenario));
    // Issue #328: a `node("id", "prop")` dependency graph error (a cycle, an
    // unknown id, a duplicate id, or a reference crossing a scene boundary —
    // `reference_cross_scene`) is a load-time structural mistake, the same
    // category as `unresolved_beat_unit` above it — always blocking,
    // unaffected by `--lenient` (see `check_node_references`'s doc comment).
    schema_errors.extend(check_node_references(&loaded.scenario));
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

/// Issue #328's `node("id", "prop")` dependency graph, checked once per
/// scene at `validate` time — a cycle, an undeclared id, a duplicate id, or
/// a reference crossing a scene boundary (`reference_cross_scene`) are all
/// decided by [`rustmotion::engine::deps::DepGraph::build`], reusing the
/// exact `(String, Vec<NodeRef>)` list
/// [`rustmotion::components::box_builder::collect_node_refs`] already builds
/// for `rustmotion::engine::render::resolve_node_references`'s per-frame,
/// single-scene call at render time.
///
/// That render-time call always passes an *empty* `other_scene_ids` (see its
/// own doc comment: no caller crosses scene boundaries there), so
/// `DepsError::CrossScene` can never actually fire from it — a reference to
/// an id declared in a different scene is reported as a plain `UnknownId`
/// instead, and only at render, never at `validate`. This is the gap issue
/// #335 exists to close: here, every *other* scene's own declared ids are
/// collected first, so a cross-scene reference is named for what it is
/// (`reference_cross_scene`) before a single frame is ever rendered.
///
/// Static and cheap — no layout, no per-frame evaluation, just the
/// `(id, refs)` list every declared id's own style expressions carry after
/// ordinary deserialization — so this runs unconditionally, not gated behind
/// `--strict-anim` the way the geometry sampler above is.
///
/// One limitation, inherited rather than introduced here: `collect_node_refs`
/// only records an entry for a node that itself declares an `id` (see that
/// function's own doc) — a *referencing* node with no `id` of its own is
/// invisible to this check too, exactly as it already is to
/// `resolve_node_references` at render time. Closing that needs a broader
/// walk than this workstream's file scope reaches (see this workstream's
/// report).
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
    // Issue #336: `timing` absent defaults to `v1` (today's semantics, which
    // subtract every transition's duration from the total). Nudge authors
    // who want beat-accurate cuts toward `v2` the same way the `scenes`
    // check above nudges toward `composition`.
    if loaded.raw.get("timing").is_none() {
        eprintln!(
            "Warning: `timing` not specified, using legacy v1 (transition durations are \
             subtracted from the total). Set `\"timing\": \"v2\"` for absolute scene \
             placement with a beat grid."
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

    /// `validate` and `render` must agree. A leftover `$word` is a warning by
    /// design (variables.rs, constat #7: a price tag or a `$PATH` is
    /// indistinguishable from a typo), and `validate` treated it that way —
    /// but `is_blocking` did not, so the same file passed validation and was
    /// refused at render, with the refusal pointing back at validate.
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

/// `validation::load_with_vars` is a second, independent load pipeline from
/// `rustmotion::loader`'s (see that module's `fold_static_expressions` doc)
/// — `validate` and `render` (which validates first) both go through *this*
/// one. Before the fold was wired in here too, a scenario using `= ...`
/// expressions passed neither: a static expression reached `Scenario`
/// deserialization as a bare string where a typed field (e.g. `x: f32`) was
/// expected, and even a `$`-free expression that *would* have deserialized
/// fine printed spurious "unresolved variable" warnings for `$W`/`$H`/etc
/// (see `crate::variables::find_unresolved`'s doc on why an expression
/// string is no longer scanned for `$name` content at all).
#[cfg(test)]
mod expr_fold_through_validation_pipeline {
    use super::*;

    /// The acceptance scenario this whole workstream exists for: a
    /// `for-each` over 8 items placing badges on a circle via
    /// `$W`/`$i`/`$count`, going through the exact pipeline `validate`/
    /// `render` use — not `rustmotion::loader`'s.
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

    /// A fully `$`-free static expression (no scope variable at all) used to
    /// be the sharpest repro of the missing fold: nothing about it looks
    /// like a `$variable`, so it reached `Scenario` deserialization as a
    /// plain string exactly once, with no other symptom.
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

/// Issue #328/#335: `reference_cross_scene` (and the rest of
/// `DepGraph::build`'s error surface) reachable from `validate`, not only
/// from `render`'s single-scene call site — see `check_node_references`'s
/// own doc comment for why the render-time call could never actually
/// produce `CrossScene` at all.
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
