//! Unknown-attribute detection for LLM-authored scenarios.
//!
//! Component structs deliberately do not use `deny_unknown_fields` (they rely
//! on `#[serde(flatten)]` for timing, which is incompatible with it), so a
//! typo'd attribute like `<rm-counter typo-attr="x">` silently disappears at
//! typed load. This module rebuilds the set of known top-level properties per
//! component from the schemars JSON Schema of `Component` and reports any
//! unknown key into `attr_warnings`.
//!
//! M5 (issue #110 / #102, decided at kickoff): unknown attributes error
//! **by default** now, not only under `--strict-attrs` as before. This
//! module still returns them separately as `(errors, warnings)` — callers
//! outside `run_checks` (e.g. this module's own tests) can inspect them
//! independently — but `validation::run_checks` folds the warnings straight
//! into `schema_errors` unconditionally before anyone sees a
//! `ValidationReport`, so `validate`/`render`/`watch` all block on them with
//! no extra wiring. `--strict-attrs` / `ValidationReport::promote_attr_warnings`
//! still exist, purely for CLI-surface stability — see their doc comments.
//!
//! It also surfaces typed-deserialization failures (e.g. an unknown CSS
//! property, rejected by `CssStyle`'s `deny_unknown_fields`, or a missing
//! required field) as blocking schema errors — today those children are
//! silently dropped at render time.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use rustmotion::components::{ChildComponent, Component};
use rustmotion::schema::ResolvedScenario;

/// Keys accepted on any component object but absent from the per-variant
/// schema properties:
/// - `position`, `x`, `y`, `z-index`: `ChildComponent` wrapper fields
///   (flattened around the component itself).
/// - `animation`: top-level `animation` is ignored by the engine and already
///   reported by the dedicated misplaced-animation warning — flagging it here
///   too would double-report.
const WRAPPER_KEYS: &[&str] = &["position", "x", "y", "z-index", "animation", "bleed"];

/// Serde enum aliases that schemars does not know about: alias tag → schema tag.
const TAG_ALIASES: &[(&str, &str)] = &[("container", "div"), ("progress_bar", "progress")];

/// Lazily-built map: component tag ("counter") → set of known top-level
/// property names, extracted from the schemars `oneOf` variants. Flattened
/// `TimingConfig` fields (`start_at`, `end_at`) are included by schemars in
/// each variant's properties, so no extra allowlist is needed for them.
fn known_props() -> &'static BTreeMap<String, BTreeSet<String>> {
    static CACHE: OnceLock<BTreeMap<String, BTreeSet<String>>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let schema = serde_json::to_value(schemars::schema_for!(Component))
            .expect("Component schema serializes");
        let mut map = BTreeMap::new();
        if let Some(one_of) = schema["oneOf"].as_array() {
            for variant in one_of {
                let Some(tag) = variant["properties"]["type"]["enum"][0].as_str() else {
                    continue;
                };
                let props: BTreeSet<String> = variant["properties"]
                    .as_object()
                    .map(|o| o.keys().cloned().collect())
                    .unwrap_or_default();
                map.insert(tag.to_string(), props);
            }
        }
        for (alias, target) in TAG_ALIASES {
            if let Some(props) = map.get(*target).cloned() {
                map.insert(alias.to_string(), props);
            }
        }
        map
    })
}

/// Check every component in the scenario. Returns `(errors, warnings)`:
/// - errors: children that fail typed deserialization (would be silently
///   dropped at render time) — always blocking;
/// - warnings: unknown top-level attributes (silently ignored at load) —
///   advisory unless `--strict-attrs`.
pub fn check_component_attrs(scenario: &ResolvedScenario) -> (Vec<String>, Vec<String>) {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    for (vi, view) in scenario.views.iter().enumerate() {
        for (si, scene) in view.scenes.iter().enumerate() {
            for (ci, child) in scene.children.iter().enumerate() {
                let path = format!("views[{vi}].scenes[{si}].children[{ci}]");
                // Typed parse at scene level covers nested children
                // transitively (containers parse their subtree).
                if let Err(e) = serde_json::from_value::<ChildComponent>(child.clone()) {
                    let kind = child.get("type").and_then(|t| t.as_str()).unwrap_or("?");
                    errors.push(format!(
                        "{path} (type={kind}): invalid component — would be silently dropped at render: {}",
                        truncate(&e.to_string(), 220)
                    ));
                }
                walk_component(child, &path, &mut warnings);
            }
        }
    }
    (errors, warnings)
}

/// Recursively check one component object (and its `children`, when the
/// component type supports children) for unknown top-level attributes.
fn walk_component(value: &serde_json::Value, path: &str, warnings: &mut Vec<String>) {
    let Some(obj) = value.as_object() else {
        return;
    };
    let Some(tag) = obj.get("type").and_then(|t| t.as_str()) else {
        return; // missing/invalid type — already covered by the typed-parse error
    };
    let Some(known) = known_props().get(tag) else {
        return; // unknown component tag — already covered by the typed-parse error
    };

    for key in obj.keys() {
        if known.contains(key) || WRAPPER_KEYS.contains(&key.as_str()) {
            continue;
        }
        warnings.push(format!(
            "{path}: unknown attribute '{key}' on '{tag}' — it is silently ignored ({})",
            suggest(key, known)
        ));
    }

    if known.contains("children") {
        if let Some(children) = obj.get("children").and_then(|c| c.as_array()) {
            for (i, child) in children.iter().enumerate() {
                walk_component(child, &format!("{path}.children[{i}]"), warnings);
            }
        }
    }
}

/// Build the "(did you mean …? known: …)" suffix: known keys sorted by edit
/// distance to the unknown one, closest first, capped at 8.
fn suggest(unknown: &str, known: &BTreeSet<String>) -> String {
    let mut ranked: Vec<(usize, &str)> = known
        .iter()
        .map(|k| (levenshtein(unknown, k), k.as_str()))
        .collect();
    ranked.sort();

    let best = ranked.first().filter(|(d, _)| *d <= 2).map(|(_, k)| *k);
    let listed: Vec<&str> = ranked.iter().take(8).map(|(_, k)| *k).collect();
    let ellipsis = if ranked.len() > 8 { ", …" } else { "" };
    match best {
        Some(k) => format!(
            "did you mean '{k}'? known: {}{}",
            listed.join(", "),
            ellipsis
        ),
        None => format!("known: {}{}", listed.join(", "), ellipsis),
    }
}

/// Classic dynamic-programming Levenshtein distance (small strings only).
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur.push((prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Cap long serde error messages (CssStyle's field list is ~150 entries).
fn truncate(msg: &str, max: usize) -> String {
    if msg.len() <= max {
        msg.to_string()
    } else {
        let mut cut = max;
        while !msg.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}…", &msg[..cut])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::validation::{load, run_checks, ValidationSource};

    fn resolved(children: serde_json::Value) -> ResolvedScenario {
        let json = serde_json::json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{ "duration": 2.0, "children": children }]
        })
        .to_string();
        load(ValidationSource::Inline(&json))
            .expect("scenario loads")
            .scenario
    }

    #[test]
    fn unknown_attribute_warns_with_exact_name() {
        let s = resolved(serde_json::json!([
            { "type": "counter", "from": 0, "to": 100, "typo-attr": "x" }
        ]));
        let (errors, warnings) = check_component_attrs(&s);
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        assert_eq!(warnings.len(), 1, "expected one warning: {warnings:?}");
        assert!(warnings[0].contains("'typo-attr'"), "got: {}", warnings[0]);
        assert!(warnings[0].contains("'counter'"), "got: {}", warnings[0]);
        assert!(
            warnings[0].contains("views[0].scenes[0].children[0]"),
            "got: {}",
            warnings[0]
        );
        // Lists known attributes so the author can self-correct.
        assert!(warnings[0].contains("known:"), "got: {}", warnings[0]);
    }

    #[test]
    fn unknown_attr_blocks_by_default_without_strict_attrs() {
        // M5 (issue #110): unknown attributes error by default — no
        // `--strict-attrs` needed. `render` used to exit 0 here, having
        // silently used default styling. `run_checks` folds them straight
        // into `schema_errors` (see its doc comment), so every caller
        // (`validate`, `render`, `watch`) blocks uniformly with zero extra
        // wiring — `report.attr_warnings` itself is already empty by the
        // time `run_checks` returns.
        let json = serde_json::json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{ "duration": 2.0, "children": [
                { "type": "counter", "from": 0, "to": 100, "typo-attr": "x" }
            ]}]
        })
        .to_string();
        let loaded = load(ValidationSource::Inline(&json)).unwrap();
        let report = run_checks(&loaded, false);
        assert!(
            report.schema_errors.iter().any(|e| e.contains("typo-attr")),
            "expected the unknown attribute in schema_errors: {:?}",
            report.schema_errors
        );
        assert!(report.attr_warnings.is_empty(), "already folded in");
        assert!(
            report.is_blocking(false),
            "unknown attributes must block by default, with no flag"
        );
    }

    #[test]
    fn strict_attrs_promotion_is_a_harmless_no_op_post_m5() {
        // `--strict-attrs` / `promote_attr_warnings` are kept for CLI-surface
        // stability, but since M5 there is nothing left for them to do by
        // the time `run_checks` has already run: `attr_warnings` is already
        // empty, so promoting it is a no-op, and the report already blocked.
        let json = serde_json::json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{ "duration": 2.0, "children": [
                { "type": "counter", "from": 0, "to": 100, "typo-attr": "x" }
            ]}]
        })
        .to_string();
        let loaded = load(ValidationSource::Inline(&json)).unwrap();
        let mut report = run_checks(&loaded, false);
        let before = report.schema_errors.clone();
        assert!(
            report.is_blocking(false),
            "must already block pre-promotion"
        );

        report.promote_attr_warnings();
        assert!(report.attr_warnings.is_empty());
        assert_eq!(
            report.schema_errors, before,
            "promotion must not change schema_errors when attr_warnings is already empty"
        );
        assert!(report.is_blocking(false), "must still block post-promotion");
    }

    #[test]
    fn flattened_and_wrapper_fields_are_not_flagged() {
        // start_at/end_at come from the flattened TimingConfig; position/x/y/
        // z-index from the ChildComponent wrapper. None may warn.
        let s = resolved(serde_json::json!([
            {
                "type": "text", "content": "hi",
                "start_at": 0.5, "end_at": 1.5,
                "position": "absolute", "x": 10, "y": 20, "z-index": 2
            }
        ]));
        let (errors, warnings) = check_component_attrs(&s);
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    }

    #[test]
    fn clean_scenario_has_zero_warnings() {
        let s = resolved(serde_json::json!([
            { "type": "text", "content": "hi", "style": { "font-size": 42 } },
            { "type": "counter", "from": 0, "to": 100, "suffix": "%" },
            { "type": "card", "children": [
                { "type": "text", "content": "nested" }
            ]}
        ]));
        let (errors, warnings) = check_component_attrs(&s);
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    }

    #[test]
    fn nested_child_unknown_attribute_gets_nested_path() {
        let s = resolved(serde_json::json!([
            { "type": "card", "children": [
                { "type": "text", "content": "hi", "contnet": "typo" }
            ]}
        ]));
        let (_, warnings) = check_component_attrs(&s);
        assert_eq!(warnings.len(), 1, "expected one warning: {warnings:?}");
        assert!(
            warnings[0].contains("views[0].scenes[0].children[0].children[0]"),
            "got: {}",
            warnings[0]
        );
        assert!(
            warnings[0].contains("did you mean 'content'?"),
            "close match should be suggested: {}",
            warnings[0]
        );
    }

    #[test]
    fn alias_tags_use_target_schema() {
        // "container" is a serde alias of "div"; "progress_bar" of "progress".
        // Their unknown attributes must be checked against the target schema.
        let s = resolved(serde_json::json!([
            { "type": "container", "typo": "x", "children": [] }
        ]));
        let (_, warnings) = check_component_attrs(&s);
        assert_eq!(warnings.len(), 1, "expected one warning: {warnings:?}");
        assert!(warnings[0].contains("'typo'"), "got: {}", warnings[0]);
    }

    #[test]
    fn invalid_component_is_a_blocking_error() {
        // Missing required field `to`: today the child is silently dropped at
        // render; validation must surface it as an error.
        let s = resolved(serde_json::json!([
            { "type": "counter", "from": 0 }
        ]));
        let (errors, _) = check_component_attrs(&s);
        assert_eq!(errors.len(), 1, "expected one error: {errors:?}");
        assert!(errors[0].contains("counter"), "got: {}", errors[0]);
    }
}
