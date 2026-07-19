use markup5ever_rcdom::Handle;
use serde_json::{Map, Value};

use crate::element::children_to_values;
use crate::element_attrs;
use crate::parse_background_attr;
use crate::style::coerce_value;
use crate::HtmlError;

/// Parse an `effects` attribute value: must be a JSON array.
fn parse_effects_attr(raw: &str) -> Result<Value, HtmlError> {
    let trimmed = raw.trim();
    serde_json::from_str(trimmed).map_err(|e| HtmlError::InvalidEffectsJson(e.to_string()))
}

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

    // background: string or inline JSON object/array
    if let Some(bg) = get("background") {
        obj.insert("background".into(), parse_background_attr(&bg)?);
    }

    // effects: JSON array of PostEffect objects
    if let Some(fx) = get("effects") {
        let parsed = parse_effects_attr(&fx)?;
        // Validate that it is an array (not just any JSON value)
        if !parsed.is_array() {
            return Err(HtmlError::InvalidEffectsJson(
                "effects must be a JSON array".into(),
            ));
        }
        obj.insert("effects".into(), parsed);
    }

    // transition: type is required; duration and easing are optional extras
    let has_transition_params =
        get("transition-duration").is_some() || get("transition-easing").is_some();
    if let Some(t) = get("transition") {
        let mut tr = Map::new();
        tr.insert("type".into(), Value::from(t));
        if let Some(dur) = get("transition-duration") {
            tr.insert("duration".into(), coerce_value(&dur));
        }
        if let Some(eas) = get("transition-easing") {
            tr.insert("easing".into(), Value::from(eas));
        }
        obj.insert("transition".into(), Value::Object(tr));
    } else if has_transition_params {
        return Err(HtmlError::TransitionParamsWithoutTransition);
    }

    obj.insert("children".into(), Value::Array(children_to_values(handle)?));
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

    // --- background ---

    #[test]
    fn scene_background_string_color() {
        let dom =
            crate::parse_fragment_dom(r##"<scene duration="2" background="#1e293b"></scene>"##);
        let el = crate::find_element(&dom.document, "scene").unwrap();
        let v = scene_to_value(&el).unwrap();
        assert_eq!(v["background"], json!("#1e293b"));
    }

    #[test]
    fn scene_background_json_object() {
        let dom = crate::parse_fragment_dom(
            r##"<scene duration="2" background='{"preset":"halo","halo":{"zones":[{"color":"#7c3aed","x":0.5,"y":0.5,"radius":0.4}]},"speed":0}'></scene>"##,
        );
        let el = crate::find_element(&dom.document, "scene").unwrap();
        let v = scene_to_value(&el).unwrap();
        assert_eq!(v["background"]["preset"], json!("halo"));
        assert_eq!(
            v["background"]["halo"]["zones"][0]["color"],
            json!("#7c3aed")
        );
    }

    #[test]
    fn scene_background_invalid_json_is_error() {
        let dom = crate::parse_fragment_dom(
            r#"<scene duration="2" background="{not valid json}"></scene>"#,
        );
        let el = crate::find_element(&dom.document, "scene").unwrap();
        let err = scene_to_value(&el).unwrap_err();
        assert!(
            matches!(err, crate::HtmlError::InvalidBackgroundJson(_)),
            "expected InvalidBackgroundJson, got: {err:?}"
        );
    }

    // --- effects ---

    #[test]
    fn scene_effects_json_array_transpiled() {
        let dom = crate::parse_fragment_dom(
            r##"<scene duration="2" effects='[{"type":"grain","intensity":0.2}]'></scene>"##,
        );
        let el = crate::find_element(&dom.document, "scene").unwrap();
        let v = scene_to_value(&el).unwrap();
        assert_eq!(v["effects"][0]["type"], json!("grain"));
        assert_eq!(v["effects"][0]["intensity"], json!(0.2));
    }

    #[test]
    fn scene_effects_invalid_json_is_error() {
        let dom =
            crate::parse_fragment_dom(r#"<scene duration="2" effects="{not valid json}"></scene>"#);
        let el = crate::find_element(&dom.document, "scene").unwrap();
        let err = scene_to_value(&el).unwrap_err();
        assert!(
            matches!(err, crate::HtmlError::InvalidEffectsJson(_)),
            "expected InvalidEffectsJson, got: {err:?}"
        );
    }

    #[test]
    fn scene_effects_non_array_is_error() {
        let dom = crate::parse_fragment_dom(
            r##"<scene duration="2" effects='{"type":"grain"}'></scene>"##,
        );
        let el = crate::find_element(&dom.document, "scene").unwrap();
        let err = scene_to_value(&el).unwrap_err();
        assert!(
            matches!(err, crate::HtmlError::InvalidEffectsJson(_)),
            "expected InvalidEffectsJson (non-array), got: {err:?}"
        );
    }

    #[test]
    fn scene_without_effects_has_no_effects_key() {
        let dom = crate::parse_fragment_dom(r#"<scene duration="2"></scene>"#);
        let el = crate::find_element(&dom.document, "scene").unwrap();
        let v = scene_to_value(&el).unwrap();
        assert!(
            v.get("effects").is_none(),
            "effects key must be absent when not declared"
        );
    }

    #[test]
    fn scene_effects_multiple_entries() {
        let dom = crate::parse_fragment_dom(
            r##"<scene duration="2" effects='[{"type":"vignette","intensity":0.6},{"type":"pixelate","size":4}]'></scene>"##,
        );
        let el = crate::find_element(&dom.document, "scene").unwrap();
        let v = scene_to_value(&el).unwrap();
        assert_eq!(v["effects"].as_array().unwrap().len(), 2);
        assert_eq!(v["effects"][0]["type"], json!("vignette"));
        assert_eq!(v["effects"][1]["type"], json!("pixelate"));
    }

    // --- transition with duration and easing ---

    #[test]
    fn scene_transition_full_params() {
        let dom = crate::parse_fragment_dom(
            r#"<scene duration="3" transition="fade" transition-duration="0.8" transition-easing="ease_in_out"></scene>"#,
        );
        let el = crate::find_element(&dom.document, "scene").unwrap();
        let v = scene_to_value(&el).unwrap();
        assert_eq!(v["transition"]["type"], json!("fade"));
        assert_eq!(v["transition"]["duration"], json!(0.8));
        assert_eq!(v["transition"]["easing"], json!("ease_in_out"));
    }

    #[test]
    fn scene_transition_duration_without_transition_is_error() {
        let dom =
            crate::parse_fragment_dom(r#"<scene duration="3" transition-duration="0.8"></scene>"#);
        let el = crate::find_element(&dom.document, "scene").unwrap();
        let err = scene_to_value(&el).unwrap_err();
        assert!(
            matches!(err, crate::HtmlError::TransitionParamsWithoutTransition),
            "expected TransitionParamsWithoutTransition, got: {err:?}"
        );
    }

    #[test]
    fn scene_transition_easing_without_transition_is_error() {
        let dom =
            crate::parse_fragment_dom(r#"<scene duration="3" transition-easing="linear"></scene>"#);
        let el = crate::find_element(&dom.document, "scene").unwrap();
        let err = scene_to_value(&el).unwrap_err();
        assert!(
            matches!(err, crate::HtmlError::TransitionParamsWithoutTransition),
            "expected TransitionParamsWithoutTransition, got: {err:?}"
        );
    }
}
