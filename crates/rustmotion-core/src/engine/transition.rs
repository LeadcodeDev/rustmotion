use crate::engine::animator::ease;
use crate::schema::{EasingType, PanBackground, TransitionType};
use skia_safe::{surfaces, Color4f, ColorType, ImageInfo, Paint, Path, Rect};

/// Composite two RGBA frames during a transition.
/// `progress` goes from 0.0 (fully frame_a) to 1.0 (fully frame_b).
pub fn apply_transition(
    frame_a: &[u8],
    frame_b: &[u8],
    width: u32,
    height: u32,
    progress: f64,
    transition_type: &TransitionType,
) -> Vec<u8> {
    let progress = progress.clamp(0.0, 1.0) as f32;

    match transition_type {
        TransitionType::Fade => blend_fade(frame_a, frame_b, progress),
        TransitionType::WipeLeft => {
            wipe(frame_a, frame_b, width, height, progress, Direction::Left)
        }
        TransitionType::WipeRight => {
            wipe(frame_a, frame_b, width, height, progress, Direction::Right)
        }
        TransitionType::WipeUp => wipe(frame_a, frame_b, width, height, progress, Direction::Up),
        TransitionType::WipeDown => {
            wipe(frame_a, frame_b, width, height, progress, Direction::Down)
        }
        TransitionType::ZoomIn => zoom_transition(frame_a, frame_b, width, height, progress, true),
        TransitionType::ZoomOut => {
            zoom_transition(frame_a, frame_b, width, height, progress, false)
        }
        TransitionType::Flip => flip_transition(frame_a, frame_b, width, height, progress),
        TransitionType::ClockWipe => clock_wipe(frame_a, frame_b, width, height, progress),
        TransitionType::Iris => iris_transition(frame_a, frame_b, width, height, progress),
        TransitionType::Slide => slide_transition(frame_a, frame_b, width, height, progress),
        TransitionType::Dissolve => dissolve_transition(frame_a, frame_b, width, height, progress),
        TransitionType::CameraPan => blend_fade(frame_a, frame_b, progress),
        TransitionType::None => {
            if progress < 0.5 {
                frame_a.to_vec()
            } else {
                frame_b.to_vec()
            }
        }
    }
}

fn blend_fade(frame_a: &[u8], frame_b: &[u8], progress: f32) -> Vec<u8> {
    let inv = 1.0 - progress;
    frame_a
        .iter()
        .zip(frame_b.iter())
        .map(|(&a, &b)| {
            let va = a as f32 * inv;
            let vb = b as f32 * progress;
            (va + vb + 0.5) as u8
        })
        .collect()
}

enum Direction {
    Left,
    Right,
    Up,
    Down,
}

fn wipe(
    frame_a: &[u8],
    frame_b: &[u8],
    width: u32,
    height: u32,
    progress: f32,
    direction: Direction,
) -> Vec<u8> {
    let mut surface = match create_skia_surface(width, height) {
        Some(s) => s,
        None => return blend_fade(frame_a, frame_b, progress),
    };
    let img_a = match frame_to_image(frame_a, width, height) {
        Some(i) => i,
        None => return blend_fade(frame_a, frame_b, progress),
    };
    let img_b = match frame_to_image(frame_b, width, height) {
        Some(i) => i,
        None => return blend_fade(frame_a, frame_b, progress),
    };

    let canvas = surface.canvas();
    let w = width as f32;
    let h = height as f32;

    // Draw frame A as background
    canvas.draw_image(&img_a, (0.0, 0.0), None);

    // Clip frame B to the wipe region
    let clip_rect = match direction {
        Direction::Left => Rect::from_xywh(0.0, 0.0, w * progress, h),
        Direction::Right => Rect::from_xywh(w * (1.0 - progress), 0.0, w * progress, h),
        Direction::Up => Rect::from_xywh(0.0, 0.0, w, h * progress),
        Direction::Down => Rect::from_xywh(0.0, h * (1.0 - progress), w, h * progress),
    };

    canvas.save();
    canvas.clip_rect(clip_rect, skia_safe::ClipOp::Intersect, true);
    canvas.draw_image(&img_b, (0.0, 0.0), None);
    canvas.restore();

    surface_to_pixels(surface, width, height)
}

fn create_skia_surface(width: u32, height: u32) -> Option<skia_safe::Surface> {
    let info = ImageInfo::new(
        (width as i32, height as i32),
        ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    surfaces::raster(&info, None, None)
}

fn frame_to_image(frame: &[u8], width: u32, height: u32) -> Option<skia_safe::Image> {
    let info = ImageInfo::new(
        (width as i32, height as i32),
        ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    let data = skia_safe::Data::new_copy(frame);
    skia_safe::images::raster_from_data(&info, data, width as usize * 4)
}

fn surface_to_pixels(mut surface: skia_safe::Surface, width: u32, height: u32) -> Vec<u8> {
    let row_bytes = width as usize * 4;
    let mut pixels = vec![0u8; row_bytes * height as usize];
    let info = ImageInfo::new(
        (width as i32, height as i32),
        ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    surface.read_pixels(&info, &mut pixels, row_bytes, (0, 0));
    pixels
}

fn zoom_transition(
    frame_a: &[u8],
    frame_b: &[u8],
    width: u32,
    height: u32,
    progress: f32,
    zoom_in: bool,
) -> Vec<u8> {
    let mut surface = match create_skia_surface(width, height) {
        Some(s) => s,
        None => return blend_fade(frame_a, frame_b, progress),
    };
    let img_a = match frame_to_image(frame_a, width, height) {
        Some(i) => i,
        None => return blend_fade(frame_a, frame_b, progress),
    };
    let img_b = match frame_to_image(frame_b, width, height) {
        Some(i) => i,
        None => return blend_fade(frame_a, frame_b, progress),
    };

    let canvas = surface.canvas();
    let w = width as f32;
    let h = height as f32;

    if zoom_in {
        // Frame A zooms in and fades out, revealing frame B
        let scale = 1.0 + progress * 0.3;
        canvas.draw_image(&img_b, (0.0, 0.0), None);
        canvas.save();
        canvas.translate((w / 2.0, h / 2.0));
        canvas.scale((scale, scale));
        canvas.translate((-w / 2.0, -h / 2.0));
        let mut paint = Paint::default();
        paint.set_alpha_f(1.0 - progress);
        canvas.draw_image(&img_a, (0.0, 0.0), Some(&paint));
        canvas.restore();
    } else {
        // Frame B zooms out from larger to normal
        canvas.draw_image(&img_a, (0.0, 0.0), None);
        let scale = 1.3 - progress * 0.3;
        canvas.save();
        canvas.translate((w / 2.0, h / 2.0));
        canvas.scale((scale, scale));
        canvas.translate((-w / 2.0, -h / 2.0));
        let mut paint = Paint::default();
        paint.set_alpha_f(progress);
        canvas.draw_image(&img_b, (0.0, 0.0), Some(&paint));
        canvas.restore();
    }

    surface_to_pixels(surface, width, height)
}

fn flip_transition(
    frame_a: &[u8],
    frame_b: &[u8],
    width: u32,
    height: u32,
    progress: f32,
) -> Vec<u8> {
    let mut surface = match create_skia_surface(width, height) {
        Some(s) => s,
        None => return blend_fade(frame_a, frame_b, progress),
    };
    let img_a = match frame_to_image(frame_a, width, height) {
        Some(i) => i,
        None => return blend_fade(frame_a, frame_b, progress),
    };
    let img_b = match frame_to_image(frame_b, width, height) {
        Some(i) => i,
        None => return blend_fade(frame_a, frame_b, progress),
    };

    let canvas = surface.canvas();
    let w = width as f32;

    // Simulate 3D flip by scaling X axis
    // First half: frame_a shrinks on X. Second half: frame_b grows on X.
    if progress < 0.5 {
        let scale_x = 1.0 - progress * 2.0; // 1.0 -> 0.0
        canvas.clear(Color4f::new(0.0, 0.0, 0.0, 1.0));
        canvas.save();
        canvas.translate((w / 2.0, 0.0));
        canvas.scale((scale_x.max(0.01), 1.0));
        canvas.translate((-w / 2.0, 0.0));
        canvas.draw_image(&img_a, (0.0, 0.0), None);
        canvas.restore();
    } else {
        let scale_x = (progress - 0.5) * 2.0; // 0.0 -> 1.0
        canvas.clear(Color4f::new(0.0, 0.0, 0.0, 1.0));
        canvas.save();
        canvas.translate((w / 2.0, 0.0));
        canvas.scale((scale_x.max(0.01), 1.0));
        canvas.translate((-w / 2.0, 0.0));
        canvas.draw_image(&img_b, (0.0, 0.0), None);
        canvas.restore();
    }

    surface_to_pixels(surface, width, height)
}

fn clock_wipe(frame_a: &[u8], frame_b: &[u8], width: u32, height: u32, progress: f32) -> Vec<u8> {
    let mut surface = match create_skia_surface(width, height) {
        Some(s) => s,
        None => return blend_fade(frame_a, frame_b, progress),
    };
    let img_a = match frame_to_image(frame_a, width, height) {
        Some(i) => i,
        None => return blend_fade(frame_a, frame_b, progress),
    };
    let img_b = match frame_to_image(frame_b, width, height) {
        Some(i) => i,
        None => return blend_fade(frame_a, frame_b, progress),
    };

    let canvas = surface.canvas();
    let w = width as f32;
    let h = height as f32;
    let cx = w / 2.0;
    let cy = h / 2.0;
    let radius = (w * w + h * h).sqrt();

    // Draw frame A as background
    canvas.draw_image(&img_a, (0.0, 0.0), None);

    // Draw frame B clipped to a clock-wipe arc
    let sweep_angle = progress * 360.0;
    let start_angle = -90.0; // Start from top

    let mut path = Path::new();
    path.move_to((cx, cy));
    path.arc_to(
        Rect::from_xywh(cx - radius, cy - radius, radius * 2.0, radius * 2.0),
        start_angle,
        sweep_angle,
        false,
    );
    path.close();

    canvas.save();
    canvas.clip_path(&path, skia_safe::ClipOp::Intersect, true);
    canvas.draw_image(&img_b, (0.0, 0.0), None);
    canvas.restore();

    surface_to_pixels(surface, width, height)
}

fn iris_transition(
    frame_a: &[u8],
    frame_b: &[u8],
    width: u32,
    height: u32,
    progress: f32,
) -> Vec<u8> {
    let mut surface = match create_skia_surface(width, height) {
        Some(s) => s,
        None => return blend_fade(frame_a, frame_b, progress),
    };
    let img_a = match frame_to_image(frame_a, width, height) {
        Some(i) => i,
        None => return blend_fade(frame_a, frame_b, progress),
    };
    let img_b = match frame_to_image(frame_b, width, height) {
        Some(i) => i,
        None => return blend_fade(frame_a, frame_b, progress),
    };

    let canvas = surface.canvas();
    let w = width as f32;
    let h = height as f32;
    let cx = w / 2.0;
    let cy = h / 2.0;
    let max_radius = (w * w + h * h).sqrt() / 2.0;
    let radius = max_radius * progress;

    // Draw frame A as background
    canvas.draw_image(&img_a, (0.0, 0.0), None);

    // Clip frame B to an expanding circle
    let mut path = Path::new();
    path.add_circle((cx, cy), radius, None);

    canvas.save();
    canvas.clip_path(&path, skia_safe::ClipOp::Intersect, true);
    canvas.draw_image(&img_b, (0.0, 0.0), None);
    canvas.restore();

    surface_to_pixels(surface, width, height)
}

fn slide_transition(
    frame_a: &[u8],
    frame_b: &[u8],
    width: u32,
    height: u32,
    progress: f32,
) -> Vec<u8> {
    let mut surface = match create_skia_surface(width, height) {
        Some(s) => s,
        None => return blend_fade(frame_a, frame_b, progress),
    };
    let img_a = match frame_to_image(frame_a, width, height) {
        Some(i) => i,
        None => return blend_fade(frame_a, frame_b, progress),
    };
    let img_b = match frame_to_image(frame_b, width, height) {
        Some(i) => i,
        None => return blend_fade(frame_a, frame_b, progress),
    };

    let canvas = surface.canvas();
    let w = width as f32;

    // Frame A slides left, frame B slides in from right
    let offset = -progress * w;
    canvas.draw_image(&img_a, (offset, 0.0), None);
    canvas.draw_image(&img_b, (offset + w, 0.0), None);

    surface_to_pixels(surface, width, height)
}

fn dissolve_transition(
    frame_a: &[u8],
    frame_b: &[u8],
    _width: u32,
    _height: u32,
    progress: f32,
) -> Vec<u8> {
    // Dissolve is a smooth cross-dissolve (same as fade in standard video editing)
    blend_fade(frame_a, frame_b, progress)
}

/// Camera pan transition: composited background + sliding foreground children.
/// `bg_a`/`bg_b` are the outgoing/incoming backgrounds, `fg_a`/`fg_b` are
/// children-only (transparent). fg_a slides out by (-dx*t, -dy*t), fg_b
/// slides in from (dx*(1-t), dy*(1-t)).
///
/// `pan_background` controls how the two backgrounds combine:
/// - `Static`: neither travels nor scales — the backdrop holds its position,
///   which is what keeps a shared ambience continuous across a beat when both
///   scenes actually share the same background (the crossfade below is then
///   a no-op, since blending a frame with itself returns that frame).
/// - `Travel`: each background moves locked to its own foreground, so the
///   two beats read as different places rather than one space.
///
/// Both modes crossfade the two background layers in f32 rather than through
/// Skia's `Paint` alpha, which quantizes to an 8-bit byte: while the byte
/// climbs, the premultiplied blend truncates ~1 LSB per channel across the
/// whole frame, and the instant it reaches 255 Skia takes the opaque fast
/// path and every pixel regains that level in a single frame — a visible
/// step at 40-80x the local per-frame rate. `blend_fade` already does this
/// crossfade correctly (see its doc); we render each background into its own
/// full-frame layer first (needed for `Travel`'s scale + translate), then
/// hand both raw buffers to it.
#[allow(clippy::too_many_arguments)]
pub fn camera_pan_transition(
    bg_a: &[u8],
    bg_b: &[u8],
    fg_a: &[u8],
    fg_b: &[u8],
    width: u32,
    height: u32,
    progress: f64,
    dx: f32,
    dy: f32,
    easing: &EasingType,
    pan_background: PanBackground,
) -> Vec<u8> {
    let t = ease(progress, easing) as f32;

    let mut surface = match create_skia_surface(width, height) {
        Some(s) => s,
        None => return bg_a.to_vec(),
    };
    let img_fg_a = match frame_to_image(fg_a, width, height) {
        Some(i) => i,
        None => return bg_a.to_vec(),
    };
    let img_fg_b = match frame_to_image(fg_b, width, height) {
        Some(i) => i,
        None => return bg_a.to_vec(),
    };

    // Offsets: the outgoing plane exits, the incoming one arrives. They tile
    // exactly, so together they always cover the frame.
    let (out_x, out_y) = (-dx * t, -dy * t);
    let (in_x, in_y) = (dx * (1.0 - t), dy * (1.0 - t));

    let blended_bg = match pan_background {
        // Travelling: each background moves with its own scene, but at a
        // fraction of the foreground's distance and fading across the pan.
        //
        // Two reasons for the fraction. It is how parallax actually works —
        // what is far away moves less — and it makes the two backgrounds
        // overlap across most of the frame instead of meeting edge to edge.
        // Opaque images laid side by side join on a hard line no crossfade can
        // hide; overlapping ones dissolve into each other.
        PanBackground::Travel => {
            let img_bg_a = match frame_to_image(bg_a, width, height) {
                Some(i) => i,
                None => return bg_a.to_vec(),
            };
            let img_bg_b = match frame_to_image(bg_b, width, height) {
                Some(i) => i,
                None => return bg_a.to_vec(),
            };

            // Backgrounds drift at a fraction of the foreground's distance —
            // that is how parallax works, and it keeps them overlapping
            // instead of meeting edge to edge, where two opaque images join on
            // a line no fade can hide.
            const BG_PARALLAX: f32 = 0.12;
            let (bax, bay) = (out_x * BG_PARALLAX, out_y * BG_PARALLAX);
            let (bbx, bby) = (in_x * BG_PARALLAX, in_y * BG_PARALLAX);

            // Translating an opaque image uncovers a strip on the opposite
            // side, and that strip reads as a hard edge just as much as a
            // join would. Each layer is therefore overscaled by exactly its
            // own current displacement — just enough to cover, never more.
            //
            // Sizing it on the *maximum* drift instead makes the margin
            // constant across the transition, including at both ends where the
            // displacement is zero. The background then jumps between a normal
            // frame and an enlarged one at every junction — measured at up to
            // 128px of halo movement in a single frame, an order of magnitude
            // beyond the drift itself. Tying the margin to the current offset
            // makes it vanish exactly where a transition meets a normal frame,
            // so the two are continuous.
            let w = width as f32;
            let h = height as f32;
            let spread = |ox: f32, oy: f32| {
                let (mx, my) = (ox.abs(), oy.abs());
                Rect::from_ltrb(-mx + ox, -my + oy, w + mx + ox, h + my + oy)
            };

            let layer_a = match render_layer(&img_bg_a, spread(bax, bay), width, height) {
                Some(p) => p,
                None => return bg_a.to_vec(),
            };
            let layer_b = match render_layer(&img_bg_b, spread(bbx, bby), width, height) {
                Some(p) => p,
                None => return bg_a.to_vec(),
            };
            blend_fade(&layer_a, &layer_b, t)
        }
        // Static: no spatial movement, but still crossfaded in place. When
        // both scenes share the same background this is a no-op — blending a
        // frame with itself is that frame, so it stays visually frozen, which
        // is what makes the junction invisible. When they don't, holding A
        // for the whole pan and jump-cutting to B on the first normal frame
        // afterward measured +8.25 mean luminance in a single frame (385x the
        // local rate) — a hard cut. Crossfading spreads that change across
        // the whole pan instead of concentrating it at the boundary.
        PanBackground::Static => blend_fade(bg_a, bg_b, t),
    };
    let img_bg = match frame_to_image(&blended_bg, width, height) {
        Some(i) => i,
        None => return bg_a.to_vec(),
    };

    let canvas = surface.canvas();
    canvas.draw_image(&img_bg, (0.0, 0.0), None);
    canvas.draw_image(&img_fg_a, (out_x, out_y), None);
    canvas.draw_image(&img_fg_b, (in_x, in_y), None);

    surface_to_pixels(surface, width, height)
}

/// Draw `img` into `dest` on a fresh full-frame surface and read back the raw
/// pixels. Used to pre-render a background plane (with its `Travel` scale +
/// translate applied) before crossfading it against its counterpart in f32.
fn render_layer(img: &skia_safe::Image, dest: Rect, width: u32, height: u32) -> Option<Vec<u8>> {
    let mut surface = create_skia_surface(width, height)?;
    surface
        .canvas()
        .draw_image_rect(img, None, dest, &Paint::default());
    Some(surface_to_pixels(surface, width, height))
}

#[cfg(test)]
mod camera_pan_tests {
    use super::*;

    fn solid(width: u32, height: u32, r: u8, g: u8, b: u8, a: u8) -> Vec<u8> {
        (0..width * height).flat_map(|_| [r, g, b, a]).collect()
    }

    // Fully transparent so the foreground planes never contribute — isolates
    // the background compositing under test.
    fn transparent(width: u32, height: u32) -> Vec<u8> {
        solid(width, height, 0, 0, 0, 0)
    }

    // Issue #124 item 2: `Static` used to hold bg_a for the entire pan and
    // hard-cut to bg_b afterward. It must now crossfade in place instead —
    // a mid-pan frame should show a genuine blend of both, not either one
    // alone.
    #[test]
    fn static_background_crossfades_instead_of_freezing() {
        let (w, h) = (4, 4);
        let bg_a = solid(w, h, 10, 10, 10, 255);
        let bg_b = solid(w, h, 200, 200, 200, 255);
        let fg = transparent(w, h);

        let out = camera_pan_transition(
            &bg_a,
            &bg_b,
            &fg,
            &fg,
            w,
            h,
            0.5,
            0.0,
            0.0,
            &EasingType::Linear,
            PanBackground::Static,
        );

        // blend_fade(10, 200, 0.5) = (10*0.5 + 200*0.5 + 0.5) as u8 = 105.
        for px in out.chunks_exact(4) {
            assert_eq!(
                px,
                [105, 105, 105, 255],
                "mid-pan Static frame must be a blend of bg_a and bg_b, not a copy of either"
            );
        }
        assert_ne!(out, bg_a, "must have moved away from bg_a by the midpoint");
        assert_ne!(
            out, bg_b,
            "must not have already reached bg_b at the midpoint"
        );
    }

    // Issue #124 item 1 + 3: the crossfade must be exact float math with no
    // residual once progress reaches 1.0 — no Skia alpha-byte quantization
    // left over from an `Option`-based alpha blend.
    #[test]
    fn static_background_reaches_bg_b_exactly_at_full_progress() {
        let (w, h) = (4, 4);
        let bg_a = solid(w, h, 10, 10, 10, 255);
        let bg_b = solid(w, h, 200, 200, 200, 255);
        let fg = transparent(w, h);

        let out = camera_pan_transition(
            &bg_a,
            &bg_b,
            &fg,
            &fg,
            w,
            h,
            1.0,
            0.0,
            0.0,
            &EasingType::Linear,
            PanBackground::Static,
        );
        assert_eq!(out, bg_b, "progress=1.0 must land exactly on bg_b");
    }

    // Travel mode's incoming layer has zero offset at t=1 (in_x = dx*(1-t) =
    // 0), so it is drawn 1:1 with no resampling — the crossfade should still
    // land exactly on bg_b there too.
    #[test]
    fn travel_background_reaches_bg_b_exactly_at_full_progress() {
        let (w, h) = (4, 4);
        let bg_a = solid(w, h, 10, 10, 10, 255);
        let bg_b = solid(w, h, 200, 200, 200, 255);
        let fg = transparent(w, h);

        let out = camera_pan_transition(
            &bg_a,
            &bg_b,
            &fg,
            &fg,
            w,
            h,
            1.0,
            100.0,
            0.0,
            &EasingType::Linear,
            PanBackground::Travel,
        );
        assert_eq!(
            out, bg_b,
            "progress=1.0 must land exactly on bg_b under Travel too"
        );
    }
}
