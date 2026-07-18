//! HTML/CSS → Rustmotion scenario JSON transpiler (browserless, compiled in).

mod element;
mod scene;
mod style;

use html5ever::serialize::{serialize, SerializeOpts, TraversalScope};
use html5ever::tendril::TendrilSink;
use html5ever::{local_name, ns, parse_fragment, Attribute, ParseOpts, QualName};
use markup5ever_rcdom::{Handle, Node, NodeData, RcDom, SerializableHandle};
use serde_json::{Map, Value};
use std::cell::RefCell;

/// Errors from transpiling HTML to a scenario value.
#[derive(Debug, thiserror::Error)]
pub enum HtmlError {
    #[error("no <rustmotion> root element found")]
    MissingRoot,
    #[error("<rustmotion> requires width and height attributes")]
    MissingDimensions,
    #[error("<rustmotion> has no <scene> elements")]
    NoScenes,
    #[error("<scene> requires a duration attribute")]
    MissingDuration,
}

/// Transpile an HTML-dialect document into the scenario `serde_json::Value` that
/// the JSON format uses. The result is fed straight into
/// `serde_json::from_value::<Scenario>` by the loader.
pub fn html_to_scenario_value(html: &str) -> Result<Value, HtmlError> {
    let dom = parse_fragment_dom(html);
    let root = find_element(&dom.document, "rustmotion").ok_or(HtmlError::MissingRoot)?;
    let attrs = element_attrs(&root);
    let get = |k: &str| attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());

    let width = get("width").ok_or(HtmlError::MissingDimensions)?;
    let height = get("height").ok_or(HtmlError::MissingDimensions)?;

    let mut video = Map::new();
    video.insert("width".into(), style::coerce_value(&width));
    video.insert("height".into(), style::coerce_value(&height));
    if let Some(fps) = get("fps") {
        video.insert("fps".into(), style::coerce_value(&fps));
    }
    if let Some(bg) = get("background") {
        video.insert("background".into(), Value::from(bg));
    }

    let mut scenes = Vec::new();
    for child in root.children.borrow().iter() {
        if tag_name(child).as_deref() == Some("scene") {
            scenes.push(scene::scene_to_value(child)?);
        }
    }
    if scenes.is_empty() {
        return Err(HtmlError::NoScenes);
    }

    Ok(serde_json::json!({ "video": Value::Object(video), "scenes": scenes }))
}

/// Parse an HTML fragment into an RcDom (browserless; html5ever).
pub(crate) fn parse_fragment_dom(html: &str) -> RcDom {
    parse_fragment(
        RcDom::default(),
        ParseOpts::default(),
        QualName::new(None, ns!(html), local_name!("div")),
        vec![],
        false,
    )
    .one(html.to_string())
}

/// The lowercase local tag name of an element handle, or `None` for non-elements.
pub(crate) fn tag_name(handle: &Handle) -> Option<String> {
    match &handle.data {
        NodeData::Element { name, .. } => Some(name.local.to_string()),
        _ => None,
    }
}

/// `(name, value)` pairs of an element's attributes (empty for non-elements).
pub(crate) fn element_attrs(handle: &Handle) -> Vec<(String, String)> {
    match &handle.data {
        NodeData::Element { attrs, .. } => attrs
            .borrow()
            .iter()
            .map(|a| (a.name.local.to_string(), a.value.to_string()))
            .collect(),
        _ => Vec::new(),
    }
}

/// Depth-first: the first descendant element with the given tag name.
pub(crate) fn find_element(handle: &Handle, tag: &str) -> Option<Handle> {
    for child in handle.children.borrow().iter() {
        if tag_name(child).as_deref() == Some(tag) {
            return Some(child.clone());
        }
        if let Some(found) = find_element(child, tag) {
            return Some(found);
        }
    }
    None
}

/// Set or replace an inline `style` property on the element addressed by the
/// JSON pointer (into the transpiled scenario), returning the rewritten HTML.
/// Used by the studio inspector to persist a property edit back into the HTML
/// source. The document is re-serialized (formatting is normalized). Returns
/// `None` if the pointer doesn't resolve to an element.
pub fn set_inline_style(html: &str, pointer: &str, prop: &str, value: &str) -> Option<String> {
    let dom = parse_fragment_dom(html);
    let root = find_element(&dom.document, "rustmotion")?;
    let target = resolve_pointer(&root, pointer)?;
    set_style_attr(&target, prop, value)?;
    Some(serialize_element(&root))
}

/// Replace the text content of the element addressed by the JSON pointer with
/// `text`, returning the rewritten HTML. Used by the studio inspector's content
/// editor (mirrors [`set_inline_style`]). Returns `None` if the pointer doesn't
/// resolve to an element.
pub fn set_text_content(html: &str, pointer: &str, text: &str) -> Option<String> {
    let dom = parse_fragment_dom(html);
    let root = find_element(&dom.document, "rustmotion")?;
    let target = resolve_pointer(&root, pointer)?;
    set_text(&target, text)?;
    Some(serialize_element(&root))
}

/// Replace an element's children with a single text node.
fn set_text(handle: &Handle, text: &str) -> Option<()> {
    if !matches!(handle.data, NodeData::Element { .. }) {
        return None;
    }
    let node = Node::new(NodeData::Text {
        contents: RefCell::new(text.into()),
    });
    *handle.children.borrow_mut() = vec![node];
    Some(())
}

/// Numeric segments of a JSON pointer, e.g. `/scenes/0/children/2` → `[0, 2]`.
/// The first is the scene index; the rest walk content-node children.
fn parse_indices(pointer: &str) -> Vec<usize> {
    pointer.split('/').filter_map(|s| s.parse::<usize>().ok()).collect()
}

/// Walk from `<rustmotion>` to the element a pointer addresses, mirroring the
/// transpiler's ordering (content nodes = elements + non-whitespace text).
fn resolve_pointer(root: &Handle, pointer: &str) -> Option<Handle> {
    let idx = parse_indices(pointer);
    let (&scene_i, rest) = idx.split_first()?;
    let mut node = nth_named_child(root, "scene", scene_i)?;
    for &i in rest {
        node = nth_content_node(&node, i)?;
    }
    Some(node)
}

fn nth_named_child(parent: &Handle, tag: &str, n: usize) -> Option<Handle> {
    parent
        .children
        .borrow()
        .iter()
        .filter(|c| tag_name(c).as_deref() == Some(tag))
        .nth(n)
        .cloned()
}

fn nth_content_node(parent: &Handle, n: usize) -> Option<Handle> {
    parent
        .children
        .borrow()
        .iter()
        .filter(|c| match &c.data {
            NodeData::Element { .. } => true,
            NodeData::Text { contents } => !contents.borrow().trim().is_empty(),
            _ => false,
        })
        .nth(n)
        .cloned()
}

fn set_style_attr(handle: &Handle, prop: &str, value: &str) -> Option<()> {
    let NodeData::Element { attrs, .. } = &handle.data else {
        return None;
    };
    let mut attrs = attrs.borrow_mut();
    let existing = attrs
        .iter()
        .find(|a| a.name.local.as_ref() == "style")
        .map(|a| a.value.to_string())
        .unwrap_or_default();
    let new_style = upsert_decl(&existing, prop, value);
    if let Some(a) = attrs.iter_mut().find(|a| a.name.local.as_ref() == "style") {
        a.value = new_style.as_str().into();
    } else {
        attrs.push(Attribute {
            name: QualName::new(None, ns!(), local_name!("style")),
            value: new_style.as_str().into(),
        });
    }
    Some(())
}

/// Set/replace one `prop: value` in a CSS declaration list, preserving order.
fn upsert_decl(decls: &str, prop: &str, value: &str) -> String {
    let mut pairs: Vec<(String, String)> = Vec::new();
    let mut found = false;
    for decl in decls.split(';') {
        let decl = decl.trim();
        if decl.is_empty() {
            continue;
        }
        if let Some((k, v)) = decl.split_once(':') {
            let k = k.trim().to_string();
            if k == prop {
                pairs.push((k, value.to_string()));
                found = true;
            } else {
                pairs.push((k, v.trim().to_string()));
            }
        }
    }
    if !found {
        pairs.push((prop.to_string(), value.to_string()));
    }
    pairs
        .iter()
        .map(|(k, v)| format!("{k}:{v}"))
        .collect::<Vec<_>>()
        .join("; ")
}

fn serialize_element(handle: &Handle) -> String {
    let mut buf = Vec::new();
    let node: SerializableHandle = handle.clone().into();
    let opts = SerializeOpts {
        traversal_scope: TraversalScope::IncludeNode,
        ..Default::default()
    };
    let _ = serialize(&mut buf, &node, opts);
    String::from_utf8(buf).unwrap_or_default()
}

#[cfg(test)]
mod lib_tests {
    use serde_json::json;

    #[test]
    fn set_inline_style_updates_property() {
        let html = r##"<rustmotion width="100" height="100"><scene duration="2"><h1 style="font-size:96; color:#fff">Hi</h1></scene></rustmotion>"##;
        let out = crate::set_inline_style(html, "/scenes/0/children/0", "font-size", "120").unwrap();
        assert!(out.contains("font-size:120"), "got: {out}");
        assert!(out.contains("color:#fff"), "kept other props: {out}");
        let v = crate::html_to_scenario_value(&out).unwrap();
        assert_eq!(v["scenes"][0]["children"][0]["style"]["font-size"], json!(120));
    }

    #[test]
    fn set_inline_style_nested_through_container() {
        let html = r##"<rustmotion width="100" height="100"><scene duration="2"><div style="gap:8"><h1 style="font-size:96">Hi</h1></div></scene></rustmotion>"##;
        let out =
            crate::set_inline_style(html, "/scenes/0/children/0/children/0", "font-size", "120")
                .unwrap();
        let v = crate::html_to_scenario_value(&out).unwrap();
        assert_eq!(
            v["scenes"][0]["children"][0]["children"][0]["style"]["font-size"],
            json!(120)
        );
    }

    #[test]
    fn set_text_content_replaces_inner_text() {
        let html = r##"<rustmotion width="100" height="100"><scene duration="2"><h1 style="font-size:96">Hi</h1></scene></rustmotion>"##;
        let out = crate::set_text_content(html, "/scenes/0/children/0", "Bonjour").unwrap();
        let v = crate::html_to_scenario_value(&out).unwrap();
        assert_eq!(v["scenes"][0]["children"][0]["content"], json!("Bonjour"));
        // The style attribute is preserved.
        assert_eq!(v["scenes"][0]["children"][0]["style"]["font-size"], json!(96));
    }

    #[test]
    fn set_inline_style_inserts_when_absent() {
        let html = r##"<rustmotion width="100" height="100"><scene duration="2"><p>Hi</p></scene></rustmotion>"##;
        let out = crate::set_inline_style(html, "/scenes/0/children/0", "color", "#ff0000").unwrap();
        assert!(out.contains("color:#ff0000"), "got: {out}");
    }

    #[test]
    fn root_maps_to_video_and_scenes() {
        let html = r##"<rustmotion width="1920" height="1080" fps="30" background="#0f172a">
            <scene duration="4"><h1 style="font-size:96">Hi</h1></scene>
        </rustmotion>"##;
        let v = crate::html_to_scenario_value(html).unwrap();
        assert_eq!(v["video"]["width"], json!(1920));
        assert_eq!(v["video"]["height"], json!(1080));
        assert_eq!(v["video"]["fps"], json!(30));
        assert_eq!(v["video"]["background"], json!("#0f172a"));
        assert_eq!(v["scenes"][0]["duration"], json!(4));
        assert_eq!(v["scenes"][0]["children"][0]["content"], json!("Hi"));
    }

    #[test]
    fn missing_root_is_an_error() {
        assert!(crate::html_to_scenario_value("<div>no root</div>").is_err());
    }
}
