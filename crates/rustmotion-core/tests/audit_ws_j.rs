use std::sync::{Arc, Mutex};

use rustmotion_core::css::style::{CssStyle, Edges, Size as CSize};
use rustmotion_core::css::taffy_bridge::ConversionContext;
use rustmotion_core::css::units::{Length, LengthPercentage as CLP};
use rustmotion_core::engine::box_tree::{AvailableSpace, BoxNode, IntrinsicMeasure};
use rustmotion_core::engine::layout_pass::run_layout;

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

type RecordedCall = ((Option<f32>, Option<f32>), (AvailableSpace, AvailableSpace));

#[derive(Default)]
struct RecordingIntrinsic {
    calls: Mutex<Vec<RecordedCall>>,
}

impl IntrinsicMeasure for RecordingIntrinsic {
    fn measure(
        &self,
        known: (Option<f32>, Option<f32>),
        available: (AvailableSpace, AvailableSpace),
    ) -> (f32, f32) {
        self.calls.lock().unwrap().push((known, available));
        (50.0, 30.0)
    }
}

#[test]
fn measure_fn_known_and_available_agree_on_content_box_width() {
    use rustmotion_core::css::style::{AlignItems, Display, FlexDirection};

    let recorder = Arc::new(RecordingIntrinsic::default());
    let leaf = BoxNode::leaf(
        CssStyle {
            padding: Some(Edges::Uniform(CLP::Px(20.0))),
            ..Default::default()
        },
        recorder.clone(),
    );
    let mut root = BoxNode::container(
        CssStyle {
            display: Some(Display::Flex),
            flex_direction: Some(FlexDirection::Column),
            align_items: Some(AlignItems::Stretch),
            width: Some(CSize::Length(CLP::Px(300.0))),
            height: Some(CSize::Length(CLP::Px(300.0))),
            ..Default::default()
        },
        vec![leaf],
    );
    root.assign_ids(1);

    run_layout(&root, (300.0, 300.0), &ConversionContext::default());

    let calls = recorder.calls.lock().unwrap();
    let definite_known_calls: Vec<_> = calls
        .iter()
        .filter(|(known, _)| known.0.is_some())
        .collect();
    assert!(
        !definite_known_calls.is_empty(),
        "expected at least one measure call with a definite known width, got {calls:?}"
    );

    for (known, available) in definite_known_calls {
        if let AvailableSpace::Definite(available_w) = available.0 {
            assert_eq!(
                known.0.unwrap(),
                available_w,
                "known.width and available.width must describe the same \
                 (content-box) box; known={known:?} available={available:?}"
            );
        }
    }
}
