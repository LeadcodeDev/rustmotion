//! HTML/CSS → Rustmotion scenario JSON transpiler (browserless, compiled in).

mod element;
mod scene;
mod style;

use html5ever::tendril::TendrilSink;
use html5ever::{local_name, ns, parse_fragment, ParseOpts, QualName};
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use serde_json::{Map, Value};

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

#[cfg(test)]
mod lib_tests {
    use serde_json::json;

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
