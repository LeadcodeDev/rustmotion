use serde_json::{Map, Value};

use crate::HtmlError;

/// Coerce a CSS value string into JSON. `true`/`false` become a JSON boolean
/// (aligned with [`coerce_dsl_value`] — without this, no `bool` schema field
/// is reachable from HTML: `auto_scroll`, `diff`, `loop`, `show_grid`,
/// `show_borders`, `pulse`, … all reject the JSON string `"true"`/`"false"`
/// that a naive coercion would otherwise produce). A bare number or `<n>px`
/// becomes a JSON number (integral → integer, so it deserializes into
/// `u32`/`f32` fields); everything else (`%`, `auto`, `fr`, colors, keywords)
/// stays a string.
pub fn coerce_value(raw: &str) -> Value {
    let t = raw.trim();
    if t == "true" {
        return Value::Bool(true);
    }
    if t == "false" {
        return Value::Bool(false);
    }
    let num = t.strip_suffix("px").unwrap_or(t).trim();
    if let Ok(f) = num.parse::<f64>() {
        if f.fract() == 0.0 && f.abs() < 9_007_199_254_740_992.0 {
            return Value::from(f as i64);
        }
        return Value::from(f);
    }
    Value::from(t.to_string())
}

/// Parse an inline `style="a:b; c:d"` declaration list into a JSON style object.
/// `grid-template-columns`/`-rows` are split into string arrays; all other
/// properties pass through their kebab-case name with a coerced value.
pub fn parse_inline_style(decls: &str) -> Map<String, Value> {
    let mut map = Map::new();
    for decl in decls.split(';') {
        let decl = decl.trim();
        if decl.is_empty() {
            continue;
        }
        let Some((prop, value)) = decl.split_once(':') else {
            continue;
        };
        let prop = prop.trim().to_string();
        let value = value.trim();
        if prop == "grid-template-columns" || prop == "grid-template-rows" {
            let arr: Vec<Value> = value
                .split_whitespace()
                .map(|t| Value::from(t.to_string()))
                .collect();
            map.insert(prop, Value::Array(arr));
        } else {
            map.insert(prop, coerce_value(value));
        }
    }
    map
}

/// Parse an `anim` attribute into the `style.animation` JSON array.
///
/// Two forms:
/// - JSON: `[{"name":"fade_in_up","delay":0.3}]` (array, inserted as-is) or
///   `{"name":"pulse"}` (single object, wrapped in an array);
/// - compact DSL: `fade-in-up delay:0.3 duration:0.8; pulse loop:true` —
///   effects separated by `;`, each effect is a preset name (kebab-case is
///   converted to snake_case) followed by space-separated `key:value` pairs.
///
/// Unknown preset names are not validated here (no schema dependency); the
/// typed deserialization of `style.animation` rejects them at validation time.
pub fn parse_anim_attr(raw: &str) -> Result<Value, HtmlError> {
    let t = raw.trim();
    if t.is_empty() {
        return Err(HtmlError::InvalidAnimDsl("empty anim attribute".into()));
    }
    if t.starts_with('[') {
        return serde_json::from_str(t).map_err(|e| HtmlError::InvalidAnimJson(e.to_string()));
    }
    if t.starts_with('{') {
        let v: Value =
            serde_json::from_str(t).map_err(|e| HtmlError::InvalidAnimJson(e.to_string()))?;
        return Ok(Value::Array(vec![v]));
    }

    let mut effects = Vec::new();
    for effect in t.split(';') {
        let effect = effect.trim();
        if effect.is_empty() {
            return Err(HtmlError::InvalidAnimDsl(format!(
                "empty effect in anim attribute '{raw}'"
            )));
        }
        let mut tokens = effect.split_whitespace();
        let name = tokens.next().expect("effect is non-empty");
        if name.contains(':') {
            return Err(HtmlError::InvalidAnimDsl(format!(
                "effect '{effect}' must start with a preset name, got '{name}'"
            )));
        }
        let mut obj = Map::new();
        obj.insert("name".into(), Value::from(kebab_to_snake(name)));
        for pair in tokens {
            let Some((k, v)) = pair.split_once(':') else {
                return Err(HtmlError::InvalidAnimDsl(format!(
                    "'{pair}' is not a key:value pair (in effect '{effect}')"
                )));
            };
            if k.is_empty() || v.is_empty() {
                return Err(HtmlError::InvalidAnimDsl(format!(
                    "'{pair}' has an empty key or value (in effect '{effect}')"
                )));
            }
            let key = kebab_to_snake(k);
            // `spring` is an object in the schema; the compact DSL cannot
            // express one, so `spring:true` coerces to `{}` (all SpringConfig
            // defaults) and `spring:false` is simply absent. Fine-grained
            // damping/stiffness/mass requires the JSON `anim` form.
            if key == "spring" {
                match v {
                    "true" => {
                        obj.insert(key, Value::Object(Map::new()));
                    }
                    "false" => {}
                    other => {
                        return Err(HtmlError::InvalidAnimDsl(format!(
                            "spring only accepts true/false in the compact DSL \
                             (got 'spring:{other}'); use the JSON anim form for \
                             a full spring config"
                        )));
                    }
                }
                continue;
            }
            obj.insert(key, coerce_dsl_value(v));
        }
        effects.push(Value::Object(obj));
    }
    Ok(Value::Array(effects))
}

/// `fade-in-up` → `fade_in_up` (snake_case passes through unchanged).
fn kebab_to_snake(s: &str) -> String {
    s.replace('-', "_")
}

/// Coerce a DSL value: `true`/`false` → bool, number → JSON number
/// (integral → integer), anything else → string (e.g. easing names).
fn coerce_dsl_value(raw: &str) -> Value {
    match raw {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        _ => {
            if let Ok(f) = raw.parse::<f64>() {
                if f.fract() == 0.0 && f.abs() < 9_007_199_254_740_992.0 {
                    Value::from(f as i64)
                } else {
                    Value::from(f)
                }
            } else {
                Value::from(raw.to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn coerce_numbers_strings_and_px() {
        assert_eq!(coerce_value("96px"), json!(96));
        assert_eq!(coerce_value("32"), json!(32));
        assert_eq!(coerce_value("3.5"), json!(3.5));
        assert_eq!(coerce_value("50%"), json!("50%"));
        assert_eq!(coerce_value("center"), json!("center"));
        assert_eq!(coerce_value("#0f172a"), json!("#0f172a"));
        assert_eq!(coerce_value("1fr"), json!("1fr"));
    }

    #[test]
    fn coerce_value_true_false_become_json_booleans() {
        assert_eq!(coerce_value("true"), json!(true));
        assert_eq!(coerce_value("false"), json!(false));
        assert_eq!(coerce_value(" true "), json!(true), "trims whitespace too");
    }

    #[test]
    fn parses_declarations_into_style_object() {
        let m = parse_inline_style("font-size:96px; color:#fff; text-align:center");
        assert_eq!(m.get("font-size"), Some(&json!(96)));
        assert_eq!(m.get("color"), Some(&json!("#fff")));
        assert_eq!(m.get("text-align"), Some(&json!("center")));
    }

    #[test]
    fn grid_template_becomes_string_array() {
        let m = parse_inline_style("grid-template-columns: 1fr 1fr");
        assert_eq!(m.get("grid-template-columns"), Some(&json!(["1fr", "1fr"])));
    }

    #[test]
    fn anim_dsl_spring_true_coerces_to_default_object() {
        let v = parse_anim_attr("bounce-in spring:true duration:0.8").unwrap();
        assert_eq!(
            v,
            json!([{ "name": "bounce_in", "spring": {}, "duration": 0.8 }])
        );
    }

    #[test]
    fn anim_dsl_spring_false_is_absent() {
        let v = parse_anim_attr("bounce-in spring:false").unwrap();
        assert_eq!(v, json!([{ "name": "bounce_in" }]));
    }

    #[test]
    fn anim_dsl_spring_non_bool_is_an_error() {
        let err = parse_anim_attr("fade-in-up spring:8").unwrap_err();
        assert!(
            err.to_string().contains("spring"),
            "error should mention spring: {err}"
        );
    }

    #[test]
    fn anim_json_form_with_spring_object_passes_intact() {
        let v =
            parse_anim_attr(r#"[{"name":"fade_in_up","spring":{"damping":8,"stiffness":120}}]"#)
                .unwrap();
        assert_eq!(
            v,
            json!([{ "name": "fade_in_up", "spring": { "damping": 8, "stiffness": 120 } }])
        );
    }
}
