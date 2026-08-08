//! HTML/CSS → Rustmotion scenario JSON transpiler (browserless, compiled in).
//!
//! ## Variable substitution (`$name`) from the CLI
//!
//! HTML scenarios cannot carry a `config` block (there is no element for it),
//! so they do not declare variables with types and defaults. However, `$name`
//! references in element text content are still replaced post-transpilation when
//! variable overrides are provided via `--var` / `--props` on the CLI:
//!
//! ```html
//! <!-- hero.html -->
//! <rustmotion width="1920" height="1080" fps="30">
//!   <scene duration="3">
//!     <h1 style="font-size:96; color:#fff">Welcome, $username!</h1>
//!   </scene>
//! </rustmotion>
//! ```
//!
//! ```sh
//! rustmotion render -f hero.html --var username=Alice -o alice.mp4
//! ```
//!
//! Because there is no `config` block, **unknown override keys are not an error**
//! — they are simply available for substitution but silently unused if no `$name`
//! reference matches. Unresolved `$name` references after substitution are also
//! silently ignored (no `UnresolvedVariable` error), since the document may
//! contain no variable references at all.
//!
//! For type-safe variables with defaults and schema validation, use a JSON
//! scenario with a `config` block instead.

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
    #[error("background attribute contains invalid JSON: {0}")]
    InvalidBackgroundJson(String),
    #[error("effects attribute contains invalid JSON: {0}")]
    InvalidEffectsJson(String),
    #[error("anim attribute contains invalid JSON: {0}")]
    InvalidAnimJson(String),
    #[error("invalid anim DSL: {0}")]
    InvalidAnimDsl(String),
    #[error("transition-duration and transition-easing require a transition attribute")]
    TransitionParamsWithoutTransition,
    /// Emitted when `<font>` has neither `path`/`src` nor `source`.
    #[error(
        "<font> requires either 'path'/'src' (local file) or 'source' (e.g. source=\"google\")"
    )]
    MissingFontAttributes,
    /// Emitted when `<font>` sets both `path`/`src` and `source` — they are mutually exclusive.
    #[error("<font family=\"{family}\">: 'path'/'src' and 'source' are mutually exclusive")]
    FontPathAndSourceConflict { family: String },
    /// Emitted for `<style>`. Unlike `<script>`/`<title>`/`<noscript>`/`<template>`
    /// (silently skipped — they never visually render in real HTML either, so
    /// skipping them matches an author's own expectation), `<style>` DOES have
    /// real, expected visual effect in HTML. The dialect has no CSS
    /// selector/cascade engine (only inline `style="..."` attributes), so a
    /// `<style>` block's rules would never take effect — refused instead of
    /// silently discarded, so the author's intent isn't dropped without a trace.
    #[error(
        "<style> blocks are not supported by the HTML dialect (no CSS selector/cascade engine) — move these declarations onto the target elements' style=\"...\" attribute"
    )]
    StyleElementUnsupported,
    /// Emitted for a native HTML tag whose real payload (`src`, nested shape
    /// markup, …) has no representation via the generic `Container`/`div`
    /// fallback — that fallback would silently render an empty box. The
    /// dialect's `rm-*` custom-element mechanism is the way to express these.
    #[error(
        "<{tag}> is not supported by the HTML dialect and would render as an empty container — use <{suggestion} ...> instead"
    )]
    UnsupportedNativeElement { tag: String, suggestion: String },
    /// Emitted when a `<scene>` is found nested inside an element other than
    /// `<rustmotion>` itself (or `<font>`, which the transpiler recurses
    /// through to work around html5ever's formatting-element reconstruction).
    /// `collect_scenes_and_fonts` only walks direct children, so a nested
    /// `<scene>` would otherwise vanish from the scenario without a trace.
    #[error(
        "<scene> found nested inside <{parent}> — <scene> elements must be direct children of <rustmotion> (only <font> is recursed into)"
    )]
    NestedScene { parent: String },
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
        video.insert("background".into(), parse_background_attr(&bg)?);
    }

    let mut scenes = Vec::new();
    let mut fonts = Vec::new();
    collect_scenes_and_fonts(&root, &mut scenes, &mut fonts)?;
    if scenes.is_empty() {
        return Err(HtmlError::NoScenes);
    }

    let mut scenario = Map::new();
    scenario.insert("video".into(), Value::Object(video));
    scenario.insert("scenes".into(), Value::Array(scenes));
    if !fonts.is_empty() {
        scenario.insert("fonts".into(), Value::Array(fonts));
    }
    Ok(Value::Object(scenario))
}

/// Parse a `background` attribute value: if it starts with `{` or `[`, treat it as
/// JSON and parse it; otherwise return it as a plain string value.
pub(crate) fn parse_background_attr(raw: &str) -> Result<Value, HtmlError> {
    let trimmed = raw.trim();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        serde_json::from_str(trimmed).map_err(|e| HtmlError::InvalidBackgroundJson(e.to_string()))
    } else {
        Ok(Value::from(raw))
    }
}

/// Map a `<font>` element to a FontEntry JSON object.
///
/// Two modes:
/// - **Local**: `<font family="Inter" path="fonts/Inter.ttf">` or `src=` alias.
/// - **Google Fonts**: `<font family="Inter" source="google" weights="400,700">`.
///
/// `path`/`src` and `source` are mutually exclusive — an error is returned if
/// both are present.
fn font_to_value(handle: &Handle) -> Result<Value, HtmlError> {
    let attrs = element_attrs(handle);
    let get = |k: &str| attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());

    let family = get("family").ok_or(HtmlError::MissingFontAttributes)?;
    let path = get("path").or_else(|| get("src"));
    let source = get("source");

    match (path, source) {
        // Conflict: both local path and remote source set.
        (Some(_), Some(_)) => Err(HtmlError::FontPathAndSourceConflict { family }),

        // Google Fonts mode.
        (None, Some(source_val)) => {
            let mut obj = serde_json::json!({ "family": family, "source": source_val });
            // Parse optional weights="400,700" CSV into a JSON array of integers.
            if let Some(weights_raw) = get("weights") {
                let parsed: Vec<u16> = weights_raw
                    .split(',')
                    .filter_map(|w| w.trim().parse::<u16>().ok())
                    .collect();
                if !parsed.is_empty() {
                    obj["weights"] = Value::Array(parsed.into_iter().map(Value::from).collect());
                }
            }
            Ok(obj)
        }

        // Local file mode.
        (Some(p), None) => Ok(serde_json::json!({ "family": family, "path": p })),

        // Neither path nor source.
        (None, None) => Err(HtmlError::MissingFontAttributes),
    }
}

/// Walk the immediate children of `parent`, collecting `<scene>` and `<font>`
/// elements. Because html5ever's HTML5 parser treats `<font>` as a formatting
/// element and nests subsequent siblings inside it, we recurse into `<font>`
/// children so that `<scene>` elements placed after `<font>` declarations are
/// still found at any depth.
///
/// Any other child is scanned (at any depth) for a nested `<scene>` — a
/// wrapper element (typo'd unclosed tag, deliberate `<div>` grouping, or
/// html5ever's own formatting-element error recovery on tags like `<b>`)
/// would otherwise make `<scene>` elements vanish from the scenario with no
/// trace, since this function only descends into direct children. A `<style>`
/// found at this level is refused for the same reason `element_to_value`
/// refuses it inside a scene: it has real expected visual effect that the
/// dialect cannot honor, so it must not be silently dropped either.
fn collect_scenes_and_fonts(
    parent: &Handle,
    scenes: &mut Vec<Value>,
    fonts: &mut Vec<Value>,
) -> Result<(), HtmlError> {
    for child in parent.children.borrow().iter() {
        match tag_name(child).as_deref() {
            Some("scene") => scenes.push(scene::scene_to_value(child)?),
            Some("font") => {
                fonts.push(font_to_value(child)?);
                // Recurse: html5ever may nest siblings inside the <font> element.
                collect_scenes_and_fonts(child, scenes, fonts)?;
            }
            Some("style") => return Err(HtmlError::StyleElementUnsupported),
            Some(other) if find_element(child, "scene").is_some() => {
                return Err(HtmlError::NestedScene {
                    parent: other.to_string(),
                });
            }
            _ => {}
        }
    }
    Ok(())
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

/// Set or replace a plain attribute on the element addressed by the JSON
/// pointer (into the transpiled scenario), returning the rewritten HTML. Used
/// by the studio inspector for component root fields (`<rm-counter from=…>`).
/// Attributes are strings; the transpiler's coercion re-types them on load.
/// An EMPTY `value` REMOVES the attribute (unset, not empty-string — the
/// transpiler skips empty attributes anyway). `anim`, `style` and every other
/// attribute are preserved (mirrors [`set_inline_style`]). Returns `None` if
/// the pointer doesn't resolve to an element.
pub fn set_attribute(html: &str, pointer: &str, name: &str, value: &str) -> Option<String> {
    let dom = parse_fragment_dom(html);
    let root = find_element(&dom.document, "rustmotion")?;
    let target = resolve_pointer(&root, pointer)?;
    set_attr(&target, name, value)?;
    Some(serialize_element(&root))
}

/// Remove one inline `style` property from the element addressed by the JSON
/// pointer, returning the rewritten HTML (an emptied inspector control unsets
/// the declaration). Other declarations and attributes are preserved. Returns
/// `None` if the pointer doesn't resolve to an element.
pub fn remove_inline_style(html: &str, pointer: &str, prop: &str) -> Option<String> {
    let dom = parse_fragment_dom(html);
    let root = find_element(&dom.document, "rustmotion")?;
    let target = resolve_pointer(&root, pointer)?;
    remove_style_decl(&target, prop)?;
    Some(serialize_element(&root))
}

/// Upsert (or, for an empty value, remove) a plain attribute on an element.
fn set_attr(handle: &Handle, name: &str, value: &str) -> Option<()> {
    let NodeData::Element { attrs, .. } = &handle.data else {
        return None;
    };
    let mut attrs = attrs.borrow_mut();
    if value.is_empty() {
        attrs.retain(|a| a.name.local.as_ref() != name);
        return Some(());
    }
    if let Some(a) = attrs.iter_mut().find(|a| a.name.local.as_ref() == name) {
        a.value = value.into();
    } else {
        attrs.push(Attribute {
            name: QualName::new(None, ns!(), name.into()),
            value: value.into(),
        });
    }
    Some(())
}

/// Drop one `prop: value` declaration from an element's `style` attribute.
fn remove_style_decl(handle: &Handle, prop: &str) -> Option<()> {
    let NodeData::Element { attrs, .. } = &handle.data else {
        return None;
    };
    let mut attrs = attrs.borrow_mut();
    let Some(a) = attrs.iter_mut().find(|a| a.name.local.as_ref() == "style") else {
        return Some(()); // no style attribute → nothing to remove
    };
    let kept: Vec<String> = a
        .value
        .split(';')
        .filter_map(|decl| {
            let decl = decl.trim();
            let (k, v) = decl.split_once(':')?;
            let k = k.trim();
            if k == prop || k.is_empty() {
                None
            } else {
                Some(format!("{k}:{}", v.trim()))
            }
        })
        .collect();
    a.value = kept.join("; ").as_str().into();
    Some(())
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
    pointer
        .split('/')
        .filter_map(|s| s.parse::<usize>().ok())
        .collect()
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
        let out =
            crate::set_inline_style(html, "/scenes/0/children/0", "font-size", "120").unwrap();
        assert!(out.contains("font-size:120"), "got: {out}");
        assert!(out.contains("color:#fff"), "kept other props: {out}");
        let v = crate::html_to_scenario_value(&out).unwrap();
        assert_eq!(
            v["scenes"][0]["children"][0]["style"]["font-size"],
            json!(120)
        );
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
        assert_eq!(
            v["scenes"][0]["children"][0]["style"]["font-size"],
            json!(96)
        );
    }

    #[test]
    fn set_inline_style_inserts_when_absent() {
        let html = r##"<rustmotion width="100" height="100"><scene duration="2"><p>Hi</p></scene></rustmotion>"##;
        let out =
            crate::set_inline_style(html, "/scenes/0/children/0", "color", "#ff0000").unwrap();
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

    // --- set_attribute (studio schema inspector) ---

    #[test]
    fn set_attribute_updates_counter_from_and_retypes_on_transpile() {
        let html = r##"<rustmotion width="100" height="100"><scene duration="2"><rm-counter from="0" to="100" anim="fade-in" style="font-size:64; color:#fff"></rm-counter></scene></rustmotion>"##;
        let out = crate::set_attribute(html, "/scenes/0/children/0", "from", "250").unwrap();
        let v = crate::html_to_scenario_value(&out).unwrap();
        let child = &v["scenes"][0]["children"][0];
        // The attribute string is re-typed to a number by the transpiler.
        assert_eq!(child["from"], json!(250));
        assert!(child["from"].is_number());
        // Other attributes are preserved: to, anim (→ style.animation), style.
        assert_eq!(child["to"], json!(100));
        assert_eq!(child["style"]["font-size"], json!(64));
        assert_eq!(child["style"]["color"], json!("#fff"));
        assert_eq!(child["style"]["animation"][0]["name"], json!("fade_in"));
    }

    #[test]
    fn set_attribute_inserts_when_absent() {
        let html = r##"<rustmotion width="100" height="100"><scene duration="2"><rm-counter from="0" to="10"></rm-counter></scene></rustmotion>"##;
        let out = crate::set_attribute(html, "/scenes/0/children/0", "suffix", "%").unwrap();
        let v = crate::html_to_scenario_value(&out).unwrap();
        assert_eq!(v["scenes"][0]["children"][0]["suffix"], json!("%"));
    }

    #[test]
    fn set_attribute_empty_value_removes_the_attribute() {
        let html = r##"<rustmotion width="100" height="100"><scene duration="2"><rm-counter from="0" to="10" suffix="%"></rm-counter></scene></rustmotion>"##;
        let out = crate::set_attribute(html, "/scenes/0/children/0", "suffix", "").unwrap();
        assert!(!out.contains("suffix"), "attribute removed: {out}");
        let v = crate::html_to_scenario_value(&out).unwrap();
        assert!(v["scenes"][0]["children"][0].get("suffix").is_none());
    }

    #[test]
    fn remove_inline_style_drops_only_that_declaration() {
        let html = r##"<rustmotion width="100" height="100"><scene duration="2"><h1 style="font-size:96; color:#fff">Hi</h1></scene></rustmotion>"##;
        let out = crate::remove_inline_style(html, "/scenes/0/children/0", "color").unwrap();
        let v = crate::html_to_scenario_value(&out).unwrap();
        let style = &v["scenes"][0]["children"][0]["style"];
        assert!(style.get("color").is_none(), "color removed");
        assert_eq!(style["font-size"], json!(96), "other declarations kept");
    }

    // --- anim round-trip (studio) ---

    #[test]
    fn set_inline_style_preserves_anim_attribute() {
        let html = r##"<rustmotion width="100" height="100"><scene duration="2"><h1 anim="fade-in-up delay:0.3" style="font-size:96">Hi</h1></scene></rustmotion>"##;
        let out =
            crate::set_inline_style(html, "/scenes/0/children/0", "font-size", "120").unwrap();
        let v = crate::html_to_scenario_value(&out).unwrap();
        let child = &v["scenes"][0]["children"][0];
        assert_eq!(child["style"]["font-size"], json!(120));
        assert_eq!(
            child["style"]["animation"],
            json!([{ "name": "fade_in_up", "delay": 0.3 }]),
            "anim attribute must survive serialize_element"
        );
    }

    #[test]
    fn set_text_content_preserves_anim_attribute() {
        let html = r##"<rustmotion width="100" height="100"><scene duration="2"><h1 anim="pulse loop:true">Hi</h1></scene></rustmotion>"##;
        let out = crate::set_text_content(html, "/scenes/0/children/0", "Bonjour").unwrap();
        let v = crate::html_to_scenario_value(&out).unwrap();
        let child = &v["scenes"][0]["children"][0];
        assert_eq!(child["content"], json!("Bonjour"));
        assert_eq!(
            child["style"]["animation"],
            json!([{ "name": "pulse", "loop": true }])
        );
    }

    // --- font tests ---

    #[test]
    fn fonts_are_collected_from_font_elements() {
        let html = r##"<rustmotion width="1920" height="1080">
            <font family="Inter" path="fonts/Inter.ttf">
            <font family="JetBrainsMono" src="fonts/JetBrainsMono.ttf">
            <scene duration="2"><h1>hi</h1></scene>
        </rustmotion>"##;
        let v = crate::html_to_scenario_value(html).unwrap();
        let fonts = &v["fonts"];
        assert_eq!(fonts[0]["family"], json!("Inter"));
        assert_eq!(fonts[0]["path"], json!("fonts/Inter.ttf"));
        assert_eq!(fonts[1]["family"], json!("JetBrainsMono"));
        assert_eq!(fonts[1]["path"], json!("fonts/JetBrainsMono.ttf"));
    }

    #[test]
    fn font_without_family_is_error() {
        let html = r##"<rustmotion width="1920" height="1080">
            <font path="fonts/Inter.ttf">
            <scene duration="2"><h1>hi</h1></scene>
        </rustmotion>"##;
        let err = crate::html_to_scenario_value(html).unwrap_err();
        assert!(
            matches!(err, crate::HtmlError::MissingFontAttributes),
            "expected MissingFontAttributes, got: {err:?}"
        );
    }

    #[test]
    fn font_without_path_is_error() {
        let html = r##"<rustmotion width="1920" height="1080">
            <font family="Inter">
            <scene duration="2"><h1>hi</h1></scene>
        </rustmotion>"##;
        let err = crate::html_to_scenario_value(html).unwrap_err();
        assert!(
            matches!(err, crate::HtmlError::MissingFontAttributes),
            "expected MissingFontAttributes, got: {err:?}"
        );
    }

    #[test]
    fn no_fonts_means_no_fonts_key() {
        let html = r##"<rustmotion width="1920" height="1080">
            <scene duration="2"><h1>hi</h1></scene>
        </rustmotion>"##;
        let v = crate::html_to_scenario_value(html).unwrap();
        assert!(
            v.get("fonts").is_none(),
            "fonts key should be absent when no fonts declared"
        );
    }

    // --- Google Fonts HTML tests ---

    #[test]
    fn font_with_source_google_transpiles_correctly() {
        let html = r##"<rustmotion width="1920" height="1080">
            <font family="Inter" source="google">
            <scene duration="2"><h1>hi</h1></scene>
        </rustmotion>"##;
        let v = crate::html_to_scenario_value(html).unwrap();
        let fonts = &v["fonts"];
        assert_eq!(fonts[0]["family"], json!("Inter"));
        assert_eq!(fonts[0]["source"], json!("google"));
        assert!(
            fonts[0].get("path").is_none(),
            "path must be absent for google source"
        );
    }

    #[test]
    fn font_with_source_google_and_weights_transpiles_correctly() {
        let html = r##"<rustmotion width="1920" height="1080">
            <font family="Inter" source="google" weights="400,700">
            <scene duration="2"><h1>hi</h1></scene>
        </rustmotion>"##;
        let v = crate::html_to_scenario_value(html).unwrap();
        let fonts = &v["fonts"];
        assert_eq!(fonts[0]["source"], json!("google"));
        assert_eq!(fonts[0]["weights"], json!([400, 700]));
    }

    #[test]
    fn font_with_path_and_source_is_error() {
        let html = r##"<rustmotion width="1920" height="1080">
            <font family="Inter" path="fonts/Inter.ttf" source="google">
            <scene duration="2"><h1>hi</h1></scene>
        </rustmotion>"##;
        let err = crate::html_to_scenario_value(html).unwrap_err();
        assert!(
            matches!(err, crate::HtmlError::FontPathAndSourceConflict { .. }),
            "expected FontPathAndSourceConflict, got: {err:?}"
        );
    }

    #[test]
    fn font_with_src_and_source_is_error() {
        let html = r##"<rustmotion width="1920" height="1080">
            <font family="Inter" src="fonts/Inter.ttf" source="google">
            <scene duration="2"><h1>hi</h1></scene>
        </rustmotion>"##;
        let err = crate::html_to_scenario_value(html).unwrap_err();
        assert!(
            matches!(err, crate::HtmlError::FontPathAndSourceConflict { .. }),
            "expected FontPathAndSourceConflict, got: {err:?}"
        );
    }

    // --- root-level background JSON ---

    #[test]
    fn root_background_json_object_is_parsed() {
        let html = r##"<rustmotion width="1920" height="1080" background='{"gradient":"linear","colors":["#0f172a","#1e3a5f"]}'>
            <scene duration="2"><h1>hi</h1></scene>
        </rustmotion>"##;
        let v = crate::html_to_scenario_value(html).unwrap();
        assert_eq!(v["video"]["background"]["gradient"], json!("linear"));
    }

    #[test]
    fn root_background_invalid_json_is_error() {
        let html = r##"<rustmotion width="1920" height="1080" background="{bad json}">
            <scene duration="2"><h1>hi</h1></scene>
        </rustmotion>"##;
        let err = crate::html_to_scenario_value(html).unwrap_err();
        assert!(
            matches!(err, crate::HtmlError::InvalidBackgroundJson(_)),
            "expected InvalidBackgroundJson, got: {err:?}"
        );
    }
}
