use rustmotion::loader::load_input;

fn write_temp(name: &str, contents: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "rm_include_vars_{name}_{}.json",
        std::process::id()
    ));
    std::fs::write(&path, contents).unwrap();
    path
}

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
    assert!(scene.resolved_scenario_vars.contains_key("keyDraw"));

    let _ = std::fs::remove_file(&child_path);
    let _ = std::fs::remove_file(&parent_path);
}

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
