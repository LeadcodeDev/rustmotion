use serde_json::Value;

/// Read a style property at the element addressed by `pointer`, e.g.
/// (`"/scenes/0/children/0"`, `"color"`). Returns the value as a string.
pub fn read_style(raw: &Value, pointer: &str, prop: &str) -> Option<String> {
    let el = raw.pointer(pointer)?;
    let v = el.get("style")?.get(prop)?;
    Some(match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    })
}

/// Set a style property (as a JSON string) on the element at `pointer`.
/// Returns the mutated clone; the caller writes it to disk.
pub fn set_style(mut raw: Value, pointer: &str, prop: &str, value: &str) -> Option<Value> {
    let el = raw.pointer_mut(pointer)?;
    let obj = el.as_object_mut()?;
    let style = obj
        .entry("style")
        .or_insert_with(|| Value::Object(Default::default()));
    let style_obj = style.as_object_mut()?;
    style_obj.insert(prop.to_string(), Value::String(value.to_string()));
    Some(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn raw() -> Value {
        json!({ "video": { "width": 1, "height": 1 },
            "scenes": [ { "duration": 1.0, "children": [
                { "type": "text", "content": "Hi", "style": { "color": "#fff" } }
            ] } ] })
    }

    #[test]
    fn reads_existing_style_prop() {
        assert_eq!(
            read_style(&raw(), "/scenes/0/children/0", "color").as_deref(),
            Some("#fff")
        );
    }

    #[test]
    fn sets_and_reads_back_a_prop() {
        let updated = set_style(raw(), "/scenes/0/children/0", "color", "#ff0000").unwrap();
        assert_eq!(
            read_style(&updated, "/scenes/0/children/0", "color").as_deref(),
            Some("#ff0000")
        );
    }

    #[test]
    fn creates_style_object_when_absent() {
        let mut r = raw();
        r["scenes"][0]["children"][0]
            .as_object_mut()
            .unwrap()
            .remove("style");
        let updated = set_style(r, "/scenes/0/children/0", "font-size", "48").unwrap();
        assert_eq!(
            read_style(&updated, "/scenes/0/children/0", "font-size").as_deref(),
            Some("48")
        );
    }
}
