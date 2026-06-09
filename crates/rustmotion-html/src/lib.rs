//! HTML/CSS → Rustmotion scenario JSON transpiler (browserless, compiled in).

mod element;
mod style;

use html5ever::tendril::TendrilSink;
use html5ever::{local_name, ns, parse_fragment, ParseOpts, QualName};
use markup5ever_rcdom::{Handle, NodeData, RcDom};

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
#[allow(dead_code)]
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
