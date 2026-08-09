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
///
/// When a `config` block is present, overrides must reference declared variables (unknown
/// names produce `UndefinedVariable`).
///
/// When there is **no** `config` block but `overrides` are provided (e.g. from the CLI for
/// an HTML scenario that cannot carry a `config` key), the overrides are applied as raw
/// value substitutions without type declarations — any `$name` found in the document is
/// replaced by the override value as-is. Unresolved references after this pass are ignored
/// (no `UnresolvedVariable` error), because the document may legitimately contain no
/// variable references at all.
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
        }
        None => {
            // No config block. If overrides were supplied (e.g. from the CLI for an HTML
            // scenario), apply them as raw substitutions — no declaration required.
            if let Some(ovr) = overrides {
                if !ovr.is_empty() {
                    substitute(value, ovr, path)?;
                }
            }
        }
    }

    // Constat #7: `find_unresolved` used to run — and hard-fail the whole
    // render/validate on its first hit — *only* inside the `Some(defs)`
    // branch above, so the exact same leftover `$word` (a price tag, a
    // terminal `$PATH`, a shell `$HOME`) was harmless in a document with no
    // `config` block and fatal the moment an unrelated `config` block
    // existed anywhere else in the same file. `find_unresolved` cannot
    // structurally tell a genuine unresolved-reference typo apart from
    // incidental literal-`$` content — by construction, every name in
    // `defs` above is always present in `merged` (defaults ∪ overrides), so
    // `substitute` can never leave a *declared* variable name unresolved;
    // everything `find_unresolved` can still find here is, definitionally,
    // *not* one of the variables this document declared. So: run the same
    // scan unconditionally (fixing the "depends on an unrelated key"
    // inconsistency), but report it as a loud warning rather than aborting
    // the whole document — same fail-loud-not-silent contract already used
    // elsewhere in this workstream (see `css::units::px_or_warn`), applied
    // here because a hard rejection would break any existing scenario that
    // legitimately has a `$` in its content and would newly break every one
    // of those the moment it also gained a `config` block.
    for name in find_unresolved(value) {
        // Reuse `UnresolvedVariable`'s existing `Display` message (see
        // `error.rs`) for the warning text instead of hand-rolling a new
        // one — this is the same diagnostic, just no longer fatal.
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

    // ---- constat #7: literal `$` fatality must not depend on an unrelated
    // `config` key (RED first) ----

    /// A document with **no** `config` block and a literal `$` in unrelated
    /// content (a `terminal` line's `$PATH`) — this already succeeds today
    /// (the bug is the *other* direction; this locks in it keeps working).
    fn doc_with_literal_dollar_no_config() -> serde_json::Value {
        json!({
            "video": { "width": 1080, "height": 1920 },
            "scenes": [{
                "duration": 3.0,
                "children": [
                    { "type": "terminal", "lines": ["echo $PATH", "cd $HOME/project"] },
                    { "type": "text", "content": "Price: $100 today only" }
                ]
            }]
        })
    }

    /// The exact same literal-`$` content, but the document also happens to
    /// declare an unrelated `config` block (e.g. because it's a reusable
    /// template with one templated field). Before the fix, this made
    /// `apply_variables` return `Err(UnresolvedVariable)` and abort the
    /// entire render/validate — for content the config block has nothing to
    /// do with.
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
                    { "type": "terminal", "lines": ["echo $PATH", "cd $HOME/project"] },
                    { "type": "text", "content": "Price: $100 today only" }
                ]
            }]
        })
    }

    #[test]
    fn literal_dollar_without_config_block_already_succeeds() {
        let mut doc = doc_with_literal_dollar_no_config();
        apply_defaults(&mut doc).expect(
            "a literal '$' in terminal/text content with no config block must not be fatal",
        );
        // Content is left as-is: nothing declared these as variables.
        assert_eq!(
            doc["scenes"][0]["children"][0]["lines"][0],
            json!("echo $PATH")
        );
    }

    #[test]
    fn literal_dollar_with_unrelated_config_block_must_not_be_fatal() {
        // RED before the fix: this currently returns
        // `Err(UnresolvedVariable { name: "PATH", .. })` (or "HOME", or
        // "100", whichever `find_unresolved` reaches first) purely because
        // *some* config block exists elsewhere in the same document — the
        // exact inconsistency named in constat #7. The declared `$title`
        // variable must still resolve correctly either way.
        let mut doc = doc_with_literal_dollar_and_unrelated_config();
        apply_defaults(&mut doc).expect(
            "a literal '$' in unrelated content must not become fatal just because the \
             document also happens to declare an unrelated `config` block",
        );
        assert_eq!(doc["scenes"][0]["children"][0]["content"], json!("Demo"));
        assert_eq!(
            doc["scenes"][0]["children"][1]["lines"][0],
            json!("echo $PATH")
        );
        assert_eq!(
            doc["scenes"][0]["children"][2]["content"],
            json!("Price: $100 today only")
        );
    }

    #[test]
    fn undeclared_override_is_still_a_hard_error_unaffected_by_the_fix() {
        // The other half of `apply_variables`'s error surface (an override
        // key that doesn't match any declared variable) is a genuine,
        // unambiguous user error — unrelated to the literal-`$`-in-content
        // ambiguity — and must remain a hard error.
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
