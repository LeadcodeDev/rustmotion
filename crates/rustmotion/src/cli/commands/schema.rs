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

/// Build the full scenario schema, with every `children` array typed
/// against the real `Component` variants (plus `for-each`/`use`) instead of
/// `serde_json::Value` ("items: true").
///
/// `Scene.children` is `Vec<serde_json::Value>` in `rustmotion-core` (schema
/// can't depend on `rustmotion-components`, which depends on it), so
/// schemars types it as "any JSON". `Component` — with its full `oneOf` over
/// all 57 component variants — is already built for internal use at
/// `validate_attrs.rs:40`; here we merge it into the scenario schema so the
/// exported schema actually documents the authoring surface.
///
/// A *nested* container's own `children` field (`card`/`flex`/`div`/…,
/// typed `Vec<ChildComponent>` in `rustmotion-components`, not
/// `rustmotion-core`) is a second, independent occurrence of the exact same
/// problem: `ChildComponent` is schemars-derived and merged in below
/// alongside `Component`'s other auxiliary types, and needs the identical
/// `for-each`/`use` treatment — a `for-each` written inside a `div`'s own
/// `children` (as every `examples/composition-*.json` file actually does)
/// is invisible to a fix that only touches `Scene.children`/`Component`.
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

    // Merge Component's auxiliary type definitions (CssStyle, AnimationEffect,
    // ChildComponent, etc. — the same underlying Rust types the scenario
    // schema may already reference elsewhere).
    for (k, v) in component_defs {
        defs_obj.entry(k.clone()).or_insert_with(|| v.clone());
    }

    // Register `Component` itself (its `oneOf` over every variant) as a
    // named definition.
    let mut component_root = component_schema.clone();
    if let Some(obj) = component_root.as_object_mut() {
        obj.remove("definitions");
        obj.remove("$schema");
    }
    defs_obj.insert("Component".to_string(), component_root);

    inject_for_each_use_definitions(defs_obj);

    // Widen both `Component` (what `Scene.children` points at) and
    // `ChildComponent` (what every container's *own* `children` field
    // points at, merged in above) to also accept a `for-each`/`use`
    // directive in place of a concrete component. Both named definitions
    // are wrapped in place — `wrap_with_directives` renames the original
    // content to `<Name>Base` and re-points every existing `$ref` to
    // `<Name>` (there are ten `ChildComponent` call sites alone) at the
    // widened union instead, with no further find-and-replace needed.
    wrap_with_directives(defs_obj, "Component");
    wrap_with_directives(defs_obj, "ChildComponent");

    // `Scene.children` itself: was untyped `serde_json::Value` ("any
    // JSON"); now the same widened `Component` union every nested
    // container's own `children` already points at.
    if let Some(children) = scenario_schema.pointer_mut("/definitions/Scene/properties/children") {
        *children = serde_json::json!({
            "type": "array",
            "items": { "$ref": "#/definitions/Component" }
        });
    }

    // `components[name].template`: a single child entry, or an array of
    // sibling entries (a fragment) — the same shape a `for-each`'s own
    // `template` accepts, and by the same reasoning may itself nest another
    // `for-each`/`use` (see `expand.rs`'s module doc on composing without
    // special-casing).
    if let Some(template) =
        scenario_schema.pointer_mut("/definitions/ComponentTemplateDef/properties/template")
    {
        *template = serde_json::json!({ "$ref": "#/definitions/TemplateValue" });
    }

    scenario_schema
}

/// Replaces `defs_obj[name]` with `anyOf(original content, ForEachDirective,
/// UseDirective)`, moving the original content to `<name>Base` so every
/// existing `$ref: "#/definitions/<name>"` elsewhere in the schema — already
/// written before this function runs, since `component_defs` was merged in
/// wholesale — resolves to the widened union without needing to be
/// rewritten individually. A no-op if `name` isn't present.
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

/// Adds `TemplateValue`/`ForEachDirective`/`UseDirective` to `defs_obj` —
/// hand-written JSON Schema fragments, not derived from a Rust type: the
/// real structs they describe (`rustmotion_core::expand::{ForEachDirective,
/// UseDirective}`) are private to that module, by design (nothing outside
/// it should construct or see one — they are pre-processing wire types,
/// consumed and discarded before `Scenario` is ever deserialized; see
/// `Scenario::components`'s doc for the same story about the `components`
/// block itself). Mirrors their shape as documented in `CLAUDE.md`'s
/// "Factorisation" section and `expand.rs`'s own module doc, not by
/// importing them (a private type across a crate/module boundary can't
/// drive `schemars::schema_for!` anyway).
///
/// `TemplateValue` — a single child entry, or an array of them (a
/// `template` written as a fragment of several sibling nodes) — is the
/// shape both a `for-each`'s own `template` and a `components[name].template`
/// share; referencing `Component` here (rather than being folded into
/// `wrap_with_directives`'s own output) keeps it correct regardless of
/// which caller resolves the reference after `Component` is widened.
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
