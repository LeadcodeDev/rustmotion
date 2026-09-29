use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("rustmotion is expected at <workspace>/crates/rustmotion")
        .to_path_buf()
}

fn skill_documents() -> Vec<PathBuf> {
    let skills = workspace_root().join("crates/rustmotion/skills");
    let mut files = vec![skills.join("SKILL.md")];
    let mut rules: Vec<PathBuf> = std::fs::read_dir(skills.join("rules"))
        .expect("rules directory")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "md"))
        .collect();
    rules.sort();
    files.extend(rules);
    files
}

struct Example {
    file: String,
    line: usize,
    body: String,
    marked_bad: bool,
}

fn json_examples(path: &Path) -> Vec<Example> {
    let text = std::fs::read_to_string(path).expect("read skill document");
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("?")
        .to_string();
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while let Some(open) = text[cursor..].find("```json\n") {
        let start = cursor + open + "```json\n".len();
        let Some(close) = text[start..].find("```") else {
            break;
        };
        let end = start + close;
        let preceding = &text[..cursor + open];
        out.push(Example {
            file: name.clone(),
            line: preceding.matches('\n').count() + 1,
            body: text[start..end].to_string(),
            marked_bad: preceding
                .rsplit("```")
                .next()
                .is_some_and(|tail| tail.contains("**BAD")),
        });
        cursor = end + 3;
    }
    out
}

const TRANSITION_TYPES: &[&str] = &[
    "fade",
    "dissolve",
    "slide",
    "wipe",
    "wipe_up",
    "wipe_down",
    "wipe_left",
    "wipe_right",
    "corner_reveal",
    "pixel_dissolve",
    "chromatic_wipe",
    "iris",
    "zoom",
    "flip",
    "zoom_blur",
    "whip",
    "mask",
    "blob",
    "camera_pan",
];

const ERRORS_THE_WRAPPER_CAUSES: &[&str] =
    &["file not found", "but scene_duration is", "Failed to read"];

fn as_scenario(value: &serde_json::Value) -> Option<serde_json::Value> {
    let object = value.as_object()?;
    if object.contains_key("scenes") || object.contains_key("composition") {
        let mut doc = value.clone();
        if !object.contains_key("video") {
            doc.as_object_mut()?.insert(
                "video".into(),
                serde_json::json!({ "width": 640, "height": 360, "fps": 30 }),
            );
        }
        return Some(doc);
    }
    let kind = object.get("type")?.as_str()?;
    if TRANSITION_TYPES.contains(&kind) {
        return None;
    }
    Some(serde_json::json!({
        "video": { "width": 640, "height": 360, "fps": 30 },
        "scenes": [{ "duration": 6.0, "children": [value] }]
    }))
}

#[test]
fn every_json_example_in_the_skill_documents_still_validates() {
    let mut checked = 0usize;
    let mut broken: Vec<String> = Vec::new();

    for path in skill_documents() {
        for example in json_examples(&path) {
            if example.marked_bad
                || example.body.contains('…')
                || example.body.contains("...")
                || example.body.contains("//")
            {
                continue;
            }
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&example.body) else {
                continue;
            };
            let Some(scenario) = as_scenario(&value) else {
                continue;
            };
            checked += 1;

            let source = scenario.to_string();
            let loaded = match rustmotion::loader::load_scenario_from_source(None, Some(&source)) {
                Ok(s) => s,
                Err(e) => {
                    broken.push(format!("{}:{} — {e}", example.file, example.line));
                    continue;
                }
            };
            let (schema_errors, _) = rustmotion::cli::validate_scenario(&loaded);
            let (attr_errors, _) = rustmotion::cli::check_component_attrs(&loaded);
            for error in schema_errors.into_iter().chain(attr_errors) {
                if ERRORS_THE_WRAPPER_CAUSES
                    .iter()
                    .any(|ignored| error.contains(ignored))
                {
                    continue;
                }
                broken.push(format!("{}:{} — {error}", example.file, example.line));
            }
        }
    }

    assert!(
        checked > 80,
        "only {checked} examples were reachable, so this test is not reading the documents"
    );
    assert!(
        broken.is_empty(),
        "a generating model reads these documents as the source of truth, and {} example(s) \
         no longer validate:\n  {}",
        broken.len(),
        broken.join("\n  ")
    );
}
