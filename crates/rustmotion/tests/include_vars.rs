//! Issue #329's fold guard, exercised through the actual `include` pipeline
//! (not just `loader::fold_static_expressions` directly): an included
//! file's own `vars` block must protect its own expressions the same way a
//! top-level scenario's does, and must survive into the merged
//! `ResolvedScenario`'s `Scene` fields.
//!
//! `include.rs`'s own `resolve_entries` calls
//! `crate::loader::fold_static_expressions` on each included file's JSON
//! value independently (see that call site's doc comment) — this is a
//! *second*, separate fold pass from the parent document's own, so the
//! `vars`-aware guard has to hold there too, not just in the parent.

use rustmotion::loader::load_input;

fn write_temp(name: &str, contents: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "rm_include_vars_{name}_{}.json",
        std::process::id()
    ));
    std::fs::write(&path, contents).unwrap();
    path
}

/// An included file that declares its own animated `vars` and reads it
/// from an expression must load cleanly through the parent — the
/// expression must survive unfolded, not error with "unknown identifier"
/// and not silently freeze.
#[test]
fn included_file_s_animated_var_is_left_unfolded() {
    let child_path = write_temp(
        "child",
        &serde_json::json!({
            "video": { "width": 100, "height": 100 },
            "vars": { "keyDraw": { "default": 0,
                "animation": [{ "at": "1s", "to": 1, "duration": "1s" }] } },
            "scenes": [{
                "duration": 2.0,
                "children": [
                    { "type": "text", "content": "c", "opacity": "= $keyDraw" }
                ]
            }]
        })
        .to_string(),
    );
    let parent_path = write_temp(
        "parent",
        &serde_json::json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{ "include": child_path.file_name().unwrap().to_str().unwrap() }]
        })
        .to_string(),
    );

    let resolved = load_input(&parent_path).expect("parent with included vars-using file loads");
    let scene = &resolved.views[0].scenes[0];
    assert_eq!(
        scene.children[0]["opacity"],
        serde_json::json!("= $keyDraw"),
        "the included file's own dynamic var reference must survive unfolded"
    );
    // The included file's own `vars` propagate onto its own scene exactly
    // like a top-level scenario's would (Scene::resolved_scenario_vars) —
    // see `rustmotion_core::schema::scenario::Scenario::propagate_time_ctx`'s
    // doc: an included file is deserialized as its own `Scenario`, so this
    // is *that* file's own declared grid/vars, not the parent's.
    assert!(scene.resolved_scenario_vars.contains_key("keyDraw"));

    let _ = std::fs::remove_file(&child_path);
    let _ = std::fs::remove_file(&parent_path);
}

/// An included file's own scenario-level *constant* `vars` entry folds to
/// a literal, exactly like a top-level scenario's would.
#[test]
fn included_file_s_constant_var_folds_to_a_literal() {
    let child_path = write_temp(
        "child_const",
        &serde_json::json!({
            "video": { "width": 100, "height": 100 },
            "vars": { "badgeCount": { "default": 8 } },
            "scenes": [{
                "duration": 1.0,
                "children": [
                    { "type": "text", "content": "c", "opacity": "= $badgeCount / 8" }
                ]
            }]
        })
        .to_string(),
    );
    let parent_path = write_temp(
        "parent_const",
        &serde_json::json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{ "include": child_path.file_name().unwrap().to_str().unwrap() }]
        })
        .to_string(),
    );

    let resolved = load_input(&parent_path).expect("parent with included const-var file loads");
    let scene = &resolved.views[0].scenes[0];
    assert_eq!(scene.children[0]["opacity"], serde_json::json!(1.0));

    let _ = std::fs::remove_file(&child_path);
    let _ = std::fs::remove_file(&parent_path);
}
