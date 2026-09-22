//! Regression tests for workstream F's 4 confirmed audit findings on the
//! HTML dialect transpiler: an attribute outside a hardcoded allowlist
//! vanishing with no error, a CSS shorthand transpiling to a string
//! the core length parser cannot read and silently resolving to 0px,
//! `<script>`/`<style>` source getting painted as text while `<img>`/`<svg>`
//! children vanish inside inline text elements, and the studio's
//! HTML write-back deleting everything outside `<rustmotion>` in the
//! author's file.

use rustmotion_html::{html_to_scenario_value, HtmlError};
use serde_json::json;

// ---------------------------------------------------------------------------
// <scene> and <rustmotion> read a hardcoded attribute allowlist;
// every other attribute is dropped with no error.
// ---------------------------------------------------------------------------

#[test]
fn scene_freeze_at_world_position_and_animated_background_are_reachable() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="2" freeze_at="1" world-position="10,20" animated-background='{"preset":"halo","halo":{"zones":[{"color":"#7c3aed","x":0.5,"y":0.5,"radius":0.4}]},"speed":0}'><h1>Hi</h1></scene></rustmotion>"##;
    let v = html_to_scenario_value(html).expect("known scene attributes must transpile");
    assert_eq!(v["scenes"][0]["freeze_at"], json!(1));
    assert_eq!(v["scenes"][0]["world-position"]["x"], json!(10.0));
    assert_eq!(v["scenes"][0]["world-position"]["y"], json!(20.0));
    assert_eq!(
        v["scenes"][0]["animated-background"]["preset"],
        json!("halo")
    );
}

#[test]
fn scene_unknown_attribute_is_refused_not_dropped() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="2" bogus-attr="x"><h1>Hi</h1></scene></rustmotion>"##;
    let err = html_to_scenario_value(html)
        .expect_err("an unknown <scene> attribute must be refused, not silently dropped");
    match err {
        HtmlError::UnknownAttributes { element, detail } => {
            assert_eq!(element, "scene");
            assert!(detail.contains("bogus-attr"), "got: {detail}");
        }
        other => panic!("expected UnknownAttributes, got: {other:?}"),
    }
}

#[test]
fn root_codec_is_reachable_and_unknown_root_attribute_is_refused() {
    let ok = r##"<rustmotion width="1920" height="1080" codec="prores"><scene duration="2"><h1>Hi</h1></scene></rustmotion>"##;
    let v = html_to_scenario_value(ok).expect("codec must transpile, not be dropped");
    assert_eq!(v["video"]["codec"], json!("prores"));

    let bad = r##"<rustmotion width="1920" height="1080" codec="prores" durationn="5"><scene duration="2"><h1>Hi</h1></scene></rustmotion>"##;
    let err = html_to_scenario_value(bad)
        .expect_err("an unknown <rustmotion> attribute must be refused, not silently dropped");
    match err {
        HtmlError::UnknownAttributes { element, detail } => {
            assert_eq!(element, "rustmotion");
            assert!(detail.contains("durationn"), "got: {detail}");
        }
        other => panic!("expected UnknownAttributes, got: {other:?}"),
    }
}

#[test]
fn container_unknown_attributes_are_refused_not_dropped() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="2"><div gap="32" width="400" id="x"></div></scene></rustmotion>"##;
    let err = html_to_scenario_value(html)
        .expect_err("unknown <div> attributes must be refused, not silently dropped");
    match err {
        HtmlError::UnknownAttributes { element, detail } => {
            assert_eq!(element, "div");
            assert!(detail.contains("gap"), "got: {detail}");
            assert!(detail.contains("width"), "got: {detail}");
            assert!(!detail.contains("'id'"), "id must stay inert: {detail}");
        }
        other => panic!("expected UnknownAttributes, got: {other:?}"),
    }
}

#[test]
fn text_tag_unknown_attribute_is_refused_not_dropped() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="2"><p foo="bar">Hi</p></scene></rustmotion>"##;
    let err = html_to_scenario_value(html)
        .expect_err("unknown <p> attributes must be refused, not silently dropped");
    match err {
        HtmlError::UnknownAttributes { element, detail } => {
            assert_eq!(element, "p");
            assert!(detail.contains("foo"), "got: {detail}");
        }
        other => panic!("expected UnknownAttributes, got: {other:?}"),
    }
}

#[test]
fn inert_attributes_stay_inert_on_container_and_text() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="2"><div class="wrapper" id="hero" data-testid="x"><p class="lead" data-x="1">Hi</p></div></scene></rustmotion>"##;
    html_to_scenario_value(html).expect("class/id/data-* must remain inert, not flagged");
}

// ---------------------------------------------------------------------------
// CSS shorthand values transpile to strings the core length parser
// cannot read, silently resolving to 0px.
// ---------------------------------------------------------------------------

#[test]
fn padding_two_value_shorthand_expands_to_edges_object() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="2"><div style="padding: 24px 48px"></div></scene></rustmotion>"##;
    let v = html_to_scenario_value(html).expect("2-value padding must transpile");
    let padding = &v["scenes"][0]["children"][0]["style"]["padding"];
    assert_eq!(padding["top"], json!(24));
    assert_eq!(padding["bottom"], json!(24));
    assert_eq!(padding["right"], json!(48));
    assert_eq!(padding["left"], json!(48));
}

#[test]
fn margin_four_value_shorthand_expands_to_edges_object() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="2"><div style="margin: 4px 8px 12px 16px"></div></scene></rustmotion>"##;
    let v = html_to_scenario_value(html).expect("4-value margin must transpile");
    let margin = &v["scenes"][0]["children"][0]["style"]["margin"];
    assert_eq!(margin["top"], json!(4));
    assert_eq!(margin["right"], json!(8));
    assert_eq!(margin["bottom"], json!(12));
    assert_eq!(margin["left"], json!(16));
}

#[test]
fn border_radius_four_value_shorthand_expands_to_corners_object() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="2"><div style="border-radius: 2px 4px 6px 8px"></div></scene></rustmotion>"##;
    let v = html_to_scenario_value(html).expect("4-value border-radius must transpile");
    let radius = &v["scenes"][0]["children"][0]["style"]["border-radius"];
    assert_eq!(radius["top-left"], json!(2));
    assert_eq!(radius["top-right"], json!(4));
    assert_eq!(radius["bottom-right"], json!(6));
    assert_eq!(radius["bottom-left"], json!(8));
}

#[test]
fn grid_template_columns_repeat_expands_to_flat_track_list() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="2"><div style="grid-template-columns: repeat(3, 1fr)"></div></scene></rustmotion>"##;
    let v = html_to_scenario_value(html).expect("repeat() must transpile");
    let tracks = v["scenes"][0]["children"][0]["style"]["grid-template-columns"]
        .as_array()
        .expect("array of tracks");
    assert_eq!(tracks, &vec![json!("1fr"), json!("1fr"), json!("1fr")]);
}

#[test]
fn grid_template_columns_minmax_expands_to_min_max_object() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="2"><div style="grid-template-columns: minmax(100px, 1fr) auto"></div></scene></rustmotion>"##;
    let v = html_to_scenario_value(html).expect("minmax() must transpile");
    let tracks = v["scenes"][0]["children"][0]["style"]["grid-template-columns"]
        .as_array()
        .expect("array of tracks");
    assert_eq!(tracks.len(), 2);
    assert_eq!(tracks[0]["min"], json!(100));
    assert_eq!(tracks[0]["max"], json!("1fr"));
    assert_eq!(tracks[1], json!("auto"));
}

#[test]
fn unhandled_multi_token_style_value_is_refused_not_an_opaque_string() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="2"><div style="gap: 8px 16px"></div></scene></rustmotion>"##;
    let err = html_to_scenario_value(html)
        .expect_err("an unsupported multi-token style value must be refused, not silently zeroed");
    match err {
        HtmlError::UnsupportedStyleShorthand { prop, value } => {
            assert_eq!(prop, "gap");
            assert_eq!(value, "8px 16px");
        }
        other => panic!("expected UnsupportedStyleShorthand, got: {other:?}"),
    }
}

#[test]
fn single_token_padding_still_transpiles_to_a_plain_value() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="2"><div style="padding: 32px"></div></scene></rustmotion>"##;
    let v = html_to_scenario_value(html).expect("uniform padding must still transpile");
    assert_eq!(v["scenes"][0]["children"][0]["style"]["padding"], json!(32));
}

#[test]
fn rgba_color_functional_notation_is_not_treated_as_multi_token() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="2"><div style="background: rgba(0, 0, 0, 0.5)"></div></scene></rustmotion>"##;
    let v = html_to_scenario_value(html).expect("rgba(...) must not be flagged as multi-token");
    assert_eq!(
        v["scenes"][0]["children"][0]["style"]["background"],
        json!("rgba(0, 0, 0, 0.5)")
    );
}

// ---------------------------------------------------------------------------
// text-tag children bypass every guard — <script>/<style> source gets
// painted, <img>/<svg>/<rm-*> vanish silently.
// ---------------------------------------------------------------------------

#[test]
fn script_nested_inside_paragraph_is_not_painted() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="2"><p>Real <script>var secret = 1; alert(2);</script></p></scene></rustmotion>"##;
    let v = html_to_scenario_value(html).expect("<script> inside <p> must not block transpilation");
    assert_eq!(
        v["scenes"][0]["children"][0]["content"],
        json!("Real"),
        "the script source must never be painted as text: {v}"
    );
}

#[test]
fn style_nested_inside_heading_is_refused_not_painted() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="2"><h1>Title<style>h1{color:#0f0}</style></h1></scene></rustmotion>"##;
    let err = html_to_scenario_value(html)
        .expect_err("<style> inside <h1> must be refused, not painted as text");
    assert!(
        matches!(err, HtmlError::StyleElementUnsupported),
        "expected StyleElementUnsupported, got: {err:?}"
    );
}

#[test]
fn img_nested_inside_paragraph_is_refused_not_dropped() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="2"><p>cap<img src="hero.png"></p></scene></rustmotion>"##;
    let err = html_to_scenario_value(html)
        .expect_err("<img> inside <p> must be refused, not silently dropped");
    match err {
        HtmlError::UnsupportedNativeElement { tag, suggestion } => {
            assert_eq!(tag, "img");
            assert_eq!(suggestion, "rm-image");
        }
        other => panic!("expected UnsupportedNativeElement, got: {other:?}"),
    }
}

#[test]
fn svg_nested_inside_heading_is_refused_not_dropped() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="2"><h1>t<svg viewBox="0 0 10 10"><circle r="4"></circle></svg></h1></scene></rustmotion>"##;
    let err = html_to_scenario_value(html)
        .expect_err("<svg> inside <h1> must be refused, not silently dropped");
    match err {
        HtmlError::UnsupportedNativeElement { tag, suggestion } => {
            assert_eq!(tag, "svg");
            assert_eq!(suggestion, "rm-svg");
        }
        other => panic!("expected UnsupportedNativeElement, got: {other:?}"),
    }
}

#[test]
fn rm_counter_nested_inside_span_is_refused_not_dropped() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="2"><span>n=<rm-counter from="0" to="100"></rm-counter></span></scene></rustmotion>"##;
    let err = html_to_scenario_value(html)
        .expect_err("<rm-counter> inside <span> must be refused, not silently dropped");
    match err {
        HtmlError::TextContentUnsupportedChild { tag } => assert_eq!(tag, "rm-counter"),
        other => panic!("expected TextContentUnsupportedChild, got: {other:?}"),
    }
}

#[test]
fn inline_formatting_tags_still_flatten_into_the_parent_text() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="2"><p>Real <strong>bold</strong> text</p></scene></rustmotion>"##;
    let v = html_to_scenario_value(html).expect("nested inline formatting must still flatten");
    assert_eq!(
        v["scenes"][0]["children"][0]["content"],
        json!("Real bold text")
    );
}

// ---------------------------------------------------------------------------
// studio HTML write-back silently deletes everything outside
// <rustmotion> in the author's file.
// ---------------------------------------------------------------------------

#[test]
fn write_back_preserves_doctype_head_and_surrounding_comments() {
    let html = concat!(
        "<!DOCTYPE html>\n",
        "<html><head><meta charset=\"utf-8\"></head><body>\n",
        "<!-- authored by hand -->\n",
        "<rustmotion width=\"100\" height=\"100\"><scene duration=\"2\">",
        "<h1 style=\"font-size:96\">Hi</h1></scene></rustmotion>\n",
        "<!-- trailing note -->\n",
        "</body></html>\n",
    );
    let out = rustmotion_html::set_inline_style(html, "/scenes/0/children/0", "font-size", "120")
        .expect("pointer resolves");
    assert!(
        out.contains("<!DOCTYPE html>"),
        "doctype must survive: {out}"
    );
    assert!(
        out.contains("<meta charset=\"utf-8\">"),
        "head must survive: {out}"
    );
    assert!(
        out.contains("<!-- authored by hand -->"),
        "leading comment must survive: {out}"
    );
    assert!(
        out.contains("<!-- trailing note -->"),
        "trailing comment must survive: {out}"
    );
    assert!(
        out.contains("font-size:120"),
        "the edit itself must still apply: {out}"
    );
    let v = html_to_scenario_value(&out).expect("round-trips through the transpiler");
    assert_eq!(
        v["scenes"][0]["children"][0]["style"]["font-size"],
        json!(120)
    );
}

#[test]
fn write_back_via_set_text_content_also_preserves_surrounding_document() {
    let html = concat!(
        "<!DOCTYPE html>\n",
        "<!-- keep me -->\n",
        "<rustmotion width=\"100\" height=\"100\"><scene duration=\"2\">",
        "<h1>Hi</h1></scene></rustmotion>\n",
        "<!-- keep me too -->\n",
    );
    let out = rustmotion_html::set_text_content(html, "/scenes/0/children/0", "Bonjour")
        .expect("pointer resolves");
    assert!(
        out.contains("<!DOCTYPE html>"),
        "doctype must survive: {out}"
    );
    assert!(out.contains("<!-- keep me -->"), "got: {out}");
    assert!(out.contains("<!-- keep me too -->"), "got: {out}");
    let v = html_to_scenario_value(&out).expect("round-trips through the transpiler");
    assert_eq!(v["scenes"][0]["children"][0]["content"], json!("Bonjour"));
}

#[test]
fn write_back_refuses_when_the_closing_tag_cannot_be_located_in_source() {
    let html = r##"<rustmotion width="100" height="100"><scene duration="2"><h1 style="font-size:96">Hi</h1></scene>"##;
    let out = rustmotion_html::set_inline_style(html, "/scenes/0/children/0", "font-size", "120");
    assert!(
        out.is_none(),
        "must refuse rather than write a file it cannot faithfully reconstruct"
    );
}
