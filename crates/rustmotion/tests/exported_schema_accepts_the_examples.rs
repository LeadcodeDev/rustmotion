use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("rustmotion is expected at <workspace>/crates/rustmotion")
        .to_path_buf()
}

fn exported_schema() -> serde_json::Value {
    let out = Command::new(env!("CARGO_BIN_EXE_rustmotion"))
        .arg("schema")
        .output()
        .expect("run `rustmotion schema`");
    assert!(out.status.success(), "`rustmotion schema` failed");
    serde_json::from_slice(&out.stdout).expect("the exported schema is JSON")
}

fn compiled(schema: serde_json::Value) -> (boon::Schemas, boon::SchemaIndex) {
    let mut schemas = boon::Schemas::new();
    let mut compiler = boon::Compiler::new();
    compiler
        .add_resource("rustmotion.json", schema)
        .expect("the exported schema is a usable resource");
    let index = compiler
        .compile("rustmotion.json", &mut schemas)
        .expect("the exported schema compiles");
    (schemas, index)
}

fn example_scenarios() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(workspace_root().join("examples"))
        .expect("examples/ exists")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();
    files
}

#[test]
fn every_example_in_the_repository_validates_against_the_exported_schema() {
    let (schemas, index) = compiled(exported_schema());
    let files = example_scenarios();
    assert!(
        files.len() >= 10,
        "test setup: only {} examples found",
        files.len()
    );

    let mut rejected = Vec::new();
    for file in &files {
        let doc: serde_json::Value =
            serde_json::from_slice(&std::fs::read(file).expect("read example"))
                .expect("an example is JSON");
        if let Err(e) = schemas.validate(&doc, index) {
            let name = file.file_name().unwrap_or_default().to_string_lossy();
            rejected.push(format!("{name}: {e}"));
        }
    }

    assert!(
        rejected.is_empty(),
        "`rustmotion schema` is what a generator reads to know what to write, so it must not \
         declare invalid what this repository ships and `rustmotion validate` accepts:\n\n{}",
        rejected.join("\n\n")
    );
}

#[test]
fn a_placeholder_is_only_accepted_where_it_looks_like_one() {
    let (schemas, index) = compiled(exported_schema());
    let scenario = |delay: serde_json::Value| {
        serde_json::json!({
            "video": { "width": 64, "height": 64, "fps": 30 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "text",
                    "content": "x",
                    "style": { "animation": [{ "name": "fade_in", "delay": delay }] }
                }]
            }]
        })
    };

    for accepted in [serde_json::json!(0.4), serde_json::json!("$delay")] {
        assert!(
            schemas.validate(&scenario(accepted.clone()), index).is_ok(),
            "{accepted} is what the engine takes for a delay"
        );
    }
    for refused in [serde_json::json!("soon"), serde_json::json!("$")] {
        assert!(
            schemas.validate(&scenario(refused.clone()), index).is_err(),
            "{refused} is not a number and not a placeholder: widening a scalar must not turn \
             it into a free-form string"
        );
    }
}

#[test]
fn a_component_tag_is_not_widened_into_a_placeholder() {
    let (schemas, index) = compiled(exported_schema());
    let with_type = |tag: serde_json::Value| {
        serde_json::json!({
            "video": { "width": 64, "height": 64, "fps": 30 },
            "scenes": [{ "duration": 1.0, "children": [{ "type": tag, "content": "x" }] }]
        })
    };
    assert!(schemas
        .validate(&with_type(serde_json::json!("text")), index)
        .is_ok());
    assert!(
        schemas
            .validate(&with_type(serde_json::json!("nonesuch")), index)
            .is_err(),
        "the `type` tag is an enum, and widening it would cost the discriminator every \
         validator uses to report inside the right branch"
    );
}

fn rejection_of(child: serde_json::Value) -> String {
    let (schemas, index) = compiled(exported_schema());
    let doc = serde_json::json!({
        "video": { "width": 64, "height": 64, "fps": 30 },
        "scenes": [{ "duration": 1.0, "children": [child] }]
    });
    let err = schemas
        .validate(&doc, index)
        .expect_err("this scenario must be rejected");
    format!("{err:#}")
}

#[test]
fn a_rejected_component_is_reported_at_the_property_that_is_wrong() {
    let typo = rejection_of(serde_json::json!({
        "type": "text",
        "content": "hi",
        "style": { "animation": [{ "name": "fade_in_upp" }] }
    }));
    assert!(
        typo.contains("fade_in_up"),
        "the report must name the spelling that was meant, not just fail the whole \
         component:\n{typo}"
    );
    assert!(
        typo.contains("animation"),
        "the report must reach the property that is wrong:\n{typo}"
    );
}

#[test]
fn a_rejected_directive_is_reported_as_that_directive() {
    let incomplete = rejection_of(serde_json::json!({ "for-each": [{ "a": 1 }] }));
    assert!(
        incomplete.contains("template"),
        "a `for-each` without its `template` must be reported as the missing key, not as a \
         component that matched no branch:\n{incomplete}"
    );

    let misspelt = rejection_of(serde_json::json!({ "use": "card", "propz": {} }));
    assert!(
        misspelt.contains("propz"),
        "the overrides key is `props`; naming the one that was written is the whole value of \
         discriminating on `use`:\n{misspelt}"
    );
}
