use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use rustmotion::components::{ChildComponent, Component};
use rustmotion::schema::ResolvedScenario;

const WRAPPER_KEYS: &[&str] = &["position", "x", "y", "z-index", "animation", "bleed", "id"];

const TAG_ALIASES: &[(&str, &str)] = &[
    ("container", "div"),
    ("progress_bar", "progress"),
    ("card", "div"),
    ("flex", "div"),
    ("grid", "div"),
    ("positioned", "div"),
];

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

pub fn check_component_attrs(scenario: &ResolvedScenario) -> (Vec<String>, Vec<String>) {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    for (vi, view) in scenario.views.iter().enumerate() {
        for (si, scene) in view.scenes.iter().enumerate() {
            for (ci, child) in scene.children.iter().enumerate() {
                let path = format!("views[{vi}].scenes[{si}].children[{ci}]");
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

fn walk_component(value: &serde_json::Value, path: &str, warnings: &mut Vec<String>) {
    let Some(obj) = value.as_object() else {
        return;
    };
    let Some(tag) = obj.get("type").and_then(|t| t.as_str()) else {
        return;
    };
    let Some(known) = known_props().get(tag) else {
        return;
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
    use crate::cli::commands::validation::{load, run_checks, ValidationSource};

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
        assert!(warnings[0].contains("known:"), "got: {}", warnings[0]);
    }

    #[test]
    fn unknown_attr_blocks_by_default_without_strict_attrs() {
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
        let s = resolved(serde_json::json!([
            { "type": "container", "typo": "x", "children": [] }
        ]));
        let (_, warnings) = check_component_attrs(&s);
        assert_eq!(warnings.len(), 1, "expected one warning: {warnings:?}");
        assert!(warnings[0].contains("'typo'"), "got: {}", warnings[0]);
    }

    #[test]
    fn invalid_component_is_a_blocking_error() {
        let s = resolved(serde_json::json!([
            { "type": "counter", "from": 0 }
        ]));
        let (errors, _) = check_component_attrs(&s);
        assert_eq!(errors.len(), 1, "expected one error: {errors:?}");
        assert!(errors[0].contains("counter"), "got: {}", errors[0]);
    }

    #[test]
    fn typo_inside_style_animation_effect_is_reported() {
        let s = resolved(serde_json::json!([
            {
                "type": "text", "content": "hi",
                "style": { "animation": [{ "name": "fade_in_up", "duratoin": 0.6 }] }
            }
        ]));
        let (errors, _) = check_component_attrs(&s);
        assert!(
            errors.iter().any(|e| e.contains("duratoin")),
            "expected the typo'd animation-effect field to be reported as an error: {errors:?}"
        );
    }

    #[test]
    fn well_formed_animation_effect_fields_are_not_flagged() {
        let s = resolved(serde_json::json!([
            {
                "type": "text", "content": "hi",
                "style": { "animation": [
                    { "name": "fade_in_up", "delay": 0.2, "duration": 0.6, "loop": false,
                      "overshoot": 0.1, "spring": { "damping": 12, "stiffness": 100, "mass": 1 } },
                    { "name": "float_3d", "duration": 1.0, "amplitude": 20 },
                    { "name": "tilt_in", "delay": 0.0, "duration": 0.4, "rotate_x": 10.0 },
                    { "name": "wiggle", "property": "translate_y", "amplitude": 5, "frequency": 2, "seed": 3 },
                    { "name": "glow", "color": "#ffffff", "radius": 10, "intensity": 1.0 },
                    { "name": "motion_blur", "samples": 4, "shutter": 0.5 },
                    { "name": "keyframes", "keyframes": [
                        { "property": "opacity", "keyframes": [
                            { "time": 0.0, "value": 0.0 }, { "time": 1.0, "value": 1.0 }
                        ] }
                    ], "delay": 0.0, "duration": 1.0, "loop": true }
                ] }
            }
        ]));
        let (errors, warnings) = check_component_attrs(&s);
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    }
}
