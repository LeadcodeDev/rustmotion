use crate::schema::TransitionType;

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
        TransitionType::ZoomIn => zoom(frame_a, frame_b, width, height, progress, true),
        TransitionType::ZoomOut => zoom(frame_a, frame_b, width, height, progress, false),
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
    frame_a
        .iter()
        .zip(frame_b.iter())
        .map(|(&a, &b)| {
            let va = a as f32 * (1.0 - progress);
            let vb = b as f32 * progress;
            (va + vb).clamp(0.0, 255.0) as u8
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
    let w = width as usize;
    let h = height as usize;
    let mut result = vec![0u8; w * h * 4];

    for row in 0..h {
        for col in 0..w {
            let idx = (row * w + col) * 4;

            let use_b = match direction {
                Direction::Left => (col as f32 / w as f32) < progress,
                Direction::Right => (col as f32 / w as f32) > (1.0 - progress),
                Direction::Up => (row as f32 / h as f32) < progress,
                Direction::Down => (row as f32 / h as f32) > (1.0 - progress),
            };

            let src = if use_b { frame_b } else { frame_a };
            result[idx..idx + 4].copy_from_slice(&src[idx..idx + 4]);
        }
    }

    result
}

fn zoom(
    frame_a: &[u8],
    frame_b: &[u8],
    _width: u32,
    _height: u32,
    progress: f32,
    _zoom_in: bool,
) -> Vec<u8> {
    // Simplified zoom: just crossfade with opacity
    // A full zoom would require scaling one of the frames, which needs Skia
    // For now, use a weighted fade that's heavier on the B side
    let adjusted = (progress * progress).clamp(0.0, 1.0);
    blend_fade(frame_a, frame_b, adjusted)
}
