use std::collections::HashMap;

use crate::error::Result;
use serde_json::Value;

use crate::error::RustmotionError;
use crate::schema::{VariableDefinition, VariableType};

fn value_matches_declared_type(value: &Value, var_type: &VariableType) -> bool {
    match var_type {
        VariableType::String => value.is_string(),
        VariableType::Number => value.is_number(),
        VariableType::Boolean => value.is_boolean(),
        VariableType::Object => value.is_object(),
        VariableType::Array => value.is_array(),
    }
}

fn json_type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn declared_type_name(var_type: &VariableType) -> &'static str {
    match var_type {
        VariableType::String => "string",
        VariableType::Number => "number",
        VariableType::Boolean => "boolean",
        VariableType::Object => "object",
        VariableType::Array => "array",
    }
}

fn merge_variables(
    definitions: &HashMap<String, VariableDefinition>,
    overrides: Option<&HashMap<String, Value>>,
    path: &str,
) -> Result<HashMap<String, Value>> {
    let mut merged = HashMap::with_capacity(definitions.len());

    for (name, def) in definitions {
        if !value_matches_declared_type(&def.default, &def.var_type) {
            return Err(RustmotionError::Generic(format!(
                "Variable '${name}' in '{path}' is declared as type \"{declared}\" but its \
                 default value is a {actual}",
                declared = declared_type_name(&def.var_type),
                actual = json_type_name(&def.default),
            )));
        }
        merged.insert(name.clone(), def.default.clone());
    }

    if let Some(ovr) = overrides {
        for (name, value) in ovr {
            let def = definitions
                .get(name)
                .ok_or_else(|| RustmotionError::UndefinedVariable {
                    name: name.clone(),
                    path: path.to_string(),
                })?;
            if !value_matches_declared_type(value, &def.var_type) {
                return Err(RustmotionError::Generic(format!(
                    "Variable '${name}' in '{path}' is declared as type \"{declared}\" but the \
                     override value is a {actual}",
                    declared = declared_type_name(&def.var_type),
                    actual = json_type_name(value),
                )));
            }
            merged.insert(name.clone(), value.clone());
        }
    }

    Ok(merged)
}

pub(crate) fn substitute(
    value: &mut Value,
    vars: &HashMap<String, Value>,
    path: &str,
) -> Result<()> {
    match value {
        Value::String(s) => {
            if let Some(var_name) = parse_single_var_ref(s) {
                if let Some(replacement) = vars.get(var_name) {
                    *value = replacement.clone();
                    return Ok(());
                }
                return Ok(());
            }

            if s.contains('$') {
                let result = interpolate_string(s, vars, path)?;
                *s = result;
            }
        }
        Value::Object(map) => {
            if map.len() == 1 {
                if let Some(var_name_val) = map.get("$var") {
                    if let Some(var_name) = var_name_val.as_str() {
                        if let Some(replacement) = vars.get(var_name) {
                            *value = replacement.clone();
                            return Ok(());
                        }
                        return Ok(());
                    }
                }
            }

            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                if key == "config" {
                    continue;
                }
                if let Some(v) = map.get_mut(&key) {
                    substitute(v, vars, path)?;
                }
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                substitute(item, vars, path)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn parse_single_var_ref(s: &str) -> Option<&str> {
    let s = s.trim();
    if !s.starts_with('$') || s.starts_with("$$") {
        return None;
    }
    let name = &s[1..];
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    if s.len() != 1 + name.len() {
        return None;
    }
    Some(name)
}

fn interpolate_string(s: &str, vars: &HashMap<String, Value>, path: &str) -> Result<String> {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '$' {
            if chars.peek() == Some(&'$') {
                chars.next();
                result.push('$');
            } else {
                let mut name = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_alphanumeric() || c == '_' {
                        name.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if name.is_empty() {
                    result.push('$');
                } else if let Some(val) = vars.get(&name) {
                    match val {
                        Value::String(s) => result.push_str(s),
                        Value::Number(n) => result.push_str(&n.to_string()),
                        Value::Bool(b) => result.push_str(&b.to_string()),
                        _ => {
                            return Err(RustmotionError::VariableInterpolationTypeError {
                                name,
                                path: path.to_string(),
                            });
                        }
                    }
                } else {
                    result.push('$');
                    result.push_str(&name);
                }
            }
        } else {
            result.push(ch);
        }
    }

    Ok(result)
}

pub fn find_unresolved(value: &Value) -> Vec<String> {
    let mut unresolved = Vec::new();
    find_unresolved_recursive(value, &mut unresolved);
    unresolved
}

fn find_unresolved_recursive(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(s) => {
            if s.trim_start().starts_with('=') {
                return;
            }
            let mut chars = s.chars().peekable();
            while let Some(ch) = chars.next() {
                if ch == '$' {
                    if chars.peek() == Some(&'$') {
                        chars.next();
                    } else {
                        let mut name = String::new();
                        while let Some(&c) = chars.peek() {
                            if c.is_alphanumeric() || c == '_' {
                                name.push(c);
                                chars.next();
                            } else {
                                break;
                            }
                        }
                        if !name.is_empty() {
                            out.push(name);
                        }
                    }
                }
            }
        }
        Value::Object(map) => {
            if map.len() == 1 {
                if let Some(val) = map.get("$var") {
                    if let Some(name) = val.as_str() {
                        out.push(name.to_string());
                        return;
                    }
                }
            }
            for (key, v) in map {
                if !matches!(key.as_str(), "config" | "template" | "props" | "components") {
                    find_unresolved_recursive(v, out);
                }
            }
        }
        Value::Array(arr) => {
            for item in arr {
                find_unresolved_recursive(item, out);
            }
        }
        _ => {}
    }
}

pub fn apply_variables(
    value: &mut Value,
    overrides: Option<&HashMap<String, Value>>,
    path: &str,
) -> Result<()> {
    let definitions = extract_variable_definitions(value)?;

    match definitions {
        Some(defs) => {
            for (name, def) in &defs {
                if def.default.is_null() {
                    return Err(RustmotionError::VariableMissingDefault {
                        name: name.clone(),
                        path: path.to_string(),
                    });
                }
            }

            let merged = merge_variables(&defs, overrides, path)?;
            if let Value::Object(map) = value {
                map.remove("config");
            }
            substitute(value, &merged, path)?;
        }
        None => {
            if let Some(ovr) = overrides {
                if !ovr.is_empty() {
                    substitute(value, ovr, path)?;
                }
            }
        }
    }

    for name in find_unresolved(value) {
        let diagnostic = RustmotionError::UnresolvedVariable {
            name,
            path: path.to_string(),
        };
        eprintln!(
            "Warning: {diagnostic} — either a typo'd variable name or literal '$' content (a \
             price, a shell $PATH, ...); the literal text is kept as-is instead of failing the \
             render."
        );
    }

    Ok(())
}

pub fn apply_defaults(value: &mut Value) -> Result<()> {
    apply_variables(value, None, "<root>")
}

fn extract_variable_definitions(
    value: &Value,
) -> Result<Option<HashMap<String, VariableDefinition>>> {
    if let Value::Object(map) = value {
        if let Some(vars_val) = map.get("config") {
            let defs: HashMap<String, VariableDefinition> =
                serde_json::from_value(vars_val.clone())?;
            return Ok(Some(defs));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_simple_string_substitution() {
        let mut val = json!({
            "text": "$greeting"
        });
        let mut vars = HashMap::new();
        vars.insert("greeting".to_string(), json!("Hello World"));
        substitute(&mut val, &vars, "test").unwrap();
        assert_eq!(val["text"], json!("Hello World"));
    }

    #[test]
    fn test_number_substitution_preserves_type() {
        let mut val = json!({
            "count": "$num"
        });
        let mut vars = HashMap::new();
        vars.insert("num".to_string(), json!(42));
        substitute(&mut val, &vars, "test").unwrap();
        assert_eq!(val["count"], json!(42));
    }

    #[test]
    fn test_var_object_syntax() {
        let mut val = json!({
            "count": { "$var": "num" }
        });
        let mut vars = HashMap::new();
        vars.insert("num".to_string(), json!(100));
        substitute(&mut val, &vars, "test").unwrap();
        assert_eq!(val["count"], json!(100));
    }

    #[test]
    fn test_string_interpolation() {
        let mut val = json!({
            "text": "Hello $name, welcome!"
        });
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), json!("Alice"));
        substitute(&mut val, &vars, "test").unwrap();
        assert_eq!(val["text"], json!("Hello Alice, welcome!"));
    }

    #[test]
    fn test_escape_dollar() {
        let mut val = json!({
            "text": "Price: $$100"
        });
        let vars = HashMap::new();
        substitute(&mut val, &vars, "test").unwrap();
        assert_eq!(val["text"], json!("Price: $100"));
    }

    #[test]
    fn test_merge_variables_rejects_undefined() {
        let mut defs = HashMap::new();
        defs.insert(
            "color".to_string(),
            VariableDefinition {
                var_type: crate::schema::VariableType::String,
                default: json!("#000"),
                description: None,
            },
        );
        let mut overrides = HashMap::new();
        overrides.insert("unknown".to_string(), json!("value"));

        let result = merge_variables(&defs, Some(&overrides), "test.json");
        assert!(result.is_err());
    }

    #[test]
    fn test_merge_variables_rejects_type_mismatched_override() {
        let mut defs = HashMap::new();
        defs.insert(
            "count".to_string(),
            VariableDefinition {
                var_type: crate::schema::VariableType::Number,
                default: json!(0),
                description: None,
            },
        );
        let mut overrides = HashMap::new();
        overrides.insert("count".to_string(), json!("not a number"));

        let result = merge_variables(&defs, Some(&overrides), "test.json");
        assert!(
            result.is_err(),
            "a string override for a declared `number` variable must be rejected"
        );
    }

    #[test]
    fn test_merge_variables_accepts_type_matched_override() {
        let mut defs = HashMap::new();
        defs.insert(
            "count".to_string(),
            VariableDefinition {
                var_type: crate::schema::VariableType::Number,
                default: json!(0),
                description: None,
            },
        );
        let mut overrides = HashMap::new();
        overrides.insert("count".to_string(), json!(42));

        let result = merge_variables(&defs, Some(&overrides), "test.json").unwrap();
        assert_eq!(result["count"], json!(42));
    }

    #[test]
    fn test_merge_variables_rejects_type_mismatched_default() {
        let mut defs = HashMap::new();
        defs.insert(
            "flag".to_string(),
            VariableDefinition {
                var_type: crate::schema::VariableType::Boolean,
                default: json!("yes"),
                description: None,
            },
        );

        let result = merge_variables(&defs, None, "test.json");
        assert!(
            result.is_err(),
            "a string default for a declared `boolean` variable must be rejected"
        );
    }

    #[test]
    fn test_interpolation_type_error() {
        let mut val = json!({
            "text": "value is $obj"
        });
        let mut vars = HashMap::new();
        vars.insert("obj".to_string(), json!({"key": "value"}));
        let result = substitute(&mut val, &vars, "test");
        assert!(result.is_err());
    }

    #[test]
    fn test_find_unresolved() {
        let val = json!({
            "text": "$missing",
            "nested": {
                "val": { "$var": "also_missing" }
            }
        });
        let unresolved = find_unresolved(&val);
        assert!(unresolved.contains(&"missing".to_string()));
        assert!(unresolved.contains(&"also_missing".to_string()));
    }

    #[test]
    fn find_unresolved_does_not_scan_inside_an_expression_string() {
        let val = json!({
            "x": "= $W/2 + cos($i / $count * TAU) * 700",
            "y": "  = $H/2",
            "plain": "$still_flagged"
        });
        let unresolved = find_unresolved(&val);
        assert!(
            !unresolved
                .iter()
                .any(|n| n == "W" || n == "H" || n == "i" || n == "count"),
            "expression identifiers must not be reported as unresolved variables: {unresolved:?}"
        );
        assert!(
            unresolved.contains(&"still_flagged".to_string()),
            "a plain (non-expression) string must still be scanned: {unresolved:?}"
        );
    }

    #[test]
    fn test_apply_defaults() {
        let mut val = json!({
            "config": {
                "color": { "type": "string", "default": "#FF0000" }
            },
            "video": { "width": 1080, "height": 1920 },
            "scenes": [
                { "duration": 5.0, "children": [
                    { "type": "text", "content": "Color is $color" }
                ]}
            ]
        });
        apply_defaults(&mut val).unwrap();
        assert_eq!(
            val["scenes"][0]["children"][0]["content"],
            json!("Color is #FF0000")
        );
        assert!(val.get("config").is_none());
    }

    #[test]
    fn test_config_key_not_substituted() {
        let mut val = json!({
            "config": {
                "name": { "type": "string", "default": "$not_a_ref" }
            },
            "text": "$name"
        });
        let mut vars = HashMap::new();
        vars.insert("name".to_string(), json!("resolved"));
        substitute(&mut val, &vars, "test").unwrap();
        assert_eq!(val["config"]["name"]["default"], json!("$not_a_ref"));
        assert_eq!(val["text"], json!("resolved"));
    }

    #[test]
    fn test_recursive_array_substitution() {
        let mut val = json!(["$a", ["$b", "$c"]]);
        let mut vars = HashMap::new();
        vars.insert("a".to_string(), json!(1));
        vars.insert("b".to_string(), json!(2));
        vars.insert("c".to_string(), json!(3));
        substitute(&mut val, &vars, "test").unwrap();
        assert_eq!(val, json!([1, [2, 3]]));
    }

    #[test]
    fn test_number_interpolation_in_string() {
        let mut val = json!({
            "text": "Count: $num items"
        });
        let mut vars = HashMap::new();
        vars.insert("num".to_string(), json!(42));
        substitute(&mut val, &vars, "test").unwrap();
        assert_eq!(val["text"], json!("Count: 42 items"));
    }

    fn doc_with_literal_dollar_no_config() -> serde_json::Value {
        json!({
            "video": { "width": 1080, "height": 1920 },
            "scenes": [{
                "duration": 3.0,
                "children": [
                    { "type": "list", "items": ["echo $PATH", "cd $HOME/project"] },
                    { "type": "text", "content": "Price: $100 today only" }
                ]
            }]
        })
    }

    fn doc_with_literal_dollar_and_unrelated_config() -> serde_json::Value {
        json!({
            "config": {
                "title": { "type": "string", "default": "Demo" }
            },
            "video": { "width": 1080, "height": 1920 },
            "scenes": [{
                "duration": 3.0,
                "children": [
                    { "type": "text", "content": "$title" },
                    { "type": "list", "items": ["echo $PATH", "cd $HOME/project"] },
                    { "type": "text", "content": "Price: $100 today only" }
                ]
            }]
        })
    }

    #[test]
    fn literal_dollar_without_config_block_already_succeeds() {
        let mut doc = doc_with_literal_dollar_no_config();
        apply_defaults(&mut doc)
            .expect("a literal '$' in list/text content with no config block must not be fatal");
        assert_eq!(
            doc["scenes"][0]["children"][0]["items"][0],
            json!("echo $PATH")
        );
    }

    #[test]
    fn literal_dollar_with_unrelated_config_block_must_not_be_fatal() {
        let mut doc = doc_with_literal_dollar_and_unrelated_config();
        apply_defaults(&mut doc).expect(
            "a literal '$' in unrelated content must not become fatal just because the \
             document also happens to declare an unrelated `config` block",
        );
        assert_eq!(doc["scenes"][0]["children"][0]["content"], json!("Demo"));
        assert_eq!(
            doc["scenes"][0]["children"][1]["items"][0],
            json!("echo $PATH")
        );
        assert_eq!(
            doc["scenes"][0]["children"][2]["content"],
            json!("Price: $100 today only")
        );
    }

    #[test]
    fn undeclared_override_is_still_a_hard_error_unaffected_by_the_fix() {
        let mut doc = json!({
            "config": { "title": { "type": "string", "default": "Demo" } },
            "video": { "width": 1, "height": 1 },
            "scenes": []
        });
        let mut overrides = HashMap::new();
        overrides.insert("nope".to_string(), json!("x"));
        let err = apply_variables(&mut doc, Some(&overrides), "test.json")
            .expect_err("an override referencing an undeclared variable must still be rejected");
        assert!(matches!(
            err,
            crate::error::RustmotionError::UndefinedVariable { .. }
        ));
    }

    #[test]
    fn declared_variable_reference_still_resolves_with_no_override() {
        let mut doc = json!({
            "config": { "greeting": { "type": "string", "default": "Hello" } },
            "video": { "width": 1, "height": 1 },
            "scenes": [{ "duration": 1.0, "children": [
                { "type": "text", "content": "$greeting" }
            ]}]
        });
        apply_defaults(&mut doc).unwrap();
        assert_eq!(doc["scenes"][0]["children"][0]["content"], json!("Hello"));
    }
}
