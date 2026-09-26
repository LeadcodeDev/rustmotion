use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;

use crate::error::{Result, RustmotionError};
use crate::schema::VariableType;
use crate::variables::substitute;

const MAX_EXPANSION_DEPTH: u32 = 64;

const MAX_EXPANSION_NODES: u64 = 2_000_000;

const MAX_FOR_EACH_ITEMS: usize = 100_000;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComponentDefinition {
    #[serde(default)]
    params: HashMap<String, ComponentParam>,
    template: Value,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComponentParam {
    #[serde(rename = "type")]
    #[allow(dead_code)]
    param_type: VariableType,
    #[serde(default)]
    default: Option<Value>,
    #[serde(default)]
    #[allow(dead_code)]
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UseDirective {
    #[serde(rename = "use")]
    use_name: String,
    #[serde(default)]
    props: HashMap<String, Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ForEachDirective {
    #[serde(rename = "for-each")]
    for_each: Value,
    template: Value,
}

fn is_for_each(v: &Value) -> bool {
    matches!(v, Value::Object(m) if m.contains_key("for-each"))
}

fn is_use(v: &Value) -> bool {
    matches!(v, Value::Object(m) if m.contains_key("use"))
}

pub fn expand_directives(value: &mut Value, file_label: &str) -> Result<()> {
    let defs = extract_component_definitions(value, file_label)?;
    let mut budget = MAX_EXPANSION_NODES;

    let Value::Object(root) = value else {
        return Ok(());
    };
    root.remove("components");

    if let Some(Value::Array(scenes)) = root.remove("scenes") {
        let mut out = Vec::with_capacity(scenes.len());
        for (i, mut scene) in scenes.into_iter().enumerate() {
            let scene_path = format!("scenes[{i}]");
            let mut stack = Vec::new();
            walk_children(
                &mut scene,
                &defs,
                file_label,
                &scene_path,
                &mut stack,
                0,
                &mut budget,
            )?;
            out.push(scene);
        }
        root.insert("scenes".to_string(), Value::Array(out));
    }

    if let Some(Value::Array(views)) = root.remove("composition") {
        let mut out_views = Vec::with_capacity(views.len());
        for (vi, mut view) in views.into_iter().enumerate() {
            if let Value::Object(vmap) = &mut view {
                if let Some(Value::Array(scenes)) = vmap.remove("scenes") {
                    let mut out = Vec::with_capacity(scenes.len());
                    for (si, mut scene) in scenes.into_iter().enumerate() {
                        let scene_path = format!("composition[{vi}].scenes[{si}]");
                        let mut stack = Vec::new();
                        walk_children(
                            &mut scene,
                            &defs,
                            file_label,
                            &scene_path,
                            &mut stack,
                            0,
                            &mut budget,
                        )?;
                        out.push(scene);
                    }
                    vmap.insert("scenes".to_string(), Value::Array(out));
                }
            }
            out_views.push(view);
        }
        root.insert("composition".to_string(), Value::Array(out_views));
    }

    warn_unresolved_after_expansion(value, file_label);
    Ok(())
}

fn consume_node_budget(budget: &mut u64, n: u64, file_label: &str, location: &str) -> Result<()> {
    match budget.checked_sub(n) {
        Some(remaining) => {
            *budget = remaining;
            Ok(())
        }
        None => Err(RustmotionError::Generic(format!(
            "expansion node budget ({MAX_EXPANSION_NODES}) exceeded at '{file_label}: {location}' \
             — for-each/use nesting multiplies past the limit"
        ))),
    }
}

fn warn_unresolved_after_expansion(value: &Value, file_label: &str) {
    for name in crate::variables::find_unresolved(value) {
        eprintln!(
            "Warning: {}",
            crate::error::RustmotionError::UnresolvedVariable {
                name,
                path: file_label.to_string(),
            }
        );
    }
}

fn extract_component_definitions(
    value: &Value,
    file_label: &str,
) -> Result<HashMap<String, ComponentDefinition>> {
    let Value::Object(root) = value else {
        return Ok(HashMap::new());
    };
    match root.get("components") {
        None => Ok(HashMap::new()),
        Some(Value::Object(defs_map)) => {
            let mut out = HashMap::with_capacity(defs_map.len());
            for (name, def_val) in defs_map {
                let def: ComponentDefinition =
                    serde_json::from_value(def_val.clone()).map_err(|e| {
                        RustmotionError::ComponentDefinitionInvalid {
                            name: name.clone(),
                            path: file_label.to_string(),
                            reason: e.to_string(),
                        }
                    })?;
                out.insert(name.clone(), def);
            }
            Ok(out)
        }
        Some(_) => Err(RustmotionError::ComponentsBlockNotObject {
            path: file_label.to_string(),
        }),
    }
}

fn walk_children(
    value: &mut Value,
    defs: &HashMap<String, ComponentDefinition>,
    file_label: &str,
    location: &str,
    stack: &mut Vec<String>,
    depth: u32,
    budget: &mut u64,
) -> Result<()> {
    match value {
        Value::Object(map) => {
            if matches!(map.get("children"), Some(Value::Array(_))) {
                if let Some(Value::Array(arr)) = map.remove("children") {
                    let mut expanded = Vec::with_capacity(arr.len());
                    for (i, entry) in arr.into_iter().enumerate() {
                        let entry_loc = format!("{location}.children[{i}]");
                        expanded.extend(resolve_entry(
                            entry, defs, file_label, &entry_loc, stack, depth, budget,
                        )?);
                    }
                    map.insert("children".to_string(), Value::Array(expanded));
                }
            }
            for (k, v) in map.iter_mut() {
                if k == "children" {
                    continue;
                }
                walk_children(v, defs, file_label, location, stack, depth, budget)?;
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                walk_children(v, defs, file_label, location, stack, depth, budget)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn resolve_entry(
    entry: Value,
    defs: &HashMap<String, ComponentDefinition>,
    file_label: &str,
    location: &str,
    stack: &mut Vec<String>,
    depth: u32,
    budget: &mut u64,
) -> Result<Vec<Value>> {
    if depth > MAX_EXPANSION_DEPTH {
        return Err(RustmotionError::ExpansionDepthExceeded {
            limit: MAX_EXPANSION_DEPTH,
            path: format!("{file_label}: {location}"),
        });
    }

    if let Value::Array(fragment) = entry {
        let mut out = Vec::with_capacity(fragment.len());
        for (i, n) in fragment.into_iter().enumerate() {
            let frag_loc = format!("{location}[{i}]");
            out.extend(resolve_entry(
                n,
                defs,
                file_label,
                &frag_loc,
                stack,
                depth + 1,
                budget,
            )?);
        }
        return Ok(out);
    }

    if is_for_each(&entry) {
        let produced = expand_for_each_directive(entry, file_label, location, budget)?;
        let mut out = Vec::with_capacity(produced.len());
        for (i, node) in produced.into_iter().enumerate() {
            let iter_loc = format!("{location}[{i}]");
            out.extend(resolve_entry(
                node,
                defs,
                file_label,
                &iter_loc,
                stack,
                depth + 1,
                budget,
            )?);
        }
        return Ok(out);
    }

    if is_use(&entry) {
        let (name, node) = expand_use_directive(entry, defs, file_label, location)?;
        if stack.contains(&name) {
            let mut chain = stack.clone();
            chain.push(name);
            return Err(RustmotionError::ComponentCycle {
                chain: chain.join(" -> "),
                path: format!("{file_label}: {location}"),
            });
        }
        stack.push(name);
        let result = resolve_entry(node, defs, file_label, location, stack, depth + 1, budget);
        stack.pop();
        return result;
    }

    let mut node = entry;
    walk_children(&mut node, defs, file_label, location, stack, depth, budget)?;
    Ok(vec![node])
}

fn expand_for_each_directive(
    entry: Value,
    file_label: &str,
    location: &str,
    budget: &mut u64,
) -> Result<Vec<Value>> {
    let directive: ForEachDirective =
        serde_json::from_value(entry).map_err(|e| RustmotionError::ForEachDirectiveInvalid {
            path: format!("{file_label}: {location}"),
            reason: e.to_string(),
        })?;

    let items = match &directive.for_each {
        Value::Array(items) => items.clone(),
        other => {
            return Err(RustmotionError::ForEachNotArray {
                path: format!("{file_label}: {location}"),
                found: describe_value(other),
            })
        }
    };

    if items.len() > MAX_FOR_EACH_ITEMS {
        return Err(RustmotionError::Generic(format!(
            "for-each at '{file_label}: {location}' has {} items, exceeding the per-directive \
             cap of {MAX_FOR_EACH_ITEMS}",
            items.len()
        )));
    }
    consume_node_budget(budget, items.len() as u64, file_label, location)?;

    let count = items.len();
    let mut out = Vec::with_capacity(count);
    for (idx, element) in items.into_iter().enumerate() {
        let mut bindings: HashMap<String, Value> = HashMap::new();
        if let Value::Object(obj) = &element {
            for (k, v) in obj {
                bindings.insert(k.clone(), v.clone());
            }
        }
        bindings
            .entry("index".to_string())
            .or_insert_with(|| Value::from(idx));
        bindings
            .entry("item".to_string())
            .or_insert_with(|| element.clone());
        bindings
            .entry("i".to_string())
            .or_insert_with(|| Value::from(idx));
        bindings
            .entry("count".to_string())
            .or_insert_with(|| Value::from(count));

        let mut node = directive.template.clone();
        substitute(&mut node, &bindings, file_label)?;
        out.push(node);
    }
    Ok(out)
}

fn expand_use_directive(
    entry: Value,
    defs: &HashMap<String, ComponentDefinition>,
    file_label: &str,
    location: &str,
) -> Result<(String, Value)> {
    let directive: UseDirective =
        serde_json::from_value(entry).map_err(|e| RustmotionError::UseDirectiveInvalid {
            path: format!("{file_label}: {location}"),
            reason: e.to_string(),
        })?;

    let def = defs
        .get(&directive.use_name)
        .ok_or_else(|| RustmotionError::UnknownComponent {
            name: directive.use_name.clone(),
            path: format!("{file_label}: {location}"),
        })?;

    for key in directive.props.keys() {
        if !def.params.contains_key(key) {
            return Err(RustmotionError::UnknownComponentParam {
                component: directive.use_name.clone(),
                param: key.clone(),
                path: format!("{file_label}: {location}"),
            });
        }
    }

    let mut bindings: HashMap<String, Value> = HashMap::with_capacity(def.params.len());
    for (pname, pdef) in &def.params {
        match directive.props.get(pname) {
            Some(v) => {
                bindings.insert(pname.clone(), v.clone());
            }
            None => match &pdef.default {
                Some(d) => {
                    bindings.insert(pname.clone(), d.clone());
                }
                None => {
                    return Err(RustmotionError::ComponentParamMissing {
                        component: directive.use_name.clone(),
                        param: pname.clone(),
                        path: format!("{file_label}: {location}"),
                    })
                }
            },
        }
    }

    let mut node = def.template.clone();
    substitute(&mut node, &bindings, file_label)?;
    Ok((directive.use_name.clone(), node))
}

fn describe_value(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => format!("boolean ({b})"),
        Value::Number(n) => format!("number ({n})"),
        Value::String(s) => {
            let preview: String = s.chars().take(40).collect();
            let ellipsis = if s.chars().count() > 40 { "…" } else { "" };
            format!(
                "string (\"{preview}{ellipsis}\"){}",
                if s.starts_with('$') {
                    " — looks like an unresolved/undeclared variable reference"
                } else {
                    ""
                }
            )
        }
        Value::Object(_) => "object".to_string(),
        Value::Array(_) => "array".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn expand(mut value: Value) -> Result<Value> {
        expand_directives(&mut value, "test.json")?;
        Ok(value)
    }

    #[test]
    fn template_bindings_are_not_reported_as_unresolved_before_expansion() {
        let doc = json!({
            "components": {
                "card": {
                    "params": { "label": { "type": "string" } },
                    "template": { "type": "text", "content": "$label" }
                }
            },
            "scenes": [{ "duration": 1.0, "children": [{
                "for-each": [{ "label": "one" }],
                "template": { "use": "card", "props": { "label": "$label" } }
            }]}]
        });
        assert!(
            crate::variables::find_unresolved(&doc).is_empty(),
            "bindings inside components/template/props belong to expansion, \
             not to the pre-expansion scan: {:?}",
            crate::variables::find_unresolved(&doc)
        );
    }

    #[test]
    fn a_typo_inside_a_template_is_still_found_after_expansion() {
        let expanded = expand(json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{ "duration": 1.0, "children": [{
                "for-each": [{ "label": "one" }],
                "template": { "type": "text", "content": "$labl" }
            }]}]
        }))
        .expect("a typo is a warning, not a hard error");
        assert_eq!(
            crate::variables::find_unresolved(&expanded),
            vec!["labl".to_string()],
            "the leftover must be visible once template/props are gone"
        );
    }

    #[test]
    fn a_correct_binding_leaves_nothing_unresolved_after_expansion() {
        let expanded = expand(json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{ "duration": 1.0, "children": [{
                "for-each": [{ "label": "one" }],
                "template": { "type": "text", "content": "$label" }
            }]}]
        }))
        .expect("expands");
        assert!(crate::variables::find_unresolved(&expanded).is_empty());
    }

    #[test]
    fn for_each_repeats_template_once_per_element_binding_its_fields() {
        let doc = json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "for-each": [
                        { "label": "Revenue", "value": 120 },
                        { "label": "Users", "value": 340 }
                    ],
                    "template": { "type": "text", "content": "$label: $value" }
                }]
            }]
        });
        let out = expand(doc).unwrap();
        let children = out["scenes"][0]["children"].as_array().unwrap();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0]["content"], json!("Revenue: 120"));
        assert_eq!(children[1]["content"], json!("Users: 340"));
    }

    #[test]
    fn for_each_binds_index_and_whole_item() {
        let doc = json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "for-each": ["a", "b", "c"],
                    "template": { "type": "text", "content": "$index:$item" }
                }]
            }]
        });
        let out = expand(doc).unwrap();
        let children = out["scenes"][0]["children"].as_array().unwrap();
        assert_eq!(children.len(), 3);
        assert_eq!(children[0]["content"], json!("0:a"));
        assert_eq!(children[1]["content"], json!("1:b"));
        assert_eq!(children[2]["content"], json!("2:c"));
    }

    #[test]
    fn for_each_binds_i_and_count_aliases_for_expr_grammar() {
        let doc = json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "for-each": ["a", "b", "c"],
                    "template": { "type": "text", "content": "$i/$count" }
                }]
            }]
        });
        let out = expand(doc).unwrap();
        let children = out["scenes"][0]["children"].as_array().unwrap();
        assert_eq!(children.len(), 3);
        assert_eq!(children[0]["content"], json!("0/3"));
        assert_eq!(children[1]["content"], json!("1/3"));
        assert_eq!(children[2]["content"], json!("2/3"));
    }

    #[test]
    fn for_each_lets_explicit_item_fields_win_over_built_in_index() {
        let doc = json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "for-each": [{ "index": "custom", "label": "x" }],
                    "template": { "type": "text", "content": "$index" }
                }]
            }]
        });
        let out = expand(doc).unwrap();
        assert_eq!(out["scenes"][0]["children"][0]["content"], json!("custom"));
    }

    #[test]
    fn for_each_over_empty_array_produces_nothing_and_is_not_an_error() {
        let doc = json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "for-each": [],
                    "template": { "type": "text", "content": "unused" }
                }]
            }]
        });
        let out = expand(doc).unwrap();
        assert_eq!(out["scenes"][0]["children"], json!([]));
    }

    #[test]
    fn for_each_source_that_is_not_an_array_is_a_named_error_not_a_silent_empty_result() {
        let doc = json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "for-each": "$itms",
                    "template": { "type": "text", "content": "$label" }
                }]
            }]
        });
        let err = expand(doc).expect_err("non-array for-each source must fail loudly");
        assert!(
            matches!(err, RustmotionError::ForEachNotArray { .. }),
            "{err}"
        );
        let msg = err.to_string();
        assert!(msg.contains("scenes[0].children[0]"), "{msg}");
        assert!(msg.contains("unresolved"), "{msg}");
    }

    #[test]
    fn for_each_missing_template_is_a_named_error() {
        let doc = json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{
                "duration": 1.0,
                "children": [{ "for-each": [1, 2, 3] }]
            }]
        });
        let err = expand(doc).expect_err("missing template must fail");
        assert!(
            matches!(err, RustmotionError::ForEachDirectiveInvalid { .. }),
            "{err}"
        );
    }

    #[test]
    fn for_each_with_a_fragment_template_splices_every_sibling_in_place_not_a_nested_array() {
        let doc = json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "for-each": [ { "label": "A" }, { "label": "B" } ],
                    "template": [
                        { "type": "icon", "icon": "lucide:dot" },
                        { "type": "text", "content": "$label" }
                    ]
                }]
            }]
        });
        let out = expand(doc).unwrap();
        let children = out["scenes"][0]["children"].as_array().unwrap();
        assert_eq!(
            children.len(),
            4,
            "2 iterations x 2 fragment nodes = 4 flat siblings, got: {children:#?}"
        );
        assert!(children.iter().all(|c| c.is_object()), "{children:#?}");
        assert_eq!(children[0]["type"], json!("icon"));
        assert_eq!(children[1]["content"], json!("A"));
        assert_eq!(children[2]["type"], json!("icon"));
        assert_eq!(children[3]["content"], json!("B"));
    }

    fn doc_with_stat_card(props: Value) -> Value {
        json!({
            "video": { "width": 100, "height": 100 },
            "components": {
                "stat_card": {
                    "params": {
                        "label": { "type": "string" },
                        "value": { "type": "number", "default": 0 },
                        "color": { "type": "string", "default": "#6366F1" }
                    },
                    "template": {
                        "type": "card",
                        "style": { "background": "$color" },
                        "children": [
                            { "type": "text", "content": "$label" },
                            { "type": "counter", "value": "$value" }
                        ]
                    }
                }
            },
            "scenes": [{
                "duration": 1.0,
                "children": [{ "use": "stat_card", "props": props }]
            }]
        })
    }

    #[test]
    fn use_instantiates_a_component_with_props_overriding_defaults() {
        let out = expand(doc_with_stat_card(
            json!({ "label": "Revenue", "value": 42 }),
        ))
        .unwrap();
        let card = &out["scenes"][0]["children"][0];
        assert_eq!(card["type"], json!("card"));
        assert_eq!(card["style"]["background"], json!("#6366F1"));
        assert_eq!(card["children"][0]["content"], json!("Revenue"));
        assert_eq!(card["children"][1]["value"], json!(42));
    }

    #[test]
    fn use_falls_back_to_param_default_when_not_overridden() {
        let out = expand(doc_with_stat_card(json!({ "label": "Users" }))).unwrap();
        assert_eq!(
            out["scenes"][0]["children"][0]["children"][1]["value"],
            json!(0)
        );
    }

    #[test]
    fn components_block_does_not_survive_expansion() {
        let out = expand(doc_with_stat_card(json!({ "label": "x" }))).unwrap();
        assert!(out.get("components").is_none());
    }

    #[test]
    fn use_of_unknown_component_is_a_named_error() {
        let doc = json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{
                "duration": 1.0,
                "children": [{ "use": "does_not_exist", "props": {} }]
            }]
        });
        let err = expand(doc).expect_err("unknown component must fail");
        match &err {
            RustmotionError::UnknownComponent { name, path } => {
                assert_eq!(name, "does_not_exist");
                assert!(path.contains("scenes[0].children[0]"), "{path}");
            }
            other => panic!("expected UnknownComponent, got {other}"),
        }
    }

    #[test]
    fn use_missing_a_required_parameter_is_a_named_error() {
        let out = expand(doc_with_stat_card(json!({})));
        let err = out.expect_err("missing required param must fail");
        match &err {
            RustmotionError::ComponentParamMissing {
                component, param, ..
            } => {
                assert_eq!(component, "stat_card");
                assert_eq!(param, "label");
            }
            other => panic!("expected ComponentParamMissing, got {other}"),
        }
    }

    #[test]
    fn use_with_an_undeclared_prop_key_is_a_named_error() {
        let out = expand(doc_with_stat_card(
            json!({ "label": "x", "labell": "typo" }),
        ));
        let err = out.expect_err("typo'd prop key must fail");
        match &err {
            RustmotionError::UnknownComponentParam { param, .. } => assert_eq!(param, "labell"),
            other => panic!("expected UnknownComponentParam, got {other}"),
        }
    }

    #[test]
    fn use_of_a_component_that_uses_itself_is_a_named_cycle_not_a_stack_overflow() {
        let doc = json!({
            "video": { "width": 100, "height": 100 },
            "components": {
                "recursive": {
                    "params": {},
                    "template": { "type": "card", "children": [ { "use": "recursive", "props": {} } ] }
                }
            },
            "scenes": [{
                "duration": 1.0,
                "children": [{ "use": "recursive", "props": {} }]
            }]
        });
        let err = expand(doc).expect_err("self-referencing component must fail");
        match &err {
            RustmotionError::ComponentCycle { chain, .. } => {
                assert!(chain.contains("recursive"), "{chain}");
            }
            other => panic!("expected ComponentCycle, got {other}"),
        }
    }

    #[test]
    fn indirect_two_hop_cycle_is_also_a_named_cycle() {
        let doc = json!({
            "video": { "width": 100, "height": 100 },
            "components": {
                "a": { "params": {}, "template": { "type": "card", "children": [ { "use": "b", "props": {} } ] } },
                "b": { "params": {}, "template": { "type": "card", "children": [ { "use": "a", "props": {} } ] } }
            },
            "scenes": [{
                "duration": 1.0,
                "children": [{ "use": "a", "props": {} }]
            }]
        });
        let err = expand(doc).expect_err("indirect cycle must fail");
        match &err {
            RustmotionError::ComponentCycle { chain, .. } => {
                assert!(chain.contains('a') && chain.contains('b'), "{chain}");
            }
            other => panic!("expected ComponentCycle, got {other}"),
        }
    }

    #[test]
    fn use_with_a_fragment_template_splices_every_sibling_in_place() {
        let doc = json!({
            "video": { "width": 100, "height": 100 },
            "components": {
                "icon_label": {
                    "params": { "label": { "type": "string" } },
                    "template": [
                        { "type": "icon", "icon": "lucide:dot" },
                        { "type": "text", "content": "$label" }
                    ]
                }
            },
            "scenes": [{
                "duration": 1.0,
                "children": [{ "use": "icon_label", "props": { "label": "hi" } }]
            }]
        });
        let out = expand(doc).unwrap();
        let children = out["scenes"][0]["children"].as_array().unwrap();
        assert_eq!(children.len(), 2, "{children:#?}");
        assert_eq!(children[0]["type"], json!("icon"));
        assert_eq!(children[1]["content"], json!("hi"));
    }

    #[test]
    fn for_each_template_can_be_a_use_directive() {
        let doc = json!({
            "video": { "width": 100, "height": 100 },
            "components": {
                "row": {
                    "params": { "label": { "type": "string" } },
                    "template": { "type": "text", "content": "$label" }
                }
            },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "for-each": [ { "label": "A" }, { "label": "B" } ],
                    "template": { "use": "row", "props": { "label": "$label" } }
                }]
            }]
        });
        let out = expand(doc).unwrap();
        let children = out["scenes"][0]["children"].as_array().unwrap();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0]["content"], json!("A"));
        assert_eq!(children[1]["content"], json!("B"));
    }

    #[test]
    fn use_template_can_contain_a_nested_for_each() {
        let doc = json!({
            "video": { "width": 100, "height": 100 },
            "components": {
                "list_card": {
                    "params": { "items": { "type": "array" } },
                    "template": {
                        "type": "card",
                        "children": [{
                            "for-each": "$items",
                            "template": { "type": "text", "content": "$item" }
                        }]
                    }
                }
            },
            "scenes": [{
                "duration": 1.0,
                "children": [{ "use": "list_card", "props": { "items": ["x", "y", "z"] } }]
            }]
        });
        let out = expand(doc).unwrap();
        let inner = out["scenes"][0]["children"][0]["children"]
            .as_array()
            .unwrap();
        assert_eq!(inner.len(), 3);
        assert_eq!(inner[2]["content"], json!("z"));
    }

    #[test]
    fn nested_children_containers_are_expanded_recursively() {
        let doc = json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "card",
                    "children": [{
                        "for-each": [{ "v": 1 }, { "v": 2 }],
                        "template": { "type": "text", "content": "$v" }
                    }]
                }]
            }]
        });
        let out = expand(doc).unwrap();
        let inner = out["scenes"][0]["children"][0]["children"]
            .as_array()
            .unwrap();
        assert_eq!(inner.len(), 2);
        assert_eq!(inner[0]["content"], json!(1));
        assert_eq!(inner[1]["content"], json!(2));
    }

    #[test]
    fn for_each_authored_tree_is_identical_to_the_hand_written_equivalent() {
        let generated = json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "for-each": [
                        { "label": "Revenue", "value": 120 },
                        { "label": "Users", "value": 340 },
                        { "label": "Growth", "value": 8 }
                    ],
                    "template": {
                        "type": "card",
                        "style": { "width": "200px" },
                        "children": [
                            { "type": "text", "content": "$label" },
                            { "type": "counter", "value": "$value" }
                        ]
                    }
                }]
            }]
        });

        let hand_written = json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{
                "duration": 1.0,
                "children": [
                    { "type": "card", "style": { "width": "200px" }, "children": [
                        { "type": "text", "content": "Revenue" },
                        { "type": "counter", "value": 120 }
                    ]},
                    { "type": "card", "style": { "width": "200px" }, "children": [
                        { "type": "text", "content": "Users" },
                        { "type": "counter", "value": 340 }
                    ]},
                    { "type": "card", "style": { "width": "200px" }, "children": [
                        { "type": "text", "content": "Growth" },
                        { "type": "counter", "value": 8 }
                    ]}
                ]
            }]
        });

        let expanded = expand(generated).unwrap();
        assert_eq!(
            expanded, hand_written,
            "the for-each-authored tree must be byte-for-byte identical (as JSON values) to the \
             hand-written equivalent — this is the only proof that factoring changes nothing about \
             what gets rendered"
        );
    }
}
