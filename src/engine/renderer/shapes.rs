use skia_safe::{Canvas, Paint, Rect};

use crate::schema::ShapeType;

/// Build a `Path` for the given shape type without drawing it.
pub(crate) fn build_shape_path(shape_type: &ShapeType, x: f32, y: f32, w: f32, h: f32, corner_radius: Option<f32>) -> Option<skia_safe::Path> {
    match shape_type {
        ShapeType::Rect => {
            let mut path = skia_safe::Path::new();
            path.add_rect(Rect::from_xywh(x, y, w, h), None);
            Some(path)
        }
        ShapeType::RoundedRect => {
            let r = corner_radius.unwrap_or(8.0);
            let rrect = skia_safe::RRect::new_rect_xy(Rect::from_xywh(x, y, w, h), r, r);
            let mut path = skia_safe::Path::new();
            path.add_rrect(rrect, None);
            Some(path)
        }
        ShapeType::Circle => {
            let radius = w.min(h) / 2.0;
            let mut path = skia_safe::Path::new();
            path.add_circle((x + w / 2.0, y + h / 2.0), radius, None);
            Some(path)
        }
        ShapeType::Ellipse => {
            let mut path = skia_safe::Path::new();
            path.add_oval(Rect::from_xywh(x, y, w, h), None);
            Some(path)
        }
        ShapeType::Triangle => {
            let mut path = skia_safe::Path::new();
            path.move_to((x + w / 2.0, y));
            path.line_to((x + w, y + h));
            path.line_to((x, y + h));
            path.close();
            Some(path)
        }
        ShapeType::Star { points } => {
            let cx = x + w / 2.0;
            let cy = y + h / 2.0;
            let outer_r = w.min(h) / 2.0;
            let inner_r = outer_r * 0.4;
            let n = *points as usize;
            let mut path = skia_safe::Path::new();
            for i in 0..(n * 2) {
                let angle = (i as f32) * std::f32::consts::PI / n as f32 - std::f32::consts::FRAC_PI_2;
                let r = if i % 2 == 0 { outer_r } else { inner_r };
                let px = cx + r * angle.cos();
                let py = cy + r * angle.sin();
                if i == 0 { path.move_to((px, py)); } else { path.line_to((px, py)); }
            }
            path.close();
            Some(path)
        }
        ShapeType::Polygon { sides } => {
            let cx = x + w / 2.0;
            let cy = y + h / 2.0;
            let r = w.min(h) / 2.0;
            let n = *sides as usize;
            let mut path = skia_safe::Path::new();
            for i in 0..n {
                let angle = (i as f32) * 2.0 * std::f32::consts::PI / n as f32 - std::f32::consts::FRAC_PI_2;
                let px = cx + r * angle.cos();
                let py = cy + r * angle.sin();
                if i == 0 { path.move_to((px, py)); } else { path.line_to((px, py)); }
            }
            path.close();
            Some(path)
        }
        ShapeType::Path { data } => {
            skia_safe::Path::from_svg(data)
        }
    }
}

pub(crate) fn draw_shape_path(canvas: &Canvas, shape_type: &ShapeType, x: f32, y: f32, w: f32, h: f32, corner_radius: Option<f32>, paint: &Paint) {
    let rect = Rect::from_xywh(x, y, w, h);
    match shape_type {
        ShapeType::Rect => { canvas.draw_rect(rect, paint); }
        ShapeType::RoundedRect => {
            let r = corner_radius.unwrap_or(8.0);
            let rrect = skia_safe::RRect::new_rect_xy(rect, r, r);
            canvas.draw_rrect(rrect, paint);
        }
        ShapeType::Circle => {
            let radius = w.min(h) / 2.0;
            canvas.draw_circle((x + w / 2.0, y + h / 2.0), radius, paint);
        }
        ShapeType::Ellipse => { canvas.draw_oval(rect, paint); }
        ShapeType::Triangle => {
            let mut path = skia_safe::Path::new();
            path.move_to((x + w / 2.0, y));
            path.line_to((x + w, y + h));
            path.line_to((x, y + h));
            path.close();
            canvas.draw_path(&path, paint);
        }
        ShapeType::Star { points } => {
            let cx = x + w / 2.0;
            let cy = y + h / 2.0;
            let outer_r = w.min(h) / 2.0;
            let inner_r = outer_r * 0.4;
            let n = *points as usize;
            let mut path = skia_safe::Path::new();
            for i in 0..(n * 2) {
                let angle = (i as f32) * std::f32::consts::PI / n as f32 - std::f32::consts::FRAC_PI_2;
                let r = if i % 2 == 0 { outer_r } else { inner_r };
                let px = cx + r * angle.cos();
                let py = cy + r * angle.sin();
                if i == 0 {
                    path.move_to((px, py));
                } else {
                    path.line_to((px, py));
                }
            }
            path.close();
            canvas.draw_path(&path, paint);
        }
        ShapeType::Polygon { sides } => {
            let cx = x + w / 2.0;
            let cy = y + h / 2.0;
            let r = w.min(h) / 2.0;
            let n = *sides as usize;
            let mut path = skia_safe::Path::new();
            for i in 0..n {
                let angle = (i as f32) * 2.0 * std::f32::consts::PI / n as f32 - std::f32::consts::FRAC_PI_2;
                let px = cx + r * angle.cos();
                let py = cy + r * angle.sin();
                if i == 0 {
                    path.move_to((px, py));
                } else {
                    path.line_to((px, py));
                }
            }
            path.close();
            canvas.draw_path(&path, paint);
        }
        ShapeType::Path { data } => {
            if let Some(path) = skia_safe::Path::from_svg(data) {
                canvas.draw_path(&path, paint);
            }
        }
    }
}
