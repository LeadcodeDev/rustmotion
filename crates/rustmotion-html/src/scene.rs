use markup5ever_rcdom::Handle;
use serde_json::{Map, Value};

use crate::element::children_to_values;
use crate::element_attrs;
use crate::style::coerce_value;
use crate::HtmlError;

/// Map a `<scene>` element to a scene JSON object. Defaults to a centered flex
/// layout (overridable via `align`/`justify`/`direction`/`gap`/`padding` attrs).
pub(crate) fn scene_to_value(handle: &Handle) -> Result<Value, HtmlError> {
    let attrs = element_attrs(handle);
    let get = |k: &str| attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());

    let duration = get("duration").ok_or(HtmlError::MissingDuration)?;
    let mut obj = Map::new();
    obj.insert("duration".into(), coerce_value(&duration));

    let mut layout = Map::new();
    layout.insert(
        "align_items".into(),
        Value::from(get("align").unwrap_or_else(|| "center".into())),
    );
    layout.insert(
        "justify_content".into(),
        Value::from(get("justify").unwrap_or_else(|| "center".into())),
    );
    if let Some(d) = get("direction") {
        layout.insert("direction".into(), Value::from(d));
    }
    if let Some(g) = get("gap") {
        layout.insert("gap".into(), coerce_value(&g));
    }
    if let Some(p) = get("padding") {
        layout.insert("padding".into(), coerce_value(&p));
    }
    obj.insert("layout".into(), Value::Object(layout));

    if let Some(t) = get("transition") {
        obj.insert("transition".into(), serde_json::json!({ "type": t }));
    }

    obj.insert("children".into(), Value::Array(children_to_values(handle)));
    Ok(Value::Object(obj))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn scene_has_duration_centered_layout_and_children() {
        let dom = crate::parse_fragment_dom(
            r#"<scene duration="4"><h1 style="font-size:96">Hi</h1></scene>"#,
        );
        let el = crate::find_element(&dom.document, "scene").unwrap();
        let v = scene_to_value(&el).unwrap();
        assert_eq!(v["duration"], json!(4));
        assert_eq!(v["layout"]["align_items"], json!("center"));
        assert_eq!(v["layout"]["justify_content"], json!("center"));
        assert_eq!(v["children"][0]["type"], json!("text"));
    }

    #[test]
    fn scene_transition_attribute() {
        let dom = crate::parse_fragment_dom(r#"<scene duration="2" transition="fade"></scene>"#);
        let el = crate::find_element(&dom.document, "scene").unwrap();
        let v = scene_to_value(&el).unwrap();
        assert_eq!(v["transition"]["type"], json!("fade"));
    }
}
