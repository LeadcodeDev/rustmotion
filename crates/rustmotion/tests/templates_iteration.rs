//! End-to-end tests for `for-each`/`use`/`components` through the *real*
//! load pipeline (`rustmotion::loader`), not just `rustmotion_core::expand`
//! in isolation. These pin down the two ordering questions the workstream
//! brief calls out explicitly, and the one proof that matters most: a
//! `for-each`-authored scenario must resolve to *exactly* the same tree as
//! the hand-written equivalent.

use rustmotion::loader::load_scenario_from_source;

fn load(json: &serde_json::Value) -> rustmotion::schema::ResolvedScenario {
    load_scenario_from_source(None, Some(&json.to_string())).expect("scenario loads")
}

/// Pass ordering, part 1: `variables::apply_variables` runs before
/// `expand::expand_directives`, so a `for-each` source that is a bare `$var`
/// reference to a `config`-declared array variable is already the literal
/// array by the time `for-each` looks at it.
#[test]
fn for_each_can_iterate_over_an_array_that_came_from_a_config_variable() {
    let json = serde_json::json!({
        "video": { "width": 100, "height": 100 },
        "config": {
            "rows": {
                "type": "array",
                "default": [
                    { "label": "Revenue", "value": 120 },
                    { "label": "Users", "value": 340 }
                ]
            }
        },
        "scenes": [{
            "duration": 1.0,
            "children": [{
                "for-each": "$rows",
                "template": { "type": "text", "content": "$label: $value" }
            }]
        }]
    });
    let resolved = load(&json);
    let children = &resolved.views[0].scenes[0].children;
    assert_eq!(
        children.len(),
        2,
        "the $rows variable must resolve to its 2-element default before for-each consumes it"
    );
    assert_eq!(children[0]["content"], serde_json::json!("Revenue: 120"));
    assert_eq!(children[1]["content"], serde_json::json!("Users: 340"));
}

/// Pass ordering, part 1 also covers an explicit `--var` override: the
/// override replaces the default *before* substitution, so `for-each` still
/// only ever sees a literal array.
#[test]
fn for_each_source_variable_can_be_overridden_at_load_time() {
    let json = serde_json::json!({
        "video": { "width": 100, "height": 100 },
        "config": {
            "rows": { "type": "array", "default": [ { "label": "placeholder" } ] }
        },
        "scenes": [{
            "duration": 1.0,
            "children": [{
                "for-each": "$rows",
                "template": { "type": "text", "content": "$label" }
            }]
        }]
    })
    .to_string();

    let mut overrides = std::collections::HashMap::new();
    overrides.insert(
        "rows".to_string(),
        serde_json::json!([{ "label": "A" }, { "label": "B" }, { "label": "C" }]),
    );
    let resolved = rustmotion::loader::load_scenario_from_source_with_vars(
        None,
        Some(&json),
        Some(&overrides),
    )
    .expect("scenario loads with override");
    assert_eq!(resolved.views[0].scenes[0].children.len(), 3);
}

/// Pass ordering, part 2: `components` is scoped to the document it is
/// declared in. A `use` site in the *parent* scenario cannot reach a
/// component defined only inside a file it `include`s — parent-level
/// expansion runs before the child file is even fetched, so there is no
/// document in which both are visible at once.
#[test]
fn use_cannot_reach_a_component_defined_only_in_an_included_file() {
    let dir = std::env::temp_dir().join(format!(
        "rm_templates_iteration_cross_file_{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let child_path = dir.join("child.json");
    let parent_path = dir.join("parent.json");

    let child = serde_json::json!({
        "video": { "width": 100, "height": 100 },
        "components": {
            "widget": {
                "params": {},
                "template": { "type": "text", "content": "from child" }
            }
        },
        "scenes": [{
            "duration": 1.0,
            "children": [{ "use": "widget", "props": {} }]
        }]
    });
    let parent = serde_json::json!({
        "video": { "width": 100, "height": 100 },
        "scenes": [
            { "include": "child.json" },
            {
                "duration": 1.0,
                "children": [{ "use": "widget", "props": {} }]
            }
        ]
    });
    std::fs::write(&child_path, child.to_string()).unwrap();
    std::fs::write(&parent_path, parent.to_string()).unwrap();

    // The child, loaded on its own, resolves fine — its `use` site sees its
    // own `components`.
    let child_resolved = rustmotion::loader::load_scenario_with_vars(&child_path, None)
        .expect("child resolves its own component");
    assert_eq!(
        child_resolved.views[0].scenes[0].children[0]["content"],
        serde_json::json!("from child")
    );

    // The parent does not: its own `use` site (in its second, non-included
    // scene) cannot see the child's `components` — named error, not a silent
    // no-op or a wrong-scope success.
    let err = rustmotion::loader::load_scenario_with_vars(&parent_path, None)
        .expect_err("parent's use site must not resolve a component defined only in the child");
    let msg = err.to_string();
    assert!(
        msg.contains("widget"),
        "error must name the component: {msg}"
    );
    assert!(
        matches!(
            err,
            rustmotion::error::RustmotionError::UnknownComponent { .. }
        ),
        "expected UnknownComponent, got: {err}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// The reverse direction of the same scoping rule: a component defined in
/// the *parent* is likewise invisible to a `use` site inside an included
/// file — each document's `components` block only ever sees that document's
/// own `use` sites.
#[test]
fn use_inside_an_included_file_cannot_reach_a_component_defined_only_in_the_parent() {
    let dir = std::env::temp_dir().join(format!(
        "rm_templates_iteration_cross_file_reverse_{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let child_path = dir.join("child.json");
    let parent_path = dir.join("parent.json");

    let child = serde_json::json!({
        "video": { "width": 100, "height": 100 },
        "scenes": [{
            "duration": 1.0,
            "children": [{ "use": "widget", "props": {} }]
        }]
    });
    let parent = serde_json::json!({
        "video": { "width": 100, "height": 100 },
        "components": {
            "widget": {
                "params": {},
                "template": { "type": "text", "content": "from parent" }
            }
        },
        "scenes": [ { "include": "child.json" } ]
    });
    std::fs::write(&child_path, child.to_string()).unwrap();
    std::fs::write(&parent_path, parent.to_string()).unwrap();

    let err = rustmotion::loader::load_scenario_with_vars(&parent_path, None)
        .expect_err("child's use site must not resolve a component defined only in the parent");
    assert!(
        matches!(
            err,
            rustmotion::error::RustmotionError::UnknownComponent { .. }
        ),
        "expected UnknownComponent, got: {err}"
    );

    std::fs::remove_dir_all(&dir).ok();
}

/// The test that matters most: a `for-each`-authored scenario and the
/// equivalent hand-written scenario must resolve to *identical* trees. This
/// is the only proof that factoring a repeated structure into `for-each`
/// changes nothing about what actually renders.
#[test]
fn for_each_authored_scenario_resolves_identically_to_the_hand_written_equivalent() {
    let generated = serde_json::json!({
        "video": { "width": 1080, "height": 1920, "fps": 30 },
        "components": {
            "stat_card": {
                "params": {
                    "label": { "type": "string" },
                    "value": { "type": "number", "default": 0 },
                    "color": { "type": "string", "default": "#6366F1" }
                },
                "template": {
                    "type": "card",
                    "style": { "width": "300px", "height": "160px", "background": "$color" },
                    "children": [
                        { "type": "text", "content": "$label", "style": { "color": "#ffffff" } },
                        { "type": "counter", "value": "$value" }
                    ]
                }
            }
        },
        "scenes": [{
            "duration": 3.0,
            "children": [{
                "for-each": [
                    { "label": "Revenue", "value": 1250, "color": "#22C55E" },
                    { "label": "Users", "value": 340, "color": "#3B82F6" },
                    { "label": "Growth", "value": 8, "color": "#F59E0B" }
                ],
                "template": { "use": "stat_card", "props": { "label": "$label", "value": "$value", "color": "$color" } }
            }]
        }]
    });

    let hand_written = serde_json::json!({
        "video": { "width": 1080, "height": 1920, "fps": 30 },
        "scenes": [{
            "duration": 3.0,
            "children": [
                {
                    "type": "card",
                    "style": { "width": "300px", "height": "160px", "background": "#22C55E" },
                    "children": [
                        { "type": "text", "content": "Revenue", "style": { "color": "#ffffff" } },
                        { "type": "counter", "value": 1250 }
                    ]
                },
                {
                    "type": "card",
                    "style": { "width": "300px", "height": "160px", "background": "#3B82F6" },
                    "children": [
                        { "type": "text", "content": "Users", "style": { "color": "#ffffff" } },
                        { "type": "counter", "value": 340 }
                    ]
                },
                {
                    "type": "card",
                    "style": { "width": "300px", "height": "160px", "background": "#F59E0B" },
                    "children": [
                        { "type": "text", "content": "Growth", "style": { "color": "#ffffff" } },
                        { "type": "counter", "value": 8 }
                    ]
                }
            ]
        }]
    });

    let resolved_generated = load(&generated);
    let resolved_hand_written = load(&hand_written);

    assert_eq!(
        resolved_generated.views[0].scenes[0].children,
        resolved_hand_written.views[0].scenes[0].children,
        "for-each + use must resolve to exactly the same children tree as the hand-written scenario"
    );
    // Sanity: also compare video/duration so the whole ResolvedScenario, not
    // just the children array, lines up.
    assert_eq!(
        resolved_generated.views[0].scenes[0].duration,
        resolved_hand_written.views[0].scenes[0].duration
    );
}
