use rustmotion_html::{html_to_scenario_value, HtmlError};

#[test]
fn style_block_is_refused_not_painted() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="3"><style>h1 { color: #0f0 }</style><script>alert(1); var x = 2;</script><h1 class="title">Styled by class</h1></scene></rustmotion>"##;
    let err = html_to_scenario_value(html).expect_err("<style> must be refused, not painted");
    assert!(
        matches!(err, HtmlError::StyleElementUnsupported),
        "expected StyleElementUnsupported, got: {err:?}"
    );
}

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

#[test]
fn scene_after_font_declaration_still_works() {
    let html = r##"<rustmotion width="1920" height="1080">
        <font family="Inter" path="fonts/Inter.ttf">
        <scene duration="2"><h1>hi</h1></scene>
    </rustmotion>"##;
    let v = html_to_scenario_value(html).expect("scene after <font> must still transpile");
    assert_eq!(v["scenes"][0]["duration"], serde_json::json!(2));
}

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
