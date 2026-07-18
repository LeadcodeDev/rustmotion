use std::collections::HashMap;

use crate::error::Result;
use serde_json::Value;

use crate::error::RustmotionError;
use crate::schema::VariableDefinition;

/// Build the final variable map: start from defaults, then apply overrides.
/// Returns an error if an override references a variable not in the definitions.
fn merge_variables(
    definitions: &HashMap<String, VariableDefinition>,
    overrides: Option<&HashMap<String, Value>>,
    path: &str,
) -> Result<HashMap<String, Value>> {
    let mut merged = HashMap::with_capacity(definitions.len());

    // Start with defaults
    for (name, def) in definitions {
        merged.insert(name.clone(), def.default.clone());
    }

    // Apply overrides
    if let Some(ovr) = overrides {
        for (name, value) in ovr {
            if !definitions.contains_key(name) {
                return Err(RustmotionError::UndefinedVariable {
                    name: name.clone(),
                    path: path.to_string(),
                });
            }
            merged.insert(name.clone(), value.clone());
        }
    }

    Ok(merged)
}

/// Recursively substitute variable references in a JSON value tree.
fn substitute(value: &mut Value, vars: &HashMap<String, Value>, path: &str) -> Result<()> {
    match value {
        Value::String(s) => {
            // Check for exact match "$name" (whole-string replacement, preserves type)
            if let Some(var_name) = parse_single_var_ref(s) {
                if let Some(replacement) = vars.get(var_name) {
                    *value = replacement.clone();
                    return Ok(());
                }
                // Not in vars — leave as-is for find_unresolved to catch
                return Ok(());
            }

            // Check for escaped $$ or interpolation
            if s.contains('$') {
                let result = interpolate_string(s, vars, path)?;
                *s = result;
            }
        }
        Value::Object(map) => {
            // Check for { "$var": "name" } pattern
            if map.len() == 1 {
                if let Some(var_name_val) = map.get("$var") {
                    if let Some(var_name) = var_name_val.as_str() {
                        if let Some(replacement) = vars.get(var_name) {
                            *value = replacement.clone();
                            return Ok(());
                        }
                        // Not found — leave as-is
                        return Ok(());
                    }
                }
            }

            // Recurse into object values, but skip "variables" key (don't substitute in definitions)
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

/// Parse a string that is exactly "$name" (single variable reference, no interpolation).
/// Returns the variable name without the leading $.
fn parse_single_var_ref(s: &str) -> Option<&str> {
    let s = s.trim();
    if !s.starts_with('$') || s.starts_with("$$") {
        return None;
    }
    let name = &s[1..];
    // Must be a simple identifier (alphanumeric + underscore)
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    // Only match if the entire string is just "$name" — no surrounding text
    if s.len() != 1 + name.len() {
        return None;
    }
    Some(name)
}

/// Perform string interpolation: replace $name occurrences within a larger string.
/// Handles $$ escape sequences.
fn interpolate_string(s: &str, vars: &HashMap<String, Value>, path: &str) -> Result<String> {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '$' {
            if chars.peek() == Some(&'$') {
                // Escaped $$  → literal $
                chars.next();
                result.push('$');
            } else {
                // Try to read a variable name
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
                    // Lone $ not followed by identifier — keep as-is
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
                    // Unknown variable — keep original text for find_unresolved
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

/// Scan a Value tree for unresolved $variable references after substitution.
pub fn find_unresolved(value: &Value) -> Vec<String> {
    let mut unresolved = Vec::new();
    find_unresolved_recursive(value, &mut unresolved);
    unresolved
}

fn find_unresolved_recursive(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(s) => {
            let mut chars = s.chars().peekable();
            while let Some(ch) = chars.next() {
                if ch == '$' {
                    if chars.peek() == Some(&'$') {
                        chars.next(); // skip escaped
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
            // Check for { "$var": "name" }
            if map.len() == 1 {
                if let Some(val) = map.get("$var") {
                    if let Some(name) = val.as_str() {
                        out.push(name.to_string());
                        return;
                    }
                }
            }
            for (key, v) in map {
                if key != "config" {
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

/// Apply variable substitution to a JSON Value.
/// Extracts the "variables" definitions, merges with optional overrides, then substitutes.
pub fn apply_variables(
    value: &mut Value,
    overrides: Option<&HashMap<String, Value>>,
    path: &str,
) -> Result<()> {
    let definitions = extract_variable_definitions(value)?;

    match definitions {
        Some(defs) => {
            // Validate that every definition has a default
            for (name, def) in &defs {
                if def.default.is_null() {
                    return Err(RustmotionError::VariableMissingDefault {
                        name: name.clone(),
                        path: path.to_string(),
                    });
                }
            }

            let merged = merge_variables(&defs, overrides, path)?;
            // Remove "config" key from the value so it doesn't interfere with deserialization
            if let Value::Object(map) = value {
                map.remove("config");
            }
            substitute(value, &merged, path)?;

            // Check for unresolved references
            let unresolved = find_unresolved(value);
            if let Some(name) = unresolved.into_iter().next() {
                return Err(RustmotionError::UnresolvedVariable {
                    name,
                    path: path.to_string(),
                });
            }

            Ok(())
        }
        None => Ok(()),
    }
}

/// For standalone rendering: apply defaults only (no overrides).
pub fn apply_defaults(value: &mut Value) -> Result<()> {
    apply_variables(value, None, "<root>")
}

/// Extract variable definitions from a JSON value (if present).
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
        // "config" key should be removed
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
        // "config" block should be untouched
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
}
