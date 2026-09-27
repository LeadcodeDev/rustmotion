use rustmotion_core::engine::transition::{apply_transition, TransitionOptions};
use rustmotion_core::schema::{IrisRing, IrisShape, TransitionType, ZoomBlurOrigin};

const W: u32 = 64;
const H: u32 = 48;

fn frames() -> (Vec<u8>, Vec<u8>) {
    let a: Vec<u8> = (0..W * H).flat_map(|_| [200u8, 200, 200, 255]).collect();
    let b: Vec<u8> = (0..W * H).flat_map(|_| [40u8, 40, 40, 255]).collect();
    (a, b)
}

fn opts(origin: Option<ZoomBlurOrigin>) -> TransitionOptions {
    TransitionOptions {
        origin,
        ..TransitionOptions::default()
    }
}

fn composite(progress: f64, o: &TransitionOptions) -> Vec<u8> {
    let (a, b) = frames();
    apply_transition(&a, &b, W, H, progress, &TransitionType::Iris, o)
}

#[test]
fn a_corner_origin_reveals_that_corner_early_instead_of_the_frame_centre() {
    let a: Vec<u8> = (0..W * H).flat_map(|_| [5u8, 6, 17, 255]).collect();
    let b: Vec<u8> = (0..W * H).flat_map(|_| [225u8, 231, 239, 255]).collect();
    let o = opts(Some(ZoomBlurOrigin { x: 0.0, y: 0.0 }));

    let out = apply_transition(&a, &b, W, H, 0.15, &TransitionType::Iris, &o);
    let top_left = &out[0..3];
    assert_eq!(
        top_left,
        &[225u8, 231, 239],
        "with `origin` pinned to the top-left corner, that corner must already show the incoming \
         scene early in the transition — the old hardcoded-centre mask left it on the outgoing \
         scene regardless of `origin`"
    );
}

#[test]
fn a_corner_origin_changes_the_mid_transition_frame() {
    let centre = composite(0.4, &opts(None));
    let corner = composite(0.4, &opts(Some(ZoomBlurOrigin { x: 2.0, y: 2.0 })));
    assert_ne!(
        centre, corner,
        "moving `origin` to a corner must change which pixels the mask has revealed — `origin` \
         was previously accepted and ignored"
    );
}

#[test]
fn every_origin_lands_on_the_incoming_frame_at_the_end() {
    let (_, b) = frames();
    for origin in [
        None,
        Some(ZoomBlurOrigin { x: 0.0, y: 0.0 }),
        Some(ZoomBlurOrigin {
            x: W as f32,
            y: H as f32,
        }),
        Some(ZoomBlurOrigin { x: 10.0, y: 40.0 }),
    ] {
        let end = composite(1.0, &opts(origin));
        assert_eq!(
            end, b,
            "progress 1.0 must show frame B alone, whatever `origin` was set to"
        );
    }
}

#[test]
fn progress_zero_is_the_source_frame_regardless_of_origin() {
    let (a, _) = frames();
    let start = composite(0.0, &opts(Some(ZoomBlurOrigin { x: 5.0, y: 5.0 })));
    assert_eq!(
        start, a,
        "progress 0.0 must show frame A alone, whatever `origin` was set to"
    );
}

#[test]
fn pill_and_circle_disagree_mid_growth() {
    let circle = TransitionOptions {
        shape: IrisShape::Circle,
        ..TransitionOptions::default()
    };
    let pill = TransitionOptions {
        shape: IrisShape::Pill,
        aspect: 3.0,
        ..TransitionOptions::default()
    };
    let out_circle = composite(0.3, &circle);
    let out_pill = composite(0.3, &pill);
    assert_ne!(
        out_circle, out_pill,
        "a pill mask with a non-1.0 aspect must reveal a different silhouette than a circle"
    );
}

#[test]
fn fill_holds_a_solid_colour_before_revealing_the_next_scene() {
    let o = TransitionOptions {
        fill: Some("#00FF00".to_string()),
        hold: 0.3,
        duration: 0.5,
        ..TransitionOptions::default()
    };
    let during_hold = composite(0.5, &o);
    for px in during_hold.as_chunks::<4>().0 {
        assert_eq!(
            *px,
            [0, 255, 0, 255],
            "the whole frame must be the fill colour while the transition holds"
        );
    }
}

#[test]
fn fill_eventually_reveals_the_incoming_scene() {
    let (_, b) = frames();
    let o = TransitionOptions {
        fill: Some("#00FF00".to_string()),
        hold: 0.1,
        duration: 0.5,
        ..TransitionOptions::default()
    };
    assert_eq!(
        composite(1.0, &o),
        b,
        "even with a fill colour and a hold, progress 1.0 must land on frame B"
    );
}

#[test]
fn without_fill_hold_has_no_effect() {
    let with_hold = composite(
        0.5,
        &TransitionOptions {
            hold: 10.0,
            ..TransitionOptions::default()
        },
    );
    let without_hold = composite(0.5, &TransitionOptions::default());
    assert_eq!(
        with_hold, without_hold,
        "`hold` without `fill` must be a no-op"
    );
}

#[test]
fn ring_paints_a_colour_absent_from_both_scenes() {
    let o = TransitionOptions {
        ring: Some(IrisRing {
            color: "#00FF00".to_string(),
            width: 6.0,
        }),
        ..TransitionOptions::default()
    };
    let out = composite(0.5, &o);
    let has_green = out
        .as_chunks::<4>()
        .0
        .iter()
        .any(|px| px[1] > 200 && px[0] < 60 && px[2] < 60);
    assert!(
        has_green,
        "a ring colour that appears in neither scene must still show up on screen"
    );
}

#[test]
fn reverse_changes_which_scene_the_mask_encloses() {
    let normal = composite(0.3, &TransitionOptions::default());
    let reversed = composite(
        0.3,
        &TransitionOptions {
            reverse: true,
            ..TransitionOptions::default()
        },
    );
    assert_ne!(
        normal, reversed,
        "`reverse` must change which scene the mask currently encloses"
    );
}

#[test]
fn reverse_still_starts_on_a_and_ends_on_b() {
    let (a, b) = frames();
    let o = TransitionOptions {
        reverse: true,
        ..TransitionOptions::default()
    };
    assert_eq!(composite(0.0, &o), a, "reverse must still start on frame A");
    assert_eq!(composite(1.0, &o), b, "reverse must still end on frame B");
}
