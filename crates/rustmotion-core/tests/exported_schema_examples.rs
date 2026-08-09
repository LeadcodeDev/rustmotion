//! Constat #5: `rustmotion schema` used to export a `Scene`/`View` with
//! `additionalProperties: false` (from `deny_unknown_fields`) but no
//! `background` property (from `#[schemars(skip)]` on a field with no
//! `JsonSchema` impl for its type). The two combined meant the exported
//! schema declared *every* scenario in `examples/` that uses `background`
//! invalid — the schema is exactly what generators (LLMs) are meant to
//! target, so this is the sink 3 users can't work around.
//!
//! This test is a minimal, dependency-free (no external JSON-Schema crate —
//! adding one is out of this workstream's file scope) structural validator:
//! it understands exactly the subset of JSON Schema draft-07 that
//! `schemars` 0.8 actually emits for this codebase (`$ref`, `definitions`,
//! `type`, `properties`/`additionalProperties`/`required`, `items`,
//! `oneOf`/`anyOf`/`allOf`, `enum`). It is not a general-purpose validator,
//! but it is precise about the one thing constat #5 is about:
//! `additionalProperties: false` combined with a missing declared property.

use serde_json::Value;
use std::collections::BTreeSet;
use std::path::PathBuf;

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

/// Resolve a `$ref` like `#/definitions/Scene` against the schema root.
fn resolve<'a>(root: &'a Value, ref_str: &str) -> &'a Value {
    let path = ref_str.strip_prefix("#/").unwrap_or(ref_str);
    let mut cur = root;
    for part in path.split('/') {
        cur = cur
            .get(part)
            .unwrap_or_else(|| panic!("dangling $ref segment '{part}' in '{ref_str}'"));
    }
    cur
}

/// Validate `instance` against `schema` (a node within `root`). Appends a
/// human-readable message to `errors` for every violation found, prefixed
/// with `path`. This intentionally does not stop at the first violation
/// (matches how the equivalent Python `jsonschema` run was cross-checked).
fn check(root: &Value, schema: &Value, instance: &Value, path: &str, errors: &mut Vec<String>) {
    // `true` / `{}` accept anything.
    if schema.as_bool() == Some(true) {
        return;
    }
    if let Some(obj) = schema.as_object() {
        if obj.is_empty() {
            return;
        }
    }

    if let Some(r) = schema.get("$ref").and_then(|v| v.as_str()) {
        check(root, resolve(root, r), instance, path, errors);
        return;
    }

    if let Some(all_of) = schema.get("allOf").and_then(|v| v.as_array()) {
        for sub in all_of {
            check(root, sub, instance, path, errors);
        }
    }

    if let Some(variants) = schema
        .get("oneOf")
        .or_else(|| schema.get("anyOf"))
        .and_then(|v| v.as_array())
    {
        let mut best: Option<Vec<String>> = None;
        for variant in variants {
            let mut sub_errors = Vec::new();
            check(root, variant, instance, path, &mut sub_errors);
            if sub_errors.is_empty() {
                return; // one matching variant is enough
            }
            if best.as_ref().is_none_or(|b| sub_errors.len() < b.len()) {
                best = Some(sub_errors);
            }
        }
        if let Some(b) = best {
            errors.push(format!(
                "{path}: matched no oneOf/anyOf variant (closest variant errors: {b:?})"
            ));
        }
        return;
    }

    if let Some(expected) = schema.get("enum").and_then(|v| v.as_array()) {
        if !expected.contains(instance) {
            errors.push(format!("{path}: {instance} is not one of {expected:?}"));
        }
        return;
    }

    if let Some(ty) = schema.get("type").and_then(|v| v.as_str()) {
        let matches = match ty {
            "object" => instance.is_object(),
            "array" => instance.is_array(),
            "string" => instance.is_string(),
            "number" => instance.is_number(),
            "integer" => instance.is_i64() || instance.is_u64(),
            "boolean" => instance.is_boolean(),
            "null" => instance.is_null(),
            _ => true,
        };
        if !matches {
            errors.push(format!("{path}: expected type {ty}, got {instance}"));
            return;
        }
    }

    if let Some(props) = schema.get("properties").and_then(|v| v.as_object()) {
        if let Some(inst_obj) = instance.as_object() {
            let required: BTreeSet<&str> = schema
                .get("required")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();
            for key in &required {
                if !inst_obj.contains_key(*key) {
                    errors.push(format!("{path}: missing required field '{key}'"));
                }
            }
            let additional = schema.get("additionalProperties");
            for (k, v) in inst_obj {
                if let Some(sub_schema) = props.get(k) {
                    check(root, sub_schema, v, &format!("{path}/{k}"), errors);
                } else {
                    match additional {
                        Some(Value::Bool(false)) => {
                            errors.push(format!(
                                "{path}: additional property '{k}' is not allowed by the schema \
                                 (declared properties: {:?})",
                                props.keys().collect::<Vec<_>>()
                            ));
                        }
                        Some(Value::Bool(true)) | None => {}
                        Some(sub_schema) => {
                            check(root, sub_schema, v, &format!("{path}/{k}"), errors);
                        }
                    }
                }
            }
        }
    }

    if let Some(items_schema) = schema.get("items") {
        if let Some(arr) = instance.as_array() {
            for (i, item) in arr.iter().enumerate() {
                check(root, items_schema, item, &format!("{path}[{i}]"), errors);
            }
        }
    }
}

/// Every `examples/*.json` file must validate against the schema
/// `rustmotion schema` exports (i.e. `generate_json_schema()` — the CLI
/// command only additionally wires `Scene.children` to the `Component`
/// union, which is irrelevant to constat #5's `background` defect and out
/// of this workstream's file scope to reproduce here).
///
/// `ferriskey-presentation.json` is excluded: it fails plain `rustmotion
/// validate` today for an unrelated, pre-existing geometry overflow (issue
/// #157, out of this workstream's scope) — but per the baseline run below,
/// it has zero *schema* violations even before this fix, so excluding it
/// from the loop changes nothing about what this test proves.
#[test]
fn all_examples_validate_against_the_exported_schema() {
    let schema = rustmotion_core::schema::generate_json_schema();
    let mut failures = Vec::new();

    let mut count = 0;
    for entry in std::fs::read_dir(examples_dir()).expect("examples/ dir must exist") {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        count += 1;
        let raw = std::fs::read_to_string(&path).unwrap();
        let doc: Value = serde_json::from_str(&raw).unwrap();
        let mut errors = Vec::new();
        check(
            &schema,
            &schema,
            &doc,
            path.file_name().unwrap().to_str().unwrap(),
            &mut errors,
        );
        if !errors.is_empty() {
            failures.push(format!(
                "{}: {} violation(s), first: {}",
                path.display(),
                errors.len(),
                errors[0]
            ));
        }
    }

    assert!(
        count >= 8,
        "expected at least 8 example files, found {count}"
    );
    assert!(
        failures.is_empty(),
        "the following examples/*.json fail to validate against `rustmotion schema`'s output:\n{}",
        failures.join("\n")
    );
}

/// Narrower, more direct regression lock for the exact defect: `Scene` and
/// `View` must both declare `background` as a property in the exported
/// schema. Kept alongside the full-document check above because this is
/// the precise structural fact constat #5 is about, independent of whatever
/// else may be in the document.
#[test]
fn scene_and_view_schema_both_declare_a_background_property() {
    let schema = rustmotion_core::schema::generate_json_schema();
    for name in ["Scene", "View"] {
        let def = schema
            .pointer(&format!("/definitions/{name}"))
            .unwrap_or_else(|| panic!("no definitions/{name} in exported schema"));
        assert_eq!(
            def.get("additionalProperties"),
            Some(&Value::Bool(false)),
            "{name} must still be closed to unknown fields (deny_unknown_fields)"
        );
        assert!(
            def.pointer("/properties/background").is_some(),
            "{name} must declare `background` as a property — it is a real, accepted field \
             (deserialize_background_value), not schemars(skip)-worthy dead schema"
        );
    }
}
