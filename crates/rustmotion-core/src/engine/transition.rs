use crate::engine::animator::ease;
use crate::schema::{
    EasingType, PanBackground, PixelDissolveOrder, Transition, TransitionCorner,
    TransitionDirection, TransitionType,
};
use skia_safe::{surfaces, Color4f, ColorType, ImageInfo, Paint, Path, Rect};

/// The per-type knobs a transition may read, bundled.
///
/// Each of these is inert for every transition but the one or two that read
/// it, so they travel together rather than as a growing tail of positional
/// arguments threaded through the render task queue — a shape that made
/// adding a transition a five-file edit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransitionOptions {
    /// `corner_reveal`: which corner the reveal grows from.
    pub corner: TransitionCorner,
    /// `pixel_dissolve`: cell edge in px.
    pub cell: f32,
    /// `pixel_dissolve`: scatter seed.
    pub seed: u32,
    /// `pixel_dissolve`: which cells turn first.
    pub order: PixelDissolveOrder,
    /// `chromatic_wipe`: which way it travels.
    pub direction: TransitionDirection,
    /// `chromatic_wipe`: channel-split multiplier.
    pub aberration: f32,
}

impl Default for TransitionOptions {
    fn default() -> Self {
        Self {
            corner: TransitionCorner::default(),
            cell: 48.0,
            seed: 11,
            order: PixelDissolveOrder::default(),
            direction: TransitionDirection::default(),
            aberration: 1.0,
        }
    }
}

impl From<&Transition> for TransitionOptions {
    fn from(t: &Transition) -> Self {
        Self {
            corner: t.corner,
            cell: t.cell,
            seed: t.seed,
            order: t.order,
            direction: t.direction,
            aberration: t.aberration,
        }
    }
}

/// Composite two RGBA frames during a transition.
/// `progress` goes from 0.0 (fully frame_a) to 1.0 (fully frame_b).
pub fn apply_transition(
    frame_a: &[u8],
    frame_b: &[u8],
    width: u32,
    height: u32,
    progress: f64,
    transition_type: &TransitionType,
    opts: &TransitionOptions,
) -> Vec<u8> {
    let progress = progress.clamp(0.0, 1.0) as f32;
    let TransitionOptions {
        corner,
        cell,
        seed,
        order,
        direction,
        aberration,
    } = *opts;

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
        TransitionType::CornerReveal => {
            corner_reveal(frame_a, frame_b, width, height, progress, corner)
        }
        TransitionType::PixelDissolve => {
            pixel_dissolve(frame_a, frame_b, width, height, progress, cell, seed, order)
        }
        TransitionType::CameraPan => blend_fade(frame_a, frame_b, progress),
        TransitionType::ChromaticWipe => chromatic_wipe(
            frame_a, frame_b, width, height, progress, direction, aberration,
        ),
        TransitionType::None => {
            if progress < 0.5 {
                frame_a.to_vec()
            } else {
                frame_b.to_vec()
            }
        }
    }
}

/// Reveal the incoming frame through a rectangle anchored at one corner.
///
/// Measured on a reference piece, over 15 frames (0.5 s): the right and top
/// edges stay pinned to the frame while the left edge travels 2160 -> 0 and the
/// bottom edge 1480 -> 2152. So it is not a wipe — `wipe_*` moves one
/// full-width band — and not an `iris`, which is a circle. Both edges move at
/// once, and the incoming scene sits still behind the growing window rather
/// than sliding in: what arrives is *uncovered*, not pushed.
fn corner_reveal(
    frame_a: &[u8],
    frame_b: &[u8],
    width: u32,
    height: u32,
    progress: f32,
    corner: TransitionCorner,
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

    let (w, h) = (width as f32, height as f32);
    let rect = corner_rect(corner, w, h, progress);

    let canvas = surface.canvas();
    canvas.draw_image(&img_a, (0.0, 0.0), None);
    canvas.save();
    canvas.clip_rect(rect, skia_safe::ClipOp::Intersect, false);
    canvas.draw_image(&img_b, (0.0, 0.0), None);
    canvas.restore();

    surface_to_pixels(surface, width, height)
}

/// The revealed rectangle at `progress`, anchored so that two edges stay on the
/// frame and two travel.
fn corner_rect(corner: TransitionCorner, w: f32, h: f32, progress: f32) -> skia_safe::Rect {
    let p = progress.clamp(0.0, 1.0);
    let (rw, rh) = (w * p, h * p);
    match corner {
        TransitionCorner::TopRight => skia_safe::Rect::from_xywh(w - rw, 0.0, rw, rh),
        TransitionCorner::TopLeft => skia_safe::Rect::from_xywh(0.0, 0.0, rw, rh),
        TransitionCorner::BottomRight => skia_safe::Rect::from_xywh(w - rw, h - rh, rw, rh),
        TransitionCorner::BottomLeft => skia_safe::Rect::from_xywh(0.0, h - rh, rw, rh),
    }
}

/// How much a cell's own position pulls its threshold, against the hash. Enough
/// to read as a front travelling inward, little enough that the front stays
/// ragged instead of collapsing to a clean rectangle closing in.
const SPATIAL_WEIGHT: f32 = 0.72;

/// Deterministic 0..1 threshold for a cell — the moment it starts to turn.
///
/// A hash of the cell's coordinates, not a random draw: the transition must
/// dissolve the same way on every render, and re-rolling per frame would make
/// the mosaic boil instead of resolve.
fn cell_hash01(col: i32, row: i32, seed: u32) -> f32 {
    let mut h = seed
        .wrapping_mul(0x9E37_79B9)
        .wrapping_add((col as u32).wrapping_mul(0x85EB_CA6B))
        .wrapping_add((row as u32).wrapping_mul(0xC2B2_AE35));
    h ^= h >> 16;
    h = h.wrapping_mul(0x7FEB_352D);
    h ^= h >> 15;
    (h & 0x00FF_FFFF) as f32 / 0x0100_0000 as f32
}

/// The threshold once the spatial order is folded in.
///
/// `EdgesIn` gives border cells an early threshold and the centre a late one,
/// so the subject in the middle is the last thing to go. The hash still
/// contributes: without it the front is a rectangle closing in, which reads as
/// a wipe rather than a dissolve.
fn cell_threshold(
    col: i32,
    row: i32,
    cols: i32,
    rows: i32,
    seed: u32,
    order: PixelDissolveOrder,
) -> f32 {
    let noise = cell_hash01(col, row, seed);
    if order == PixelDissolveOrder::Random {
        return noise;
    }
    // Chebyshev distance from the centre, 0 at the middle and 1 at the border:
    // it follows the frame's own rectangle, where a Euclidean radius would
    // leave the corners lagging behind the edges.
    let (cx, cy) = ((cols - 1) as f32 / 2.0, (rows - 1) as f32 / 2.0);
    let dx = if cx > 0.0 {
        (col as f32 - cx).abs() / cx
    } else {
        0.0
    };
    let dy = if cy > 0.0 {
        (row as f32 - cy).abs() / cy
    } else {
        0.0
    };
    let edge = dx.max(dy).clamp(0.0, 1.0);
    let spatial = match order {
        PixelDissolveOrder::EdgesIn => 1.0 - edge,
        PixelDissolveOrder::CenterOut => edge,
        PixelDissolveOrder::Random => unreachable!("handled above"),
    };
    (spatial * SPATIAL_WEIGHT + noise * (1.0 - SPATIAL_WEIGHT)).clamp(0.0, 1.0)
}

/// Cross-fade the two frames cell by cell on a square lattice.
///
/// Each cell has its own start time, so at any instant the frame is a mosaic of
/// both scenes with a band of half-faded cells between them — which is what
/// separates this from `dissolve` (one global opacity, no structure) and from
/// the wipes (a single hard boundary). `feather` is what makes a cell *fade*
/// rather than flip: with it at 0 the effect degrades to a hard checkerboard.
fn pixel_dissolve(
    frame_a: &[u8],
    frame_b: &[u8],
    width: u32,
    height: u32,
    progress: f32,
    cell: f32,
    seed: u32,
    order: PixelDissolveOrder,
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

    let cell = cell.max(1.0);
    let cols = (width as f32 / cell).ceil() as i32;
    let rows = (height as f32 / cell).ceil() as i32;

    let canvas = surface.canvas();
    canvas.draw_image(&img_a, (0.0, 0.0), None);

    // The whole run has to finish by progress 1, so the schedule is compressed
    // to leave room for the last cell's own fade.
    const FEATHER: f32 = 0.35;
    let p = progress.clamp(0.0, 1.0) * (1.0 + FEATHER);

    for row in 0..rows {
        for col in 0..cols {
            let t = cell_threshold(col, row, cols, rows, seed, order);
            let alpha = ((p - t) / FEATHER).clamp(0.0, 1.0);
            if alpha <= 0.001 {
                continue;
            }
            let rect = Rect::from_xywh(col as f32 * cell, row as f32 * cell, cell, cell);
            canvas.save();
            canvas.clip_rect(rect, skia_safe::ClipOp::Intersect, false);
            let mut paint = Paint::default();
            paint.set_alpha_f(alpha);
            canvas.draw_image(&img_b, (0.0, 0.0), Some(&paint));
            canvas.restore();
        }
    }

    surface_to_pixels(surface, width, height)
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

/// A fast slide whose reveal edge splits into red and cyan at the peak.
///
/// Both frames travel the same way — the incoming one is simply one screen
/// behind — so the edge between them is a hard seam rather than a dissolve.
/// The channel split is applied to that composite, peaking mid-transition and
/// gone by the time it lands, so the flash reads as an artefact of the *speed*
/// of the cut rather than as a colour treatment on either scene.
fn chromatic_wipe(
    frame_a: &[u8],
    frame_b: &[u8],
    width: u32,
    height: u32,
    progress: f32,
    direction: TransitionDirection,
    aberration: f32,
) -> Vec<u8> {
    let (w, h) = (width as f32, height as f32);
    // Slide axis, as a unit vector. Both frames move along it; B starts one
    // full screen back.
    let (ux, uy) = match direction {
        TransitionDirection::Left => (-1.0, 0.0),
        TransitionDirection::Right => (1.0, 0.0),
        TransitionDirection::Up => (0.0, -1.0),
        TransitionDirection::Down => (0.0, 1.0),
    };

    let slid = {
        let mut surface = match create_skia_surface(width, height) {
            Some(s) => s,
            None => return blend_fade(frame_a, frame_b, progress),
        };
        let (Some(img_a), Some(img_b)) = (
            frame_to_image(frame_a, width, height),
            frame_to_image(frame_b, width, height),
        ) else {
            return blend_fade(frame_a, frame_b, progress);
        };
        let canvas = surface.canvas();
        let (dx, dy) = (ux * progress * w, uy * progress * h);
        canvas.draw_image(&img_a, (dx, dy), None);
        canvas.draw_image(&img_b, (dx - ux * w, dy - uy * h), None);
        surface_to_pixels(surface, width, height)
    };

    // Peak at the midpoint, nothing at either end: a split still present on
    // the last frame would bleed into the scene that follows.
    let peak = 1.0 - (progress * 2.0 - 1.0).abs();
    let shift = (aberration.max(0.0) * peak * w * 0.012).round() as i32;
    if shift == 0 {
        return slid;
    }

    // Red leads the travel, blue trails it — the two channels sampled from
    // either side of where green is, which is what a lens does under speed.
    let mut out = slid.clone();
    let (sx, sy) = (
        (ux * shift as f32).round() as i32,
        (uy * shift as f32).round() as i32,
    );
    let sample = |buf: &[u8], x: i32, y: i32, channel: usize| -> u8 {
        let cx = x.clamp(0, width as i32 - 1);
        let cy = y.clamp(0, height as i32 - 1);
        buf[((cy as u32 * width + cx as u32) * 4) as usize + channel]
    };
    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let base = ((y as u32 * width + x as u32) * 4) as usize;
            out[base] = sample(&slid, x - sx, y - sy, 0);
            out[base + 2] = sample(&slid, x + sx, y + sy, 2);
        }
    }
    out
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

    // The scene being left behind dissolves rather than sliding off as a solid
    // slab, and the arriving one materialises. Drift alone gives the two planes
    // different speeds; letting them also come and go is what reads as depth
    // instead of a sheet of paper being pulled sideways.
    //
    // Both curves are pinned at their own end — `fg_a` is fully opaque at t=0,
    // `fg_b` fully opaque at t=1 — because a transition frame sits directly
    // against a normal frame at each junction and any alpha short of 1 there is
    // a visible step. Mirrored exponents (rather than a plain crossfade) keep
    // both planes at 67% through the middle instead of 50%, so the frame never
    // washes out to near-empty half way through.
    const FG_DISSOLVE: f32 = 1.6;
    let mut fg_paint = Paint::default();

    fg_paint.set_alpha_f(1.0 - t.powf(FG_DISSOLVE));
    canvas.draw_image(&img_fg_a, (out_x, out_y), Some(&fg_paint));

    fg_paint.set_alpha_f(1.0 - (1.0 - t).powf(FG_DISSOLVE));
    canvas.draw_image(&img_fg_b, (in_x, in_y), Some(&fg_paint));

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

    // The junction invariant. A transition frame sits directly against a
    // normal frame at each end, so the dissolve must be a no-op exactly there:
    // at progress 0 the outgoing scene is untouched, at progress 1 the
    // incoming one is. Any alpha short of 1 at an endpoint is a visible step,
    // which is the class of bug that produced the halo jumps.
    #[test]
    fn the_foreground_dissolve_is_a_noop_at_both_junctions() {
        let (w, h) = (8u32, 4u32);
        let bg = solid(w, h, 0, 0, 0, 255);
        let fg_a = solid(w, h, 255, 0, 0, 255);
        let fg_b = solid(w, h, 0, 0, 255, 255);

        for (progress, expected) in [(0.0, [255u8, 0, 0]), (1.0, [0, 0, 255])] {
            let out = camera_pan_transition(
                &bg,
                &bg,
                &fg_a,
                &fg_b,
                w,
                h,
                progress,
                8.0,
                0.0,
                &EasingType::Linear,
                PanBackground::Static,
            );
            assert_eq!(
                &out[0..3],
                &expected,
                "at progress {progress} the adjacent scene must render untouched",
            );
        }
    }

    // Mid-pan both planes are partly transparent — that is the effect — but
    // neither may collapse to near-nothing or the frame reads as empty.
    #[test]
    fn mid_pan_both_planes_stay_substantially_visible() {
        let (w, h) = (8u32, 4u32);
        let bg = solid(w, h, 0, 0, 0, 255);
        let fg_a = solid(w, h, 255, 0, 0, 255);
        let fg_b = solid(w, h, 0, 0, 255, 255);

        let out = camera_pan_transition(
            &bg,
            &bg,
            &fg_a,
            &fg_b,
            w,
            h,
            0.5,
            8.0,
            0.0,
            &EasingType::Linear,
            PanBackground::Static,
        );
        // Left half carries the outgoing plane, right half the incoming one.
        let left_red = out[0];
        let right_blue = out[((w - 1) * 4 + 2) as usize];
        assert!(left_red > 128, "outgoing plane faded too far: {left_red}");
        assert!(
            right_blue > 128,
            "incoming plane still too faint: {right_blue}"
        );
    }

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

#[cfg(test)]
mod corner_reveal_tests {
    use super::*;

    /// Two edges stay on the frame, two travel. Measured on the reference
    /// piece: the right and top edges never move while the left runs
    /// 2160 -> 0 and the bottom 1480 -> 2152, over 15 frames.
    #[test]
    fn the_anchored_edges_never_move() {
        for p in [0.05, 0.3, 0.5, 0.8, 1.0] {
            let r = corner_rect(TransitionCorner::TopRight, 1920.0, 1080.0, p);
            assert!((r.right - 1920.0).abs() < 1e-3, "right edge moved at {p}");
            assert!(r.top.abs() < 1e-3, "top edge moved at {p}");
        }
    }

    /// …and the travelling edges do move, monotonically, in the direction the
    /// corner names.
    #[test]
    fn the_travelling_edges_open_from_the_corner() {
        let at = |p| corner_rect(TransitionCorner::TopRight, 1920.0, 1080.0, p);
        let (a, b, c) = (at(0.2), at(0.5), at(0.9));
        assert!(
            a.left > b.left && b.left > c.left,
            "left edge must travel left"
        );
        assert!(
            a.bottom < b.bottom && b.bottom < c.bottom,
            "bottom must travel down"
        );
    }

    /// The ends are the whole point: nothing revealed at 0, everything at 1.
    #[test]
    fn it_starts_empty_and_ends_full() {
        let empty = corner_rect(TransitionCorner::TopRight, 1920.0, 1080.0, 0.0);
        assert_eq!((empty.width(), empty.height()), (0.0, 0.0));
        let full = corner_rect(TransitionCorner::TopRight, 1920.0, 1080.0, 1.0);
        assert_eq!(
            (full.left, full.top, full.right, full.bottom),
            (0.0, 0.0, 1920.0, 1080.0)
        );
    }

    /// Each corner anchors its own two edges — otherwise `corner` is decoration.
    #[test]
    fn every_corner_anchors_its_own_edges() {
        let (w, h, p) = (1920.0f32, 1080.0f32, 0.4);
        let tl = corner_rect(TransitionCorner::TopLeft, w, h, p);
        assert!(tl.left.abs() < 1e-3 && tl.top.abs() < 1e-3);
        let br = corner_rect(TransitionCorner::BottomRight, w, h, p);
        assert!((br.right - w).abs() < 1e-3 && (br.bottom - h).abs() < 1e-3);
        let bl = corner_rect(TransitionCorner::BottomLeft, w, h, p);
        assert!(bl.left.abs() < 1e-3 && (bl.bottom - h).abs() < 1e-3);
    }

    /// Progress outside 0..1 must clamp, not invert the rectangle: a negative
    /// width would make the clip empty and the transition would look like a cut.
    #[test]
    fn out_of_range_progress_clamps() {
        for p in [-0.5, 1.5] {
            let r = corner_rect(TransitionCorner::TopRight, 1920.0, 1080.0, p);
            assert!(
                r.width() >= 0.0 && r.height() >= 0.0,
                "inverted rect at {p}"
            );
        }
    }
}

#[cfg(test)]
mod pixel_dissolve_tests {
    use super::*;

    /// `edges_in` must turn the border before the middle — that is the whole
    /// point: whatever sits in the centre is the last thing to go.
    #[test]
    fn edges_in_turns_the_border_first() {
        let (cols, rows) = (40, 24);
        let border: Vec<f32> = (0..cols)
            .map(|c| cell_threshold(c, 0, cols, rows, 11, PixelDissolveOrder::EdgesIn))
            .collect();
        let middle: Vec<f32> = (0..cols)
            .map(|c| cell_threshold(c, rows / 2, cols, rows, 11, PixelDissolveOrder::EdgesIn))
            .collect();
        let avg = |v: &Vec<f32>| v.iter().sum::<f32>() / v.len() as f32;
        assert!(
            avg(&border) < avg(&middle) - 0.15,
            "border {:.2} must clearly precede the middle {:.2}",
            avg(&border),
            avg(&middle)
        );
        // The very centre goes last.
        let centre = cell_threshold(
            cols / 2,
            rows / 2,
            cols,
            rows,
            11,
            PixelDissolveOrder::EdgesIn,
        );
        assert!(centre > 0.6, "the centre cell must be late, got {centre}");
    }

    /// …and `center_out` is its mirror, or the option is decoration.
    #[test]
    fn center_out_is_the_mirror_of_edges_in() {
        let (cols, rows) = (40, 24);
        for (c, r) in [(0, 0), (20, 12), (39, 5)] {
            let a = cell_threshold(c, r, cols, rows, 11, PixelDissolveOrder::EdgesIn);
            let b = cell_threshold(c, r, cols, rows, 11, PixelDissolveOrder::CenterOut);
            // Same hash contribution, opposite spatial term.
            assert!(
                (a + b - (SPATIAL_WEIGHT + 2.0 * (1.0 - SPATIAL_WEIGHT) * cell_hash01(c, r, 11)))
                    .abs()
                    < 1e-5
            );
        }
    }

    /// The front has to stay ragged. A purely spatial threshold would close a
    /// clean rectangle inward, which reads as a wipe, not a dissolve — so
    /// neighbours at the same distance from the centre must still differ.
    #[test]
    fn the_front_is_ragged_not_a_closing_rectangle() {
        let (cols, rows) = (40, 24);
        let top: Vec<f32> = (0..cols)
            .map(|c| cell_threshold(c, 0, cols, rows, 11, PixelDissolveOrder::EdgesIn))
            .collect();
        let spread = top.iter().cloned().fold(f32::MIN, f32::max)
            - top.iter().cloned().fold(f32::MAX, f32::min);
        assert!(
            spread > 0.15,
            "the border turns as one block: spread {spread}"
        );
    }

    /// `random` keeps its old behaviour — the spatial term must not leak in.
    #[test]
    fn random_ignores_position() {
        let t = cell_threshold(7, 3, 40, 24, 11, PixelDissolveOrder::Random);
        assert_eq!(t, cell_hash01(7, 3, 11));
    }

    /// The same cell must turn at the same moment on every render: a per-frame
    /// draw would make the mosaic boil instead of resolve.
    #[test]
    fn a_cell_keeps_its_threshold() {
        assert_eq!(cell_hash01(4, 9, 11), cell_hash01(4, 9, 11));
        let t = cell_hash01(4, 9, 11);
        assert!(
            (0.0..1.0).contains(&t),
            "threshold must be a fraction, got {t}"
        );
    }

    /// …and two seeds must dissolve in a different order, or `seed` is a lie.
    #[test]
    fn the_seed_changes_the_order() {
        let a: Vec<f32> = (0..40).map(|i| cell_hash01(i, 0, 11)).collect();
        let b: Vec<f32> = (0..40).map(|i| cell_hash01(i, 0, 12)).collect();
        assert_ne!(a, b);
    }

    /// Neighbours must not turn in step — a threshold that tracks the
    /// coordinate sweeps a diagonal line, which is a wipe, not a dissolve.
    #[test]
    fn neighbouring_cells_turn_at_unrelated_times() {
        let close = (0..30)
            .flat_map(|c| (0..30).map(move |r| (c, r)))
            .filter(|&(c, r)| (cell_hash01(c, r, 11) - cell_hash01(c + 1, r, 11)).abs() < 0.05)
            .count();
        // 900 pairs; a swept threshold would put nearly all of them under 0.05.
        assert!(
            close < 200,
            "{close}/900 neighbours turn together — that is a wipe"
        );
    }

    /// The spread is the whole point: at half-way the frame must hold cells in
    /// *both* states plus some mid-fade, not one global opacity.
    #[test]
    fn midway_the_frame_holds_both_scenes_and_a_fading_band() {
        const FEATHER: f32 = 0.35;
        let p = 0.5 * (1.0 + FEATHER);
        let alphas: Vec<f32> = (0..40)
            .flat_map(|c| (0..40).map(move |r| cell_hash01(c, r, 11)))
            .map(|t| ((p - t) / FEATHER).clamp(0.0, 1.0))
            .collect();
        let done = alphas.iter().filter(|&&a| a >= 0.999).count();
        let waiting = alphas.iter().filter(|&&a| a <= 0.001).count();
        let fading = alphas.iter().filter(|&&a| a > 0.001 && a < 0.999).count();
        assert!(done > 100 && waiting > 100, "both states must be present");
        assert!(
            fading > 50,
            "cells must fade, not flip: only {fading} mid-transition"
        );
    }

    /// Every cell must be settled by the end, or the last of the outgoing scene
    /// survives into the next one.
    #[test]
    fn every_cell_completes_by_the_end() {
        const FEATHER: f32 = 0.35;
        let p = 1.0 * (1.0 + FEATHER);
        for c in 0..60 {
            for r in 0..60 {
                let a = ((p - cell_hash01(c, r, 11)) / FEATHER).clamp(0.0, 1.0);
                assert!(
                    a >= 0.999,
                    "cell ({c},{r}) still at {a} when the transition ends"
                );
            }
        }
    }
}
