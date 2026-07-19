use serde_json::Value;

/// Read a style property at the element addressed by `pointer`, e.g.
/// (`"/scenes/0/children/0"`, `"color"`). Returns the value as a string.
#[allow(dead_code)] // symmetric API counterpart to set_style; consumed as the inspector grows
pub fn read_style(raw: &Value, pointer: &str, prop: &str) -> Option<String> {
    let el = raw.pointer(pointer)?;
    let v = el.get("style")?.get(prop)?;
    Some(match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    })
}

/// Read the whole `style` object of the element at `pointer` (or `Null`). The
/// inspector renders every supported property from this single value, which
/// keeps it stable (frame-independent) for memoization.
pub fn read_style_object(raw: &Value, pointer: &str) -> Value {
    raw.pointer(pointer)
        .and_then(|el| el.get("style"))
        .cloned()
        .unwrap_or(Value::Null)
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

/// Read a top-level field (e.g. `"content"`) of the element at `pointer` as a
/// string. Unlike [`read_style`], this reads a field on the element itself, not
/// inside its `style` object.
pub fn read_field(raw: &Value, pointer: &str, field: &str) -> Option<String> {
    let v = raw.pointer(pointer)?.get(field)?;
    Some(match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    })
}

/// Set a top-level field (e.g. `"content"`, as a JSON string) on the element at
/// `pointer`. Returns the mutated clone; the caller writes it to disk.
pub fn set_field(mut raw: Value, pointer: &str, field: &str, value: &str) -> Option<Value> {
    let el = raw.pointer_mut(pointer)?;
    let obj = el.as_object_mut()?;
    obj.insert(field.to_string(), Value::String(value.to_string()));
    Some(raw)
}

/// Typed variant of [`set_field`]: writes the JSON value as-is (a number stays
/// a number, a bool a bool). `Value::Null` REMOVES the field — an emptied
/// control unsets rather than writing an empty string. Returns the mutated
/// clone.
pub fn set_field_value(mut raw: Value, pointer: &str, field: &str, value: Value) -> Option<Value> {
    let el = raw.pointer_mut(pointer)?;
    let obj = el.as_object_mut()?;
    if value.is_null() {
        obj.remove(field);
    } else {
        obj.insert(field.to_string(), value);
    }
    Some(raw)
}

/// Typed variant of [`set_style`]: writes the JSON value as-is into the
/// element's `style` object. `Value::Null` REMOVES the property. Returns the
/// mutated clone.
pub fn set_style_value(mut raw: Value, pointer: &str, prop: &str, value: Value) -> Option<Value> {
    let el = raw.pointer_mut(pointer)?;
    let obj = el.as_object_mut()?;
    if value.is_null() {
        if let Some(style) = obj.get_mut("style").and_then(|s| s.as_object_mut()) {
            style.remove(prop);
        }
        return Some(raw);
    }
    let style = obj
        .entry("style")
        .or_insert_with(|| Value::Object(Default::default()));
    let style_obj = style.as_object_mut()?;
    style_obj.insert(prop.to_string(), value);
    Some(raw)
}

/// Append an annotation object to `raw["annotations"]` (creating the array if
/// absent). Returns the mutated clone.
pub fn append_annotation(mut raw: Value, annotation: Value) -> Value {
    if let Some(obj) = raw.as_object_mut() {
        let arr = obj
            .entry("annotations")
            .or_insert_with(|| Value::Array(vec![]));
        if let Value::Array(a) = arr {
            a.push(annotation);
        }
    }
    raw
}

/// Remove the annotation with the given id from `raw["annotations"]`.
pub fn remove_annotation(mut raw: Value, id: &str) -> Value {
    if let Some(Value::Array(a)) = raw.get_mut("annotations") {
        a.retain(|x| x.get("id").and_then(|v| v.as_str()) != Some(id));
    }
    raw
}

/// List the annotations as (id, note, frame, kind) tuples for the panel.
pub fn list_annotations(raw: &Value) -> Vec<(String, String, u64, String)> {
    raw.get("annotations")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .map(|x| {
                    let id = x
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let note = x
                        .get("note")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let frame = x.get("frame").and_then(|v| v.as_u64()).unwrap_or(0);
                    let kind = x
                        .get("target")
                        .and_then(|t| t.get("kind"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    (id, note, frame, kind)
                })
                .collect()
        })
        .unwrap_or_default()
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

    #[test]
    fn set_field_value_keeps_json_types_round_trip() {
        let updated = set_field_value(raw(), "/scenes/0/children/0", "from", json!(250)).unwrap();
        let text = serde_json::to_string(&updated).unwrap();
        let back: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(back["scenes"][0]["children"][0]["from"], json!(250));
        assert!(
            back["scenes"][0]["children"][0]["from"].is_number(),
            "a number must stay a number, not a string"
        );
        let updated =
            set_field_value(updated, "/scenes/0/children/0", "wrap", json!(false)).unwrap();
        assert_eq!(updated["scenes"][0]["children"][0]["wrap"], json!(false));
    }

    #[test]
    fn set_field_value_null_removes_the_field() {
        let updated =
            set_field_value(raw(), "/scenes/0/children/0", "content", Value::Null).unwrap();
        assert!(
            updated["scenes"][0]["children"][0].get("content").is_none(),
            "null must remove the key"
        );
    }

    #[test]
    fn set_style_value_null_removes_the_property() {
        let updated = set_style_value(raw(), "/scenes/0/children/0", "color", Value::Null).unwrap();
        assert!(updated["scenes"][0]["children"][0]["style"]
            .get("color")
            .is_none());
        // Typed write keeps the number.
        let updated =
            set_style_value(updated, "/scenes/0/children/0", "font-size", json!(64)).unwrap();
        assert_eq!(
            updated["scenes"][0]["children"][0]["style"]["font-size"],
            json!(64)
        );
    }

    #[test]
    fn reads_and_sets_a_top_level_field() {
        assert_eq!(
            read_field(&raw(), "/scenes/0/children/0", "content").as_deref(),
            Some("Hi")
        );
        let updated = set_field(raw(), "/scenes/0/children/0", "content", "Hello").unwrap();
        assert_eq!(
            read_field(&updated, "/scenes/0/children/0", "content").as_deref(),
            Some("Hello")
        );
    }

    #[test]
    fn append_then_list_then_remove() {
        let raw = json!({ "video": { "width": 1, "height": 1 }, "scenes": [] });
        let ann = json!({ "id": "an_1", "note": "smaller", "status": "open", "frame": 5,
            "target": { "pointer": "/scenes/0/children/0", "kind": "text" } });
        let raw = append_annotation(raw, ann);
        let list = list_annotations(&raw);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].0, "an_1");
        assert_eq!(list[0].1, "smaller");
        assert_eq!(list[0].2, 5);
        assert_eq!(list[0].3, "text");
        let raw = remove_annotation(raw, "an_1");
        assert!(list_annotations(&raw).is_empty());
    }
}
