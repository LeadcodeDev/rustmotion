use rustmotion_core::engine::transition::{apply_transition, TransitionOptions};
use rustmotion_core::schema::{TransitionDirection, TransitionType};

const W: u32 = 64;
const H: u32 = 48;

fn solid(width: u32, height: u32, r: u8, g: u8, b: u8) -> Vec<u8> {
    (0..width * height).flat_map(|_| [r, g, b, 255]).collect()
}

fn off_centre_stripe(width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    let (x0, x1) = (width * 5 / 8, width * 7 / 8);
    for _y in 0..height {
        for x in 0..width {
            if x >= x0 && x < x1 {
                out.extend_from_slice(&[240, 240, 240, 255]);
            } else {
                out.extend_from_slice(&[10, 10, 10, 255]);
            }
        }
    }
    out
}

fn frames() -> (Vec<u8>, Vec<u8>) {
    (solid(W, H, 200, 200, 200), solid(W, H, 40, 40, 40))
}

fn opts(strength: f32, direction: TransitionDirection) -> TransitionOptions {
    TransitionOptions {
        strength,
        direction,
        ..TransitionOptions::default()
    }
}

fn composite(progress: f64, o: &TransitionOptions) -> Vec<u8> {
    let (a, b) = frames();
    apply_transition(&a, &b, W, H, progress, &TransitionType::Whip, o)
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
fn zero_at_both_ends_even_with_a_strong_streak() {
    let (a, b) = frames();
    let o = opts(6.0, TransitionDirection::Left);

    let at_start = apply_transition(&a, &b, W, H, 0.0, &TransitionType::Whip, &o);
    assert_eq!(
        at_start, a,
        "progress 0 must be pixel-identical to the source frame — no residual streak"
    );

    let at_end = apply_transition(&a, &b, W, H, 1.0, &TransitionType::Whip, &o);
    assert_eq!(
        at_end, b,
        "progress 1 must be pixel-identical to the destination frame — a leftover streak here \
         would bleed into the next scene"
    );
}

#[test]
fn strength_zero_is_a_plain_slide_at_every_progress() {
    for p in [0.0, 0.2, 0.4, 0.6, 0.8, 1.0] {
        assert_eq!(
            composite(p, &opts(0.0, TransitionDirection::Left)),
            slide(p),
            "with no strength, `whip` must be byte-identical to `slide` at progress {p}"
        );
    }
}

#[test]
fn nonzero_strength_changes_the_mid_transition_frame() {
    let with_streak = composite(0.5, &opts(3.0, TransitionDirection::Left));
    let plain = slide(0.5);
    assert_ne!(
        with_streak, plain,
        "a non-zero strength must visibly change the mid-transition frame"
    );
}

#[test]
fn a_bigger_strength_streaks_further() {
    let a = off_centre_stripe(W, H);
    let b = solid(W, H, 40, 40, 40);
    let row = H / 2;

    let pixel = |buf: &[u8], x: u32| -> u8 {
        let i = ((row * W + x) * 4) as usize;
        buf[i]
    };

    let front = |strength: f32| -> u32 {
        let o = opts(strength, TransitionDirection::Left);
        let out = apply_transition(&a, &b, W, H, 0.5, &TransitionType::Whip, &o);
        (0..W)
            .rev()
            .find(|&x| pixel(&out, x) > 50)
            .expect("some part of the stripe must remain visible")
    };

    let sharp_front = front(0.0);
    let subtle_front = front(1.0);
    let loud_front = front(4.0);

    assert!(
        subtle_front >= sharp_front,
        "a non-zero strength should not pull the trailing edge backward \
         (sharp={sharp_front}, subtle={subtle_front})"
    );
    assert!(
        loud_front > subtle_front,
        "a bigger strength must streak further than a smaller one \
         (subtle={subtle_front}, loud={loud_front})"
    );
}

#[test]
fn every_direction_lands_on_the_incoming_frame() {
    let (_, b) = frames();
    for direction in [
        TransitionDirection::Left,
        TransitionDirection::Right,
        TransitionDirection::Up,
        TransitionDirection::Down,
    ] {
        let end = composite(1.0, &opts(2.0, direction));
        assert_eq!(
            end, b,
            "travelling {direction:?}, progress 1.0 must show frame B alone"
        );
    }
}

#[test]
fn each_direction_produces_a_different_mid_frame() {
    let a = off_centre_stripe(W, H);
    let b = solid(W, H, 40, 40, 40);
    let mids: Vec<Vec<u8>> = [
        TransitionDirection::Left,
        TransitionDirection::Right,
        TransitionDirection::Up,
        TransitionDirection::Down,
    ]
    .into_iter()
    .map(|d| {
        let o = opts(2.0, d);
        apply_transition(&a, &b, W, H, 0.4, &TransitionType::Whip, &o)
    })
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

#[test]
fn deterministic_across_repeated_renders() {
    let (a, b) = frames();
    let o = opts(3.0, TransitionDirection::Right);
    let first = apply_transition(&a, &b, W, H, 0.37, &TransitionType::Whip, &o);
    let second = apply_transition(&a, &b, W, H, 0.37, &TransitionType::Whip, &o);
    assert_eq!(
        first, second,
        "two renders of the same frame must be byte-identical"
    );
}
