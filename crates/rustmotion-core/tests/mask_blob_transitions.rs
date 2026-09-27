use rustmotion_core::engine::transition::{apply_transition, TransitionOptions};
use rustmotion_core::schema::{MaskShape, TransitionType, ZoomBlurOrigin};

const W: u32 = 100;
const H: u32 = 100;

fn frames() -> (Vec<u8>, Vec<u8>) {
    let a: Vec<u8> = (0..W * H).flat_map(|_| [200u8, 200, 200, 255]).collect();
    let b: Vec<u8> = (0..W * H).flat_map(|_| [40u8, 40, 40, 255]).collect();
    (a, b)
}

fn pixel(buf: &[u8], x: u32, y: u32) -> [u8; 4] {
    let base = ((y * W + x) * 4) as usize;
    [buf[base], buf[base + 1], buf[base + 2], buf[base + 3]]
}

fn square_silhouette() -> MaskShape {
    MaskShape::Polygon {
        points: vec![(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)],
    }
}

fn mask_opts(from_scale: f32, to_scale: f32) -> TransitionOptions {
    TransitionOptions {
        silhouette: Some(square_silhouette()),
        origin: Some(ZoomBlurOrigin { x: 50.0, y: 50.0 }),
        from_scale,
        to_scale,
        ..TransitionOptions::default()
    }
}

fn composite_mask(progress: f64, o: &TransitionOptions) -> Vec<u8> {
    let (a, b) = frames();
    apply_transition(&a, &b, W, H, progress, &TransitionType::Mask, o)
}

fn composite_blob(progress: f64, o: &TransitionOptions) -> Vec<u8> {
    let (a, b) = frames();
    apply_transition(&a, &b, W, H, progress, &TransitionType::Blob, o)
}

fn composite_wipe(progress: f64, wipe: &TransitionType, o: &TransitionOptions) -> Vec<u8> {
    let (a, b) = frames();
    apply_transition(&a, &b, W, H, progress, wipe, o)
}

#[test]
fn mask_progress_zero_is_byte_identical_to_frame_a_even_with_a_visible_from_scale() {
    let (a, _) = frames();
    let o = mask_opts(0.2, 40.0);
    assert_eq!(
        composite_mask(0.0, &o),
        a,
        "progress 0.0 must be the untouched outgoing frame, even though from_scale is nonzero \
         and would otherwise leave a visible speck of the incoming scene"
    );
}

#[test]
fn mask_progress_one_is_byte_identical_to_frame_b() {
    let (_, b) = frames();
    let o = mask_opts(0.2, 40.0);
    assert_eq!(
        composite_mask(1.0, &o),
        b,
        "progress 1.0 must be the untouched incoming frame"
    );
}

#[test]
fn mask_reveals_only_the_area_the_silhouette_covers_mid_growth() {
    let o = mask_opts(0.0, 10.0);
    let out = composite_mask(0.5, &o);
    assert_eq!(
        pixel(&out, 50, 50),
        [40, 40, 40, 255],
        "frame centre, well inside the grown square, must already show the incoming scene"
    );
    assert_eq!(
        pixel(&out, 5, 5),
        [200, 200, 200, 255],
        "a far corner, outside the grown square, must still show the outgoing scene"
    );
}

#[test]
fn mask_origin_relocates_which_area_is_revealed() {
    let centred = composite_mask(
        0.4,
        &TransitionOptions {
            silhouette: Some(square_silhouette()),
            origin: Some(ZoomBlurOrigin { x: 50.0, y: 50.0 }),
            from_scale: 0.0,
            to_scale: 10.0,
            ..TransitionOptions::default()
        },
    );
    let corner_origin = composite_mask(
        0.4,
        &TransitionOptions {
            silhouette: Some(square_silhouette()),
            origin: Some(ZoomBlurOrigin { x: 5.0, y: 5.0 }),
            from_scale: 0.0,
            to_scale: 10.0,
            ..TransitionOptions::default()
        },
    );
    assert_ne!(
        centred, corner_origin,
        "moving `origin` must change which area the silhouette grows around"
    );
}

#[test]
fn mask_without_a_silhouette_falls_back_to_a_plain_fade_instead_of_panicking() {
    let o = TransitionOptions {
        silhouette: None,
        ..TransitionOptions::default()
    };
    let out = composite_mask(0.5, &o);
    let px = pixel(&out, 50, 50);
    assert_eq!(
        px,
        [120, 120, 120, 255],
        "a `mask` transition with no `silhouette` must fall back to a fade instead of leaving \
         the frame untouched or panicking"
    );
}

#[test]
fn mask_with_a_degenerate_polygon_falls_back_to_a_plain_fade() {
    let o = TransitionOptions {
        silhouette: Some(MaskShape::Polygon {
            points: vec![(0.0, 0.0), (1.0, 1.0)],
        }),
        ..TransitionOptions::default()
    };
    let out = composite_mask(0.5, &o);
    assert_eq!(
        pixel(&out, 50, 50),
        [120, 120, 120, 255],
        "fewer than 3 points cannot describe a silhouette; it must fall back loudly to a fade"
    );
}

#[test]
fn mask_reuses_the_clip_path_path_grammar() {
    let star_d = "M50 0 L61 35 L98 35 L68 57 L79 91 L50 70 L21 91 L32 57 L2 35 L39 35 Z";
    let o = TransitionOptions {
        silhouette: Some(MaskShape::Path {
            d: star_d.to_string(),
        }),
        origin: Some(ZoomBlurOrigin { x: 50.0, y: 50.0 }),
        from_scale: 0.0,
        to_scale: 3.0,
        ..TransitionOptions::default()
    };
    let out = composite_mask(0.6, &o);
    assert_eq!(
        pixel(&out, 50, 45),
        [40, 40, 40, 255],
        "the star's own centre must be inside its own silhouette once it has grown"
    );
}

#[test]
fn blob_progress_zero_is_byte_identical_to_frame_a_at_strong_settings() {
    let (a, _) = frames();
    let o = TransitionOptions {
        lobes: 12,
        wobble: 0.6,
        seed: 99,
        ..TransitionOptions::default()
    };
    assert_eq!(
        composite_blob(0.0, &o),
        a,
        "progress 0.0 must be the untouched outgoing frame regardless of lobes/wobble"
    );
}

#[test]
fn blob_progress_one_is_byte_identical_to_frame_b_at_strong_settings() {
    let (_, b) = frames();
    let o = TransitionOptions {
        lobes: 12,
        wobble: 0.6,
        seed: 99,
        ..TransitionOptions::default()
    };
    assert_eq!(
        composite_blob(1.0, &o),
        b,
        "progress 1.0 must be the untouched incoming frame regardless of lobes/wobble"
    );
}

#[test]
fn blob_covers_the_whole_frame_by_the_end_of_growth_from_any_origin() {
    let (_, b) = frames();
    for origin in [
        None,
        Some(ZoomBlurOrigin { x: 0.0, y: 0.0 }),
        Some(ZoomBlurOrigin {
            x: W as f32,
            y: H as f32,
        }),
    ] {
        let o = TransitionOptions {
            origin,
            lobes: 7,
            wobble: 0.2,
            seed: 3,
            ..TransitionOptions::default()
        };
        assert_eq!(
            composite_blob(0.999, &o),
            b,
            "just short of the end the blob's automatically solved covering radius must already \
             reveal the whole frame, whatever origin it grows from"
        );
    }
}

#[test]
fn blob_same_seed_reproduces_the_same_silhouette() {
    let o = TransitionOptions {
        lobes: 9,
        wobble: 0.3,
        seed: 42,
        ..TransitionOptions::default()
    };
    assert_eq!(
        composite_blob(0.3, &o),
        composite_blob(0.3, &o),
        "the same seed, lobes and wobble must grow an identical silhouette"
    );
}

#[test]
fn blob_a_different_seed_grows_a_different_silhouette() {
    let a = TransitionOptions {
        lobes: 9,
        wobble: 0.3,
        seed: 42,
        ..TransitionOptions::default()
    };
    let b = TransitionOptions {
        lobes: 9,
        wobble: 0.3,
        seed: 7,
        ..TransitionOptions::default()
    };
    assert_ne!(
        composite_blob(0.3, &a),
        composite_blob(0.3, &b),
        "a different seed must wobble the lobes differently"
    );
}

#[test]
fn feathered_wipe_progress_zero_is_byte_identical_to_frame_a() {
    let (a, _) = frames();
    let o = TransitionOptions {
        feather: 40.0,
        band_color: Some("#00FF00".to_string()),
        ..TransitionOptions::default()
    };
    assert_eq!(
        composite_wipe(0.0, &TransitionType::WipeLeft, &o),
        a,
        "a large feather and a band colour must still be exactly zero at progress 0.0"
    );
}

#[test]
fn feathered_wipe_progress_one_is_byte_identical_to_frame_b() {
    let (_, b) = frames();
    let o = TransitionOptions {
        feather: 40.0,
        band_color: Some("#00FF00".to_string()),
        ..TransitionOptions::default()
    };
    assert_eq!(
        composite_wipe(1.0, &TransitionType::WipeLeft, &o),
        b,
        "a large feather and a band colour must still be exactly complete at progress 1.0"
    );
}

#[test]
fn zero_feather_wipe_has_no_partially_blended_pixels() {
    let o = TransitionOptions {
        feather: 0.0,
        ..TransitionOptions::default()
    };
    let out = composite_wipe(0.5, &TransitionType::WipeLeft, &o);
    for px in out.as_chunks::<4>().0 {
        assert!(
            *px == [200, 200, 200, 255] || *px == [40, 40, 40, 255],
            "with feather 0 every pixel must be exactly one of the two source colours, got {px:?}"
        );
    }
}

#[test]
fn a_positive_feather_creates_a_blended_band_at_the_edge() {
    let o = TransitionOptions {
        feather: 30.0,
        ..TransitionOptions::default()
    };
    let out = composite_wipe(0.5, &TransitionType::WipeLeft, &o);
    let has_blend = out.as_chunks::<4>().0.iter().any(|px| {
        px[0] != 200 && px[0] != 40 // neither source grey level
    });
    assert!(
        has_blend,
        "feather must create pixels that are neither source frame's exact colour"
    );
}

#[test]
fn band_color_tints_the_feathered_edge_with_a_hue_absent_from_both_scenes() {
    let o = TransitionOptions {
        feather: 30.0,
        band_color: Some("#00FF00".to_string()),
        ..TransitionOptions::default()
    };
    let out = composite_wipe(0.5, &TransitionType::WipeLeft, &o);
    let has_green_fringe = out
        .as_chunks::<4>()
        .0
        .iter()
        .any(|px| px[1] > px[0] + 30 && px[1] > px[2] + 30);
    assert!(
        has_green_fringe,
        "a band colour absent from both grey scenes must still show up somewhere along the \
         feathered edge"
    );
}

#[test]
fn band_color_is_a_no_op_without_a_feather() {
    let with_band = composite_wipe(
        0.5,
        &TransitionType::WipeLeft,
        &TransitionOptions {
            feather: 0.0,
            band_color: Some("#00FF00".to_string()),
            ..TransitionOptions::default()
        },
    );
    let without_band = composite_wipe(
        0.5,
        &TransitionType::WipeLeft,
        &TransitionOptions {
            feather: 0.0,
            band_color: None,
            ..TransitionOptions::default()
        },
    );
    assert_eq!(
        with_band, without_band,
        "a hard edge (feather 0) has no band to tint, so band_color must be ignored"
    );
}
