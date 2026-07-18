use markup5ever_rcdom::{Handle, NodeData};
use serde_json::{Map, Value};

use crate::style::{coerce_value, parse_anim_attr, parse_inline_style};
use crate::{element_attrs, tag_name, HtmlError};

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

/// Pull `style="..."` and `anim="..."` from an element's attributes into one
/// JSON style object. `anim` (JSON or compact DSL, see
/// [`crate::style::parse_anim_attr`]) lands in `style.animation` — inline CSS
/// cannot express animation arrays, so `anim` is the only writer of that key.
fn style_object(attrs: &[(String, String)]) -> Result<Option<Value>, HtmlError> {
    let mut map = attrs
        .iter()
        .find(|(k, _)| k == "style")
        .map(|(_, raw)| parse_inline_style(raw))
        .unwrap_or_default();
    if let Some((_, anim)) = attrs.iter().find(|(k, _)| k == "anim") {
        map.insert("animation".into(), parse_anim_attr(anim)?);
    }
    if map.is_empty() {
        Ok(None)
    } else {
        Ok(Some(Value::Object(map)))
    }
}

/// Map a single element handle to its component JSON value, or `None` to skip.
pub(crate) fn element_to_value(handle: &Handle) -> Result<Option<Value>, HtmlError> {
    let Some(tag) = tag_name(handle) else {
        return Ok(None);
    };
    let attrs = element_attrs(handle);
    match tag_kind(&tag) {
        TagKind::Text => {
            let mut obj = Map::new();
            obj.insert("type".into(), Value::from("text"));
            obj.insert("content".into(), Value::from(inner_text(handle)));
            if let Some(style) = style_object(&attrs)? {
                obj.insert("style".into(), style);
            }
            Ok(Some(Value::Object(obj)))
        }
        TagKind::Container => {
            let mut obj = Map::new();
            obj.insert("type".into(), Value::from("div"));
            if let Some(style) = style_object(&attrs)? {
                obj.insert("style".into(), style);
            }
            let children = children_to_values(handle)?;
            if !children.is_empty() {
                obj.insert("children".into(), Value::Array(children));
            }
            Ok(Some(Value::Object(obj)))
        }
        TagKind::Custom(type_name) => {
            let mut obj = Map::new();
            obj.insert("type".into(), Value::from(type_name));
            for (k, v) in &attrs {
                if k == "style" || k == "class" || k == "anim" || v.is_empty() {
                    continue;
                }
                obj.insert(k.clone(), coerce_value(v));
            }
            if let Some(style) = style_object(&attrs)? {
                obj.insert("style".into(), style);
            }
            let children = children_to_values(handle)?;
            if !children.is_empty() {
                obj.insert("children".into(), Value::Array(children));
            }
            Ok(Some(Value::Object(obj)))
        }
    }
}

/// Map a container's children: element children via `element_to_value`, and
/// non-whitespace bare text nodes into `text` components.
pub(crate) fn children_to_values(handle: &Handle) -> Result<Vec<Value>, HtmlError> {
    let mut out = Vec::new();
    for child in handle.children.borrow().iter() {
        match &child.data {
            NodeData::Element { .. } => {
                if let Some(v) = element_to_value(child)? {
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
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn map_first(html: &str) -> Value {
        let dom = crate::parse_fragment_dom(html);
        let first = find_first_element(&dom.document).expect("an element");
        element_to_value(&first)
            .expect("no transpile error")
            .expect("maps to a value")
    }

    fn map_first_err(html: &str) -> crate::HtmlError {
        let dom = crate::parse_fragment_dom(html);
        let first = find_first_element(&dom.document).expect("an element");
        element_to_value(&first).expect_err("expected a transpile error")
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

    // --- anim attribute ---

    #[test]
    fn anim_json_array_goes_to_style_animation() {
        let v = map_first(r#"<h1 anim='[{"name":"fade_in_up","delay":0.3}]'>Hi</h1>"#);
        assert_eq!(
            v["style"]["animation"],
            json!([{ "name": "fade_in_up", "delay": 0.3 }])
        );
    }

    #[test]
    fn anim_json_object_is_wrapped_in_array() {
        let v = map_first(r#"<p anim='{"name":"pulse"}'>Hi</p>"#);
        assert_eq!(v["style"]["animation"], json!([{ "name": "pulse" }]));
    }

    #[test]
    fn anim_dsl_single_effect() {
        let v = map_first(r#"<h1 anim="fade-in-up delay:0.3 duration:0.8">Hi</h1>"#);
        assert_eq!(
            v["style"]["animation"],
            json!([{ "name": "fade_in_up", "delay": 0.3, "duration": 0.8 }])
        );
    }

    #[test]
    fn anim_dsl_multi_effects_with_bool() {
        let v = map_first(
            r#"<h1 anim="fade-in-up delay:0.3 duration:0.8; pulse delay:2 loop:true">Hi</h1>"#,
        );
        let anim = v["style"]["animation"].as_array().expect("array");
        assert_eq!(anim.len(), 2);
        assert_eq!(anim[0]["name"], json!("fade_in_up"));
        assert_eq!(
            anim[1],
            json!({ "name": "pulse", "delay": 2, "loop": true })
        );
    }

    #[test]
    fn anim_dsl_kebab_name_and_keys_become_snake() {
        let v = map_first(r#"<h1 anim="slide-in-left my-key:1">Hi</h1>"#);
        let effect = &v["style"]["animation"][0];
        assert_eq!(effect["name"], json!("slide_in_left"));
        assert_eq!(effect["my_key"], json!(1));
        assert!(
            effect.get("my-key").is_none(),
            "kebab key must be converted"
        );
    }

    #[test]
    fn anim_dsl_snake_case_accepted_as_is() {
        let v = map_first(r#"<h1 anim="fade_in_up delay:0.3">Hi</h1>"#);
        assert_eq!(v["style"]["animation"][0]["name"], json!("fade_in_up"));
    }

    #[test]
    fn anim_dsl_pair_without_colon_is_error() {
        let e = map_first_err(r#"<h1 anim="fade-in delay0.3">Hi</h1>"#);
        assert!(
            matches!(e, crate::HtmlError::InvalidAnimDsl(_)),
            "expected InvalidAnimDsl, got: {e:?}"
        );
    }

    #[test]
    fn anim_empty_is_error() {
        let e = map_first_err(r#"<h1 anim="">Hi</h1>"#);
        assert!(
            matches!(e, crate::HtmlError::InvalidAnimDsl(_)),
            "expected InvalidAnimDsl, got: {e:?}"
        );
    }

    #[test]
    fn anim_empty_effect_between_separators_is_error() {
        let e = map_first_err(r#"<h1 anim="fade-in; ; pulse">Hi</h1>"#);
        assert!(
            matches!(e, crate::HtmlError::InvalidAnimDsl(_)),
            "expected InvalidAnimDsl, got: {e:?}"
        );
    }

    #[test]
    fn anim_invalid_json_is_error() {
        let e = map_first_err(r#"<h1 anim="[not valid json]">Hi</h1>"#);
        assert!(
            matches!(e, crate::HtmlError::InvalidAnimJson(_)),
            "expected InvalidAnimJson, got: {e:?}"
        );
    }

    #[test]
    fn anim_with_existing_style_keeps_other_props() {
        let v = map_first(r##"<h1 style="font-size:96; color:#fff" anim="fade-in">Hi</h1>"##);
        assert_eq!(v["style"]["font-size"], json!(96));
        assert_eq!(v["style"]["color"], json!("#fff"));
        assert_eq!(v["style"]["animation"][0]["name"], json!("fade_in"));
    }

    #[test]
    fn anim_on_custom_element_is_not_a_root_field() {
        let v = map_first(r#"<rm-counter from="0" to="10" anim="fade-in"></rm-counter>"#);
        assert!(
            v.get("anim").is_none(),
            "anim must not leak as a component field: {v}"
        );
        assert_eq!(v["style"]["animation"][0]["name"], json!("fade_in"));
        assert_eq!(v["from"], json!(0));
    }
}
