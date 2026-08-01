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

/// Build the full scenario schema, with `Scene.children` typed against the
/// real `Component` variants instead of `serde_json::Value` ("items: true").
///
/// `Scene.children` is `Vec<serde_json::Value>` in `rustmotion-core` (schema
/// can't depend on `rustmotion-components`, which depends on it), so
/// schemars types it as "any JSON". `Component` — with its full `oneOf` over
/// all 57 component variants — is already built for internal use at
/// `validate_attrs.rs:40`; here we merge it into the scenario schema so the
/// exported schema actually documents the authoring surface.
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
    // etc. — the same underlying Rust types the scenario schema may already
    // reference elsewhere).
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

    // Point `Scene.children` at `Component` instead of the untyped
    // `serde_json::Value` ("items: true").
    if let Some(children) = scenario_schema.pointer_mut("/definitions/Scene/properties/children")
    {
        *children = serde_json::json!({
            "type": "array",
            "items": { "$ref": "#/definitions/Component" }
        });
    }

    scenario_schema
}
