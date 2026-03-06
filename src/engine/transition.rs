use crate::schema::TransitionType;
use skia_safe::{
    surfaces, Color4f, ColorType, ImageInfo, Paint, Path, Rect,
};

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
        TransitionType::WipeLeft => wipe(frame_a, frame_b, width, height, progress, Direction::Left),
        TransitionType::WipeRight => wipe(frame_a, frame_b, width, height, progress, Direction::Right),
        TransitionType::WipeUp => wipe(frame_a, frame_b, width, height, progress, Direction::Up),
        TransitionType::WipeDown => wipe(frame_a, frame_b, width, height, progress, Direction::Down),
        TransitionType::ZoomIn => zoom_transition(frame_a, frame_b, width, height, progress, true),
        TransitionType::ZoomOut => zoom_transition(frame_a, frame_b, width, height, progress, false),
        TransitionType::Flip => flip_transition(frame_a, frame_b, width, height, progress),
        TransitionType::ClockWipe => clock_wipe(frame_a, frame_b, width, height, progress),
        TransitionType::Iris => iris_transition(frame_a, frame_b, width, height, progress),
        TransitionType::Slide => slide_transition(frame_a, frame_b, width, height, progress),
        TransitionType::Dissolve => dissolve_transition(frame_a, frame_b, width, height, progress),
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

fn zoom_transition(frame_a: &[u8], frame_b: &[u8], width: u32, height: u32, progress: f32, zoom_in: bool) -> Vec<u8> {
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

fn flip_transition(frame_a: &[u8], frame_b: &[u8], width: u32, height: u32, progress: f32) -> Vec<u8> {
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

fn iris_transition(frame_a: &[u8], frame_b: &[u8], width: u32, height: u32, progress: f32) -> Vec<u8> {
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

fn slide_transition(frame_a: &[u8], frame_b: &[u8], width: u32, height: u32, progress: f32) -> Vec<u8> {
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

fn dissolve_transition(frame_a: &[u8], frame_b: &[u8], _width: u32, _height: u32, progress: f32) -> Vec<u8> {
    // Dissolve is a smooth cross-dissolve (same as fade in standard video editing)
    blend_fade(frame_a, frame_b, progress)
}
