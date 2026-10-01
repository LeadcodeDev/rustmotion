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

const PLACEHOLDER_PATTERN: &str = "^\\$[A-Za-z_][A-Za-z0-9_]*$";

const PLACEHOLDER_DESCRIPTION: &str = "A `$name` placeholder, substituted before the scenario is \
    deserialized — from `config`, from a `for-each` element's fields, or from a `use`'s `props`. \
    See CLAUDE.md's \"Factorisation\" section.";

fn is_scalar_schema(map: &serde_json::Map<String, serde_json::Value>) -> bool {
    if map.contains_key("enum") || map.contains_key("const") {
        return false;
    }
    let scalar = |name: &str| matches!(name, "number" | "integer" | "boolean");
    match map.get("type") {
        Some(serde_json::Value::String(name)) => scalar(name),
        Some(serde_json::Value::Array(names)) => {
            names.iter().filter_map(|v| v.as_str()).any(scalar)
        }
        _ => false,
    }
}

fn accept_a_placeholder_too(value: &mut serde_json::Value) {
    let Some(map) = value.as_object_mut() else {
        return;
    };
    let description = map.remove("description");
    let default = map.get("default").cloned();
    let mut widened = serde_json::json!({
        "anyOf": [
            value.clone(),
            { "$ref": "#/definitions/TemplatePlaceholder" }
        ]
    });
    if let Some(description) = description {
        widened["description"] = description;
    }
    if let Some(default) = default {
        widened["default"] = default;
    }
    *value = widened;
}

fn accept_placeholders_where_a_scalar_is_declared(value: &mut serde_json::Value) {
    const SCHEMA_VALUED: &[&str] = &[
        "additionalProperties",
        "additionalItems",
        "not",
        "if",
        "then",
        "else",
        "propertyNames",
        "contains",
    ];
    const SCHEMA_MAPS: &[&str] = &[
        "properties",
        "patternProperties",
        "definitions",
        "dependencies",
    ];
    const SCHEMA_LISTS: &[&str] = &["allOf", "anyOf", "oneOf"];

    let Some(map) = value.as_object_mut() else {
        return;
    };
    for key in SCHEMA_VALUED {
        if let Some(child) = map.get_mut(*key) {
            accept_placeholders_where_a_scalar_is_declared(child);
        }
    }
    for key in SCHEMA_MAPS {
        if let Some(serde_json::Value::Object(children)) = map.get_mut(*key) {
            for child in children.values_mut() {
                accept_placeholders_where_a_scalar_is_declared(child);
            }
        }
    }
    for key in SCHEMA_LISTS {
        if let Some(serde_json::Value::Array(children)) = map.get_mut(*key) {
            for child in children.iter_mut() {
                accept_placeholders_where_a_scalar_is_declared(child);
            }
        }
    }
    match map.get_mut("items") {
        Some(serde_json::Value::Array(children)) => {
            for child in children.iter_mut() {
                accept_placeholders_where_a_scalar_is_declared(child);
            }
        }
        Some(child) => accept_placeholders_where_a_scalar_is_declared(child),
        None => {}
    }

    if is_scalar_schema(map) {
        accept_a_placeholder_too(value);
    }
}

fn tag_values(branch: &serde_json::Value, key: &str) -> Option<Vec<String>> {
    let declared = branch.get("properties")?.get(key)?;
    if let Some(one) = declared.get("const").and_then(|v| v.as_str()) {
        return Some(vec![one.to_string()]);
    }
    let listed = declared.get("enum")?.as_array()?;
    let values: Vec<String> = listed
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
    (!values.is_empty() && values.len() == listed.len()).then_some(values)
}

fn discriminating_key(branches: &[serde_json::Value]) -> Option<String> {
    let first = branches.first()?.get("properties")?.as_object()?;
    for key in first.keys() {
        let Some(per_branch) = branches
            .iter()
            .map(|b| tag_values(b, key))
            .collect::<Option<Vec<_>>>()
        else {
            continue;
        };
        let mut seen = std::collections::HashSet::new();
        if per_branch
            .iter()
            .flatten()
            .all(|value| seen.insert(value.clone()))
        {
            return Some(key.clone());
        }
    }
    None
}

fn steer_tagged_unions_on_their_tag(value: &mut serde_json::Value) {
    let Some(map) = value.as_object_mut() else {
        return;
    };
    for child in map.values_mut() {
        match child {
            serde_json::Value::Array(items) => {
                for item in items.iter_mut() {
                    steer_tagged_unions_on_their_tag(item);
                }
            }
            other => steer_tagged_unions_on_their_tag(other),
        }
    }

    let union_key = if map.contains_key("oneOf") {
        "oneOf"
    } else if map.contains_key("anyOf") {
        "anyOf"
    } else {
        return;
    };
    let Some(branches) = map.get(union_key).and_then(|b| b.as_array()).cloned() else {
        return;
    };
    let Some(key) = discriminating_key(&branches) else {
        return;
    };

    let every_branch_requires_the_tag = branches.iter().all(|b| {
        b.get("required")
            .and_then(|r| r.as_array())
            .is_some_and(|r| r.iter().any(|v| v.as_str() == Some(key.as_str())))
    });

    let mut steered: Vec<serde_json::Value> = Vec::with_capacity(branches.len() + 1);
    let all: Vec<String> = branches
        .iter()
        .filter_map(|b| tag_values(b, &key))
        .flatten()
        .collect();
    let mut guard = serde_json::json!({ "properties": { key.clone(): { "enum": all } } });
    if every_branch_requires_the_tag {
        guard["required"] = serde_json::json!([key.clone()]);
    }
    steered.push(guard);
    for branch in branches {
        let Some(values) = tag_values(&branch, &key) else {
            return;
        };
        steered.push(serde_json::json!({
            "if": {
                "required": [key.clone()],
                "properties": { key.clone(): { "enum": values } }
            },
            "then": branch
        }));
    }

    map.remove(union_key);
    map.insert("allOf".to_string(), serde_json::Value::Array(steered));
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

    accept_placeholders_where_a_scalar_is_declared(&mut scenario_schema);
    steer_tagged_unions_on_their_tag(&mut scenario_schema);
    if let Some(defs) = scenario_schema
        .pointer_mut("/definitions")
        .and_then(|d| d.as_object_mut())
    {
        defs.insert(
            "TemplatePlaceholder".to_string(),
            serde_json::json!({
                "type": "string",
                "pattern": PLACEHOLDER_PATTERN,
                "description": PLACEHOLDER_DESCRIPTION
            }),
        );
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
            "if": { "required": ["for-each"] },
            "then": { "$ref": "#/definitions/ForEachDirective" },
            "else": {
                "if": { "required": ["use"] },
                "then": { "$ref": "#/definitions/UseDirective" },
                "else": { "$ref": format!("#/definitions/{base_name}") }
            }
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

    fn inline_refs(schema: &serde_json::Value, node: &serde_json::Value) -> serde_json::Value {
        fn step(
            defs: &serde_json::Value,
            node: &serde_json::Value,
            depth: u8,
        ) -> serde_json::Value {
            if depth == 0 {
                return node.clone();
            }
            match node {
                serde_json::Value::Object(map) => {
                    if let Some(name) = map
                        .get("$ref")
                        .and_then(|r| r.as_str())
                        .and_then(|r| r.strip_prefix("#/definitions/"))
                    {
                        if let Some(target) = defs.get(name) {
                            return step(defs, target, depth - 1);
                        }
                    }
                    serde_json::Value::Object(
                        map.iter()
                            .map(|(k, v)| (k.clone(), step(defs, v, depth - 1)))
                            .collect(),
                    )
                }
                serde_json::Value::Array(items) => serde_json::Value::Array(
                    items.iter().map(|v| step(defs, v, depth - 1)).collect(),
                ),
                other => other.clone(),
            }
        }
        let defs = schema
            .get("definitions")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        step(&defs, node, 6)
    }

    fn union_branches(node: &serde_json::Value) -> Vec<&serde_json::Value> {
        if let Some(branches) = node
            .get("oneOf")
            .or_else(|| node.get("anyOf"))
            .and_then(|b| b.as_array())
        {
            return branches.iter().collect();
        }
        node.get("allOf")
            .and_then(|b| b.as_array())
            .map(|entries| entries.iter().filter_map(|e| e.get("then")).collect())
            .unwrap_or_default()
    }

    fn preset_animation_branch<'a>(
        schema: &'a serde_json::Value,
        name: &str,
    ) -> &'a serde_json::Value {
        let node = schema
            .pointer("/definitions/AnimationEffect")
            .expect("AnimationEffect is defined");
        union_branches(node)
            .into_iter()
            .find(|branch| {
                branch
                    .pointer("/properties/name/enum")
                    .and_then(|e| e.as_array())
                    .is_some_and(|values| values.iter().any(|v| v.as_str() == Some(name)))
            })
            .unwrap_or_else(|| panic!("no AnimationEffect branch tagged {name}"))
    }

    #[test]
    fn animation_timing_is_declared_the_way_its_wire_type_parses() {
        let schema = build_schema();
        let fade = preset_animation_branch(&schema, "fade_in");

        let required: Vec<&str> = fade
            .get("required")
            .and_then(|r| r.as_array())
            .map(|r| r.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        assert_eq!(
            required,
            ["name"],
            "AnimationTiming carries `repeat: bool` with no serde default, so schemars made              `loop` required — while AnimationTimingWire defaults it"
        );
        assert!(
            fade.pointer("/properties/repeat_count").is_none(),
            "repeat_count is an output of RepeatSpec::into_parts, not a wire field, and the              wire type is deny_unknown_fields: advertising it hands a generator a key that              drops the whole component"
        );

        let text =
            inline_refs(&schema, fade.pointer("/properties/loop").expect("loop")).to_string();
        assert!(
            text.contains("boolean") && text.contains("integer"),
            "`loop` takes a bool or a play count (#330), not a bool alone: {text}"
        );

        let parses = |v: serde_json::Value| {
            serde_json::from_value::<rustmotion::schema::AnimationEffect>(v).is_ok()
        };
        assert!(parses(serde_json::json!({ "name": "fade_in" })));
        assert!(parses(serde_json::json!({ "name": "fade_in", "loop": 12 })));
        assert!(
            !parses(serde_json::json!({ "name": "fade_in", "repeat_count": 3 })),
            "if this ever starts parsing, repeat_count belongs back in the schema"
        );
    }

    #[test]
    fn font_weight_is_declared_the_way_its_visitor_parses() {
        let schema = build_schema();
        let node = schema
            .pointer("/definitions/RichTextSpan/properties/font-weight")
            .expect("a rich_text span carries font-weight");
        let described = inline_refs(&schema, node).to_string();
        assert!(
            described.contains("\"bold\""),
            "the visitor takes \"bold\"; schemars derived Rust's `Bold` from the variant \
             name: {described}"
        );
        assert!(
            !described.contains("\"Bold\""),
            "\"Bold\" is what the derive emitted and what the parser refuses: {described}"
        );

        let parses = |v: serde_json::Value| {
            serde_json::from_value::<rustmotion::schema::FontWeight>(v).is_ok()
        };
        assert!(parses(serde_json::json!("bold")));
        assert!(parses(serde_json::json!(700)));
        assert!(!parses(serde_json::json!("Bold")));
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
