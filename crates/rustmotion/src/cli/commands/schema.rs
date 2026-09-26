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
