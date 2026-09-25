use rustmotion_html::{html_to_scenario_value, set_inline_style, HtmlError};
use serde_json::json;

#[test]
fn rm_chart_data_array_is_reachable_via_json_attribute() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="2"><rm-chart type="bar" data='[{"label":"Jan","value":10},{"label":"Feb","value":20}]' style="width:400; height:300"></rm-chart></scene></rustmotion>"##;
    let v = html_to_scenario_value(html).expect("JSON array attribute must transpile");
    let data = v["scenes"][0]["children"][0]["data"]
        .as_array()
        .expect("data is an array");
    assert_eq!(data.len(), 2);
    assert_eq!(data[0]["label"], json!("Jan"));
    assert_eq!(data[1]["value"], json!(20));
}

#[test]
fn rm_custom_element_malformed_json_attribute_is_named_not_silently_stringified() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="2"><rm-chart type="bar" data="[bad json]" style="width:400; height:300"></rm-chart></scene></rustmotion>"##;
    let err = html_to_scenario_value(html)
        .expect_err("malformed JSON-looking attribute value must be refused");
    match err {
        HtmlError::InvalidAttributeJson { tag, attr, .. } => {
            assert_eq!(tag, "rm-chart");
            assert_eq!(attr, "data");
        }
        other => panic!("expected InvalidAttributeJson, got: {other:?}"),
    }
}

#[test]
fn extreme_div_nesting_is_refused_not_a_stack_overflow() {
    let mut html = String::from(r#"<rustmotion width="1920" height="1080"><scene duration="2">"#);
    for _ in 0..500 {
        html.push_str("<div>");
    }
    html.push_str("deep");
    for _ in 0..500 {
        html.push_str("</div>");
    }
    html.push_str("</scene></rustmotion>");
    let err =
        html_to_scenario_value(&html).expect_err("pathological nesting must be refused, not abort");
    assert!(
        matches!(err, HtmlError::NestingTooDeep { .. }),
        "expected NestingTooDeep, got: {err:?}"
    );
}

#[test]
fn ordinary_nesting_still_transpiles() {
    let mut html = String::from(r#"<rustmotion width="1920" height="1080"><scene duration="2">"#);
    for _ in 0..10 {
        html.push_str("<div>");
    }
    html.push_str("<p>hi</p>");
    for _ in 0..10 {
        html.push_str("</div>");
    }
    html.push_str("</scene></rustmotion>");
    html_to_scenario_value(&html).expect("ordinary nesting depth must still transpile");
}

#[test]
fn box_shadow_json_style_value_is_reachable() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="2"><div style='box-shadow:[{"offset-x":0,"offset-y":4,"blur":8,"color":"#00000055"}]'></div></scene></rustmotion>"##;
    let v = html_to_scenario_value(html).expect("JSON box-shadow value must transpile");
    let shadow = &v["scenes"][0]["children"][0]["style"]["box-shadow"][0];
    assert_eq!(shadow["offset-y"], json!(4));
    assert_eq!(shadow["color"], json!("#00000055"));
}

#[test]
fn malformed_json_style_value_is_named_not_dropped() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="2"><div style="transform:[bad]"></div></scene></rustmotion>"##;
    let err = html_to_scenario_value(html)
        .expect_err("malformed JSON-looking style value must be refused");
    match err {
        HtmlError::InvalidStylePropertyJson { prop, .. } => assert_eq!(prop, "transform"),
        other => panic!("expected InvalidStylePropertyJson, got: {other:?}"),
    }
}

#[test]
fn source_indentation_collapses_to_a_single_space() {
    let html = "<rustmotion width=\"1920\" height=\"1080\"><scene duration=\"2\"><h1>Hello\n        World</h1></scene></rustmotion>";
    let v = html_to_scenario_value(html).expect("must transpile");
    assert_eq!(
        v["scenes"][0]["children"][0]["content"],
        json!("Hello World")
    );
}

#[test]
fn white_space_pre_opts_out_of_collapsing() {
    let html = "<rustmotion width=\"1920\" height=\"1080\"><scene duration=\"2\"><h1 style=\"white-space:pre\">Hello\n  World</h1></scene></rustmotion>";
    let v = html_to_scenario_value(html).expect("must transpile");
    assert_eq!(
        v["scenes"][0]["children"][0]["content"],
        json!("Hello\n  World")
    );
}

#[test]
fn style_declaration_missing_colon_is_refused_not_dropped() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="2"><div style="font-size 400"></div></scene></rustmotion>"##;
    let err = html_to_scenario_value(html)
        .expect_err("a style declaration with no ':' must be refused, not silently dropped");
    assert!(
        matches!(err, HtmlError::StyleDeclarationMissingColon { .. }),
        "expected StyleDeclarationMissingColon, got: {err:?}"
    );
}

#[test]
fn stray_root_level_div_is_refused_not_silently_dropped() {
    let html = r##"<rustmotion width="1920" height="1080"><div style="padding:10">stray</div><scene duration="2"><h1>top</h1></scene></rustmotion>"##;
    let err = html_to_scenario_value(html)
        .expect_err("a stray element outside any <scene> must be refused, not dropped");
    match err {
        HtmlError::UnsupportedRootChild { tag } => assert_eq!(tag, "div"),
        other => panic!("expected UnsupportedRootChild, got: {other:?}"),
    }
}

#[test]
fn ignored_tags_at_root_level_stay_silently_skipped() {
    let html = r##"<rustmotion width="1920" height="1080"><script>alert(1)</script><scene duration="2"><h1>top</h1></scene></rustmotion>"##;
    html_to_scenario_value(html)
        .expect("<script> at root level must stay silently skipped, matching real HTML");
}

#[test]
fn root_background_json_object_is_refused() {
    let html = r##"<rustmotion width="1920" height="1080" background='{"gradient":"linear","colors":["#000","#fff"]}'><scene duration="2"><h1>hi</h1></scene></rustmotion>"##;
    let err = html_to_scenario_value(html)
        .expect_err("a JSON object root background must be refused, not transpiled dead");
    assert!(
        matches!(err, HtmlError::VideoBackgroundMustBeString(_)),
        "expected VideoBackgroundMustBeString, got: {err:?}"
    );
}

#[test]
fn root_background_plain_string_still_works() {
    let html = r##"<rustmotion width="1920" height="1080" background="#0f172a"><scene duration="2"><h1>hi</h1></scene></rustmotion>"##;
    let v = html_to_scenario_value(html).expect("plain color string must still transpile");
    assert_eq!(v["video"]["background"], json!("#0f172a"));
}

#[test]
fn typo_d_heading_tag_is_refused_not_a_silent_div() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="2"><h7>Hello</h7></scene></rustmotion>"##;
    let err = html_to_scenario_value(html)
        .expect_err("an unrecognized tag must be refused, not silently downgraded to a div");
    match err {
        HtmlError::UnknownTag { tag, .. } => assert_eq!(tag, "h7"),
        other => panic!("expected UnknownTag, got: {other:?}"),
    }
}

#[test]
fn typo_d_div_tag_suggests_the_real_tag() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="2"><dvi>Hello</dvi></scene></rustmotion>"##;
    let err = html_to_scenario_value(html).expect_err("must be refused");
    match err {
        HtmlError::UnknownTag { tag, suggestion } => {
            assert_eq!(tag, "dvi");
            assert_eq!(suggestion.as_deref(), Some("div"));
        }
        other => panic!("expected UnknownTag, got: {other:?}"),
    }
}

#[test]
fn known_container_tags_still_work() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="2"><section style="gap:8"><p>ok</p></section></scene></rustmotion>"##;
    html_to_scenario_value(html).expect("a recognized container tag must still transpile");
}

#[test]
fn b_and_code_flatten_into_the_parent_text() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="2"><p>Hello <b>bold</b> and <code>mono</code></p></scene></rustmotion>"##;
    let v = html_to_scenario_value(html).expect("must transpile");
    assert_eq!(
        v["scenes"][0]["children"][0]["content"],
        json!("Hello bold and mono")
    );
}

#[test]
fn br_becomes_a_literal_newline_in_text_content() {
    let html = r##"<rustmotion width="1920" height="1080"><scene duration="2"><p>Line one<br>Line two</p></scene></rustmotion>"##;
    let v = html_to_scenario_value(html).expect("must transpile");
    assert_eq!(
        v["scenes"][0]["children"][0]["content"],
        json!("Line one\nLine two")
    );
}

#[test]
fn br_outside_text_content_is_refused() {
    let html =
        r##"<rustmotion width="1920" height="1080"><scene duration="2"><br></scene></rustmotion>"##;
    let err = html_to_scenario_value(html).expect_err("a top-level <br> must be refused");
    assert!(
        matches!(err, HtmlError::BrOutsideTextContent),
        "expected BrOutsideTextContent, got: {err:?}"
    );
}

#[test]
fn write_back_ignores_the_literal_inside_an_earlier_comment() {
    let html = concat!(
        "<!-- old draft: <rustmotion width=\"1\" height=\"1\"></rustmotion> -->\n",
        "<rustmotion width=\"100\" height=\"100\"><scene duration=\"2\">",
        "<h1 style=\"font-size:96\">Hi</h1></scene></rustmotion>\n",
    );
    let out = set_inline_style(html, "/scenes/0/children/0", "font-size", "120")
        .expect("pointer resolves to the real root, not the commented-out draft");
    assert!(
        out.contains("<!-- old draft: <rustmotion width=\"1\" height=\"1\"></rustmotion> -->"),
        "the comment must survive untouched: {out}"
    );
    assert!(out.contains("font-size:120"), "the edit must apply: {out}");
    let v = html_to_scenario_value(&out).expect("must still round-trip");
    assert_eq!(
        v["scenes"][0]["children"][0]["style"]["font-size"],
        json!(120)
    );
}

#[test]
fn invalid_font_weight_entry_is_refused_not_dropped() {
    let html = r##"<rustmotion width="1920" height="1080"><font family="Inter" source="google" weights="40O,700"><scene duration="2"><h1>hi</h1></scene></rustmotion>"##;
    let err = html_to_scenario_value(html)
        .expect_err("an invalid weight entry must be refused, not silently dropped");
    match err {
        HtmlError::InvalidFontWeight { value } => assert_eq!(value, "40O"),
        other => panic!("expected InvalidFontWeight, got: {other:?}"),
    }
}

#[test]
fn root_audio_track_array_is_reachable() {
    let html = r##"<rustmotion width="1920" height="1080" audio='[{"src":"music.mp3","volume":0.8}]'><scene duration="2"><h1>hi</h1></scene></rustmotion>"##;
    let v = html_to_scenario_value(html).expect("audio attribute must transpile");
    assert_eq!(v["audio"][0]["src"], json!("music.mp3"));
    assert_eq!(v["audio"][0]["volume"], json!(0.8));
}

#[test]
fn root_audio_non_array_json_is_refused() {
    let html = r##"<rustmotion width="1920" height="1080" audio='{"src":"music.mp3"}'><scene duration="2"><h1>hi</h1></scene></rustmotion>"##;
    let err = html_to_scenario_value(html).expect_err("a non-array audio value must be refused");
    assert!(
        matches!(err, HtmlError::InvalidAudioJson(_)),
        "expected InvalidAudioJson, got: {err:?}"
    );
}
