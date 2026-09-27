use skia_safe::{Canvas, Paint, Path, PathBuilder, PathMeasure, PathVerb, Point, Rect};

use crate::schema::ShapeType;

pub fn trim_path_between(path: &Path, start: f32, end: f32) -> Path {
    let (start, end) = (start.clamp(0.0, 1.0), end.clamp(0.0, 1.0));
    let (start, end) = if start <= end {
        (start, end)
    } else {
        (end, start)
    };
    if end - start <= 0.0 {
        return PathBuilder::new().detach();
    }

    let mut measure = PathMeasure::new(path, false, None);
    let length = measure.length();
    if length <= 0.0 {
        return PathBuilder::new().detach();
    }

    let mut builder = PathBuilder::new();
    let extracted = measure.get_segment(length * start, length * end, &mut builder, true);
    if extracted {
        builder.detach()
    } else {
        PathBuilder::new().detach()
    }
}

pub fn interpolate_path_data(from: &str, to: &str, t: f32) -> Option<Path> {
    let path_from = Path::from_svg(from)?;
    let path_to = Path::from_svg(to)?;
    let verbs_from: Vec<(PathVerb, Vec<Point>)> = path_from
        .iter()
        .map(|rec| (rec.verb(), rec.points().to_vec()))
        .collect();
    let verbs_to: Vec<(PathVerb, Vec<Point>)> = path_to
        .iter()
        .map(|rec| (rec.verb(), rec.points().to_vec()))
        .collect();

    if verbs_from.len() != verbs_to.len() {
        eprintln!(
            "rustmotion: path keyframe mismatch — \"{from}\" has {} command(s), \"{to}\" has \
             {}; interpolating path data needs the same command structure on every keyframe. \
             Holding the first keyframe's shape instead of snapping.",
            verbs_from.len(),
            verbs_to.len()
        );
        return None;
    }

    let t = t.clamp(0.0, 1.0);
    let mut builder = PathBuilder::new();
    for ((verb_from, pts_from), (verb_to, pts_to)) in verbs_from.iter().zip(verbs_to.iter()) {
        let (verb_from, verb_to) = (*verb_from, *verb_to);
        if verb_from != verb_to || pts_from.len() != pts_to.len() {
            eprintln!(
                "rustmotion: path keyframe mismatch — \"{from}\" and \"{to}\" use a different \
                 command at the same position ({verb_from:?} vs {verb_to:?}); interpolating \
                 path data needs the same command structure on every keyframe. Holding the \
                 first keyframe's shape instead of snapping."
            );
            return None;
        }

        let lerped: Vec<Point> = pts_from
            .iter()
            .zip(pts_to.iter())
            .map(|(p, q)| Point::new(p.x + (q.x - p.x) * t, p.y + (q.y - p.y) * t))
            .collect();

        match (verb_from, lerped.as_slice()) {
            (PathVerb::Move, [p]) => {
                builder.move_to(*p);
            }
            (PathVerb::Line, [_, p1]) => {
                builder.line_to(*p1);
            }
            (PathVerb::Quad, [_, c, p1]) => {
                builder.quad_to(*c, *p1);
            }
            (PathVerb::Conic, [_, c, p1]) => {
                builder.quad_to(*c, *p1);
            }
            (PathVerb::Cubic, [_, c1, c2, p1]) => {
                builder.cubic_to(*c1, *c2, *p1);
            }
            (PathVerb::Close, _) => {
                builder.close();
            }
            _ => {}
        }
    }

    Some(builder.detach())
}

pub fn build_shape_path(
    shape_type: &ShapeType,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    corner_radius: Option<f32>,
) -> Option<skia_safe::Path> {
    match shape_type {
        ShapeType::Rect => {
            let mut path = skia_safe::PathBuilder::new();
            path.add_rect(Rect::from_xywh(x, y, w, h), None, None);
            Some(path.detach())
        }
        ShapeType::RoundedRect => {
            let r = corner_radius.unwrap_or(8.0);
            let rrect = skia_safe::RRect::new_rect_xy(Rect::from_xywh(x, y, w, h), r, r);
            let mut path = skia_safe::PathBuilder::new();
            path.add_rrect(rrect, None, None);
            Some(path.detach())
        }
        ShapeType::Circle => {
            let radius = w.min(h) / 2.0;
            let mut path = skia_safe::PathBuilder::new();
            path.add_circle((x + w / 2.0, y + h / 2.0), radius, None);
            Some(path.detach())
        }
        ShapeType::Ellipse => {
            let mut path = skia_safe::PathBuilder::new();
            path.add_oval(Rect::from_xywh(x, y, w, h), None, None);
            Some(path.detach())
        }
        ShapeType::Triangle => {
            let mut path = skia_safe::PathBuilder::new();
            path.move_to((x + w / 2.0, y));
            path.line_to((x + w, y + h));
            path.line_to((x, y + h));
            path.close();
            Some(path.detach())
        }
        ShapeType::Star { points } => {
            let cx = x + w / 2.0;
            let cy = y + h / 2.0;
            let outer_r = w.min(h) / 2.0;
            let inner_r = outer_r * 0.4;
            let n = *points as usize;
            let mut path = skia_safe::PathBuilder::new();
            for i in 0..(n * 2) {
                let angle =
                    (i as f32) * std::f32::consts::PI / n as f32 - std::f32::consts::FRAC_PI_2;
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
            Some(path.detach())
        }
        ShapeType::Polygon { sides } => {
            let cx = x + w / 2.0;
            let cy = y + h / 2.0;
            let r = w.min(h) / 2.0;
            let n = *sides as usize;
            let mut path = skia_safe::PathBuilder::new();
            for i in 0..n {
                let angle = (i as f32) * 2.0 * std::f32::consts::PI / n as f32
                    - std::f32::consts::FRAC_PI_2;
                let px = cx + r * angle.cos();
                let py = cy + r * angle.sin();
                if i == 0 {
                    path.move_to((px, py));
                } else {
                    path.line_to((px, py));
                }
            }
            path.close();
            Some(path.detach())
        }
        ShapeType::Path { data } => skia_safe::Path::from_svg(data),
    }
}

pub fn draw_shape_path(
    canvas: &Canvas,
    shape_type: &ShapeType,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    corner_radius: Option<f32>,
    paint: &Paint,
) {
    let rect = Rect::from_xywh(x, y, w, h);
    match shape_type {
        ShapeType::Rect => {
            canvas.draw_rect(rect, paint);
        }
        ShapeType::RoundedRect => {
            let r = corner_radius.unwrap_or(8.0);
            let rrect = skia_safe::RRect::new_rect_xy(rect, r, r);
            canvas.draw_rrect(rrect, paint);
        }
        ShapeType::Circle => {
            let radius = w.min(h) / 2.0;
            canvas.draw_circle((x + w / 2.0, y + h / 2.0), radius, paint);
        }
        ShapeType::Ellipse => {
            canvas.draw_oval(rect, paint);
        }
        ShapeType::Triangle => {
            let mut path = skia_safe::PathBuilder::new();
            path.move_to((x + w / 2.0, y));
            path.line_to((x + w, y + h));
            path.line_to((x, y + h));
            path.close();
            canvas.draw_path(&path.detach(), paint);
        }
        ShapeType::Star { points } => {
            let cx = x + w / 2.0;
            let cy = y + h / 2.0;
            let outer_r = w.min(h) / 2.0;
            let inner_r = outer_r * 0.4;
            let n = *points as usize;
            let mut path = skia_safe::PathBuilder::new();
            for i in 0..(n * 2) {
                let angle =
                    (i as f32) * std::f32::consts::PI / n as f32 - std::f32::consts::FRAC_PI_2;
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
            canvas.draw_path(&path.detach(), paint);
        }
        ShapeType::Polygon { sides } => {
            let cx = x + w / 2.0;
            let cy = y + h / 2.0;
            let r = w.min(h) / 2.0;
            let n = *sides as usize;
            let mut path = skia_safe::PathBuilder::new();
            for i in 0..n {
                let angle = (i as f32) * 2.0 * std::f32::consts::PI / n as f32
                    - std::f32::consts::FRAC_PI_2;
                let px = cx + r * angle.cos();
                let py = cy + r * angle.sin();
                if i == 0 {
                    path.move_to((px, py));
                } else {
                    path.line_to((px, py));
                }
            }
            path.close();
            canvas.draw_path(&path.detach(), paint);
        }
        ShapeType::Path { data } => {
            if let Some(path) = skia_safe::Path::from_svg(data) {
                canvas.draw_path(&path, paint);
            }
        }
    }
}
