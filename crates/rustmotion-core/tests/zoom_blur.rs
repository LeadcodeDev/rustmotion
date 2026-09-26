use rustmotion_core::engine::transition::{apply_transition, TransitionOptions};
use rustmotion_core::schema::{TransitionType, ZoomBlurOrigin};

const W: u32 = 64;
const H: u32 = 48;

fn solid(width: u32, height: u32, r: u8, g: u8, b: u8) -> Vec<u8> {
    (0..width * height).flat_map(|_| [r, g, b, 255]).collect()
}

fn split(width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((width * height * 4) as usize);
    for _y in 0..height {
        for x in 0..width {
            if x < width / 2 {
                out.extend_from_slice(&[240, 240, 240, 255]);
            } else {
                out.extend_from_slice(&[10, 10, 10, 255]);
            }
        }
    }
    out
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

fn opts(strength: f32) -> TransitionOptions {
    TransitionOptions {
        strength,
        ..TransitionOptions::default()
    }
}

fn composite(progress: f64, o: &TransitionOptions) -> Vec<u8> {
    let (a, b) = frames();
    apply_transition(&a, &b, W, H, progress, &TransitionType::ZoomBlur, o)
}

fn pixel(buf: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * width + x) * 4) as usize;
    [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
}

#[test]
fn strength_zero_is_a_plain_zoom_at_every_progress() {
    for p in [0.0, 0.1, 0.25, 0.5, 0.75, 0.9, 1.0] {
        let with_strength_zero = composite(p, &opts(0.0));
        let with_strength_zero_again = composite(p, &opts(0.0));
        assert_eq!(
            with_strength_zero, with_strength_zero_again,
            "must be deterministic at progress {p}"
        );
    }

    let (a, b) = frames();
    let plain_zoom_at_start =
        apply_transition(&a, &b, W, H, 0.0, &TransitionType::ZoomBlur, &opts(0.0));
    assert_eq!(
        plain_zoom_at_start, a,
        "strength 0 at progress 0 must be exactly the source frame"
    );

    let plain_zoom_at_end =
        apply_transition(&a, &b, W, H, 1.0, &TransitionType::ZoomBlur, &opts(0.0));
    assert_eq!(
        plain_zoom_at_end, b,
        "strength 0 at progress 1 must be exactly the destination frame"
    );
}

#[test]
fn nonzero_strength_changes_the_mid_transition_frame() {
    let big_strength_at_midpoint = composite(0.5, &opts(5.0));
    let zero_strength_at_midpoint = composite(0.5, &opts(0.0));
    assert_ne!(
        big_strength_at_midpoint, zero_strength_at_midpoint,
        "a non-zero strength must visibly change the mid-transition frame"
    );
}

#[test]
fn zero_at_both_ends_even_with_strong_streaks() {
    let (a, b) = frames();
    let o = opts(4.0);

    let at_start = apply_transition(&a, &b, W, H, 0.0, &TransitionType::ZoomBlur, &o);
    assert_eq!(
        at_start, a,
        "progress 0 must be pixel-identical to the source frame — no residual smear"
    );

    let at_end = apply_transition(&a, &b, W, H, 1.0, &TransitionType::ZoomBlur, &o);
    assert_eq!(
        at_end, b,
        "progress 1 must be pixel-identical to the destination frame — a leftover streak here \
         would bleed into the next scene"
    );
}

#[test]
fn mid_transition_a_sharp_edge_is_measurably_smeared() {
    let a = off_centre_stripe(W, H);
    let b = solid(W, H, 40, 40, 40);

    let sharp = apply_transition(&a, &b, W, H, 0.5, &TransitionType::ZoomBlur, &opts(0.0));
    let blurred = apply_transition(&a, &b, W, H, 0.5, &TransitionType::ZoomBlur, &opts(3.0));

    let row = H / 2;
    let distinct_sharp = (0..W)
        .map(|x| pixel(&sharp, W, x, row)[0])
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    let distinct_blurred = (0..W)
        .map(|x| pixel(&blurred, W, x, row)[0])
        .collect::<std::collections::BTreeSet<_>>()
        .len();

    assert!(
        distinct_blurred > distinct_sharp + 2,
        "the blurred pass must introduce intermediate values across the edge, away from a \
         stripe that does not sit on the zoom's pivot \
         (sharp had {distinct_sharp} distinct red values, blurred had {distinct_blurred})"
    );
}

#[test]
fn a_bigger_strength_smears_further() {
    let a = off_centre_stripe(W, H);
    let b = solid(W, H, 40, 40, 40);
    let row = H / 2;

    let front = |strength: f32| -> u32 {
        let out = apply_transition(
            &a,
            &b,
            W,
            H,
            0.5,
            &TransitionType::ZoomBlur,
            &opts(strength),
        );
        (0..W)
            .rev()
            .find(|&x| pixel(&out, W, x, row)[0] > 50)
            .expect("some part of the stripe must remain visible")
    };

    let sharp_front = front(0.0);
    let subtle_front = front(0.3);
    let loud_front = front(1.2);

    assert!(
        subtle_front > sharp_front,
        "a non-zero strength must push the trailing edge of the streak past the plain zoom \
         (sharp front={sharp_front}, subtle front={subtle_front})"
    );
    assert!(
        loud_front > subtle_front,
        "a bigger strength must reach further than a smaller one \
         (subtle front={subtle_front}, loud front={loud_front})"
    );
}

#[test]
fn custom_origin_shifts_where_the_streaks_radiate_from() {
    let a = split(W, H);
    let b = solid(W, H, 40, 40, 40);

    let default_origin = TransitionOptions {
        strength: 3.0,
        ..TransitionOptions::default()
    };
    let corner_origin = TransitionOptions {
        strength: 3.0,
        origin: Some(ZoomBlurOrigin { x: 0.0, y: 0.0 }),
        ..TransitionOptions::default()
    };

    let out_default = apply_transition(
        &a,
        &b,
        W,
        H,
        0.5,
        &TransitionType::ZoomBlur,
        &default_origin,
    );
    let out_corner = apply_transition(&a, &b, W, H, 0.5, &TransitionType::ZoomBlur, &corner_origin);

    assert_ne!(
        out_default, out_corner,
        "moving the origin to a corner must change the mid-transition frame"
    );
}

#[test]
fn deterministic_across_repeated_renders() {
    let (a, b) = frames();
    let o = opts(2.0);
    let first = apply_transition(&a, &b, W, H, 0.42, &TransitionType::ZoomBlur, &o);
    let second = apply_transition(&a, &b, W, H, 0.42, &TransitionType::ZoomBlur, &o);
    assert_eq!(
        first, second,
        "two renders of the same frame must be byte-identical"
    );
}
