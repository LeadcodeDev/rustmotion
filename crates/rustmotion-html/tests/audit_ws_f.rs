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
