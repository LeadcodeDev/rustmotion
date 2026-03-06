use anyhow::Result;
use skia_safe::{Canvas, PaintStyle, Rect, RRect};

use crate::engine::renderer::{color4f_from_hex, paint_from_hex};
use crate::schema::ProgressBarLayer;

pub fn render_progress_bar(canvas: &Canvas, layer: &ProgressBarLayer) -> Result<()> {
    let x = layer.position.x;
    let y = layer.position.y;
    let w = layer.width;
    let h = layer.height;
    let radius = layer.border_radius;
    let progress = layer.progress.clamp(0.0, 1.0) as f32;

    // Background
    let mut bg_paint = skia_safe::Paint::new(color4f_from_hex(&layer.background_color), None);
    bg_paint.set_style(PaintStyle::Fill);
    bg_paint.set_anti_alias(true);

    let bg_rect = Rect::from_xywh(x, y, w, h);
    let bg_rrect = RRect::new_rect_xy(bg_rect, radius, radius);
    canvas.draw_rrect(bg_rrect, &bg_paint);

    // Fill (progress)
    if progress > 0.001 {
        let mut fill_paint = paint_from_hex(&layer.fill_color);
        fill_paint.set_style(PaintStyle::Fill);
        fill_paint.set_anti_alias(true);

        let fill_w = w * progress;
        let fill_rect = Rect::from_xywh(x, y, fill_w, h);

        // Clip to the rounded background shape to prevent fill from exceeding bounds
        canvas.save();
        canvas.clip_rrect(bg_rrect, skia_safe::ClipOp::Intersect, true);
        canvas.draw_rect(fill_rect, &fill_paint);
        canvas.restore();
    }

    Ok(())
}
