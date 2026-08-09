//! Regression test — audit round 4, lot LAYOUT, constat 5.
//!
//! `.claude/skills/rustmotion/rules/html-css-mental-model.md` — a rules file
//! read by the LLMs that generate rustmotion scenarios — documented
//! `"margin-top"` / `"margin-left"` as valid per-child JSON keys, and
//! `"padding": [40, 60]` as a valid CSS-shorthand-style array. Neither is:
//! `CssStyle` only exposes `margin: Option<Edges>` / `padding: Option<Edges>`
//! (`#[serde(deny_unknown_fields)]`), and `Edges` is `Uniform(LengthPercentage)`
//! or a `{top, right, bottom, left}` object — never a per-key kebab-case field,
//! never an array. Following the old doc verbatim makes the whole component
//! fail typed deserialization, so it silently disappears from the render
//! instead of being spaced as intended.
//!
//! These tests pin both failure modes (so a future change can't quietly
//! re-introduce them) and confirm the corrected syntax the doc now teaches
//! actually round-trips.

use rustmotion_components::Component;

fn component_json_parses(json: serde_json::Value) -> bool {
    serde_json::from_value::<Component>(json).is_ok()
}

#[test]
fn old_doc_margin_top_shorthand_fails_to_deserialize() {
    // What the doc used to teach (html-css-mental-model.md, old line 87):
    // `"margin-top": 16` as a direct style key.
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
    // What the doc used to teach (html-css-mental-model.md, old line 90):
    // `"margin-left": "auto"` as a direct style key.
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
    // What the doc used to teach (html-css-mental-model.md, old line 177):
    // `"padding": [40, 60]`, a CSS `padding: 40px 60px` -style array shorthand
    // `Edges` does not implement.
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
