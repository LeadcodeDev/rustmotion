use markup5ever_rcdom::{Handle, NodeData};
use serde_json::{Map, Value};

use crate::style::{coerce_value, parse_inline_style};
use crate::{element_attrs, tag_name};

enum TagKind {
    Container,
    Text,
    Custom(String),
}

fn tag_kind(tag: &str) -> TagKind {
    match tag {
        "div" | "section" | "main" | "header" | "footer" | "article" => TagKind::Container,
        "p" | "span" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "strong" | "em" | "label" => {
            TagKind::Text
        }
        t if t.starts_with("rm-") => TagKind::Custom(t["rm-".len()..].to_string()),
        _ => TagKind::Container,
    }
}

/// Concatenated text of an element and all its descendants.
pub(crate) fn inner_text(handle: &Handle) -> String {
    let mut out = String::new();
    collect_text(handle, &mut out);
    out.trim().to_string()
}

fn collect_text(handle: &Handle, out: &mut String) {
    if let NodeData::Text { contents } = &handle.data {
        out.push_str(&contents.borrow());
    }
    for child in handle.children.borrow().iter() {
        collect_text(child, out);
    }
}

/// Pull `style="..."` from an element's attributes into a JSON style object.
fn style_object(attrs: &[(String, String)]) -> Option<Value> {
    let raw = attrs.iter().find(|(k, _)| k == "style").map(|(_, v)| v)?;
    let map = parse_inline_style(raw);
    if map.is_empty() {
        None
    } else {
        Some(Value::Object(map))
    }
}

/// Map a single element handle to its component JSON value, or `None` to skip.
pub(crate) fn element_to_value(handle: &Handle) -> Option<Value> {
    let tag = tag_name(handle)?;
    let attrs = element_attrs(handle);
    match tag_kind(&tag) {
        TagKind::Text => {
            let mut obj = Map::new();
            obj.insert("type".into(), Value::from("text"));
            obj.insert("content".into(), Value::from(inner_text(handle)));
            if let Some(style) = style_object(&attrs) {
                obj.insert("style".into(), style);
            }
            Some(Value::Object(obj))
        }
        TagKind::Container => {
            let mut obj = Map::new();
            obj.insert("type".into(), Value::from("div"));
            if let Some(style) = style_object(&attrs) {
                obj.insert("style".into(), style);
            }
            let children = children_to_values(handle);
            if !children.is_empty() {
                obj.insert("children".into(), Value::Array(children));
            }
            Some(Value::Object(obj))
        }
        TagKind::Custom(type_name) => {
            let mut obj = Map::new();
            obj.insert("type".into(), Value::from(type_name));
            for (k, v) in &attrs {
                if k == "style" || k == "class" || v.is_empty() {
                    continue;
                }
                obj.insert(k.clone(), coerce_value(v));
            }
            if let Some(style) = style_object(&attrs) {
                obj.insert("style".into(), style);
            }
            let children = children_to_values(handle);
            if !children.is_empty() {
                obj.insert("children".into(), Value::Array(children));
            }
            Some(Value::Object(obj))
        }
    }
}

/// Map a container's children: element children via `element_to_value`, and
/// non-whitespace bare text nodes into `text` components.
pub(crate) fn children_to_values(handle: &Handle) -> Vec<Value> {
    let mut out = Vec::new();
    for child in handle.children.borrow().iter() {
        match &child.data {
            NodeData::Element { .. } => {
                if let Some(v) = element_to_value(child) {
                    out.push(v);
                }
            }
            NodeData::Text { contents } => {
                let text = contents.borrow();
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    out.push(serde_json::json!({ "type": "text", "content": trimmed }));
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn map_first(html: &str) -> Value {
        let dom = crate::parse_fragment_dom(html);
        let first = find_first_element(&dom.document).expect("an element");
        element_to_value(&first).expect("maps to a value")
    }

    // Skips the html/head/body wrappers html5ever inserts around a fragment,
    // returning the first real content element.
    fn find_first_element(handle: &Handle) -> Option<Handle> {
        for child in handle.children.borrow().iter() {
            if let Some(tag) = crate::tag_name(child) {
                if tag == "html" || tag == "head" || tag == "body" {
                    if let Some(found) = find_first_element(child) {
                        return Some(found);
                    }
                    continue;
                }
                return Some(child.clone());
            }
            if let Some(found) = find_first_element(child) {
                return Some(found);
            }
        }
        None
    }

    #[test]
    fn div_maps_to_div_with_style_and_children() {
        let v = map_first(
            r#"<div style="gap:32; flex-direction:column"><h1 style="font-size:96">Hi</h1></div>"#,
        );
        assert_eq!(v["type"], json!("div"));
        assert_eq!(v["style"]["gap"], json!(32));
        assert_eq!(v["style"]["flex-direction"], json!("column"));
        assert_eq!(v["children"][0]["type"], json!("text"));
        assert_eq!(v["children"][0]["content"], json!("Hi"));
        assert_eq!(v["children"][0]["style"]["font-size"], json!(96));
    }

    #[test]
    fn paragraph_maps_to_text() {
        let v = map_first(r#"<p style="color:#fff">Built in Rust</p>"#);
        assert_eq!(v["type"], json!("text"));
        assert_eq!(v["content"], json!("Built in Rust"));
        assert_eq!(v["style"]["color"], json!("#fff"));
    }

    #[test]
    fn bare_text_in_container_becomes_text_component() {
        let v = map_first(r#"<div>loose text</div>"#);
        assert_eq!(v["children"][0]["type"], json!("text"));
        assert_eq!(v["children"][0]["content"], json!("loose text"));
    }

    #[test]
    fn custom_element_attrs_become_root_fields() {
        let v = map_first(r#"<rm-counter from="0" to="1250" suffix="€"></rm-counter>"#);
        assert_eq!(v["type"], json!("counter"));
        assert_eq!(v["from"], json!(0));
        assert_eq!(v["to"], json!(1250));
        assert_eq!(v["suffix"], json!("€"));
    }

    #[test]
    fn custom_element_keeps_style_and_children() {
        let v = map_first(r#"<rm-card style="padding:24"><p>inside</p></rm-card>"#);
        assert_eq!(v["type"], json!("card"));
        assert_eq!(v["style"]["padding"], json!(24));
        assert_eq!(v["children"][0]["type"], json!("text"));
        assert_eq!(v["children"][0]["content"], json!("inside"));
    }
}
