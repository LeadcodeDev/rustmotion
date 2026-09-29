use crate::engine::animator::ease;
use crate::engine::renderer::{color4f_from_hex, paint_from_hex};
use crate::schema::{
    EasingType, IrisRing, IrisShape, MaskShape, PanBackground, PixelDissolveOrder, Transition,
    TransitionCorner, TransitionDirection, TransitionType, ZoomBlurOrigin,
};
use skia_safe::{
    surfaces, BlurStyle, Color4f, ColorType, Image, ImageInfo, MaskFilter, Matrix, Paint,
    PaintStyle, PathBuilder, Rect,
};

#[derive(Debug, Clone, PartialEq)]
pub struct TransitionOptions {
    pub corner: TransitionCorner,
    pub cell: f32,
    pub seed: u32,
    pub order: PixelDissolveOrder,
    pub direction: TransitionDirection,
    pub aberration: f32,
    pub strength: f32,
    pub origin: Option<ZoomBlurOrigin>,
    pub shape: IrisShape,
    pub aspect: f32,
    pub fill: Option<String>,
    pub hold: f32,
    pub ring: Option<IrisRing>,
    pub reverse: bool,
    pub silhouette: Option<MaskShape>,
    pub from_scale: f32,
    pub to_scale: f32,
    pub lobes: u32,
    pub wobble: f32,
    pub feather: f32,
    pub band_color: Option<String>,
    pub duration: f64,
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
            strength: 1.0,
            origin: None,
            shape: IrisShape::default(),
            aspect: 1.0,
            fill: None,
            hold: 0.0,
            ring: None,
            reverse: false,
            silhouette: None,
            from_scale: 0.0,
            to_scale: 20.0,
            lobes: 8,
            wobble: 0.15,
            feather: 0.0,
            band_color: None,
            duration: 0.5,
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
            strength: t.strength,
            origin: t.origin,
            shape: t.shape,
            aspect: t.aspect,
            fill: t.fill.clone(),
            hold: t.hold,
            ring: t.ring.clone(),
            reverse: t.reverse,
            silhouette: t.silhouette.clone(),
            from_scale: t.from_scale,
            to_scale: t.to_scale,
            lobes: t.lobes,
            wobble: t.wobble,
            feather: t.feather,
            band_color: t.band_color.clone(),
            duration: t.duration,
        }
    }
}

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
        strength,
        origin,
        shape,
        aspect,
        fill,
        hold,
        ring,
        reverse,
        silhouette,
        from_scale,
        to_scale,
        lobes,
        wobble,
        feather,
        band_color,
        duration,
    } = opts.clone();

    match transition_type {
        TransitionType::Fade => blend_fade(frame_a, frame_b, progress),
        TransitionType::WipeLeft => wipe(
            frame_a,
            frame_b,
            width,
            height,
            progress,
            Direction::Left,
            feather,
            band_color.as_deref(),
        ),
        TransitionType::WipeRight => wipe(
            frame_a,
            frame_b,
            width,
            height,
            progress,
            Direction::Right,
            feather,
            band_color.as_deref(),
        ),
        TransitionType::WipeUp => wipe(
            frame_a,
            frame_b,
            width,
            height,
            progress,
            Direction::Up,
            feather,
            band_color.as_deref(),
        ),
        TransitionType::WipeDown => wipe(
            frame_a,
            frame_b,
            width,
            height,
            progress,
            Direction::Down,
            feather,
            band_color.as_deref(),
        ),
        TransitionType::ZoomIn => zoom_transition(frame_a, frame_b, width, height, progress, true),
        TransitionType::ZoomOut => {
            zoom_transition(frame_a, frame_b, width, height, progress, false)
        }
        TransitionType::Flip => flip_transition(frame_a, frame_b, width, height, progress),
        TransitionType::ClockWipe => clock_wipe(frame_a, frame_b, width, height, progress),
        TransitionType::Iris => iris_transition(
            frame_a,
            frame_b,
            width,
            height,
            progress,
            origin,
            shape,
            aspect,
            fill.as_deref(),
            hold,
            duration,
            ring.as_ref(),
            reverse,
        ),
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
        TransitionType::ZoomBlur => {
            zoom_blur_transition(frame_a, frame_b, width, height, progress, strength, origin)
        }
        TransitionType::Whip => whip_transition(
            frame_a, frame_b, width, height, progress, strength, direction,
        ),
        TransitionType::Mask => mask_transition(
            frame_a,
            frame_b,
            width,
            height,
            progress,
            silhouette.as_ref(),
            origin,
            from_scale,
            to_scale,
            feather,
            band_color.as_deref(),
        ),
        TransitionType::Blob => blob_transition(
            frame_a,
            frame_b,
            width,
            height,
            progress,
            origin,
            lobes,
            wobble,
            seed,
            feather,
            band_color.as_deref(),
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

const SPATIAL_WEIGHT: f32 = 0.72;

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

fn wipe_reveal_rect(direction: &Direction, w: f32, h: f32, progress: f32) -> Rect {
    match direction {
        Direction::Left => Rect::from_xywh(0.0, 0.0, w * progress, h),
        Direction::Right => Rect::from_xywh(w * (1.0 - progress), 0.0, w * progress, h),
        Direction::Up => Rect::from_xywh(0.0, 0.0, w, h * progress),
        Direction::Down => Rect::from_xywh(0.0, h * (1.0 - progress), w, h * progress),
    }
}

const WIPE_FIXED_EDGE_MARGIN_FEATHER_FACTOR: f32 = 6.0;
const WIPE_FIXED_EDGE_MARGIN_FLOOR: f32 = 16.0;

fn wipe_fixed_edge_margin(feather: f32) -> f32 {
    feather.max(0.0) * WIPE_FIXED_EDGE_MARGIN_FEATHER_FACTOR + WIPE_FIXED_EDGE_MARGIN_FLOOR
}

fn wipe_feathered_reveal_rect(
    direction: &Direction,
    w: f32,
    h: f32,
    progress: f32,
    feather: f32,
) -> Rect {
    let margin = wipe_fixed_edge_margin(feather);
    match direction {
        Direction::Left => Rect::from_ltrb(-margin, -margin, w * progress, h + margin),
        Direction::Right => Rect::from_ltrb(w * (1.0 - progress), -margin, w + margin, h + margin),
        Direction::Up => Rect::from_ltrb(-margin, -margin, w + margin, h * progress),
        Direction::Down => Rect::from_ltrb(-margin, h * (1.0 - progress), w + margin, h + margin),
    }
}

fn wipe(
    frame_a: &[u8],
    frame_b: &[u8],
    width: u32,
    height: u32,
    progress: f32,
    direction: Direction,
    feather: f32,
    band_color: Option<&str>,
) -> Vec<u8> {
    if progress <= 0.0 {
        return frame_a.to_vec();
    }
    if progress >= 1.0 {
        return frame_b.to_vec();
    }

    let (w, h) = (width as f32, height as f32);
    let reveal = wipe_reveal_rect(&direction, w, h, progress);

    if feather <= 0.0 && band_color.is_none() {
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
        canvas.draw_image(&img_a, (0.0, 0.0), None);
        canvas.save();
        canvas.clip_rect(reveal, skia_safe::ClipOp::Intersect, true);
        canvas.draw_image(&img_b, (0.0, 0.0), None);
        canvas.restore();

        return surface_to_pixels(surface, width, height);
    }

    let feathered_reveal = wipe_feathered_reveal_rect(&direction, w, h, progress, feather);
    let mut builder = PathBuilder::new();
    builder.add_rect(feathered_reveal, None, None);
    let path = builder.detach();
    composite_through_mask(
        frame_a,
        frame_b,
        width,
        height,
        &path,
        feather.max(0.0),
        band_color,
    )
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

    if progress < 0.5 {
        let scale_x = 1.0 - progress * 2.0;
        canvas.clear(Color4f::new(0.0, 0.0, 0.0, 1.0));
        canvas.save();
        canvas.translate((w / 2.0, 0.0));
        canvas.scale((scale_x.max(0.01), 1.0));
        canvas.translate((-w / 2.0, 0.0));
        canvas.draw_image(&img_a, (0.0, 0.0), None);
        canvas.restore();
    } else {
        let scale_x = (progress - 0.5) * 2.0;
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

    canvas.draw_image(&img_a, (0.0, 0.0), None);

    let sweep_angle = progress * 360.0;
    let start_angle = -90.0;

    let mut path = PathBuilder::new();
    path.move_to((cx, cy));
    path.arc_to(
        Rect::from_xywh(cx - radius, cy - radius, radius * 2.0, radius * 2.0),
        start_angle,
        sweep_angle,
        false,
    );
    path.close();

    canvas.save();
    canvas.clip_path(&path.detach(), skia_safe::ClipOp::Intersect, true);
    canvas.draw_image(&img_b, (0.0, 0.0), None);
    canvas.restore();

    surface_to_pixels(surface, width, height)
}

const IRIS_PILL_OVERSHOOT: f32 = 1.45;
const IRIS_PILL_CORNER_FRACTION: f32 = 1.0;

fn iris_max_radius(
    origin: (f32, f32),
    width: f32,
    height: f32,
    shape: IrisShape,
    aspect: f32,
) -> f32 {
    let fx = origin.0.max(width - origin.0);
    let fy = origin.1.max(height - origin.1);
    match shape {
        IrisShape::Circle => (fx * fx + fy * fy).sqrt(),
        IrisShape::Pill => {
            let sqrt_aspect = aspect.max(0.05).sqrt();
            let base = (fx / sqrt_aspect).max(fy * sqrt_aspect);
            base * IRIS_PILL_OVERSHOOT
        }
    }
}

fn iris_mask_path(
    origin: (f32, f32),
    shape: IrisShape,
    aspect: f32,
    radius: f32,
) -> skia_safe::Path {
    let radius = radius.max(0.0);
    let mut builder = PathBuilder::new();
    match shape {
        IrisShape::Circle => {
            builder.add_circle((origin.0, origin.1), radius, None);
        }
        IrisShape::Pill => {
            let sqrt_aspect = aspect.max(0.05).sqrt();
            let half_w = radius * sqrt_aspect;
            let half_h = radius / sqrt_aspect;
            let corner = half_w.min(half_h) * IRIS_PILL_CORNER_FRACTION;
            let rect = Rect::from_ltrb(
                origin.0 - half_w,
                origin.1 - half_h,
                origin.0 + half_w,
                origin.1 + half_h,
            );
            let rrect = skia_safe::RRect::new_rect_xy(rect, corner, corner);
            builder.add_rrect(rrect, None, None);
        }
    }
    builder.detach()
}

fn solid_frame(width: u32, height: u32, hex: &str) -> Vec<u8> {
    let color = color4f_from_hex(hex);
    let (r, g, b, a) = (
        (color.r * 255.0).round() as u8,
        (color.g * 255.0).round() as u8,
        (color.b * 255.0).round() as u8,
        (color.a * 255.0).round() as u8,
    );
    (0..width * height).flat_map(|_| [r, g, b, a]).collect()
}

#[allow(clippy::too_many_arguments)]
fn iris_composite(
    outer: &[u8],
    inner: &[u8],
    width: u32,
    height: u32,
    origin: (f32, f32),
    shape: IrisShape,
    aspect: f32,
    radius: f32,
    ring: Option<&IrisRing>,
) -> Vec<u8> {
    let mut surface = match create_skia_surface(width, height) {
        Some(s) => s,
        None => return outer.to_vec(),
    };
    let (Some(img_outer), Some(img_inner)): (Option<Image>, Option<Image>) = (
        frame_to_image(outer, width, height),
        frame_to_image(inner, width, height),
    ) else {
        return outer.to_vec();
    };

    let path = iris_mask_path(origin, shape, aspect, radius);

    let canvas = surface.canvas();
    canvas.draw_image(&img_outer, (0.0, 0.0), None);
    canvas.save();
    canvas.clip_path(&path, skia_safe::ClipOp::Intersect, true);
    canvas.draw_image(&img_inner, (0.0, 0.0), None);
    canvas.restore();

    if let Some(ring) = ring {
        if radius > 1.0 {
            let mut paint = paint_from_hex(&ring.color);
            paint.set_style(PaintStyle::Stroke);
            paint.set_stroke_width(ring.width.max(0.0));
            canvas.draw_path(&path, &paint);
        }
    }

    surface_to_pixels(surface, width, height)
}

#[allow(clippy::too_many_arguments)]
fn iris_transition(
    frame_a: &[u8],
    frame_b: &[u8],
    width: u32,
    height: u32,
    progress: f32,
    origin: Option<ZoomBlurOrigin>,
    shape: IrisShape,
    aspect: f32,
    fill: Option<&str>,
    hold: f32,
    duration: f64,
    ring: Option<&IrisRing>,
    reverse: bool,
) -> Vec<u8> {
    let (w, h) = (width as f32, height as f32);
    let origin = match origin {
        Some(o) => (o.x, o.y),
        None => (w / 2.0, h / 2.0),
    };
    let max_radius = iris_max_radius(origin, w, h, shape, aspect);

    let Some(fill_hex) = fill else {
        let t = progress.clamp(0.0, 1.0);
        let (outer, inner, radius) = if reverse {
            (frame_b, frame_a, max_radius * (1.0 - t))
        } else {
            (frame_a, frame_b, max_radius * t)
        };
        return iris_composite(
            outer, inner, width, height, origin, shape, aspect, radius, ring,
        );
    };

    let hold_fraction = if duration > 0.0 {
        (hold as f64 / duration).clamp(0.0, 0.9) as f32
    } else {
        0.0
    };
    let remaining = (1.0 - hold_fraction).max(0.0001);
    let grow_span = remaining * 0.5;
    let reveal_start = grow_span + hold_fraction;
    let filled = solid_frame(width, height, fill_hex);

    if progress < grow_span {
        let t = (progress / grow_span).clamp(0.0, 1.0);
        let (outer, inner, radius): (&[u8], &[u8], f32) = if reverse {
            (&filled, frame_a, max_radius * (1.0 - t))
        } else {
            (frame_a, &filled, max_radius * t)
        };
        return iris_composite(
            outer, inner, width, height, origin, shape, aspect, radius, ring,
        );
    }

    if progress < reveal_start {
        return filled;
    }

    let reveal_span = (1.0 - reveal_start).max(0.0001);
    let t = ((progress - reveal_start) / reveal_span).clamp(0.0, 1.0);
    blend_fade(&filled, frame_b, t)
}

pub fn mask_shape_to_local_path(shape: &MaskShape) -> Option<skia_safe::Path> {
    match shape {
        MaskShape::Polygon { points } => {
            if points.len() < 3 {
                return None;
            }
            let mut builder = PathBuilder::new();
            for (i, (x, y)) in points.iter().enumerate() {
                if i == 0 {
                    builder.move_to((*x, *y));
                } else {
                    builder.line_to((*x, *y));
                }
            }
            builder.close();
            Some(builder.detach())
        }
        MaskShape::Path { d } => skia_safe::Path::from_svg(d),
    }
}

pub(crate) fn scaled_mask_path(
    local: &skia_safe::Path,
    scale: f32,
    origin: (f32, f32),
) -> skia_safe::Path {
    let bounds = *local.bounds();
    let cx = (bounds.left + bounds.right) / 2.0;
    let cy = (bounds.top + bounds.bottom) / 2.0;
    let safe_scale = scale.max(0.0001);
    let mut matrix = Matrix::default();
    matrix.pre_translate((origin.0, origin.1));
    matrix.pre_scale((safe_scale, safe_scale), None);
    matrix.pre_translate((-cx, -cy));
    local.with_transform(&matrix)
}

fn mask_alpha_buffer(
    path: &skia_safe::Path,
    width: u32,
    height: u32,
    feather: f32,
) -> Option<Vec<u8>> {
    let info = ImageInfo::new(
        (width as i32, height as i32),
        ColorType::Alpha8,
        skia_safe::AlphaType::Premul,
        None,
    );
    let mut surface = surfaces::raster(&info, None, None)?;
    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    paint.set_style(PaintStyle::Fill);
    paint.set_alpha(255);
    let sigma = (feather.max(0.0) / 3.0).max(0.05);
    if let Some(mask_filter) = MaskFilter::blur(BlurStyle::Normal, sigma, None) {
        paint.set_mask_filter(mask_filter);
    }
    surface.canvas().draw_path(path, &paint);
    let row_bytes = width as usize;
    let mut buf = vec![0u8; row_bytes * height as usize];
    surface.read_pixels(&info, &mut buf, row_bytes, (0, 0));
    Some(buf)
}

fn hard_mask_composite(
    outer: &[u8],
    inner: &[u8],
    width: u32,
    height: u32,
    path: &skia_safe::Path,
) -> Vec<u8> {
    let mut surface = match create_skia_surface(width, height) {
        Some(s) => s,
        None => return outer.to_vec(),
    };
    let (Some(img_outer), Some(img_inner)) = (
        frame_to_image(outer, width, height),
        frame_to_image(inner, width, height),
    ) else {
        return outer.to_vec();
    };

    let canvas = surface.canvas();
    canvas.draw_image(&img_outer, (0.0, 0.0), None);
    canvas.save();
    canvas.clip_path(path, skia_safe::ClipOp::Intersect, true);
    canvas.draw_image(&img_inner, (0.0, 0.0), None);
    canvas.restore();

    surface_to_pixels(surface, width, height)
}

fn composite_through_mask(
    outer: &[u8],
    inner: &[u8],
    width: u32,
    height: u32,
    path: &skia_safe::Path,
    feather: f32,
    band_color: Option<&str>,
) -> Vec<u8> {
    if feather <= 0.0 {
        return hard_mask_composite(outer, inner, width, height, path);
    }
    let Some(alpha) = mask_alpha_buffer(path, width, height, feather) else {
        return hard_mask_composite(outer, inner, width, height, path);
    };

    let band = band_color.map(color4f_from_hex);
    let pixel_count = (width * height) as usize;
    let mut out = vec![0u8; pixel_count * 4];
    for (i, &alpha_byte) in alpha.iter().enumerate().take(pixel_count) {
        let a = alpha_byte as f32 / 255.0;
        let base = i * 4;
        let mut rgb = [0f32; 3];
        for (c, slot) in rgb.iter_mut().enumerate() {
            let o = outer[base + c] as f32;
            let n = inner[base + c] as f32;
            *slot = o * (1.0 - a) + n * a;
        }
        if let Some(band) = &band {
            let weight = 4.0 * a * (1.0 - a);
            let band_rgb = [band.r * 255.0, band.g * 255.0, band.b * 255.0];
            for (slot, band_channel) in rgb.iter_mut().zip(band_rgb) {
                *slot = *slot * (1.0 - weight) + band_channel * weight;
            }
        }
        for (c, value) in rgb.into_iter().enumerate() {
            out[base + c] = value.round().clamp(0.0, 255.0) as u8;
        }
        out[base + 3] = outer[base + 3].max(inner[base + 3]);
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn mask_transition(
    frame_a: &[u8],
    frame_b: &[u8],
    width: u32,
    height: u32,
    progress: f32,
    silhouette: Option<&MaskShape>,
    origin: Option<ZoomBlurOrigin>,
    from_scale: f32,
    to_scale: f32,
    feather: f32,
    band_color: Option<&str>,
) -> Vec<u8> {
    if progress <= 0.0 {
        return frame_a.to_vec();
    }
    if progress >= 1.0 {
        return frame_b.to_vec();
    }
    let Some(shape) = silhouette else {
        eprintln!(
            "rustmotion: transition type \"mask\" needs a `silhouette` (polygon or path); \
             falling back to a plain fade"
        );
        return blend_fade(frame_a, frame_b, progress);
    };
    let Some(local) = mask_shape_to_local_path(shape) else {
        eprintln!(
            "rustmotion: transition \"mask\" `silhouette` did not resolve to a path (a \
             `polygon` needs at least 3 points, a `path`'s `d` must be valid SVG path data); \
             falling back to a plain fade"
        );
        return blend_fade(frame_a, frame_b, progress);
    };

    let (w, h) = (width as f32, height as f32);
    let origin_px = match origin {
        Some(o) => (o.x, o.y),
        None => (w / 2.0, h / 2.0),
    };
    let scale = from_scale.max(0.0) + (to_scale.max(0.0) - from_scale.max(0.0)) * progress;
    let path = scaled_mask_path(&local, scale, origin_px);
    composite_through_mask(
        frame_a,
        frame_b,
        width,
        height,
        &path,
        feather.max(0.0),
        band_color,
    )
}

pub(crate) fn blob_local_path(lobes: u32, wobble: f32, seed: u32) -> skia_safe::Path {
    let n = lobes.max(3);
    let wobble = wobble.clamp(0.0, 0.95);
    let points: Vec<(f32, f32)> = (0..n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let angle = t * std::f32::consts::TAU;
            let noise = cell_hash01(i as i32, 0, seed) * 2.0 - 1.0;
            let radius = 1.0 + wobble * noise;
            (angle.cos() * radius, angle.sin() * radius)
        })
        .collect();

    let midpoint = |a: (f32, f32), b: (f32, f32)| ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
    let n = n as usize;
    let mut builder = PathBuilder::new();
    builder.move_to(midpoint(points[n - 1], points[0]));
    for i in 0..n {
        let next = points[(i + 1) % n];
        builder.quad_to(points[i], midpoint(points[i], next));
    }
    builder.close();
    builder.detach()
}

#[allow(clippy::too_many_arguments)]
fn blob_transition(
    frame_a: &[u8],
    frame_b: &[u8],
    width: u32,
    height: u32,
    progress: f32,
    origin: Option<ZoomBlurOrigin>,
    lobes: u32,
    wobble: f32,
    seed: u32,
    feather: f32,
    band_color: Option<&str>,
) -> Vec<u8> {
    if progress <= 0.0 {
        return frame_a.to_vec();
    }
    if progress >= 1.0 {
        return frame_b.to_vec();
    }

    let (w, h) = (width as f32, height as f32);
    let origin_px = match origin {
        Some(o) => (o.x, o.y),
        None => (w / 2.0, h / 2.0),
    };
    let coverage_radius = iris_max_radius(origin_px, w, h, IrisShape::Circle, 1.0);
    let wobble = wobble.clamp(0.0, 0.95);
    let shrunk_lobe_safety = (1.0 - wobble).max(0.05);
    let max_radius = coverage_radius / shrunk_lobe_safety;
    let local = blob_local_path(lobes, wobble, seed);
    let scale = max_radius * progress;
    let path = scaled_mask_path(&local, scale, origin_px);
    composite_through_mask(
        frame_a,
        frame_b,
        width,
        height,
        &path,
        feather.max(0.0),
        band_color,
    )
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

    let offset = -progress * w;
    canvas.draw_image(&img_a, (offset, 0.0), None);
    canvas.draw_image(&img_b, (offset + w, 0.0), None);

    surface_to_pixels(surface, width, height)
}

fn direction_vector(direction: TransitionDirection) -> (f32, f32) {
    match direction {
        TransitionDirection::Left => (-1.0, 0.0),
        TransitionDirection::Right => (1.0, 0.0),
        TransitionDirection::Up => (0.0, -1.0),
        TransitionDirection::Down => (0.0, 1.0),
    }
}

fn directional_slide(
    frame_a: &[u8],
    frame_b: &[u8],
    width: u32,
    height: u32,
    progress: f32,
    ux: f32,
    uy: f32,
) -> Option<Vec<u8>> {
    let mut surface = create_skia_surface(width, height)?;
    let (Some(img_a), Some(img_b)) = (
        frame_to_image(frame_a, width, height),
        frame_to_image(frame_b, width, height),
    ) else {
        return None;
    };
    let (w, h) = (width as f32, height as f32);
    let canvas = surface.canvas();
    let (dx, dy) = (ux * progress * w, uy * progress * h);
    canvas.draw_image(&img_a, (dx, dy), None);
    canvas.draw_image(&img_b, (dx - ux * w, dy - uy * h), None);
    Some(surface_to_pixels(surface, width, height))
}

fn chromatic_wipe(
    frame_a: &[u8],
    frame_b: &[u8],
    width: u32,
    height: u32,
    progress: f32,
    direction: TransitionDirection,
    aberration: f32,
) -> Vec<u8> {
    let w = width as f32;
    let (ux, uy) = direction_vector(direction);

    let Some(slid) = directional_slide(frame_a, frame_b, width, height, progress, ux, uy) else {
        return blend_fade(frame_a, frame_b, progress);
    };

    let peak = 1.0 - (progress * 2.0 - 1.0).abs();
    let shift = (aberration.max(0.0) * peak * w * 0.012).round() as i32;
    if shift == 0 {
        return slid;
    }

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

const ZOOM_BLUR_ZOOM_REACH: f32 = 0.5;
const ZOOM_BLUR_STEPS: usize = 10;
const ZOOM_BLUR_MAX_EXTRA_SCALE: f32 = 0.6;

fn zoom_blur_transition(
    frame_a: &[u8],
    frame_b: &[u8],
    width: u32,
    height: u32,
    progress: f32,
    strength: f32,
    origin: Option<ZoomBlurOrigin>,
) -> Vec<u8> {
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

    let (w, h) = (width as f32, height as f32);
    let (ox, oy) = match origin {
        Some(o) => (o.x, o.y),
        None => (w / 2.0, h / 2.0),
    };

    let scale_now = 1.0 + progress * ZOOM_BLUR_ZOOM_REACH;
    let alpha_a = 1.0 - progress;

    {
        let canvas = surface.canvas();
        canvas.draw_image(&img_b, (0.0, 0.0), None);
        canvas.save();
        canvas.translate((ox, oy));
        canvas.scale((scale_now, scale_now));
        canvas.translate((-ox, -oy));
        let mut paint = Paint::default();
        paint.set_alpha_f(alpha_a);
        canvas.draw_image(&img_a, (0.0, 0.0), Some(&paint));
        canvas.restore();
    }
    let sharp = surface_to_pixels(surface, width, height);

    let peak = 1.0 - (progress * 2.0 - 1.0).abs();
    let reach = strength.max(0.0) * peak;
    if reach <= 0.0 {
        return sharp;
    }

    let mut streak_surface = match create_skia_surface(width, height) {
        Some(s) => s,
        None => return sharp,
    };
    let extra = reach * ZOOM_BLUR_MAX_EXTRA_SCALE;
    let canvas = streak_surface.canvas();
    canvas.draw_image(&img_b, (0.0, 0.0), None);
    for i in (0..ZOOM_BLUR_STEPS).rev() {
        let t = i as f32 / (ZOOM_BLUR_STEPS - 1) as f32;
        let s = scale_now + extra * t;
        let weight = (1.0 - t).powf(1.5);
        let mut streak_paint = Paint::default();
        streak_paint.set_alpha_f((alpha_a * weight).clamp(0.0, 1.0));
        canvas.save();
        canvas.translate((ox, oy));
        canvas.scale((s, s));
        canvas.translate((-ox, -oy));
        canvas.draw_image(&img_a, (0.0, 0.0), Some(&streak_paint));
        canvas.restore();
    }

    surface_to_pixels(streak_surface, width, height)
}

const WHIP_MAX_REACH: f32 = 0.5;
const WHIP_STEPS: usize = 120;

fn whip_transition(
    frame_a: &[u8],
    frame_b: &[u8],
    width: u32,
    height: u32,
    progress: f32,
    strength: f32,
    direction: TransitionDirection,
) -> Vec<u8> {
    let (ux, uy) = direction_vector(direction);
    let Some(sharp) = directional_slide(frame_a, frame_b, width, height, progress, ux, uy) else {
        return blend_fade(frame_a, frame_b, progress);
    };

    let peak = 1.0 - (progress * 2.0 - 1.0).abs();
    let reach = strength.max(0.0) * peak;
    if reach <= 0.0 {
        return sharp;
    }

    let (Some(img_a), Some(img_b)) = (
        frame_to_image(frame_a, width, height),
        frame_to_image(frame_b, width, height),
    ) else {
        return sharp;
    };
    let Some(img_sharp) = frame_to_image(&sharp, width, height) else {
        return sharp;
    };
    let mut surface = match create_skia_surface(width, height) {
        Some(s) => s,
        None => return sharp,
    };

    let (w, h) = (width as f32, height as f32);
    let axis_len = if uy == 0.0 { w } else { h };
    let (dx_a, dy_a) = (ux * progress * w, uy * progress * h);
    let (dx_b, dy_b) = (dx_a - ux * w, dy_a - uy * h);
    let reach_px = reach * axis_len * WHIP_MAX_REACH;

    let alpha_a = (1.0 - progress).clamp(0.0, 1.0);
    let alpha_b = progress.clamp(0.0, 1.0);

    let canvas = surface.canvas();
    canvas.draw_image(&img_sharp, (0.0, 0.0), None);

    for i in (0..WHIP_STEPS).rev() {
        let t = i as f32 / (WHIP_STEPS - 1) as f32;
        let trail = reach_px * t;
        let weight = (1.0 - t).powf(1.5);

        let mut paint_a = Paint::default();
        paint_a.set_alpha_f((alpha_a * weight).clamp(0.0, 1.0));
        canvas.draw_image(
            &img_a,
            (dx_a - ux * trail, dy_a - uy * trail),
            Some(&paint_a),
        );

        let mut paint_b = Paint::default();
        paint_b.set_alpha_f((alpha_b * weight).clamp(0.0, 1.0));
        canvas.draw_image(
            &img_b,
            (dx_b - ux * trail, dy_b - uy * trail),
            Some(&paint_b),
        );
    }

    surface_to_pixels(surface, width, height)
}

fn dissolve_transition(
    frame_a: &[u8],
    frame_b: &[u8],
    _width: u32,
    _height: u32,
    progress: f32,
) -> Vec<u8> {
    blend_fade(frame_a, frame_b, progress)
}

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

    let (out_x, out_y) = (-dx * t, -dy * t);
    let (in_x, in_y) = (dx * (1.0 - t), dy * (1.0 - t));

    let blended_bg = match pan_background {
        PanBackground::Travel => {
            let img_bg_a = match frame_to_image(bg_a, width, height) {
                Some(i) => i,
                None => return bg_a.to_vec(),
            };
            let img_bg_b = match frame_to_image(bg_b, width, height) {
                Some(i) => i,
                None => return bg_a.to_vec(),
            };

            const BG_PARALLAX: f32 = 0.12;
            let (bax, bay) = (out_x * BG_PARALLAX, out_y * BG_PARALLAX);
            let (bbx, bby) = (in_x * BG_PARALLAX, in_y * BG_PARALLAX);

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
        PanBackground::Static => blend_fade(bg_a, bg_b, t),
    };
    let img_bg = match frame_to_image(&blended_bg, width, height) {
        Some(i) => i,
        None => return bg_a.to_vec(),
    };

    let canvas = surface.canvas();
    canvas.draw_image(&img_bg, (0.0, 0.0), None);

    const FG_DISSOLVE: f32 = 1.6;
    let mut fg_paint = Paint::default();

    fg_paint.set_alpha_f(1.0 - t.powf(FG_DISSOLVE));
    canvas.draw_image(&img_fg_a, (out_x, out_y), Some(&fg_paint));

    fg_paint.set_alpha_f(1.0 - (1.0 - t).powf(FG_DISSOLVE));
    canvas.draw_image(&img_fg_b, (in_x, in_y), Some(&fg_paint));

    surface_to_pixels(surface, width, height)
}

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

    fn transparent(width: u32, height: u32) -> Vec<u8> {
        solid(width, height, 0, 0, 0, 0)
    }

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

        for px in out.as_chunks::<4>().0.iter() {
            assert_eq!(
                *px,
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

    #[test]
    fn the_anchored_edges_never_move() {
        for p in [0.05, 0.3, 0.5, 0.8, 1.0] {
            let r = corner_rect(TransitionCorner::TopRight, 1920.0, 1080.0, p);
            assert!((r.right - 1920.0).abs() < 1e-3, "right edge moved at {p}");
            assert!(r.top.abs() < 1e-3, "top edge moved at {p}");
        }
    }

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

    #[test]
    fn center_out_is_the_mirror_of_edges_in() {
        let (cols, rows) = (40, 24);
        for (c, r) in [(0, 0), (20, 12), (39, 5)] {
            let a = cell_threshold(c, r, cols, rows, 11, PixelDissolveOrder::EdgesIn);
            let b = cell_threshold(c, r, cols, rows, 11, PixelDissolveOrder::CenterOut);
            assert!(
                (a + b - (SPATIAL_WEIGHT + 2.0 * (1.0 - SPATIAL_WEIGHT) * cell_hash01(c, r, 11)))
                    .abs()
                    < 1e-5
            );
        }
    }

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

    #[test]
    fn random_ignores_position() {
        let t = cell_threshold(7, 3, 40, 24, 11, PixelDissolveOrder::Random);
        assert_eq!(t, cell_hash01(7, 3, 11));
    }

    #[test]
    fn a_cell_keeps_its_threshold() {
        assert_eq!(cell_hash01(4, 9, 11), cell_hash01(4, 9, 11));
        let t = cell_hash01(4, 9, 11);
        assert!(
            (0.0..1.0).contains(&t),
            "threshold must be a fraction, got {t}"
        );
    }

    #[test]
    fn the_seed_changes_the_order() {
        let a: Vec<f32> = (0..40).map(|i| cell_hash01(i, 0, 11)).collect();
        let b: Vec<f32> = (0..40).map(|i| cell_hash01(i, 0, 12)).collect();
        assert_ne!(a, b);
    }

    #[test]
    fn neighbouring_cells_turn_at_unrelated_times() {
        let close = (0..30)
            .flat_map(|c| (0..30).map(move |r| (c, r)))
            .filter(|&(c, r)| (cell_hash01(c, r, 11) - cell_hash01(c + 1, r, 11)).abs() < 0.05)
            .count();
        assert!(
            close < 200,
            "{close}/900 neighbours turn together — that is a wipe"
        );
    }

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

#[cfg(test)]
mod wipe_fixed_edge_tests {
    use super::*;

    const W: u32 = 1280;
    const H: u32 = 720;

    fn frames() -> (Vec<u8>, Vec<u8>) {
        let a: Vec<u8> = (0..W * H).flat_map(|_| [200u8, 200, 200, 255]).collect();
        let b: Vec<u8> = (0..W * H).flat_map(|_| [40u8, 40, 40, 255]).collect();
        (a, b)
    }

    fn pixel(buf: &[u8], x: u32, y: u32) -> [u8; 4] {
        let base = ((y * W + x) * 4) as usize;
        [buf[base], buf[base + 1], buf[base + 2], buf[base + 3]]
    }

    #[test]
    fn a_wide_feather_and_band_color_do_not_leak_onto_the_three_fixed_edges_of_a_left_wipe() {
        let (a, b) = frames();
        let opts = TransitionOptions {
            feather: 320.0,
            band_color: Some("#FF8CC6".to_string()),
            ..TransitionOptions::default()
        };
        let out = apply_transition(&a, &b, W, H, 0.8, &TransitionType::WipeLeft, &opts);

        assert_eq!(
            pixel(&out, 640, 3),
            [40, 40, 40, 255],
            "the top edge never moves for a left wipe — a pixel well inside the revealed area \
             but close to y=0 must be the exact incoming colour, not softened or tinted pink \
             just because it happens to sit near the frame's own fixed top border"
        );
        assert_eq!(
            pixel(&out, 640, H - 4),
            [40, 40, 40, 255],
            "the bottom edge never moves for a left wipe either — same leak, opposite border"
        );
        assert_eq!(
            pixel(&out, 2, 360),
            [40, 40, 40, 255],
            "the left edge is the wipe's own starting edge, always flush with the frame's left \
             border — it must never show a blended or tinted pixel, only the moving right edge \
             may"
        );
    }

    #[test]
    fn the_moving_edge_still_feathers_normally_once_the_fixed_edges_are_excluded() {
        let (a, b) = frames();
        let opts = TransitionOptions {
            feather: 320.0,
            ..TransitionOptions::default()
        };
        let out = apply_transition(&a, &b, W, H, 0.8, &TransitionType::WipeLeft, &opts);
        let moving_edge_x = (W as f32 * 0.8) as u32;
        let blended = pixel(&out, moving_edge_x, 360);
        assert!(
            blended[0] != 200 && blended[0] != 40,
            "the wipe's own moving reveal boundary must still show a blended pixel from a wide \
             feather, got {blended:?} — the fixed-edge fix must not have flattened the real \
             moving edge along with the fake ones"
        );
    }
}

#[cfg(test)]
mod iris_pill_shape_tests {
    use super::*;

    #[test]
    fn a_pill_with_aspect_one_degenerates_into_a_true_circle_not_a_barely_rounded_square() {
        let origin = (100.0, 100.0);
        let radius = 50.0;
        let path = iris_mask_path(origin, IrisShape::Pill, 1.0, radius);
        let beyond_the_radius_on_the_diagonal = radius * 1.2 / std::f32::consts::SQRT_2;
        let corner_point = (
            origin.0 + beyond_the_radius_on_the_diagonal,
            origin.1 + beyond_the_radius_on_the_diagonal,
        );
        assert!(
            !path.contains(corner_point),
            "a pill with aspect 1.0 must degenerate into a circle — a point just beyond the \
             radius on the diagonal must fall outside it; the old 0.2 corner fraction left a \
             barely-rounded square whose corner reached out to radius*sqrt(2), well past this \
             point"
        );
    }
}

#[cfg(test)]
mod whip_continuity_tests {
    use super::*;

    fn solid_rgb(width: u32, height: u32, r: u8, g: u8, b: u8) -> Vec<u8> {
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

    #[test]
    fn a_wide_streak_has_no_large_jump_between_adjacent_pixels() {
        const W: u32 = 640;
        const H: u32 = 48;
        let a = off_centre_stripe(W, H);
        let b = solid_rgb(W, H, 40, 40, 40);
        let out = whip_transition(&a, &b, W, H, 0.5, 6.0, TransitionDirection::Left);

        let row = H / 2;
        let value_at = |x: u32| -> i32 {
            let i = ((row * W + x) * 4) as usize;
            out[i] as i32
        };

        let shift = (0.5 * W as f32) as u32;
        let sharp_edges = [W * 5 / 8 - shift, W * 7 / 8 - shift];
        let excluded = |x: u32| sharp_edges.iter().any(|&e| x.abs_diff(e) < 20);

        let mut max_jump = 0i32;
        let mut jump_at = 0u32;
        for x in 0..W - 1 {
            if excluded(x) || excluded(x + 1) {
                continue;
            }
            let jump = (value_at(x + 1) - value_at(x)).abs();
            if jump > max_jump {
                max_jump = jump;
                jump_at = x;
            }
        }

        assert!(
            max_jump < 40,
            "the streak must fade continuously across the trail, not in visible discrete \
             bands stepping between a handful of stamped copies — the biggest single-pixel \
             jump found was {max_jump} at x={jump_at} (row {row})"
        );
    }
}
