mod element;
mod scene;
mod style;

use html5ever::serialize::{serialize, SerializeOpts, TraversalScope};
use html5ever::tendril::TendrilSink;
use html5ever::{local_name, ns, parse_fragment, Attribute, ParseOpts, QualName};
use markup5ever_rcdom::{Handle, Node, NodeData, RcDom, SerializableHandle};
use serde_json::{Map, Value};
use std::cell::RefCell;

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
    #[error(
        "<font> requires either 'path'/'src' (local file) or 'source' (e.g. source=\"google\")"
    )]
    MissingFontAttributes,
    #[error("<font family=\"{family}\">: 'path'/'src' and 'source' are mutually exclusive")]
    FontPathAndSourceConflict { family: String },
    #[error(
        "<style> blocks are not supported by the HTML dialect (no CSS selector/cascade engine) — move these declarations onto the target elements' style=\"...\" attribute"
    )]
    StyleElementUnsupported,
    #[error(
        "<{tag}> is not supported by the HTML dialect and would render as an empty container — use <{suggestion} ...> instead"
    )]
    UnsupportedNativeElement { tag: String, suggestion: String },
    #[error(
        "<scene> found nested inside <{parent}> — <scene> elements must be direct children of <rustmotion> (only <font> is recursed into)"
    )]
    NestedScene { parent: String },
    #[error("<{element}> has unsupported attribute(s): {detail} — these are silently ignored today; fix the typo, drop them, or use the attribute the dialect actually reads")]
    UnknownAttributes { element: String, detail: String },
    #[error("world-position=\"{0}\" is not \"x,y\" or a JSON object {{\"x\":..,\"y\":..}}")]
    InvalidWorldPosition(String),
    #[error("animated-background attribute contains invalid JSON: {0}")]
    InvalidAnimatedBackgroundJson(String),
    #[error("style property '{prop}' has an unsupported multi-token value '{value}' — supported multi-token forms are the padding/margin/border-radius box shorthand and grid-template-columns/-rows track lists with repeat()/minmax(); rewrite as a single value")]
    UnsupportedStyleShorthand { prop: String, value: String },
    #[error("<{tag}> cannot appear inside an inline text element (p/span/h1..h6/strong/em/label) — those flatten their content to a plain string, so <{tag}>'s own content would be silently lost; move it outside as a sibling, or wrap the text in a <div>/<rm-*> container instead")]
    TextContentUnsupportedChild { tag: String },
    #[error("failed to serialize the rewritten HTML: {0}")]
    SerializeFailed(String),
}

pub fn html_to_scenario_value(html: &str) -> Result<Value, HtmlError> {
    let dom = parse_fragment_dom(html);
    let root = find_element(&dom.document, "rustmotion").ok_or(HtmlError::MissingRoot)?;
    let attrs = element_attrs(&root);
    let get = |k: &str| attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());

    check_known_attrs(
        "rustmotion",
        &attrs,
        &["width", "height", "fps", "background", "codec", "crf"],
    )?;

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
    if let Some(codec) = get("codec") {
        video.insert("codec".into(), style::coerce_value(&codec));
    }
    if let Some(crf) = get("crf") {
        video.insert("crf".into(), style::coerce_value(&crf));
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

pub(crate) fn parse_background_attr(raw: &str) -> Result<Value, HtmlError> {
    let trimmed = raw.trim();
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        serde_json::from_str(trimmed).map_err(|e| HtmlError::InvalidBackgroundJson(e.to_string()))
    } else {
        Ok(Value::from(raw))
    }
}

fn font_to_value(handle: &Handle) -> Result<Value, HtmlError> {
    let attrs = element_attrs(handle);
    let get = |k: &str| attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());

    let family = get("family").ok_or(HtmlError::MissingFontAttributes)?;
    let path = get("path").or_else(|| get("src"));
    let source = get("source");

    match (path, source) {
        (Some(_), Some(_)) => Err(HtmlError::FontPathAndSourceConflict { family }),

        (None, Some(source_val)) => {
            let mut obj = serde_json::json!({ "family": family, "source": source_val });
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

        (Some(p), None) => Ok(serde_json::json!({ "family": family, "path": p })),

        (None, None) => Err(HtmlError::MissingFontAttributes),
    }
}

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

pub(crate) fn tag_name(handle: &Handle) -> Option<String> {
    match &handle.data {
        NodeData::Element { name, .. } => Some(name.local.to_string()),
        _ => None,
    }
}

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

fn is_inert_attr(name: &str) -> bool {
    name == "class" || name == "id" || name.starts_with("data-")
}

pub(crate) fn check_known_attrs(
    element: &str,
    attrs: &[(String, String)],
    known: &[&str],
) -> Result<(), HtmlError> {
    let unknown: Vec<&str> = attrs
        .iter()
        .map(|(k, _)| k.as_str())
        .filter(|k| !known.contains(k) && !is_inert_attr(k))
        .collect();
    if unknown.is_empty() {
        return Ok(());
    }
    let detail = unknown
        .iter()
        .map(|name| match suggest(name, known) {
            Some(k) => format!("'{name}' (did you mean '{k}'?)"),
            None => format!("'{name}'"),
        })
        .collect::<Vec<_>>()
        .join(", ");
    Err(HtmlError::UnknownAttributes {
        element: element.to_string(),
        detail,
    })
}

fn suggest<'a>(name: &str, known: &[&'a str]) -> Option<&'a str> {
    known
        .iter()
        .map(|k| (levenshtein(name, k), *k))
        .min_by_key(|(distance, _)| *distance)
        .filter(|(distance, _)| *distance <= 2)
        .map(|(_, k)| k)
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur.push((prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

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

pub fn set_inline_style(html: &str, pointer: &str, prop: &str, value: &str) -> Option<String> {
    let dom = parse_fragment_dom(html);
    let root = find_element(&dom.document, "rustmotion")?;
    let target = resolve_pointer(&root, pointer)?;
    set_style_attr(&target, prop, value)?;
    splice_rustmotion_subtree(html, &root)
}

pub fn set_text_content(html: &str, pointer: &str, text: &str) -> Option<String> {
    let dom = parse_fragment_dom(html);
    let root = find_element(&dom.document, "rustmotion")?;
    let target = resolve_pointer(&root, pointer)?;
    set_text(&target, text)?;
    splice_rustmotion_subtree(html, &root)
}

pub fn set_attribute(html: &str, pointer: &str, name: &str, value: &str) -> Option<String> {
    let dom = parse_fragment_dom(html);
    let root = find_element(&dom.document, "rustmotion")?;
    let target = resolve_pointer(&root, pointer)?;
    set_attr(&target, name, value)?;
    splice_rustmotion_subtree(html, &root)
}

pub fn remove_inline_style(html: &str, pointer: &str, prop: &str) -> Option<String> {
    let dom = parse_fragment_dom(html);
    let root = find_element(&dom.document, "rustmotion")?;
    let target = resolve_pointer(&root, pointer)?;
    remove_style_decl(&target, prop)?;
    splice_rustmotion_subtree(html, &root)
}

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

fn remove_style_decl(handle: &Handle, prop: &str) -> Option<()> {
    let NodeData::Element { attrs, .. } = &handle.data else {
        return None;
    };
    let mut attrs = attrs.borrow_mut();
    let Some(a) = attrs.iter_mut().find(|a| a.name.local.as_ref() == "style") else {
        return Some(());
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

fn parse_indices(pointer: &str) -> Vec<usize> {
    pointer
        .split('/')
        .filter_map(|s| s.parse::<usize>().ok())
        .collect()
}

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

fn splice_rustmotion_subtree(original: &str, root: &Handle) -> Option<String> {
    let open_start = original.find("<rustmotion")?;
    let close_start = original.rfind("</rustmotion")?;
    let close_end = close_start + original[close_start..].find('>')? + 1;
    if close_end <= open_start {
        return None;
    }
    let serialized = serialize_element(root).ok()?;
    let mut out = String::with_capacity(original.len() + serialized.len());
    out.push_str(&original[..open_start]);
    out.push_str(&serialized);
    out.push_str(&original[close_end..]);
    Some(out)
}

fn serialize_element(handle: &Handle) -> Result<String, HtmlError> {
    let mut buf = Vec::new();
    let node: SerializableHandle = handle.clone().into();
    let opts = SerializeOpts {
        traversal_scope: TraversalScope::IncludeNode,
        ..Default::default()
    };
    serialize(&mut buf, &node, opts).map_err(|e| HtmlError::SerializeFailed(e.to_string()))?;
    String::from_utf8(buf).map_err(|e| HtmlError::SerializeFailed(e.to_string()))
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

    #[test]
    fn set_attribute_updates_counter_from_and_retypes_on_transpile() {
        let html = r##"<rustmotion width="100" height="100"><scene duration="2"><rm-counter from="0" to="100" anim="fade-in" style="font-size:64; color:#fff"></rm-counter></scene></rustmotion>"##;
        let out = crate::set_attribute(html, "/scenes/0/children/0", "from", "250").unwrap();
        let v = crate::html_to_scenario_value(&out).unwrap();
        let child = &v["scenes"][0]["children"][0];
        assert_eq!(child["from"], json!(250));
        assert!(child["from"].is_number());
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
