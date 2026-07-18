//! Pure scenario diff for the studio's before/after review mode.
//!
//! Compares the baseline raw scenario JSON against the current one and
//! reports element-level changes, identified by JSON pointer. Matching is
//! positional (by pointer): an element is compared with whatever sits at the
//! same index in the other tree. Consequently a reorder of different-typed
//! siblings shows as remove+add (type mismatch at both indices), and a
//! reorder of same-typed siblings shows as paired field modifications.
//!
//! Top-level `annotations` are deliberately ignored (studio feedback, not
//! scenario content).

use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq)]
pub enum ChangeKind {
    Added,
    Removed,
    Modified,
}

/// One changed field of a modified element, dot-flattened (`style.font-size`).
/// `before`/`after` are display strings; an empty string means the field was
/// absent on that side.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldChange {
    pub field: String,
    pub before: String,
    pub after: String,
}

/// One changed element (or the video config / document root), addressed by a
/// JSON pointer into the current scenario (or the baseline for removals).
#[derive(Debug, Clone, PartialEq)]
pub struct ElementChange {
    pub pointer: String,
    pub kind: ChangeKind,
    /// Component `type` ("text", "card", …), or "scene" / "video" / "document".
    pub element_type: String,
    /// Short display label (content snippet or the type).
    pub label: String,
    /// Display group ("Video", "Scene 1", "View 2 · Scene 1", "Document").
    pub group: String,
    /// Per-field before → after list (only for `Modified`).
    pub fields: Vec<FieldChange>,
    /// The element's `start_at`, for click-to-scrub.
    pub start_at: Option<f64>,
}

/// Diff two raw scenario JSON documents. Pure.
pub fn diff_scenarios(baseline: &Value, current: &Value) -> Vec<ElementChange> {
    let mut out = Vec::new();

    // Video config as one pseudo-element.
    let empty = Map::new();
    let b_video = baseline
        .get("video")
        .and_then(|v| v.as_object())
        .unwrap_or(&empty);
    let c_video = current
        .get("video")
        .and_then(|v| v.as_object())
        .unwrap_or(&empty);
    let mut fields = Vec::new();
    diff_fields("", b_video, c_video, &mut fields);
    if !fields.is_empty() {
        out.push(ElementChange {
            pointer: "/video".into(),
            kind: ChangeKind::Modified,
            element_type: "video".into(),
            label: "video".into(),
            group: "Video".into(),
            fields,
            start_at: None,
        });
    }

    // Scene lists: top-level `scenes` and/or `composition/*/scenes`, keyed by
    // pointer prefix so mismatched shapes fall out as remove+add naturally.
    for (prefix, group_prefix) in scene_prefixes(baseline, current) {
        let b_scenes = scenes_at(baseline, &prefix);
        let c_scenes = scenes_at(current, &prefix);
        let max = b_scenes.len().max(c_scenes.len());
        for i in 0..max {
            let pointer = format!("{prefix}/{i}");
            let group = format!("{group_prefix}Scene {}", i + 1);
            match (b_scenes.get(i), c_scenes.get(i)) {
                (Some(b), Some(c)) => {
                    diff_element(&pointer, &group, b, c, "scene", &mut out);
                }
                (Some(b), None) => out.push(entry(&pointer, ChangeKind::Removed, b, &group)),
                (None, Some(c)) => out.push(entry(&pointer, ChangeKind::Added, c, &group)),
                (None, None) => {}
            }
        }
    }

    // Any other top-level drift (fonts, audio, config, backgrounds…) except
    // scenes/composition (walked above) and annotations (studio feedback).
    let skip = ["video", "scenes", "composition", "annotations"];
    let b_root = baseline.as_object().cloned().unwrap_or_default();
    let c_root = current.as_object().cloned().unwrap_or_default();
    let b_rest: Map<String, Value> = b_root
        .into_iter()
        .filter(|(k, _)| !skip.contains(&k.as_str()))
        .collect();
    let c_rest: Map<String, Value> = c_root
        .into_iter()
        .filter(|(k, _)| !skip.contains(&k.as_str()))
        .collect();
    let mut rest_fields = Vec::new();
    diff_fields("", &b_rest, &c_rest, &mut rest_fields);
    if !rest_fields.is_empty() {
        out.push(ElementChange {
            pointer: "/".into(),
            kind: ChangeKind::Modified,
            element_type: "document".into(),
            label: "document".into(),
            group: "Document".into(),
            fields: rest_fields,
            start_at: None,
        });
    }

    out
}

/// The scene-array pointer prefixes present in either document, with their
/// display group prefix (`""` or `"View N · "`), deduplicated, stable order.
fn scene_prefixes(baseline: &Value, current: &Value) -> Vec<(String, String)> {
    let mut prefixes = Vec::new();
    let mut push = |p: String, g: String| {
        if !prefixes.iter().any(|(q, _)| *q == p) {
            prefixes.push((p, g));
        }
    };
    for doc in [baseline, current] {
        if doc.get("scenes").and_then(|s| s.as_array()).is_some() {
            push("/scenes".to_string(), String::new());
        }
        if let Some(views) = doc.get("composition").and_then(|c| c.as_array()) {
            for (v, _) in views.iter().enumerate() {
                push(
                    format!("/composition/{v}/scenes"),
                    format!("View {} · ", v + 1),
                );
            }
        }
    }
    prefixes
}

/// The scene array at a pointer prefix (empty when absent).
fn scenes_at<'a>(doc: &'a Value, prefix: &str) -> &'a [Value] {
    doc.pointer(prefix)
        .and_then(|v| v.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(&[])
}

/// Compare two elements at the same pointer. Same type → field diff (dot
/// flattened, recursive into objects like `style`, skipping `children`) then
/// recurse into children pairwise. Different type → remove + add.
fn diff_element(
    pointer: &str,
    group: &str,
    b: &Value,
    c: &Value,
    fallback_type: &str,
    out: &mut Vec<ElementChange>,
) {
    let b_type = b.get("type").and_then(|t| t.as_str());
    let c_type = c.get("type").and_then(|t| t.as_str());
    if b_type != c_type {
        out.push(entry(pointer, ChangeKind::Removed, b, group));
        out.push(entry(pointer, ChangeKind::Added, c, group));
        return;
    }

    let empty = Map::new();
    let b_obj = b.as_object().unwrap_or(&empty);
    let c_obj = c.as_object().unwrap_or(&empty);
    let mut fields = Vec::new();
    diff_fields("", b_obj, c_obj, &mut fields);
    if !fields.is_empty() {
        out.push(ElementChange {
            pointer: pointer.to_string(),
            kind: ChangeKind::Modified,
            element_type: b_type.unwrap_or(fallback_type).to_string(),
            label: label_of(c, fallback_type),
            group: group.to_string(),
            fields,
            start_at: start_at_of(c),
        });
    }

    let none: &[Value] = &[];
    let b_children = b
        .get("children")
        .and_then(|v| v.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(none);
    let c_children = c
        .get("children")
        .and_then(|v| v.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(none);
    let max = b_children.len().max(c_children.len());
    for j in 0..max {
        let child_ptr = format!("{pointer}/children/{j}");
        match (b_children.get(j), c_children.get(j)) {
            (Some(bc), Some(cc)) => diff_element(&child_ptr, group, bc, cc, "element", out),
            (Some(bc), None) => out.push(entry(&child_ptr, ChangeKind::Removed, bc, group)),
            (None, Some(cc)) => out.push(entry(&child_ptr, ChangeKind::Added, cc, group)),
            (None, None) => {}
        }
    }
}

/// Build an Added/Removed entry for a whole element (no recursion inside — one
/// entry per added/removed subtree).
fn entry(pointer: &str, kind: ChangeKind, el: &Value, group: &str) -> ElementChange {
    let element_type = el
        .get("type")
        .and_then(|t| t.as_str())
        .unwrap_or("scene")
        .to_string();
    ElementChange {
        pointer: pointer.to_string(),
        kind,
        label: label_of(el, &element_type),
        element_type,
        group: group.to_string(),
        fields: Vec::new(),
        start_at: start_at_of(el),
    }
}

/// Dot-flattened field diff over two objects, recursive into nested objects
/// (style etc.). `children` is skipped at the element's top level only.
/// Key order: baseline keys first, then current-only keys.
fn diff_fields(
    prefix: &str,
    b: &Map<String, Value>,
    c: &Map<String, Value>,
    out: &mut Vec<FieldChange>,
) {
    let mut keys: Vec<&String> = b.keys().collect();
    for k in c.keys() {
        if !b.contains_key(k) {
            keys.push(k);
        }
    }
    for k in keys {
        if prefix.is_empty() && k == "children" {
            continue;
        }
        let field = format!("{prefix}{k}");
        match (b.get(k), c.get(k)) {
            (Some(bv), Some(cv)) if bv == cv => {}
            (Some(Value::Object(bo)), Some(Value::Object(co))) => {
                diff_fields(&format!("{field}."), bo, co, out);
            }
            (Some(bv), Some(cv)) => out.push(FieldChange {
                field,
                before: fmt(bv),
                after: fmt(cv),
            }),
            (Some(bv), None) => out.push(FieldChange {
                field,
                before: fmt(bv),
                after: String::new(),
            }),
            (None, Some(cv)) => out.push(FieldChange {
                field,
                before: String::new(),
                after: fmt(cv),
            }),
            (None, None) => {}
        }
    }
}

/// Display string for a JSON value (strings unquoted; floats keep one decimal
/// via serde's canonical form).
fn fmt(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Short display label: content snippet (24 chars max) or the type.
fn label_of(el: &Value, fallback: &str) -> String {
    match el.get("content").and_then(|c| c.as_str()) {
        Some(s) if !s.is_empty() => {
            let mut label: String = s.chars().take(24).collect();
            if s.chars().count() > 24 {
                label.push('…');
            }
            label
        }
        _ => fallback.to_string(),
    }
}

fn start_at_of(el: &Value) -> Option<f64> {
    el.get("start_at").and_then(|v| v.as_f64())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn base() -> Value {
        json!({
            "video": { "width": 1920, "height": 1080 },
            "scenes": [
                { "duration": 3.0, "children": [
                    { "type": "text", "content": "Hi", "style": { "font-size": 48, "color": "#fff" } }
                ] }
            ]
        })
    }

    #[test]
    fn equal_scenarios_produce_empty_diff() {
        assert!(diff_scenarios(&base(), &base()).is_empty());
    }

    #[test]
    fn added_element_detected() {
        let mut cur = base();
        cur["scenes"][0]["children"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "type": "shape", "start_at": 1.5 }));
        let d = diff_scenarios(&base(), &cur);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].kind, ChangeKind::Added);
        assert_eq!(d[0].pointer, "/scenes/0/children/1");
        assert_eq!(d[0].element_type, "shape");
        assert_eq!(d[0].group, "Scene 1");
        assert_eq!(d[0].start_at, Some(1.5));
    }

    #[test]
    fn removed_element_detected() {
        let mut cur = base();
        cur["scenes"][0]["children"].as_array_mut().unwrap().clear();
        let d = diff_scenarios(&base(), &cur);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].kind, ChangeKind::Removed);
        assert_eq!(d[0].pointer, "/scenes/0/children/0");
        assert_eq!(d[0].element_type, "text");
        assert_eq!(d[0].label, "Hi");
    }

    #[test]
    fn style_modification_reports_exact_before_after() {
        let mut cur = base();
        cur["scenes"][0]["children"][0]["style"]["font-size"] = json!(64);
        let d = diff_scenarios(&base(), &cur);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].kind, ChangeKind::Modified);
        assert_eq!(d[0].pointer, "/scenes/0/children/0");
        assert_eq!(
            d[0].fields,
            vec![FieldChange {
                field: "style.font-size".into(),
                before: "48".into(),
                after: "64".into(),
            }]
        );
    }

    #[test]
    fn content_modification_detected() {
        let mut cur = base();
        cur["scenes"][0]["children"][0]["content"] = json!("Hello");
        let d = diff_scenarios(&base(), &cur);
        assert_eq!(d.len(), 1);
        assert_eq!(
            d[0].fields,
            vec![FieldChange {
                field: "content".into(),
                before: "Hi".into(),
                after: "Hello".into(),
            }]
        );
    }

    #[test]
    fn nested_children_modification_detected() {
        let nested = json!({
            "video": { "width": 1, "height": 1 },
            "scenes": [ { "duration": 1.0, "children": [
                { "type": "card", "children": [
                    { "type": "text", "content": "a" },
                    { "type": "text", "content": "b" }
                ] }
            ] } ]
        });
        let mut cur = nested.clone();
        cur["scenes"][0]["children"][0]["children"][1]["content"] = json!("B!");
        let d = diff_scenarios(&nested, &cur);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].pointer, "/scenes/0/children/0/children/1");
        assert_eq!(d[0].kind, ChangeKind::Modified);
        assert_eq!(d[0].fields[0].field, "content");
    }

    #[test]
    fn reorder_of_different_types_shows_remove_plus_add() {
        let b = json!({
            "video": { "width": 1, "height": 1 },
            "scenes": [ { "duration": 1.0, "children": [
                { "type": "text", "content": "t" },
                { "type": "shape" }
            ] } ]
        });
        let mut cur = b.clone();
        cur["scenes"][0]["children"]
            .as_array_mut()
            .unwrap()
            .reverse();
        let d = diff_scenarios(&b, &cur);
        // Type mismatch at both indices → remove+add per slot.
        assert_eq!(d.len(), 4);
        assert_eq!(
            d.iter().filter(|c| c.kind == ChangeKind::Removed).count(),
            2
        );
        assert_eq!(d.iter().filter(|c| c.kind == ChangeKind::Added).count(), 2);
    }

    #[test]
    fn scene_added_detected() {
        let mut cur = base();
        cur["scenes"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "duration": 2.0 }));
        let d = diff_scenarios(&base(), &cur);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].kind, ChangeKind::Added);
        assert_eq!(d[0].pointer, "/scenes/1");
        assert_eq!(d[0].element_type, "scene");
        assert_eq!(d[0].group, "Scene 2");
    }

    #[test]
    fn scene_duration_modification_detected() {
        let mut cur = base();
        cur["scenes"][0]["duration"] = json!(5.0);
        let d = diff_scenarios(&base(), &cur);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].pointer, "/scenes/0");
        assert_eq!(d[0].element_type, "scene");
        assert_eq!(d[0].fields[0].field, "duration");
        assert_eq!(d[0].fields[0].before, "3.0");
        assert_eq!(d[0].fields[0].after, "5.0");
    }

    #[test]
    fn video_config_change_detected() {
        let mut cur = base();
        cur["video"]["width"] = json!(1080);
        let d = diff_scenarios(&base(), &cur);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].pointer, "/video");
        assert_eq!(d[0].group, "Video");
        assert_eq!(
            d[0].fields,
            vec![FieldChange {
                field: "width".into(),
                before: "1920".into(),
                after: "1080".into(),
            }]
        );
    }

    #[test]
    fn annotations_are_ignored() {
        let mut cur = base();
        cur["annotations"] = json!([{ "id": "a", "note": "n",
            "target": { "pointer": "/scenes/0" } }]);
        assert!(diff_scenarios(&base(), &cur).is_empty());
    }

    #[test]
    fn composition_scenes_get_view_scoped_pointers() {
        let b = json!({
            "video": { "width": 1, "height": 1 },
            "composition": [ { "type": "slide", "scenes": [
                { "duration": 1.0, "children": [ { "type": "text", "content": "x" } ] }
            ] } ]
        });
        let mut cur = b.clone();
        cur["composition"][0]["scenes"][0]["children"][0]["content"] = json!("y");
        let d = diff_scenarios(&b, &cur);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].pointer, "/composition/0/scenes/0/children/0");
        assert_eq!(d[0].group, "View 1 · Scene 1");
    }
}
