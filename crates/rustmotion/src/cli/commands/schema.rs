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
    ("ComponentBase", "progress", &["progress_bar"]),
    ("ChildComponentBase", "progress", &["progress_bar"]),
    (
        "ComponentBase",
        "div",
        &["container", "card", "flex", "grid", "positioned"],
    ),
    (
        "ChildComponentBase",
        "div",
        &["container", "card", "flex", "grid", "positioned"],
    ),
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
    ("GradientTextStop", "position", &["offset"]),
    ("FilterFn", "radius-x", &["radius_x"]),
    ("FilterFn", "radius-y", &["radius_y"]),
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

    struct DeclaredAlias {
        file: String,
        alias: String,
        renamed_sibling: Option<String>,
    }

    fn quoted_value_after(attribute: &str, key: &str) -> Option<String> {
        let needle = format!("{key} = \"");
        let at = attribute.find(&needle)?;
        let rest = &attribute[at + needle.len()..];
        let close = rest.find('"')?;
        Some(rest[..close].to_string())
    }

    fn serde_attributes(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut rest = text;
        while let Some(at) = rest.find("#[serde(") {
            rest = &rest[at..];
            let mut depth = 0usize;
            let mut end = None;
            for (i, c) in rest.char_indices() {
                match c {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = Some(i + 1);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let Some(end) = end else { break };
            out.push(rest[..end].split_whitespace().collect::<Vec<_>>().join(" "));
            rest = &rest[end..];
        }
        out
    }

    fn aliases_declared_in_the_sources() -> Vec<DeclaredAlias> {
        let mut files = Vec::new();
        rust_sources(&workspace_root().join("crates"), &mut files);
        let mut found = Vec::new();
        for file in files {
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            for attribute in serde_attributes(&text) {
                if !attribute.contains("alias = \"") {
                    continue;
                }
                let renamed_sibling = quoted_value_after(&attribute, "rename");
                let mut rest = attribute.as_str();
                while let Some(at) = rest.find("alias = \"") {
                    rest = &rest[at + "alias = \"".len()..];
                    let Some(close) = rest.find('"') else { break };
                    found.push(DeclaredAlias {
                        file: file.display().to_string(),
                        alias: rest[..close].to_string(),
                        renamed_sibling: renamed_sibling.clone(),
                    });
                    rest = &rest[close..];
                }
            }
        }
        found
    }

    fn some_enum_array_carries_both(
        value: &serde_json::Value,
        canonical: &str,
        alias: &str,
    ) -> bool {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(serde_json::Value::Array(variants)) = map.get("enum") {
                    let has = |want: &str| variants.iter().any(|v| v.as_str() == Some(want));
                    if has(canonical) && has(alias) {
                        return true;
                    }
                }
                map.values()
                    .any(|child| some_enum_array_carries_both(child, canonical, alias))
            }
            serde_json::Value::Array(items) => items
                .iter()
                .any(|item| some_enum_array_carries_both(item, canonical, alias)),
            _ => false,
        }
    }

    fn some_properties_object_carries_both(
        value: &serde_json::Value,
        canonical: &str,
        alias: &str,
    ) -> bool {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(serde_json::Value::Object(properties)) = map.get("properties") {
                    if properties.contains_key(canonical) && properties.contains_key(alias) {
                        return true;
                    }
                }
                map.values()
                    .any(|child| some_properties_object_carries_both(child, canonical, alias))
            }
            serde_json::Value::Array(items) => items
                .iter()
                .any(|item| some_properties_object_carries_both(item, canonical, alias)),
            _ => false,
        }
    }

    #[test]
    fn every_serde_alias_in_the_sources_is_reachable_from_the_exported_schema() {
        let declared = aliases_declared_in_the_sources();
        assert!(
            declared.len() >= 10,
            "test setup: the scanner found only {} aliases, so it is not reading the sources",
            declared.len()
        );

        let schema = build_schema();
        let flat = serde_json::to_string(&schema).expect("schema serializes");
        let missing: Vec<String> = declared
            .iter()
            .filter(|d| match &d.renamed_sibling {
                Some(canonical) => {
                    !some_properties_object_carries_both(&schema, canonical, &d.alias)
                        && !some_enum_array_carries_both(&schema, canonical, &d.alias)
                }
                None => !flat.contains(&format!("\"{}\"", d.alias)),
            })
            .map(|d| match &d.renamed_sibling {
                Some(canonical) => format!(
                    "{} (declared in {}, expected beside {canonical}, as a sibling property or \
                     as another value of the same enum)",
                    d.alias, d.file
                ),
                None => format!("{} (declared in {})", d.alias, d.file),
            })
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
    fn every_entry_in_the_two_tables_actually_lands_in_the_exported_schema() {
        let schema = build_schema();
        let defs = schema
            .get("definitions")
            .and_then(|d| d.as_object())
            .expect("definitions");

        let mut missing: Vec<String> = Vec::new();

        for (definition, canonical, aliases) in SERDE_ALIASES {
            let Some(entry) = defs.get(*definition) else {
                continue;
            };
            let text = entry.to_string();
            for alias in *aliases {
                if !text.contains(&format!("\"{alias}\"")) {
                    missing.push(format!("{definition}: enum {canonical} lacks {alias}"));
                }
            }
        }

        for (definition, canonical, aliases) in SERDE_FIELD_ALIASES {
            let Some(entry) = defs.get(*definition) else {
                missing.push(format!("{definition}: no such definition"));
                continue;
            };
            let text = entry.to_string();
            for alias in *aliases {
                if !text.contains(&format!("\"{alias}\"")) {
                    missing.push(format!(
                        "{definition}: property {canonical} has no {alias} alongside it"
                    ));
                }
            }
        }

        assert!(
            missing.is_empty(),
            "a table entry that names a definition or a canonical the schema does not carry is \
             inert, and inert is exactly how this drifts — the wider scan over the sources \
             cannot see it, because an alias string often already appears elsewhere in the \
             schema as some other type's field name:\n  {}",
            missing.join("\n  ")
        );
    }

    #[test]
    fn a_gradient_text_stop_accepts_both_spellings_of_its_position() {
        let schema = build_schema();
        let stop = schema
            .pointer("/definitions/GradientTextStop/properties")
            .and_then(|v| v.as_object())
            .expect("GradientTextStop carries properties");
        assert!(
            stop.contains_key("position") && stop.contains_key("offset"),
            "the parser accepts offset as an alias, so --strict-attrs must not reject it: {:?}",
            stop.keys().collect::<Vec<_>>()
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
