use rustmotion::components::Component;
use rustmotion::error::Result;
use rustmotion::schema;

pub fn cmd_schema(output: Option<&std::path::Path>) -> Result<()> {
    let schema = build_schema();
    let json = serde_json::to_string_pretty(&schema)?;

    if let Some(path) = output {
        std::fs::write(path, &json)?;
        eprintln!("Schema written to {}", path.display());
    } else {
        println!("{}", json);
    }

    Ok(())
}

const SERDE_ALIASES: &[(&str, &str, &[&str])] = &[
    ("CardAlign", "start", &["flex-start", "flex_start"]),
    ("CardAlign", "end", &["flex-end", "flex_end"]),
    ("CardJustify", "start", &["flex-start", "flex_start"]),
    ("CardJustify", "end", &["flex-end", "flex_end"]),
    ("CardJustify", "space_between", &["space-between"]),
    ("CardJustify", "space_around", &["space-around"]),
    ("CardJustify", "space_evenly", &["space-evenly"]),
    ("AnimationPreset", "float3d", &["float_3d"]),
    ("AnimationEffect", "float3d", &["float_3d"]),
    ("Component", "progress", &["progress_bar"]),
    ("ComponentBase", "progress", &["progress_bar"]),
    ("ChildComponent", "progress", &["progress_bar"]),
    ("ChildComponentBase", "progress", &["progress_bar"]),
];

fn widen_enums_with(value: &mut serde_json::Value, canonical: &str, aliases: &[&str]) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::Array(variants)) = map.get_mut("enum") {
                if variants.iter().any(|v| v.as_str() == Some(canonical)) {
                    for alias in aliases {
                        let alias = serde_json::Value::String((*alias).to_string());
                        if !variants.contains(&alias) {
                            variants.push(alias);
                        }
                    }
                }
            }
            for (_, child) in map.iter_mut() {
                widen_enums_with(child, canonical, aliases);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                widen_enums_with(item, canonical, aliases);
            }
        }
        _ => {}
    }
}

const SERDE_FIELD_ALIASES: &[(&str, &str, &[&str])] = &[
    ("BorderRadius", "top-left", &["top_left"]),
    ("BorderRadius", "top-right", &["top_right"]),
    ("BorderRadius", "bottom-right", &["bottom_right"]),
    ("BorderRadius", "bottom-left", &["bottom_left"]),
];

fn widen_properties_with(value: &mut serde_json::Value, canonical: &str, aliases: &[&str]) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::Object(properties)) = map.get_mut("properties") {
                if let Some(schema) = properties.get(canonical).cloned() {
                    for alias in aliases {
                        properties
                            .entry((*alias).to_string())
                            .or_insert_with(|| schema.clone());
                    }
                }
            }
            for (key, child) in map.iter_mut() {
                if key != "properties" {
                    widen_properties_with(child, canonical, aliases);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                widen_properties_with(item, canonical, aliases);
            }
        }
        _ => {}
    }
}

fn expose_serde_aliases(defs: &mut serde_json::Map<String, serde_json::Value>) {
    for (definition, canonical, aliases) in SERDE_ALIASES {
        if let Some(entry) = defs.get_mut(*definition) {
            widen_enums_with(entry, canonical, aliases);
        }
    }
    for (definition, canonical, aliases) in SERDE_FIELD_ALIASES {
        if let Some(entry) = defs.get_mut(*definition) {
            widen_properties_with(entry, canonical, aliases);
        }
    }
}

fn build_schema() -> serde_json::Value {
    let mut scenario_schema = schema::generate_json_schema();
    let component_schema = serde_json::to_value(schemars::schema_for!(Component))
        .expect("Component schema serializes");

    let Some(component_defs) = component_schema
        .get("definitions")
        .and_then(|d| d.as_object())
    else {
        return scenario_schema;
    };

    let Some(scenario_obj) = scenario_schema.as_object_mut() else {
        return scenario_schema;
    };
    let defs = scenario_obj
        .entry("definitions")
        .or_insert_with(|| serde_json::Value::Object(Default::default()));
    let Some(defs_obj) = defs.as_object_mut() else {
        return scenario_schema;
    };

    for (k, v) in component_defs {
        defs_obj.entry(k.clone()).or_insert_with(|| v.clone());
    }

    let mut component_root = component_schema.clone();
    if let Some(obj) = component_root.as_object_mut() {
        obj.remove("definitions");
        obj.remove("$schema");
    }
    defs_obj.insert("Component".to_string(), component_root);

    inject_for_each_use_definitions(defs_obj);

    wrap_with_directives(defs_obj, "Component");
    wrap_with_directives(defs_obj, "ChildComponent");
    expose_serde_aliases(defs_obj);

    if let Some(children) = scenario_schema.pointer_mut("/definitions/Scene/properties/children") {
        *children = serde_json::json!({
            "type": "array",
            "items": { "$ref": "#/definitions/Component" }
        });
    }

    if let Some(template) =
        scenario_schema.pointer_mut("/definitions/ComponentTemplateDef/properties/template")
    {
        *template = serde_json::json!({ "$ref": "#/definitions/TemplateValue" });
    }

    scenario_schema
}

fn wrap_with_directives(defs_obj: &mut serde_json::Map<String, serde_json::Value>, name: &str) {
    let Some(original) = defs_obj.remove(name) else {
        return;
    };
    let base_name = format!("{name}Base");
    defs_obj.insert(base_name.clone(), original);
    defs_obj.insert(
        name.to_string(),
        serde_json::json!({
            "description": "A concrete component, or a `for-each`/`use` directive that expands \
                into one or more components before rendering — see CLAUDE.md's \
                \"Factorisation\" section.",
            "anyOf": [
                { "$ref": format!("#/definitions/{base_name}") },
                { "$ref": "#/definitions/ForEachDirective" },
                { "$ref": "#/definitions/UseDirective" }
            ]
        }),
    );
}

fn inject_for_each_use_definitions(defs_obj: &mut serde_json::Map<String, serde_json::Value>) {
    defs_obj.insert(
        "TemplateValue".to_string(),
        serde_json::json!({
            "description": "A `template`'s value (`for-each`'s own, or a `components[name]` \
                entry's): a single child entry, or an array of sibling child entries spliced in \
                place. May itself nest another `for-each`/`use`.",
            "anyOf": [
                { "$ref": "#/definitions/Component" },
                {
                    "type": "array",
                    "items": { "$ref": "#/definitions/Component" }
                }
            ]
        }),
    );
    defs_obj.insert(
        "ForEachDirective".to_string(),
        serde_json::json!({
            "type": "object",
            "description": "Repeats `template` once per element of an array, binding each \
                element's own fields directly (plus `$index` and `$item`) into it.",
            "properties": {
                "for-each": {
                    "description": "The array to iterate: a literal JSON array, or a \
                        `$variable` reference to a `config`-declared array.",
                    "anyOf": [
                        { "type": "array" },
                        { "type": "string" }
                    ]
                },
                "template": { "$ref": "#/definitions/TemplateValue" }
            },
            "required": ["for-each", "template"],
            "additionalProperties": false
        }),
    );
    defs_obj.insert(
        "UseDirective".to_string(),
        serde_json::json!({
            "type": "object",
            "description": "Instantiates a named template declared in the top-level \
                `components` block.",
            "properties": {
                "use": {
                    "type": "string",
                    "description": "Name of the `components` entry to instantiate."
                },
                "props": {
                    "type": "object",
                    "description": "Overrides bound into the template's `$name` placeholders \
                        — deliberately not named `config`, which is reserved for the \
                        scenario-level declarations block (see `expand.rs`'s module doc)."
                }
            },
            "required": ["use"],
            "additionalProperties": false
        }),
    );
}

#[cfg(test)]
mod serde_alias_exposure_tests {
    use super::*;

    fn workspace_root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .expect("rustmotion is expected at <workspace>/crates/rustmotion")
            .to_path_buf()
    }

    fn rust_sources(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|n| n == "target") {
                    continue;
                }
                rust_sources(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }

    fn aliases_declared_in_the_sources() -> Vec<(String, String)> {
        let mut files = Vec::new();
        rust_sources(&workspace_root().join("crates"), &mut files);
        let mut found = Vec::new();
        for file in files {
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            for line in text.lines() {
                let trimmed = line.trim_start();
                if !trimmed.starts_with("#[serde(") || !trimmed.contains("alias = \"") {
                    continue;
                }
                let mut rest = trimmed;
                while let Some(at) = rest.find("alias = \"") {
                    rest = &rest[at + "alias = \"".len()..];
                    let Some(close) = rest.find('"') else { break };
                    found.push((file.display().to_string(), rest[..close].to_string()));
                    rest = &rest[close..];
                }
            }
        }
        found
    }

    #[test]
    fn every_serde_alias_in_the_sources_is_reachable_from_the_exported_schema() {
        let declared = aliases_declared_in_the_sources();
        assert!(
            declared.len() >= 10,
            "test setup: the scanner found only {} aliases, so it is not reading the sources",
            declared.len()
        );

        let schema = serde_json::to_string(&build_schema()).expect("schema serializes");
        let missing: Vec<String> = declared
            .iter()
            .filter(|(_, alias)| !schema.contains(&format!("\"{alias}\"")))
            .map(|(file, alias)| format!("{alias} (declared in {file})"))
            .collect();

        assert!(
            missing.is_empty(),
            "schemars does not emit #[serde(alias = ...)], so the exported schema declares \
             invalid what the engine accepts. Add each of these to SERDE_ALIASES or \
             SERDE_FIELD_ALIASES in this file:\n  {}",
            missing.join("\n  ")
        );
    }

    #[test]
    fn the_canonical_spelling_is_never_replaced_by_its_alias() {
        let schema = build_schema();
        let card_align = schema
            .pointer("/definitions/CardAlign/enum")
            .and_then(|v| v.as_array())
            .expect("CardAlign is an enum of strings");
        let values: Vec<&str> = card_align.iter().filter_map(|v| v.as_str()).collect();
        assert!(
            values.contains(&"start") && values.contains(&"flex-start"),
            "an alias is widened onto the canonical spelling, never swapped for it: {values:?}"
        );
    }

    #[test]
    fn an_enum_that_shares_a_variant_name_but_not_its_aliases_is_left_alone() {
        let schema = build_schema();
        let text = serde_json::to_string(&schema).expect("schema serializes");
        assert!(
            text.contains("\"space-between\""),
            "CardJustify's own aliases are there"
        );
        let justify_content = schema
            .pointer("/definitions/JustifyContent/enum")
            .and_then(|v| v.as_array());
        if let Some(values) = justify_content {
            let values: Vec<&str> = values.iter().filter_map(|v| v.as_str()).collect();
            assert!(
                !values.contains(&"flex_start"),
                "the table is keyed by definition, not by value, so a different enum with a \
                 variant of the same name must not inherit aliases it does not have: {values:?}"
            );
        }
    }
}
