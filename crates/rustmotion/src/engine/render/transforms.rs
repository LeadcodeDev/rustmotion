use skia_safe::Canvas;

use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::renderer::paint_from_hex;
use crate::schema::InnerShadow;

/// Draw an inner (inset) shadow inside an element.
///
/// Technique: clip to the element bounds, then draw a large inverted rect
/// with blur that bleeds inward from the edges.
pub(super) fn draw_inner_shadow(
    canvas: &Canvas,
    width: f32,
    height: f32,
    corner_radius: f32,
    shadow: &InnerShadow,
) {
    use skia_safe::{PaintStyle, Path, Rect, RRect, ClipOp};

    let bounds = Rect::from_xywh(0.0, 0.0, width, height);
    let rrect = RRect::new_rect_xy(bounds, corner_radius, corner_radius);

    canvas.save();
    // Clip to element bounds
    canvas.clip_rrect(rrect, ClipOp::Intersect, true);

    // Draw a large rect around the element with a hole in the center,
    // offset by the shadow offset, with blur applied.
    let expand = shadow.blur * 3.0 + 50.0;
    let outer = Rect::from_xywh(
        -expand + shadow.offset_x,
        -expand + shadow.offset_y,
        width + expand * 2.0,
        height + expand * 2.0,
    );
    let inner = Rect::from_xywh(shadow.offset_x, shadow.offset_y, width, height);
    let inner_rrect = RRect::new_rect_xy(inner, corner_radius, corner_radius);

    let mut path = Path::new();
    path.add_rect(outer, None);
    path.add_rrect(inner_rrect, None);
    path.set_fill_type(skia_safe::PathFillType::EvenOdd);

    let mut paint = paint_from_hex(&shadow.color);
    paint.set_style(PaintStyle::Fill);
    if shadow.blur > 0.0 {
        paint.set_mask_filter(skia_safe::MaskFilter::blur(
            skia_safe::BlurStyle::Normal,
            shadow.blur / 2.0,
            false,
        ));
    }

    canvas.draw_path(&path, &paint);
    canvas.restore();
}

/// Draw a 3D-adaptive ground-plane shadow.
///
/// When a component has 3D rotation (rotate_x/rotate_y), the shadow is drawn
/// on the "ground plane" (before 3D transforms) with offset/blur adapted to
/// the tilt angle. This creates a realistic shadow that grows and shifts
/// as the element tilts away from the viewer.
pub(super) fn draw_3d_shadow(
    canvas: &Canvas,
    width: f32,
    height: f32,
    corner_radius: f32,
    shadow: &crate::schema::CardShadow,
    props: &AnimatedProperties,
) {
    use skia_safe::{Rect, RRect};

    let rx = props.rotate_x * std::f32::consts::PI / 180.0;
    let ry = props.rotate_y * std::f32::consts::PI / 180.0;

    // Shadow offset shifts based on rotation angle (light from top-left)
    let depth_factor = (height * 0.4).min(80.0);
    let extra_offset_x = shadow.offset_x - ry.sin() * depth_factor;
    let extra_offset_y = shadow.offset_y + rx.sin() * depth_factor;

    // Shadow blur increases with tilt angle
    let tilt_magnitude = (rx.abs() + ry.abs()).min(1.0);
    let extra_blur = shadow.blur + tilt_magnitude * 20.0;

    // Shadow scales slightly based on perspective foreshortening
    let scale_x = 1.0 + ry.sin().abs() * 0.15;
    let scale_y = 1.0 + rx.sin().abs() * 0.15;
    let sw = width * scale_x;
    let sh = height * scale_y;
    let sx = extra_offset_x - (sw - width) / 2.0;
    let sy = extra_offset_y - (sh - height) / 2.0;

    let shadow_rect = Rect::from_xywh(sx, sy, sw, sh);
    let shadow_rrect = RRect::new_rect_xy(shadow_rect, corner_radius, corner_radius);

    let mut shadow_paint = paint_from_hex(&shadow.color);
    if extra_blur > 0.0 {
        shadow_paint.set_mask_filter(skia_safe::MaskFilter::blur(
            skia_safe::BlurStyle::Normal,
            extra_blur / 2.0,
            false,
        ));
    }
    canvas.draw_rrect(shadow_rrect, &shadow_paint);
}
