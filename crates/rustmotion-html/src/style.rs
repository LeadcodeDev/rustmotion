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

/// Parse an inline `style="a:b; c:d"` declaration list into a JSON style
/// object. `padding`/`margin`/`border-radius` accept the CSS 1/2/3/4-value
/// box shorthand, expanded into the `{top,right,bottom,left}` /
/// `{top-left,top-right,bottom-right,bottom-left}` object the core CSS
/// engine's `Edges`/`BorderRadius` types deserialize. `grid-template-columns`/
/// `-rows` accept a track list, `repeat()`/`minmax()` included. Any other
/// property whose value is more than one top-level (paren-aware) token is
/// refused rather than passed through as an opaque string the core length
/// parser cannot read (see [`HtmlError::UnsupportedStyleShorthand`]).
pub fn parse_inline_style(decls: &str) -> Result<Map<String, Value>, HtmlError> {
    let mut map = Map::new();
    for decl in decls.split(';') {
        let decl = decl.trim();
        if decl.is_empty() {
            continue;
        }
        let Some((prop, value)) = decl.split_once(':') else {
            return Err(HtmlError::StyleDeclarationMissingColon {
                decl: decl.to_string(),
            });
        };
        let prop = prop.trim().to_string();
        let value = value.trim();
        match prop.as_str() {
            "grid-template-columns" | "grid-template-rows" => {
                map.insert(
                    prop.clone(),
                    Value::Array(parse_grid_template(&prop, value)?),
                );
            }
            "padding" | "margin" => {
                let tokens = split_top_level_tokens(value);
                match tokens.len() {
                    1 => {
                        map.insert(prop, coerce_value(value));
                    }
                    2..=4 => {
                        map.insert(prop, expand_box_edges(&tokens));
                    }
                    _ => {
                        return Err(HtmlError::UnsupportedStyleShorthand {
                            prop,
                            value: value.to_string(),
                        })
                    }
                }
            }
            "border-radius" => {
                let tokens = split_top_level_tokens(value);
                match tokens.len() {
                    1 => {
                        map.insert(prop, coerce_value(value));
                    }
                    2..=4 => {
                        map.insert(prop, expand_border_radius_corners(&tokens));
                    }
                    _ => {
                        return Err(HtmlError::UnsupportedStyleShorthand {
                            prop,
                            value: value.to_string(),
                        })
                    }
                }
            }
            _ => {
                let trimmed = value.trim();
                if trimmed.starts_with('{') || trimmed.starts_with('[') {
                    let parsed: Value = serde_json::from_str(trimmed).map_err(|e| {
                        HtmlError::InvalidStylePropertyJson {
                            prop: prop.clone(),
                            value: value.to_string(),
                            error: e.to_string(),
                        }
                    })?;
                    map.insert(prop, parsed);
                } else if split_top_level_tokens(value).len() > 1 {
                    return Err(HtmlError::UnsupportedStyleShorthand {
                        prop,
                        value: value.to_string(),
                    });
                } else {
                    map.insert(prop, coerce_value(value));
                }
            }
        }
    }
    Ok(map)
}

/// Split a CSS value on top-level whitespace: whitespace inside a `(...)`
/// span (e.g. the argument list of `rgba(0, 0, 0, 0.5)` or `repeat(3, 1fr)`)
/// does not count as a separator, so a single functional-notation value
/// stays one token while a genuine multi-value shorthand (`24px 48px`)
/// splits into its parts.
fn split_top_level_tokens(s: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut depth = 0i32;
    let mut token_start: Option<usize> = None;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if c.is_whitespace() && depth == 0 {
            if let Some(start) = token_start.take() {
                tokens.push(&s[start..i]);
            }
        } else if token_start.is_none() {
            token_start = Some(i);
        }
    }
    if let Some(start) = token_start {
        tokens.push(&s[start..]);
    }
    tokens
}

/// Expand a 2/3/4-value `padding`/`margin` shorthand into the
/// `{top,right,bottom,left}` object `Edges::Sides` deserializes, following
/// the standard CSS clockwise-from-top expansion rule.
fn expand_box_edges(tokens: &[&str]) -> Value {
    let (top, right, bottom, left) = match tokens {
        [a, b] => (*a, *b, *a, *b),
        [a, b, c] => (*a, *b, *c, *b),
        [a, b, c, d] => (*a, *b, *c, *d),
        _ => unreachable!("caller only passes 2..=4 tokens"),
    };
    let mut edges = Map::new();
    edges.insert("top".into(), coerce_value(top));
    edges.insert("right".into(), coerce_value(right));
    edges.insert("bottom".into(), coerce_value(bottom));
    edges.insert("left".into(), coerce_value(left));
    Value::Object(edges)
}

/// Expand a 2/3/4-value `border-radius` shorthand into the
/// `{top-left,top-right,bottom-right,bottom-left}` object
/// `BorderRadius::Corners` deserializes, following the standard CSS
/// clockwise-from-top-left expansion rule (a different starting corner than
/// [`expand_box_edges`], per the CSS box-shorthand spec).
fn expand_border_radius_corners(tokens: &[&str]) -> Value {
    let (top_left, top_right, bottom_right, bottom_left) = match tokens {
        [a, b] => (*a, *b, *a, *b),
        [a, b, c] => (*a, *b, *c, *b),
        [a, b, c, d] => (*a, *b, *c, *d),
        _ => unreachable!("caller only passes 2..=4 tokens"),
    };
    let mut corners = Map::new();
    corners.insert("top-left".into(), coerce_value(top_left));
    corners.insert("top-right".into(), coerce_value(top_right));
    corners.insert("bottom-right".into(), coerce_value(bottom_right));
    corners.insert("bottom-left".into(), coerce_value(bottom_left));
    Value::Object(corners)
}

/// Parse a `grid-template-columns`/`-rows` track list into the flat
/// `Vec<GridTrack>` JSON the core CSS engine expects: `repeat(n, track)`
/// expands into `n` copies of `track`, `minmax(min, max)` becomes
/// `{"min":..,"max":..}`, and every other token passes through
/// [`coerce_value`] unchanged (a bare number for `fr`, a keyword string, or
/// an explicit length).
fn parse_grid_template(prop: &str, value: &str) -> Result<Vec<Value>, HtmlError> {
    let mut out = Vec::new();
    for token in split_top_level_tokens(value) {
        push_grid_track(prop, token, &mut out)?;
    }
    Ok(out)
}

fn push_grid_track(prop: &str, token: &str, out: &mut Vec<Value>) -> Result<(), HtmlError> {
    if let Some(inner) = token
        .strip_prefix("repeat(")
        .and_then(|s| s.strip_suffix(')'))
    {
        let (count_str, pattern) =
            inner
                .split_once(',')
                .ok_or_else(|| HtmlError::UnsupportedStyleShorthand {
                    prop: prop.to_string(),
                    value: token.to_string(),
                })?;
        let count: usize =
            count_str
                .trim()
                .parse()
                .map_err(|_| HtmlError::UnsupportedStyleShorthand {
                    prop: prop.to_string(),
                    value: token.to_string(),
                })?;
        let pattern_tokens = split_top_level_tokens(pattern.trim());
        if pattern_tokens.is_empty() {
            return Err(HtmlError::UnsupportedStyleShorthand {
                prop: prop.to_string(),
                value: token.to_string(),
            });
        }
        for _ in 0..count {
            for t in &pattern_tokens {
                out.push(parse_single_grid_track(prop, t)?);
            }
        }
        return Ok(());
    }
    out.push(parse_single_grid_track(prop, token)?);
    Ok(())
}

fn parse_single_grid_track(prop: &str, token: &str) -> Result<Value, HtmlError> {
    if let Some(inner) = token
        .strip_prefix("minmax(")
        .and_then(|s| s.strip_suffix(')'))
    {
        let (min_s, max_s) =
            inner
                .split_once(',')
                .ok_or_else(|| HtmlError::UnsupportedStyleShorthand {
                    prop: prop.to_string(),
                    value: token.to_string(),
                })?;
        let mut minmax = Map::new();
        minmax.insert("min".into(), coerce_value(min_s.trim()));
        minmax.insert("max".into(), coerce_value(max_s.trim()));
        return Ok(Value::Object(minmax));
    }
    Ok(coerce_value(token))
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
        let m = parse_inline_style("font-size:96px; color:#fff; text-align:center").unwrap();
        assert_eq!(m.get("font-size"), Some(&json!(96)));
        assert_eq!(m.get("color"), Some(&json!("#fff")));
        assert_eq!(m.get("text-align"), Some(&json!("center")));
    }

    #[test]
    fn grid_template_becomes_string_array() {
        let m = parse_inline_style("grid-template-columns: 1fr 1fr").unwrap();
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
