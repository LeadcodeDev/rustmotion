//! `chromatic_wipe`: a fast slide whose reveal edge splits into red and cyan
//! at the peak and recombines as it lands.
//!
//! The two properties worth pinning are that `aberration: 0` degrades to a
//! plain slide (so the knob genuinely goes to zero rather than to "a bit
//! less"), and that the split is *gone* by the time the transition ends —
//! a residual channel offset on the last frame would bleed a coloured fringe
//! into the scene that follows.

use rustmotion_core::engine::transition::{apply_transition, TransitionOptions};
use rustmotion_core::schema::{TransitionDirection, TransitionType};

const W: u32 = 64;
const H: u32 = 48;

/// Two flat frames, so any channel offset shows up as a colour that is in
/// neither of them.
fn frames() -> (Vec<u8>, Vec<u8>) {
    let a: Vec<u8> = (0..W * H).flat_map(|_| [200u8, 200, 200, 255]).collect();
    let b: Vec<u8> = (0..W * H).flat_map(|_| [40u8, 40, 40, 255]).collect();
    (a, b)
}

fn opts(aberration: f32, direction: TransitionDirection) -> TransitionOptions {
    TransitionOptions {
        aberration,
        direction,
        ..TransitionOptions::default()
    }
}

fn composite(progress: f64, o: &TransitionOptions) -> Vec<u8> {
    let (a, b) = frames();
    apply_transition(&a, &b, W, H, progress, &TransitionType::ChromaticWipe, o)
}

fn slide(progress: f64) -> Vec<u8> {
    let (a, b) = frames();
    apply_transition(
        &a,
        &b,
        W,
        H,
        progress,
        &TransitionType::Slide,
        &TransitionOptions::default(),
    )
}

#[test]
fn aberration_zero_is_a_plain_slide() {
    for p in [0.0, 0.25, 0.5, 0.75, 1.0] {
        assert_eq!(
            composite(p, &opts(0.0, TransitionDirection::Left)),
            slide(p),
            "with the split turned off, the composite must be byte-identical to `slide` at \
             progress {p}"
        );
    }
}

#[test]
fn the_split_peaks_in_the_middle_and_is_gone_at_both_ends() {
    let o = opts(1.0, TransitionDirection::Left);
    // At the ends the frames are whole, so the composite matches a plain
    // slide exactly.
    assert_eq!(
        composite(0.0, &o),
        slide(0.0),
        "no split on the first frame of the transition"
    );
    assert_eq!(
        composite(1.0, &o),
        slide(1.0),
        "no split on the last frame — a residual fringe would bleed into the next scene"
    );
    // In the middle, the seam between the two frames has to differ.
    assert_ne!(
        composite(0.5, &o),
        slide(0.5),
        "mid-transition the channels should be visibly split"
    );
}

#[test]
fn a_bigger_aberration_splits_further() {
    let mid = 0.5;
    let subtle = composite(mid, &opts(0.5, TransitionDirection::Left));
    let loud = composite(mid, &opts(2.0, TransitionDirection::Left));
    let plain = slide(mid);

    // Distance from the un-split composite: more aberration, more difference.
    let distance = |x: &[u8]| -> u64 {
        x.iter()
            .zip(plain.iter())
            .map(|(a, b)| (*a as i32 - *b as i32).unsigned_abs() as u64)
            .sum()
    };
    assert!(
        distance(&loud) > distance(&subtle),
        "aberration 2.0 should split further than 0.5 (loud={}, subtle={})",
        distance(&loud),
        distance(&subtle)
    );
}

#[test]
fn every_direction_lands_on_the_incoming_frame() {
    // Whichever way it travels, the transition has to *finish*: a direction
    // whose arithmetic left the incoming frame off-screen at progress 1
    // would end the cut on the wrong scene.
    let (_, b) = frames();
    for direction in [
        TransitionDirection::Left,
        TransitionDirection::Right,
        TransitionDirection::Up,
        TransitionDirection::Down,
    ] {
        let end = composite(1.0, &opts(1.0, direction));
        assert_eq!(
            end, b,
            "travelling {direction:?}, progress 1.0 must show frame B alone"
        );
    }
}

#[test]
fn each_direction_produces_a_different_mid_frame() {
    let mids: Vec<Vec<u8>> = [
        TransitionDirection::Left,
        TransitionDirection::Right,
        TransitionDirection::Up,
        TransitionDirection::Down,
    ]
    .into_iter()
    .map(|d| composite(0.35, &opts(1.0, d)))
    .collect();

    for i in 0..mids.len() {
        for j in (i + 1)..mids.len() {
            assert_ne!(
                mids[i], mids[j],
                "directions {i} and {j} produced the same frame — the direction is being ignored"
            );
        }
    }
}
