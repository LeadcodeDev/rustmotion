//! Regression tests for the workstream J (layout pass & CSS unit resolution)
//! audit findings on unit resolution and intrinsic measurement.

use rustmotion_core::css::style::{CssStyle, Edges, Size as CSize};
use rustmotion_core::css::taffy_bridge::ConversionContext;
use rustmotion_core::css::units::{Length, LengthPercentage as CLP};
use rustmotion_core::engine::box_tree::BoxNode;
use rustmotion_core::engine::layout_pass::run_layout;

// ---- `em` on layout properties must resolve against the element's
// own (inherited) font-size, not a constant 16px ----

/// A card sized by its parent's explicit `font-size` (48px) inherits that
/// font-size down to its own layout resolution even though it never sets
/// `font-size` itself. Its `padding: "1em"` must therefore resolve to 48px
/// (`1 * inherited_font_size`), not the pre-fix constant 16px.
#[test]
fn em_padding_resolves_against_inherited_font_size_not_a_constant_16px() {
    let mut root = BoxNode::container(
        CssStyle {
            font_size: Some(Length::Px(48.0)),
            width: Some(CSize::Length(CLP::Px(400.0))),
            height: Some(CSize::Length(CLP::Px(400.0))),
            ..Default::default()
        },
        vec![BoxNode::container(
            CssStyle {
                padding: Some(Edges::Uniform(CLP::String("1em".into()))),
                width: Some(CSize::Length(CLP::Px(200.0))),
                height: Some(CSize::Length(CLP::Px(200.0))),
                ..Default::default()
            },
            vec![],
        )],
    );
    root.assign_ids(1);

    let res = run_layout(&root, (400.0, 400.0), &ConversionContext::default());
    let child = res.get(2).expect("child laid out");
    let (content_x, _, content_w, _) = child.content_box();

    assert_eq!(
        content_x, 48.0,
        "1em padding must resolve against the inherited 48px font-size"
    );
    assert_eq!(content_w, 200.0 - 2.0 * 48.0);
}
