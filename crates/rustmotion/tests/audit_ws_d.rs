//! Regression tests for the media-decoding, frame-cache, and render/batch
//! CLI hardening pass on this branch.
//!
//! Video frame preextraction (`preload::preextract_video_frames`) is
//! exercised through the crate's public `rustmotion::engine::preload`
//! surface directly — no subprocess needed, since the budget math and the
//! cache it guards are both `pub`.

use rustmotion::engine::preload::{
    video_frame_byte_size, would_exceed_cache_budget, VIDEO_FRAME_CACHE_BUDGET_BYTES,
};

// ─── Video frame cache: byte-size arithmetic and budget accounting ────────

/// Ordinary dimensions must still compute the exact byte count a naive
/// `width * height * 4` would — the fix only changes what happens past
/// `u32::MAX`, not everyday results.
#[test]
fn frame_byte_size_matches_plain_multiplication_for_ordinary_dimensions() {
    assert_eq!(video_frame_byte_size(1920, 1080), 1920u64 * 1080 * 4);
    assert_eq!(video_frame_byte_size(0, 0), 0);
}

/// The secondary hazard this closes: `width * height * 4` in plain `u32`
/// wraps for a large-enough declared size (65536×16384 wraps to 0, which
/// used to turn into a division by zero downstream). `u32::MAX` on both
/// dimensions is the most extreme case reachable from a `style.width`/
/// `style.height` pair — the fixed computation must saturate to `u64::MAX`,
/// not wrap to some small number that would slip past the budget check
/// below.
#[test]
fn frame_byte_size_saturates_instead_of_wrapping_on_extreme_dimensions() {
    let huge = video_frame_byte_size(u32::MAX, u32::MAX);
    assert_eq!(
        huge,
        u64::MAX,
        "must saturate at u64::MAX, never wrap silently to a small number"
    );
    assert!(
        would_exceed_cache_budget(0, huge),
        "a saturated size must always fail the budget check"
    );
}

/// The exact policy `preextract_video_frames` applies before spawning
/// ffmpeg: refuse only once the *sum* crosses the ceiling, and never let a
/// saturated `u64::MAX` addend wrap the sum back under it.
#[test]
fn would_exceed_cache_budget_rejects_only_once_the_sum_crosses_the_ceiling() {
    assert!(!would_exceed_cache_budget(
        0,
        VIDEO_FRAME_CACHE_BUDGET_BYTES
    ));
    assert!(would_exceed_cache_budget(
        0,
        VIDEO_FRAME_CACHE_BUDGET_BYTES + 1
    ));
    assert!(would_exceed_cache_budget(VIDEO_FRAME_CACHE_BUDGET_BYTES, 1));
    assert!(
        would_exceed_cache_budget(u64::MAX, 1),
        "saturating add must not wrap past the ceiling"
    );
}
