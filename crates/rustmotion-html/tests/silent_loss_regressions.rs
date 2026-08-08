//! Regression tests for the 4 confirmed audit findings on the HTML dialect
//! ("lot html"): each used to degrade silently (wrong-but-valid JSON,
//! `validate` exiting 0) instead of failing loudly or naming the loss.
//!
//! Each test reproduces the exact (or closest faithful) scenario from the
//! audit brief and asserts the *fixed* behavior — refuse (named error) or, for
//! constructs with zero real-HTML visual equivalent, silently skip only when
//! that skip matches what any HTML author already expects from a browser.

use rustmotion_html::{html_to_scenario_value, HtmlError};

// ---------------------------------------------------------------------------
// Constat 1: <style>/<script> content must never be painted as a `text`
// component.
// ---------------------------------------------------------------------------

/// Exact reproduction from the brief: a `<style>` + `<script>` + `<h1
/// class="title">` inside one scene used to transpile into three sibling
/// `text`/`div` components, with the CSS and JS source painted on screen.
#[test]
fn style_block_is_refused_not_painted() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="3"><style>h1 { color: #0f0 }</style><script>alert(1); var x = 2;</script><h1 class="title">Styled by class</h1></scene></rustmotion>"##;
    let err = html_to_scenario_value(html).expect_err("<style> must be refused, not painted");
    assert!(
        matches!(err, HtmlError::StyleElementUnsupported),
        "expected StyleElementUnsupported, got: {err:?}"
    );
}

/// `<script>` alone (no `<style>` alongside it) must not surface its source as
/// a painted component either, but — unlike `<style>` — it has zero visual
/// equivalent in real HTML either, so it is silently skipped rather than
/// refused.
#[test]
fn script_alone_is_skipped_not_painted() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="3"><script>alert(1); var x = 2;</script><h1>Real content</h1></scene></rustmotion>"##;
    let v = html_to_scenario_value(html).expect("<script> alone must not block transpilation");
    let children = v["scenes"][0]["children"]
        .as_array()
        .expect("children array");
    assert_eq!(
        children.len(),
        1,
        "<script> content must never become a painted component: {v}"
    );
    assert_eq!(children[0]["content"], serde_json::json!("Real content"));
}

/// A `<style>` block placed at the document root (sibling of `<scene>`, not
/// nested inside one) must also be refused, not silently swallowed by the
/// root-level scene/font collector.
#[test]
fn root_level_style_block_is_also_refused() {
    let html = r##"<rustmotion width="1920" height="1080"><style>h1 { color: red }</style><scene duration="2"><h1>Hi</h1></scene></rustmotion>"##;
    let err =
        html_to_scenario_value(html).expect_err("root-level <style> must be refused, not dropped");
    assert!(
        matches!(err, HtmlError::StyleElementUnsupported),
        "expected StyleElementUnsupported, got: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// Constat 2: a <scene> nested inside a container must never silently vanish.
// ---------------------------------------------------------------------------

/// Exact reproduction from the brief: a `<scene>` wrapped in a `<div
/// class="wrapper">` used to disappear entirely — `validate` reported "1
/// scene(s)" / "Duration: 2.0s", silently dropping 3 seconds of video.
#[test]
fn scene_nested_in_a_wrapper_div_is_refused_not_dropped() {
    let html = r#"<rustmotion width=1920 height=1080><div class="wrapper"><scene duration="3"><h1>inside a wrapper</h1></scene></div><scene duration="2"><h1>top level</h1></scene></rustmotion>"#;
    let err = html_to_scenario_value(html)
        .expect_err("a <scene> nested inside <div> must be refused, not silently dropped");
    match err {
        HtmlError::NestedScene { parent } => assert_eq!(parent, "div"),
        other => panic!("expected NestedScene {{ parent: \"div\" }}, got: {other:?}"),
    }
}

/// Stronger, more realistic repro flagged by the adversarial verifier: an
/// unclosed `<b>` (a common typo, and the dialect's most reachable formatting
/// element) makes html5ever's formatting-element reconstruction nest BOTH
/// scenes inside `<b>`. Before the fix this produced a generic `NoScenes`
/// (all scenes swallowed) with no indication of the real cause; the fix must
/// name `<b>` as the offending parent instead.
#[test]
fn scenes_nested_via_html5_error_recovery_on_unclosed_b_are_named() {
    let html = r#"<rustmotion width=1920 height=1080 fps=30><b>note<scene duration="3"><h1>A</h1></scene><scene duration="2"><h1>B</h1></scene></rustmotion>"#;
    let err = html_to_scenario_value(html)
        .expect_err("scenes nested inside <b> via error recovery must be refused");
    match err {
        HtmlError::NestedScene { parent } => assert_eq!(parent, "b"),
        other => panic!("expected NestedScene {{ parent: \"b\" }}, got: {other:?}"),
    }
}

/// Even gnarlier repro: an unclosed `<p>` before the scenes. HTML5's
/// implicit-close-on-heading rule pops `<p>` (and the still-open `<scene>`
/// with it) as soon as the `<h1>` inside the first scene is seen, orphaning
/// that `<h1>` as a stray root-level sibling and leaving `scene[0]` with zero
/// children — content silently vanishes *inside* a scene that still "exists"
/// (scene count preserved, so the old `NoScenes` guard never fired). The
/// `<scene>` node itself is still nested inside `<p>` in the DOM, so refusing
/// on nested-scene detection catches this too.
#[test]
fn scene_content_lost_via_unclosed_p_is_refused() {
    let html = r#"<rustmotion width=1920 height=1080 fps=30><p>note<scene duration="3"><h1>A</h1></scene><scene duration="2"><h1>B</h1></scene></rustmotion>"#;
    let err = html_to_scenario_value(html)
        .expect_err("scene content lost via unclosed <p> must be refused, not silently accepted");
    match err {
        HtmlError::NestedScene { parent } => assert_eq!(parent, "p"),
        other => panic!("expected NestedScene {{ parent: \"p\" }}, got: {other:?}"),
    }
}

/// A `<font>` element remains the one legitimate exception: it is a real
/// element of the dialect, and html5ever nests siblings inside it as a
/// parsing quirk, not authoring error — scenes placed after a `<font>`
/// declaration must keep working exactly as before.
#[test]
fn scene_after_font_declaration_still_works() {
    let html = r##"<rustmotion width="1920" height="1080">
        <font family="Inter" path="fonts/Inter.ttf">
        <scene duration="2"><h1>hi</h1></scene>
    </rustmotion>"##;
    let v = html_to_scenario_value(html).expect("scene after <font> must still transpile");
    assert_eq!(v["scenes"][0]["duration"], serde_json::json!(2));
}

// ---------------------------------------------------------------------------
// Constat 3: unknown HTML tags with a real dialect equivalent must never
// silently degrade into an empty <div>.
// ---------------------------------------------------------------------------

/// Exact reproduction from the brief: `<img>`/`<svg>` used to transpile into
/// `{"type":"div"}` / `{"type":"div","children":[{"type":"div"}]}` — content
/// (`src`, shapes) entirely lost, `validate` exiting 0.
#[test]
fn img_tag_is_refused_not_an_empty_div() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="3"><img src="hero.png" width="400" height="300"></scene></rustmotion>"##;
    let err = html_to_scenario_value(html)
        .expect_err("<img> must be refused, not silently degraded to an empty div");
    match err {
        HtmlError::UnsupportedNativeElement { tag, suggestion } => {
            assert_eq!(tag, "img");
            assert_eq!(suggestion, "rm-image");
        }
        other => panic!("expected UnsupportedNativeElement, got: {other:?}"),
    }
}

#[test]
fn svg_tag_is_refused_not_an_empty_div() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="3"><svg viewBox="0 0 10 10"><circle r="4" fill="#f00"></circle></svg></scene></rustmotion>"##;
    let err = html_to_scenario_value(html)
        .expect_err("<svg> must be refused, not silently degraded to nested empty divs");
    match err {
        HtmlError::UnsupportedNativeElement { tag, suggestion } => {
            assert_eq!(tag, "svg");
            assert_eq!(suggestion, "rm-svg");
        }
        other => panic!("expected UnsupportedNativeElement, got: {other:?}"),
    }
}

/// `<video>` is named explicitly by the audit's own proposed correction
/// alongside `img`/`svg` and shares the exact same empty-div failure mode.
#[test]
fn video_tag_is_refused_not_an_empty_div() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="3"><video src="clip.mp4" width="400" height="300"></video></scene></rustmotion>"##;
    let err = html_to_scenario_value(html)
        .expect_err("<video> must be refused, not silently degraded to an empty div");
    match err {
        HtmlError::UnsupportedNativeElement { tag, suggestion } => {
            assert_eq!(tag, "video");
            assert_eq!(suggestion, "rm-video");
        }
        other => panic!("expected UnsupportedNativeElement, got: {other:?}"),
    }
}

/// The asymmetric, already-working path must remain untouched: `<rm-image
/// src="hero.png">` (the dialect's real mechanism for images) still
/// transpiles into a proper `image` component.
#[test]
fn rm_image_custom_element_still_works() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="3"><rm-image src="hero.png" style="width:400; height:300"></rm-image></scene></rustmotion>"##;
    let v = html_to_scenario_value(html).expect("<rm-image> must keep working");
    assert_eq!(
        v["scenes"][0]["children"][0]["type"],
        serde_json::json!("image")
    );
    assert_eq!(
        v["scenes"][0]["children"][0]["src"],
        serde_json::json!("hero.png")
    );
}

// ---------------------------------------------------------------------------
// Constat 4: HTML must be able to express boolean schema fields.
// ---------------------------------------------------------------------------

/// Exact reproduction from the brief: `auto_scroll="false"` used to transpile
/// into the JSON string `"false"`, rejected at validate time with "invalid
/// type: string \"false\", expected a boolean" — no way to write this field
/// from HTML at all.
#[test]
fn explicit_false_string_becomes_json_boolean() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="3"><rm-codeblock language="rust" code="fn main(){}" auto_scroll="false" style="width:400; height:200"></rm-codeblock></scene></rustmotion>"##;
    let v = html_to_scenario_value(html).expect("auto_scroll=\"false\" must transpile");
    assert_eq!(
        v["scenes"][0]["children"][0]["auto_scroll"],
        serde_json::json!(false)
    );
}

#[test]
fn explicit_true_string_becomes_json_boolean() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="3"><rm-codeblock language="rust" code="fn main(){}" auto_scroll="true" style="width:400; height:200"></rm-codeblock></scene></rustmotion>"##;
    let v = html_to_scenario_value(html).expect("auto_scroll=\"true\" must transpile");
    assert_eq!(
        v["scenes"][0]["children"][0]["auto_scroll"],
        serde_json::json!(true)
    );
}

/// AGGRAVANT flagged by the verifier: a bare HTML boolean attribute
/// (`<rm-codeblock diff>`, no `="value"`) used to be silently dropped by the
/// `v.is_empty()` filter — worse than the `="false"` case, because it failed
/// *silently* instead of loudly. It must now become `true`.
#[test]
fn bare_boolean_attribute_becomes_true_not_dropped() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="3"><rm-codeblock language="rust" code="fn main(){}" diff style="width:400; height:200"></rm-codeblock></scene></rustmotion>"##;
    let v = html_to_scenario_value(html).expect("bare `diff` attribute must transpile");
    assert_eq!(
        v["scenes"][0]["children"][0]["diff"],
        serde_json::json!(true),
        "bare boolean attribute must become true, not be silently dropped: {v}"
    );
}
