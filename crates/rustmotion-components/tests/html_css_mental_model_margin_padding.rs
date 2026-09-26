use rustmotion_components::Component;

fn component_json_parses(json: serde_json::Value) -> bool {
    serde_json::from_value::<Component>(json).is_ok()
}

#[test]
fn old_doc_margin_top_shorthand_fails_to_deserialize() {
    let json = serde_json::json!({
        "type": "text",
        "content": "hi",
        "style": { "margin-top": 16 }
    });
    let err = serde_json::from_value::<Component>(json)
        .expect_err("margin-top must not deserialize — printed below for the audit red-phase log");
    eprintln!("captured deserialize error (constat 5 red phase): {err}");
    assert!(
        err.to_string().contains("margin-top") || err.to_string().contains("unknown field"),
        "expected an unknown-field error mentioning the rejected key, got: {err}"
    );
}

#[test]
fn old_doc_margin_left_auto_shorthand_fails_to_deserialize() {
    let json = serde_json::json!({
        "type": "badge",
        "text": "NEW",
        "style": { "margin-left": "auto" }
    });
    assert!(
        !component_json_parses(json),
        "`margin-left` is not a CssStyle field — same failure mode as margin-top"
    );
}

#[test]
fn old_doc_padding_array_shorthand_fails_to_deserialize() {
    let json = serde_json::json!({
        "type": "card",
        "style": { "padding": [40, 60] },
        "children": []
    });
    assert!(
        !component_json_parses(json),
        "`padding` only accepts a uniform scalar or a {{top,right,bottom,left}} \
         object, never an array — the old doc's example should fail"
    );
}

#[test]
fn corrected_margin_object_syntax_round_trips() {
    let json = serde_json::json!({
        "type": "text",
        "content": "hi",
        "style": { "margin": { "top": 16 } }
    });
    assert!(
        component_json_parses(json),
        "the doc's corrected `\"margin\": {{\"top\": 16}}` syntax must actually parse"
    );
}

#[test]
fn corrected_margin_left_auto_object_syntax_round_trips() {
    let json = serde_json::json!({
        "type": "badge",
        "text": "NEW",
        "style": { "margin": { "left": "auto" } }
    });
    assert!(
        component_json_parses(json),
        "the doc's corrected `\"margin\": {{\"left\": \"auto\"}}` syntax must actually parse"
    );
}

#[test]
fn corrected_padding_object_syntax_round_trips() {
    let json = serde_json::json!({
        "type": "card",
        "style": { "padding": { "top": 40, "bottom": 40, "left": 60, "right": 60 } },
        "children": []
    });
    assert!(
        component_json_parses(json),
        "the doc's corrected per-side `padding` object syntax must actually parse"
    );
}
