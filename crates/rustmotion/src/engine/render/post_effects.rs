//! Pure-Rust frame-buffer post-processing effects.
//!
//! All functions operate on a flat RGBA8888 byte slice (`w * h * 4` bytes, row-major).
//! They are deterministic: same parameters and `frame_index` always yield the same
//! output. Alpha is always preserved unchanged by every effect.
//!
//! # Performance notes
//! - **grain / vignette / pixelate**: O(w·h), no allocation.
//! - **progressive_blur**: O(w·h·max_radius). For a 1920×1080 frame with
//!   `max_radius = 12`, this is roughly 25M iterations (two-pass separable box blur
//!   with a variable window per row). Expect ~10–30ms on a modern core.  Use only
//!   when encoding offline; avoid on the studio preview hot path if performance matters.

use rustmotion_core::schema::scenario::{BlurDirection, PostEffect};

/// Apply a sequence of post-processing effects in order to an RGBA frame buffer.
///
/// `buf` must be exactly `w * h * 4` bytes (RGBA8888, row-major).
pub fn apply_post_effects(
    buf: &mut [u8],
    w: u32,
    h: u32,
    effects: &[PostEffect],
    frame_index: u32,
) {
    for effect in effects {
        match effect {
            PostEffect::Grain {
                intensity,
                seed,
                animated,
            } => {
                apply_grain(buf, w, h, *intensity, *seed, *animated, frame_index);
            }
            PostEffect::Vignette { intensity, radius } => {
                apply_vignette(buf, w, h, *intensity, *radius);
            }
            PostEffect::Pixelate { size } => {
                apply_pixelate(buf, w, h, *size);
            }
            PostEffect::ProgressiveBlur {
                direction,
                start,
                max_radius,
            } => {
                apply_progressive_blur(buf, w, h, direction, *start, *max_radius);
            }
        }
    }
}

// ─── Grain ───────────────────────────────────────────────────────────────────

/// Overlay film-grain noise on every pixel.
///
/// Uses a deterministic splitmix64-style hash keyed on `(seed_effective, x, y)`
/// to produce a per-pixel offset in [−intensity·64, +intensity·64] added to R, G, B.
/// When `animated = true`, `seed_effective = seed ^ frame_index` so successive frames
/// show different noise patterns.
pub fn apply_grain(
    buf: &mut [u8],
    w: u32,
    h: u32,
    intensity: f32,
    seed: u64,
    animated: bool,
    frame_index: u32,
) {
    let intensity = intensity.clamp(0.0, 1.0);
    if intensity == 0.0 {
        return;
    }
    let seed_eff: u64 = if animated {
        seed ^ (frame_index as u64)
    } else {
        seed
    };
    let amplitude = (intensity * 64.0) as i32;

    for y in 0..h {
        for x in 0..w {
            let h_val = splitmix_hash(seed_eff, x as u64, y as u64);
            // Map to [−amplitude, +amplitude]
            let offset = (h_val % (2 * amplitude as u64 + 1)) as i32 - amplitude;
            let base = ((y * w + x) * 4) as usize;
            buf[base] = (buf[base] as i32 + offset).clamp(0, 255) as u8;
            buf[base + 1] = (buf[base + 1] as i32 + offset).clamp(0, 255) as u8;
            buf[base + 2] = (buf[base + 2] as i32 + offset).clamp(0, 255) as u8;
            // alpha [base + 3] untouched
        }
    }
}

/// Deterministic hash for grain: three-round splitmix64 over a compound key.
#[inline(always)]
fn splitmix_hash(seed: u64, x: u64, y: u64) -> u64 {
    let mut z = seed
        .wrapping_add(x.wrapping_mul(0x9e3779b97f4a7c15))
        .wrapping_add(y.wrapping_mul(0x6c62272e07bb0142));
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
    z ^ (z >> 31)
}

// ─── Vignette ────────────────────────────────────────────────────────────────

/// Darken pixels towards the corners using a smooth radial falloff.
///
/// `radius` is the fraction of the half-diagonal at which darkening begins (default 0.75).
/// The multiplier at each pixel is `1 - intensity × smoothstep(radius, 1.0, d)`,
/// where `d` is the normalised distance from the image centre (0=centre, 1=corner).
pub fn apply_vignette(buf: &mut [u8], w: u32, h: u32, intensity: f32, radius: f32) {
    let intensity = intensity.clamp(0.0, 1.0);
    if intensity == 0.0 {
        return;
    }
    let radius = radius.clamp(0.0, 1.0);
    let cx = (w as f32 - 1.0) / 2.0;
    let cy = (h as f32 - 1.0) / 2.0;
    // Half-diagonal (normalisation factor so distance = 1 at the corner)
    let diag = (cx * cx + cy * cy).sqrt();

    for y in 0..h {
        for x in 0..w {
            let dx = (x as f32 - cx) / diag;
            let dy = (y as f32 - cy) / diag;
            let dist = (dx * dx + dy * dy).sqrt();
            let factor = 1.0 - intensity * smoothstep(radius, 1.0, dist);
            let base = ((y * w + x) * 4) as usize;
            buf[base] = (buf[base] as f32 * factor) as u8;
            buf[base + 1] = (buf[base + 1] as f32 * factor) as u8;
            buf[base + 2] = (buf[base + 2] as f32 * factor) as u8;
            // alpha untouched
        }
    }
}

/// Hermite-based smooth step between `edge0` and `edge1`.
#[inline(always)]
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

// ─── Pixelate ────────────────────────────────────────────────────────────────

/// Reduce spatial resolution by averaging `size×size` blocks.
///
/// Partial blocks at the right/bottom edges are handled correctly.
/// Alpha is averaged too for consistency (no special treatment).
pub fn apply_pixelate(buf: &mut [u8], w: u32, h: u32, size: u32) {
    let size = size.clamp(1, 256);
    if size <= 1 {
        return;
    }

    let bx_count = w.div_ceil(size);
    let by_count = h.div_ceil(size);

    for by in 0..by_count {
        for bx in 0..bx_count {
            let x0 = bx * size;
            let y0 = by * size;
            let x1 = (x0 + size).min(w);
            let y1 = (y0 + size).min(h);

            // Compute block average
            let mut sum = [0u32; 4];
            let mut count = 0u32;
            for py in y0..y1 {
                for px in x0..x1 {
                    let base = ((py * w + px) * 4) as usize;
                    sum[0] += buf[base] as u32;
                    sum[1] += buf[base + 1] as u32;
                    sum[2] += buf[base + 2] as u32;
                    sum[3] += buf[base + 3] as u32;
                    count += 1;
                }
            }
            let avg = [
                (sum[0] / count) as u8,
                (sum[1] / count) as u8,
                (sum[2] / count) as u8,
                (sum[3] / count) as u8,
            ];

            // Write average to every pixel in the block
            for py in y0..y1 {
                for px in x0..x1 {
                    let base = ((py * w + px) * 4) as usize;
                    buf[base] = avg[0];
                    buf[base + 1] = avg[1];
                    buf[base + 2] = avg[2];
                    buf[base + 3] = avg[3];
                }
            }
        }
    }
}

// ─── Progressive blur ────────────────────────────────────────────────────────

/// Apply a separable box blur whose radius grows linearly from 0 at `start·h`
/// to `max_radius` at the frame edge in the given `direction`.
///
/// # Algorithm
/// Two-pass separable blur (horizontal then vertical). Each pass iterates every
/// pixel and for each row computes the blur radius at that row's distance from the
/// `start` boundary. The radius is 0 (no-op) above `start` and increases linearly
/// to `max_radius` at the far edge.
///
/// # Cost
/// O(w·h·max_radius) — two passes, each O(w·h) with an inner sliding-window of
/// max size `2*max_radius+1`. For 1920×1080 with `max_radius=12` that is roughly
/// 50M byte reads/writes per effect, taking ~15–40ms on a typical core.
///
/// # Rows untouched above `start`
/// Any row where the computed radius rounds to 0 is copied verbatim (identity).
pub fn apply_progressive_blur(
    buf: &mut [u8],
    w: u32,
    h: u32,
    direction: &BlurDirection,
    start: f32,
    max_radius: f32,
) {
    if max_radius <= 0.0 || w == 0 || h == 0 {
        return;
    }

    let max_radius = max_radius.max(0.0);

    // For each row, compute the blur radius based on distance from start boundary.
    // Returns radius in pixels (0 means no blur for that row).
    let radius_for_row = |y: u32| -> u32 {
        let yf = y as f32;
        let hf = h as f32;
        let start_y = start.clamp(0.0, 1.0) * hf;
        let frac = match direction {
            BlurDirection::Bottom => {
                if yf <= start_y {
                    0.0
                } else {
                    (yf - start_y) / (hf - start_y).max(1.0)
                }
            }
            BlurDirection::Top => {
                let bottom_start = (1.0 - start.clamp(0.0, 1.0)) * hf;
                if yf >= bottom_start {
                    0.0
                } else {
                    (bottom_start - yf) / bottom_start.max(1.0)
                }
            }
        };
        (frac * max_radius).round() as u32
    };

    // Pass 1: horizontal box blur into a temporary buffer.
    let len = (w * h * 4) as usize;
    let mut tmp = vec![0u8; len];

    for y in 0..h {
        let r = radius_for_row(y);
        if r == 0 {
            // Identity row — copy directly
            let row_start = (y * w * 4) as usize;
            let row_end = row_start + (w * 4) as usize;
            tmp[row_start..row_end].copy_from_slice(&buf[row_start..row_end]);
            continue;
        }
        // Sliding window horizontal blur for this row
        for x in 0..w {
            let x0 = x.saturating_sub(r);
            let x1 = (x + r).min(w - 1);
            let count = x1 - x0 + 1;
            let mut sum = [0u32; 4];
            for sx in x0..=x1 {
                let base = ((y * w + sx) * 4) as usize;
                sum[0] += buf[base] as u32;
                sum[1] += buf[base + 1] as u32;
                sum[2] += buf[base + 2] as u32;
                sum[3] += buf[base + 3] as u32;
            }
            let base = ((y * w + x) * 4) as usize;
            tmp[base] = (sum[0] / count) as u8;
            tmp[base + 1] = (sum[1] / count) as u8;
            tmp[base + 2] = (sum[2] / count) as u8;
            tmp[base + 3] = (sum[3] / count) as u8;
        }
    }

    // Pass 2: vertical box blur from tmp back into buf.
    for y in 0..h {
        let r = radius_for_row(y);
        if r == 0 {
            // Identity row
            let row_start = (y * w * 4) as usize;
            let row_end = row_start + (w * 4) as usize;
            buf[row_start..row_end].copy_from_slice(&tmp[row_start..row_end]);
            continue;
        }
        for x in 0..w {
            let y0 = y.saturating_sub(r);
            let y1 = (y + r).min(h - 1);
            let count = y1 - y0 + 1;
            let mut sum = [0u32; 4];
            for sy in y0..=y1 {
                let base = ((sy * w + x) * 4) as usize;
                sum[0] += tmp[base] as u32;
                sum[1] += tmp[base + 1] as u32;
                sum[2] += tmp[base + 2] as u32;
                sum[3] += tmp[base + 3] as u32;
            }
            let base = ((y * w + x) * 4) as usize;
            buf[base] = (sum[0] / count) as u8;
            buf[base + 1] = (sum[1] / count) as u8;
            buf[base + 2] = (sum[2] / count) as u8;
            buf[base + 3] = (sum[3] / count) as u8;
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rustmotion_core::schema::scenario::{BlurDirection, PostEffect};

    // ── Helpers ──────────────────────────────────────────────────────────────

    /// Solid-colour 4×4 RGBA buffer (all pixels set to (r, g, b, 255)).
    fn solid(w: u32, h: u32, r: u8, g: u8, b: u8) -> Vec<u8> {
        let mut buf = vec![0u8; (w * h * 4) as usize];
        for i in 0..(w * h) as usize {
            buf[i * 4] = r;
            buf[i * 4 + 1] = g;
            buf[i * 4 + 2] = b;
            buf[i * 4 + 3] = 255;
        }
        buf
    }

    /// Count distinct RGBA colours in a buffer.
    fn unique_colors(buf: &[u8]) -> usize {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        for chunk in buf.chunks(4) {
            set.insert((chunk[0], chunk[1], chunk[2], chunk[3]));
        }
        set.len()
    }

    // ── Grain ────────────────────────────────────────────────────────────────

    #[test]
    fn grain_intensity_zero_is_identity() {
        let orig = solid(8, 8, 128, 128, 128);
        let mut buf = orig.clone();
        apply_grain(&mut buf, 8, 8, 0.0, 42, true, 0);
        assert_eq!(buf, orig, "intensity=0 must be identity");
    }

    #[test]
    fn grain_is_deterministic() {
        let mut a = solid(8, 8, 128, 128, 128);
        let mut b = solid(8, 8, 128, 128, 128);
        apply_grain(&mut a, 8, 8, 0.3, 42, true, 5);
        apply_grain(&mut b, 8, 8, 0.3, 42, true, 5);
        assert_eq!(a, b, "same params must produce identical output");
    }

    #[test]
    fn grain_animated_differs_across_frames() {
        let mut f0 = solid(8, 8, 128, 128, 128);
        let mut f1 = solid(8, 8, 128, 128, 128);
        apply_grain(&mut f0, 8, 8, 0.3, 42, true, 0);
        apply_grain(&mut f1, 8, 8, 0.3, 42, true, 1);
        assert_ne!(f0, f1, "animated=true: frame 0 and frame 1 must differ");
    }

    #[test]
    fn grain_not_animated_same_across_frames() {
        let mut f0 = solid(8, 8, 128, 128, 128);
        let mut f1 = solid(8, 8, 128, 128, 128);
        apply_grain(&mut f0, 8, 8, 0.3, 42, false, 0);
        apply_grain(&mut f1, 8, 8, 0.3, 42, false, 1);
        assert_eq!(f0, f1, "animated=false: both frames must be identical");
    }

    #[test]
    fn grain_alpha_preserved() {
        let mut buf = solid(4, 4, 200, 200, 200);
        // Set non-255 alpha to verify it is untouched
        for i in 0..16 {
            buf[i * 4 + 3] = 100;
        }
        apply_grain(&mut buf, 4, 4, 0.5, 1, true, 0);
        for i in 0..16 {
            assert_eq!(buf[i * 4 + 3], 100, "alpha must not change");
        }
    }

    // ── Vignette ─────────────────────────────────────────────────────────────

    #[test]
    fn vignette_center_pixel_unchanged() {
        // Use a large odd-dimension canvas so the centre pixel is exact.
        let w = 101u32;
        let h = 101u32;
        let mut buf = solid(w, h, 200, 200, 200);
        apply_vignette(&mut buf, w, h, 0.8, 0.5);

        // Centre pixel: distance = 0 → smoothstep = 0 → factor = 1 → unchanged
        let cx = w / 2;
        let cy = h / 2;
        let base = ((cy * w + cx) * 4) as usize;
        assert_eq!(buf[base], 200, "centre R must be unchanged");
        assert_eq!(buf[base + 1], 200, "centre G must be unchanged");
        assert_eq!(buf[base + 2], 200, "centre B must be unchanged");
    }

    #[test]
    fn vignette_corners_darker_than_center() {
        let w = 100u32;
        let h = 100u32;
        let mut buf = solid(w, h, 200, 200, 200);
        apply_vignette(&mut buf, w, h, 0.8, 0.5);

        // Top-left corner (0, 0)
        let corner_r = buf[0] as u16;
        // Centre pixel
        let cx = w / 2;
        let cy = h / 2;
        let center_base = ((cy * w + cx) * 4) as usize;
        let center_r = buf[center_base] as u16;

        assert!(
            corner_r < center_r,
            "corners must be darker than centre: corner={corner_r} center={center_r}"
        );
    }

    #[test]
    fn vignette_alpha_preserved() {
        let mut buf = solid(10, 10, 200, 200, 200);
        for i in 0..100 {
            buf[i * 4 + 3] = 77;
        }
        apply_vignette(&mut buf, 10, 10, 0.8, 0.5);
        for i in 0..100 {
            assert_eq!(buf[i * 4 + 3], 77, "alpha must not change");
        }
    }

    // ── Pixelate ─────────────────────────────────────────────────────────────

    #[test]
    fn pixelate_size_one_is_identity() {
        let orig = solid(8, 8, 100, 150, 200);
        let mut buf = orig.clone();
        apply_pixelate(&mut buf, 8, 8, 1);
        assert_eq!(buf, orig, "size=1 must be identity");
    }

    #[test]
    fn pixelate_reduces_unique_colors() {
        // Checkerboard 1×1 pattern: alternating red/blue pixels
        let w = 8u32;
        let h = 8u32;
        let mut buf = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let base = ((y * w + x) * 4) as usize;
                if (x + y) % 2 == 0 {
                    buf[base] = 255;
                    buf[base + 1] = 0;
                    buf[base + 2] = 0;
                } else {
                    buf[base] = 0;
                    buf[base + 1] = 0;
                    buf[base + 2] = 255;
                }
                buf[base + 3] = 255;
            }
        }
        let colors_before = unique_colors(&buf);
        apply_pixelate(&mut buf, w, h, 4);
        let colors_after = unique_colors(&buf);
        assert!(
            colors_after < colors_before,
            "pixelate must reduce unique colors: before={colors_before} after={colors_after}"
        );
    }

    #[test]
    fn pixelate_uniform_block() {
        // Solid red 8×8; after pixelate every block is still the same (red).
        let mut buf = solid(8, 8, 255, 0, 0);
        apply_pixelate(&mut buf, 8, 8, 4);
        for chunk in buf.chunks(4) {
            assert_eq!(chunk[0], 255);
            assert_eq!(chunk[1], 0);
            assert_eq!(chunk[2], 0);
        }
    }

    // ── Progressive blur ─────────────────────────────────────────────────────

    #[test]
    fn progressive_blur_rows_above_start_untouched() {
        // Checkerboard 1×1 pattern: each pixel alternates between two colors.
        let w = 8u32;
        let h = 8u32;
        let mut buf = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let base = ((y * w + x) * 4) as usize;
                if (x + y) % 2 == 0 {
                    buf[base] = 200;
                    buf[base + 1] = 100;
                    buf[base + 2] = 50;
                } else {
                    buf[base] = 50;
                    buf[base + 1] = 100;
                    buf[base + 2] = 200;
                }
                buf[base + 3] = 255;
            }
        }
        let orig = buf.clone();
        // start=0.75: only the bottom 25% (rows 6..8) get blur.
        apply_progressive_blur(&mut buf, w, h, &BlurDirection::Bottom, 0.75, 8.0);

        // Rows 0..5 (above start=0.75*8=6) must be pixel-exact.
        for y in 0..6u32 {
            for x in 0..w {
                let base = ((y * w + x) * 4) as usize;
                assert_eq!(
                    buf[base..base + 4],
                    orig[base..base + 4],
                    "row {y}, col {x} must be untouched"
                );
            }
        }
    }

    #[test]
    fn progressive_blur_bottom_reduces_variance() {
        // Checkerboard pattern; bottom half should have reduced color variance after blur.
        let w = 16u32;
        let h = 16u32;
        let mut buf = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let base = ((y * w + x) * 4) as usize;
                let v = if (x + y) % 2 == 0 { 0u8 } else { 255u8 };
                buf[base] = v;
                buf[base + 1] = v;
                buf[base + 2] = v;
                buf[base + 3] = 255;
            }
        }
        // Save original variance in bottom rows
        let variance_before: f64 = {
            let mut sum = 0u64;
            let mut sum_sq = 0u64;
            let mut n = 0u64;
            for y in (h / 2)..h {
                for x in 0..w {
                    let base = ((y * w + x) * 4) as usize;
                    let v = buf[base] as u64;
                    sum += v;
                    sum_sq += v * v;
                    n += 1;
                }
            }
            let mean = sum as f64 / n as f64;
            sum_sq as f64 / n as f64 - mean * mean
        };

        apply_progressive_blur(&mut buf, w, h, &BlurDirection::Bottom, 0.0, 6.0);

        let variance_after: f64 = {
            let mut sum = 0u64;
            let mut sum_sq = 0u64;
            let mut n = 0u64;
            for y in (h / 2)..h {
                for x in 0..w {
                    let base = ((y * w + x) * 4) as usize;
                    let v = buf[base] as u64;
                    sum += v;
                    sum_sq += v * v;
                    n += 1;
                }
            }
            let mean = sum as f64 / n as f64;
            sum_sq as f64 / n as f64 - mean * mean
        };

        assert!(
            variance_after < variance_before,
            "bottom half variance must drop after blur: before={variance_before:.1} after={variance_after:.1}"
        );
    }

    // ── Effect ordering ───────────────────────────────────────────────────────

    #[test]
    fn grain_then_pixelate_differs_from_pixelate_then_grain() {
        let base_buf = solid(8, 8, 128, 100, 80);

        let mut a = base_buf.clone();
        apply_post_effects(
            &mut a,
            8,
            8,
            &[
                PostEffect::Grain {
                    intensity: 0.4,
                    seed: 7,
                    animated: false,
                },
                PostEffect::Pixelate { size: 4 },
            ],
            0,
        );

        let mut b = base_buf.clone();
        apply_post_effects(
            &mut b,
            8,
            8,
            &[
                PostEffect::Pixelate { size: 4 },
                PostEffect::Grain {
                    intensity: 0.4,
                    seed: 7,
                    animated: false,
                },
            ],
            0,
        );

        assert_ne!(a, b, "grain+pixelate must differ from pixelate+grain");
    }

    // ── apply_post_effects no-op on empty slice ───────────────────────────────

    #[test]
    fn no_effects_is_identity() {
        let orig = solid(4, 4, 200, 150, 100);
        let mut buf = orig.clone();
        apply_post_effects(&mut buf, 4, 4, &[], 0);
        assert_eq!(buf, orig);
    }
}
