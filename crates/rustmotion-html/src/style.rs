use serde_json::{Map, Value};

/// Coerce a CSS value string into JSON. A bare number or `<n>px` becomes a JSON
/// number (integral → integer, so it deserializes into `u32`/`f32` fields);
/// everything else (`%`, `auto`, `fr`, colors, keywords) stays a string.
pub fn coerce_value(raw: &str) -> Value {
    let t = raw.trim();
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
}
