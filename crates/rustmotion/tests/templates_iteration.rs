use rustmotion::loader::load_scenario_from_source;

fn load(json: &serde_json::Value) -> rustmotion::schema::ResolvedScenario {
    load_scenario_from_source(None, Some(&json.to_string())).expect("scenario loads")
}

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

    let child_resolved = rustmotion::loader::load_scenario_with_vars(&child_path, None)
        .expect("child resolves its own component");
    assert_eq!(
        child_resolved.views[0].scenes[0].children[0]["content"],
        serde_json::json!("from child")
    );

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
    assert_eq!(
        resolved_generated.views[0].scenes[0].duration,
        resolved_hand_written.views[0].scenes[0].duration
    );
}

#[test]
fn an_included_files_own_expressions_are_folded_too() {
    let dir = std::env::temp_dir().join(format!(
        "rm_templates_iteration_include_expr_{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let child_path = dir.join("child_expr.json");
    let parent_path = dir.join("parent_expr.json");

    let child = serde_json::json!({
        "video": { "width": 1080, "height": 1920 },
        "scenes": [{
            "duration": 1.0,
            "children": [{
                "for-each": [1, 2, 3, 4],
                "template": {
                    "type": "text",
                    "content": "badge",
                    "position": "absolute",
                    "x": "= $W/2 + cos($i / $count * TAU) * 200"
                }
            }]
        }]
    });
    let parent = serde_json::json!({
        "video": { "width": 1080, "height": 1920 },
        "scenes": [{ "include": "child_expr.json" }]
    });
    std::fs::write(&child_path, child.to_string()).unwrap();
    std::fs::write(&parent_path, parent.to_string()).unwrap();

    let resolved = rustmotion::loader::load_scenario_with_vars(&parent_path, None)
        .expect("parent including child with expressions resolves");
    let children = &resolved.views[0].scenes[0].children;
    assert_eq!(children.len(), 4);
    for (i, child) in children.iter().enumerate() {
        let x = child["x"]
            .as_f64()
            .unwrap_or_else(|| panic!("child {i}'s x must fold to a number, got {:?}", child["x"]));
        let want = 1080.0 / 2.0 + (i as f64 / 4.0 * std::f64::consts::TAU).cos() * 200.0;
        assert!((x - want).abs() < 1e-9, "child {i}: got {x}, want {want}");
    }

    let _ = std::fs::remove_file(&child_path);
    let _ = std::fs::remove_file(&parent_path);
    let _ = std::fs::remove_dir(&dir);
}
