use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{
    Canvas, ColorType, ImageInfo, Matrix, Paint, PaintCap, PaintJoin, PaintStyle, Path,
    PathBuilder, PathMeasure, Rect,
};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::asset_cache;
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum SvgReveal {
    /// Trace each path's outline progressively (current/legacy behavior).
    #[default]
    Stroke,
    /// Sweep a mask across each path's full, already-painted shape (fills,
    /// gradients included) instead of tracing a contour.
    Fill,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Svg {
    #[serde(default)]
    pub src: Option<String>,
    #[serde(default)]
    pub data: Option<String>,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
    /// Force draw-on mode even when draw_progress is 1.0 (static draw trace view, no animation needed).
    #[serde(default)]
    pub draw: bool,
    /// Stroke width used when tracing fill-only paths (no stroke in the SVG).
    #[serde(default = "default_draw_stroke_width")]
    pub draw_stroke_width: f32,
    /// Overlap factor between paths during draw-on animation.
    /// 0.0 = strictly sequential (default); 1.0 = all paths drawn in parallel.
    #[serde(default)]
    pub draw_overlap: f32,
    /// How draw-on animation reveals paths: `stroke` traces contours (default,
    /// unchanged), `fill` sweeps a mask across each path's full painted shape.
    #[serde(default)]
    pub reveal: SvgReveal,
}

fn default_draw_stroke_width() -> f32 {
    2.0
}

fn shared_svg_fontdb() -> Arc<usvg::fontdb::Database> {
    static DB: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    DB.get_or_init(|| {
        let mut db = usvg::fontdb::Database::new();
        db.load_system_fonts();
        Arc::new(db)
    })
    .clone()
}

fn svg_parse_options() -> usvg::Options<'static> {
    usvg::Options {
        fontdb: shared_svg_fontdb(),
        ..Default::default()
    }
}

fn count_svg_text_elements(svg_data: &[u8]) -> usize {
    let Ok(source) = std::str::from_utf8(svg_data) else {
        return 0;
    };
    let Ok(doc) = usvg::roxmltree::Document::parse(source) else {
        return 0;
    };
    doc.descendants()
        .filter(|node| node.is_element() && node.tag_name().name() == "text")
        .count()
}

fn count_resolved_text_nodes(group: &usvg::Group) -> usize {
    group.children().iter().fold(0, |count, node| {
        count
            + match node {
                usvg::Node::Group(g) => count_resolved_text_nodes(g),
                usvg::Node::Text(_) => 1,
                _ => 0,
            }
    })
}

fn already_warned_svg_texts() -> &'static Mutex<HashSet<u64>> {
    static WARNED: OnceLock<Mutex<HashSet<u64>>> = OnceLock::new();
    WARNED.get_or_init(|| Mutex::new(HashSet::new()))
}

fn svg_data_fingerprint(svg_data: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    svg_data.hash(&mut hasher);
    hasher.finish()
}

fn warn_on_unresolved_svg_text(svg_data: &[u8], tree: &usvg::Tree) {
    let declared = count_svg_text_elements(svg_data);
    if declared == 0 {
        return;
    }

    let resolved = count_resolved_text_nodes(tree.root());
    if resolved >= declared {
        return;
    }

    let fingerprint = svg_data_fingerprint(svg_data);
    let mut warned = already_warned_svg_texts()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if !warned.insert(fingerprint) {
        return;
    }
    drop(warned);

    eprintln!(
        "Warning: svg: {} of {} <text> element(s) have no matching font face for their font-family and will not be drawn. Declare the family in the scenario's `fonts` list (a \"google\" source or a local `path`), or use a font already installed on the system.",
        declared - resolved,
        declared
    );
}

rustmotion_core::impl_traits!(Svg {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

fn tiny_path_to_skia(tsp: &tiny_skia::Path, abs_transform: tiny_skia::Transform) -> Path {
    let t = abs_transform;
    let matrix = Matrix::new_all(t.sx, t.kx, t.tx, t.ky, t.sy, t.ty, 0.0, 0.0, 1.0);

    let mut skia_path = PathBuilder::new();
    for segment in tsp.segments() {
        match segment {
            tiny_skia::PathSegment::MoveTo(p) => {
                let pt = matrix.map_point((p.x, p.y));
                skia_path.move_to(pt);
            }
            tiny_skia::PathSegment::LineTo(p) => {
                let pt = matrix.map_point((p.x, p.y));
                skia_path.line_to(pt);
            }
            tiny_skia::PathSegment::QuadTo(p1, p2) => {
                let cp = matrix.map_point((p1.x, p1.y));
                let ep = matrix.map_point((p2.x, p2.y));
                skia_path.quad_to(cp, ep);
            }
            tiny_skia::PathSegment::CubicTo(p1, p2, p3) => {
                let cp1 = matrix.map_point((p1.x, p1.y));
                let cp2 = matrix.map_point((p2.x, p2.y));
                let ep = matrix.map_point((p3.x, p3.y));
                skia_path.cubic_to(cp1, cp2, ep);
            }
            tiny_skia::PathSegment::Close => {
                skia_path.close();
            }
        }
    }
    skia_path.detach()
}

struct DrawSegment {
    path: Path,
    color: skia_safe::Color,
    stroke_width: f32,
    cap: PaintCap,
    join: PaintJoin,
}

fn to_skia_cap(cap: usvg::LineCap) -> PaintCap {
    match cap {
        usvg::LineCap::Butt => PaintCap::Butt,
        usvg::LineCap::Round => PaintCap::Round,
        usvg::LineCap::Square => PaintCap::Square,
    }
}

fn to_skia_join(join: usvg::LineJoin) -> PaintJoin {
    match join {
        usvg::LineJoin::Miter | usvg::LineJoin::MiterClip => PaintJoin::Miter,
        usvg::LineJoin::Round => PaintJoin::Round,
        usvg::LineJoin::Bevel => PaintJoin::Bevel,
    }
}

fn collect_paths(group: &usvg::Group, draw_stroke_width: f32, out: &mut Vec<DrawSegment>) {
    for node in group.children() {
        match node {
            usvg::Node::Group(g) => {
                collect_paths(g, draw_stroke_width, out);
            }
            usvg::Node::Path(p) => {
                if !p.is_visible() {
                    continue;
                }
                let skia_path = tiny_path_to_skia(p.data(), p.abs_transform());
                let (color, stroke_width, cap, join) = if let Some(stroke) = p.stroke() {
                    let sw = stroke.width().get();
                    let c = match stroke.paint() {
                        usvg::Paint::Color(col) => {
                            let alpha = (stroke.opacity().get() * 255.0) as u8;
                            skia_safe::Color::from_argb(alpha, col.red, col.green, col.blue)
                        }
                        _ => skia_safe::Color::WHITE,
                    };
                    (
                        c,
                        sw,
                        to_skia_cap(stroke.linecap()),
                        to_skia_join(stroke.linejoin()),
                    )
                } else if let Some(fill) = p.fill() {
                    let c = match fill.paint() {
                        usvg::Paint::Color(col) => {
                            let alpha = (fill.opacity().get() * 255.0) as u8;
                            skia_safe::Color::from_argb(alpha, col.red, col.green, col.blue)
                        }
                        _ => skia_safe::Color::WHITE,
                    };
                    (c, draw_stroke_width, PaintCap::Butt, PaintJoin::Miter)
                } else {
                    (
                        skia_safe::Color::WHITE,
                        draw_stroke_width,
                        PaintCap::Butt,
                        PaintJoin::Miter,
                    )
                };
                out.push(DrawSegment {
                    path: skia_path,
                    color,
                    stroke_width,
                    cap,
                    join,
                });
            }
            _ => {}
        }
    }
}

fn collect_paths_for_fill(group: &usvg::Group, out: &mut Vec<Path>) {
    for node in group.children() {
        match node {
            usvg::Node::Group(g) => {
                collect_paths_for_fill(g, out);
            }
            usvg::Node::Path(p) => {
                if !p.is_visible() {
                    continue;
                }
                let mut skia_path = tiny_path_to_skia(p.data(), p.abs_transform());
                let fill_type = match p.fill().map(|f| f.rule()) {
                    Some(usvg::FillRule::EvenOdd) => skia_safe::PathFillType::EvenOdd,
                    _ => skia_safe::PathFillType::Winding,
                };
                skia_path.set_fill_type(fill_type);
                out.push(skia_path);
            }
            _ => {}
        }
    }
}

fn paint_fill_reveal(
    canvas: &Canvas,
    group: &usvg::Group,
    svg_size: usvg::Size,
    layout: &BoxLayout,
    progress: f32,
    draw_overlap: f32,
    full_image: &skia_safe::Image,
) {
    let progress = progress.clamp(0.0, 1.0);

    let mut paths: Vec<Path> = Vec::new();
    collect_paths_for_fill(group, &mut paths);

    if paths.is_empty() {
        return;
    }

    let scale_x = if svg_size.width() > 0.0 {
        layout.width / svg_size.width()
    } else {
        1.0
    };
    let scale_y = if svg_size.height() > 0.0 {
        layout.height / svg_size.height()
    } else {
        1.0
    };

    let lengths: Vec<f32> = paths
        .iter()
        .map(|path| {
            let mut pm = PathMeasure::new(path, false, None);
            pm.length()
        })
        .collect();

    let total_length: f32 = lengths.iter().sum();
    if total_length <= 0.0 {
        return;
    }

    let overlap = draw_overlap.clamp(0.0, 1.0);
    let image_dst = Rect::from_xywh(0.0, 0.0, svg_size.width(), svg_size.height());
    let paint = Paint::default();

    let mut cumulative = 0.0f32;
    for (path, length) in paths.iter().zip(lengths.iter()) {
        let base_frac = length / total_length;
        let window_size = base_frac * (1.0 - overlap) + overlap;
        let start_frac = cumulative * (1.0 - overlap);
        cumulative += base_frac;

        let local_t = if window_size > 0.0 {
            ((progress - start_frac) / window_size).clamp(0.0, 1.0)
        } else if progress >= start_frac {
            1.0
        } else {
            0.0
        };

        if local_t <= 0.0 {
            continue;
        }

        canvas.save();
        canvas.scale((scale_x, scale_y));
        canvas.clip_path(path, None, true);

        if local_t < 1.0 {
            let bounds = path.bounds();
            let revealed_w = bounds.width() * local_t;
            let sweep = Rect::from_ltrb(
                bounds.left,
                bounds.top - 1.0,
                bounds.left + revealed_w,
                bounds.bottom + 1.0,
            );
            canvas.clip_rect(sweep, None, true);
        }

        canvas.draw_image_rect(full_image, None, image_dst, &paint);
        canvas.restore();
    }
}

fn paint_draw_on(
    canvas: &Canvas,
    group: &usvg::Group,
    svg_size: usvg::Size,
    layout: &BoxLayout,
    progress: f32,
    draw_stroke_width: f32,
    draw_overlap: f32,
) {
    let progress = progress.clamp(0.0, 1.0);

    let mut segments: Vec<DrawSegment> = Vec::new();
    collect_paths(group, draw_stroke_width, &mut segments);

    if segments.is_empty() {
        return;
    }

    let scale_x = if svg_size.width() > 0.0 {
        layout.width / svg_size.width()
    } else {
        1.0
    };
    let scale_y = if svg_size.height() > 0.0 {
        layout.height / svg_size.height()
    } else {
        1.0
    };

    canvas.save();
    canvas.scale((scale_x, scale_y));

    let lengths: Vec<f32> = segments
        .iter()
        .map(|segment| {
            let mut pm = PathMeasure::new(&segment.path, false, None);
            pm.length()
        })
        .collect();

    let total_length: f32 = lengths.iter().sum();
    if total_length <= 0.0 {
        canvas.restore();
        return;
    }

    let overlap = draw_overlap.clamp(0.0, 1.0);

    let mut cumulative = 0.0f32;
    for (segment, length) in segments.iter().zip(lengths.iter()) {
        let base_frac = length / total_length;
        let window_size = base_frac * (1.0 - overlap) + overlap;
        let start_frac = cumulative * (1.0 - overlap);
        cumulative += base_frac;

        let local_t = if window_size > 0.0 {
            ((progress - start_frac) / window_size).clamp(0.0, 1.0)
        } else if progress >= start_frac {
            1.0
        } else {
            0.0
        };

        if local_t <= 0.0 {
            continue;
        }

        let draw_len = length * local_t;

        let mut paint = Paint::default();
        paint.set_color(segment.color);
        paint.set_style(PaintStyle::Stroke);
        paint.set_stroke_width(segment.stroke_width);
        paint.set_stroke_cap(segment.cap);
        paint.set_stroke_join(segment.join);
        paint.set_anti_alias(true);

        if local_t < 1.0 && draw_len > 0.0 {
            let remaining = length - draw_len;
            let intervals = [draw_len, remaining + 0.01];
            if let Some(dash) = skia_safe::PathEffect::dash(&intervals, 0.0) {
                paint.set_path_effect(dash);
            }
        }

        canvas.draw_path(&segment.path, &paint);
    }

    canvas.restore();
}

impl Painter for Svg {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        props: &AnimatedProperties,
        _ctx: &PaintCtx,
    ) {
        let draw_active = self.draw || (props.draw_progress >= 0.0 && props.draw_progress < 1.0);

        if draw_active {
            let progress = if props.draw_progress >= 0.0 {
                props.draw_progress
            } else {
                1.0
            };

            if progress <= 0.0 {
                return;
            }

            let svg_data = if let Some(ref src) = self.src {
                match std::fs::read(src) {
                    Ok(d) => d,
                    Err(_) => return,
                }
            } else if let Some(ref data) = self.data {
                data.as_bytes().to_vec()
            } else {
                return;
            };

            let opt = svg_parse_options();
            let Ok(tree) = usvg::Tree::from_data(&svg_data, &opt) else {
                return;
            };
            warn_on_unresolved_svg_text(&svg_data, &tree);

            let svg_size = tree.size();

            if progress >= 1.0 {
                self.paint_resvg(canvas, layout, &svg_data, &tree, svg_size);
            } else if self.reveal == SvgReveal::Fill {
                let Some(full_image) = self.cached_full_image(layout) else {
                    return;
                };
                paint_fill_reveal(
                    canvas,
                    tree.root(),
                    svg_size,
                    layout,
                    progress,
                    self.draw_overlap,
                    &full_image,
                );
            } else {
                paint_draw_on(
                    canvas,
                    tree.root(),
                    svg_size,
                    layout,
                    progress,
                    self.draw_stroke_width,
                    self.draw_overlap,
                );
            }
        } else {
            self.paint_static(canvas, layout);
        }
    }
}

impl Svg {
    fn paint_static(&self, canvas: &Canvas, layout: &BoxLayout) {
        let Some(img) = self.cached_full_image(layout) else {
            return;
        };

        let dst = Rect::from_xywh(0.0, 0.0, layout.width, layout.height);
        let paint = Paint::default();
        canvas.draw_image_rect(img, None, dst, &paint);
    }

    fn cached_full_image(&self, layout: &BoxLayout) -> Option<skia_safe::Image> {
        let target_w_opt: Option<u32> = if layout.width > 0.0 {
            Some(layout.width as u32)
        } else {
            None
        };
        let target_h_opt: Option<u32> = if layout.height > 0.0 {
            Some(layout.height as u32)
        } else {
            None
        };

        let cache_key = if let Some(ref src) = self.src {
            format!(
                "svg:{}:{}x{}",
                src,
                target_w_opt.unwrap_or(0),
                target_h_opt.unwrap_or(0)
            )
        } else {
            let data = self.data.as_ref()?;
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            data.hash(&mut hasher);
            format!(
                "svg-inline:{}:{}x{}",
                hasher.finish(),
                target_w_opt.unwrap_or(0),
                target_h_opt.unwrap_or(0)
            )
        };

        let cache = asset_cache();
        if let Some(cached) = cache.get(&cache_key) {
            return Some(cached.clone());
        }

        let svg_data = if let Some(ref src) = self.src {
            std::fs::read(src).ok()?
        } else {
            self.data.as_ref()?.as_bytes().to_vec()
        };

        let opt = svg_parse_options();
        let tree = usvg::Tree::from_data(&svg_data, &opt).ok()?;
        warn_on_unresolved_svg_text(&svg_data, &tree);

        let svg_size = tree.size();
        let target_w = target_w_opt.unwrap_or(svg_size.width() as u32);
        let target_h = target_h_opt.unwrap_or(svg_size.height() as u32);

        let mut pixmap = tiny_skia::Pixmap::new(target_w, target_h)?;

        let scale_x = target_w as f32 / svg_size.width();
        let scale_y = target_h as f32 / svg_size.height();
        let transform = tiny_skia::Transform::from_scale(scale_x, scale_y);

        resvg::render(&tree, transform, &mut pixmap.as_mut());

        let img_data = skia_safe::Data::new_copy(pixmap.data());
        let img_info = ImageInfo::new(
            (target_w as i32, target_h as i32),
            ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let decoded =
            skia_safe::images::raster_from_data(&img_info, img_data, target_w as usize * 4)?;
        cache.insert(cache_key, decoded.clone());
        Some(decoded)
    }

    fn paint_resvg(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        svg_data: &[u8],
        tree: &usvg::Tree,
        svg_size: usvg::Size,
    ) {
        let target_w = if layout.width > 0.0 {
            layout.width as u32
        } else {
            svg_size.width() as u32
        };
        let target_h = if layout.height > 0.0 {
            layout.height as u32
        } else {
            svg_size.height() as u32
        };

        if target_w == 0 || target_h == 0 {
            return;
        }

        let Some(mut pixmap) = tiny_skia::Pixmap::new(target_w, target_h) else {
            return;
        };

        let scale_x = target_w as f32 / svg_size.width();
        let scale_y = target_h as f32 / svg_size.height();
        let transform = tiny_skia::Transform::from_scale(scale_x, scale_y);
        resvg::render(tree, transform, &mut pixmap.as_mut());

        let img_data = skia_safe::Data::new_copy(pixmap.data());
        let img_info = ImageInfo::new(
            (target_w as i32, target_h as i32),
            ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let Some(img) =
            skia_safe::images::raster_from_data(&img_info, img_data, target_w as usize * 4)
        else {
            return;
        };

        let dst = Rect::from_xywh(0.0, 0.0, layout.width, layout.height);
        let paint = Paint::default();
        canvas.draw_image_rect(img, None, dst, &paint);

        let _ = svg_data;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmotion_core::engine::layout_pass::Insets;

    const W: i32 = 100;
    const H: i32 = 100;

    fn filled_square_svg() -> Svg {
        Svg {
            src: None,
            data: Some(
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
                    <rect x="10" y="10" width="80" height="80" fill="#ff0000"/>
                </svg>"##
                    .to_string(),
            ),
            timing: Default::default(),
            style: Default::default(),
            timeline: Vec::new(),
            stagger: None,
            draw: false,
            draw_stroke_width: default_draw_stroke_width(),
            draw_overlap: 0.0,
            reveal: SvgReveal::Fill,
        }
    }

    fn test_layout() -> BoxLayout {
        BoxLayout {
            x: 0.0,
            y: 0.0,
            width: W as f32,
            height: H as f32,
            border: Insets::default(),
            padding: Insets::default(),
        }
    }

    fn test_ctx() -> PaintCtx {
        PaintCtx {
            time: 0.0,
            scenario_time: 0.0,
            scene_duration: 1.0,
            frame_index: 0,
            fps: 30,
            video_width: 1920,
            video_height: 1080,
            stagger_offset: 0.0,
        }
    }

    fn red_alpha_at(surface: &mut skia_safe::Surface, x: i32, y: i32) -> (u8, u8, u8, u8) {
        let snapshot = surface.image_snapshot();
        let info = skia_safe::ImageInfo::new(
            (W, H),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Unpremul,
            None,
        );
        let mut buf = vec![0u8; (W * H * 4) as usize];
        let ok = snapshot.read_pixels(
            &info,
            &mut buf,
            (W * 4) as usize,
            skia_safe::IPoint::new(0, 0),
            skia_safe::image::CachingHint::Disallow,
        );
        assert!(ok, "pixel read should succeed");
        let idx = ((y * W + x) * 4) as usize;
        (buf[idx], buf[idx + 1], buf[idx + 2], buf[idx + 3])
    }

    #[test]
    fn fill_reveal_paints_interior_pixels_at_partial_progress() {
        let svg = filled_square_svg();
        let layout = test_layout();
        let props = AnimatedProperties {
            draw_progress: 0.5,
            ..Default::default()
        };
        let ctx = test_ctx();

        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        {
            let canvas = surface.canvas();
            svg.paint_content(canvas, &layout, &props, &ctx);
        }

        let (r, g, b, a) = red_alpha_at(&mut surface, 30, 50);
        assert!(
            a > 200 && r > 200 && g < 50 && b < 50,
            "fill reveal at draw_progress=0.5 must paint filled interior pixels, got rgba=({r},{g},{b},{a}) at (30,50)"
        );
    }

    #[test]
    fn stroke_reveal_default_leaves_interior_unfilled_at_partial_progress() {
        let mut svg = filled_square_svg();
        svg.reveal = SvgReveal::Stroke;
        let layout = test_layout();
        let props = AnimatedProperties {
            draw_progress: 0.5,
            ..Default::default()
        };
        let ctx = test_ctx();

        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        {
            let canvas = surface.canvas();
            svg.paint_content(canvas, &layout, &props, &ctx);
        }

        let (_, _, _, a) = red_alpha_at(&mut surface, 50, 50);
        assert!(
            a < 50,
            "default stroke reveal must not fill the interior at partial progress, got alpha={a} at (50,50)"
        );
    }

    fn line_stroke_svg(stroke_width: f32, cap: &str) -> Svg {
        Svg {
            src: None,
            data: Some(format!(
                r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"><path d="M10 50 L90 50" stroke="#000000" stroke-width="{stroke_width}" stroke-linecap="{cap}" fill="none"/></svg>"##
            )),
            timing: Default::default(),
            style: Default::default(),
            timeline: Vec::new(),
            stagger: None,
            draw: false,
            draw_stroke_width: default_draw_stroke_width(),
            draw_overlap: 0.0,
            reveal: SvgReveal::Stroke,
        }
    }

    fn layout_for(size: f32) -> BoxLayout {
        BoxLayout {
            x: 0.0,
            y: 0.0,
            width: size,
            height: size,
            border: Insets::default(),
            padding: Insets::default(),
        }
    }

    fn render_alpha_buffer(
        svg: &Svg,
        layout: &BoxLayout,
        draw_progress: f32,
        size: i32,
    ) -> Vec<u8> {
        let props = AnimatedProperties {
            draw_progress,
            ..Default::default()
        };
        let ctx = test_ctx();

        let mut surface =
            skia_safe::surfaces::raster_n32_premul((size, size)).expect("raster surface");
        {
            let canvas = surface.canvas();
            svg.paint_content(canvas, layout, &props, &ctx);
        }

        let snapshot = surface.image_snapshot();
        let info = skia_safe::ImageInfo::new(
            (size, size),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Unpremul,
            None,
        );
        let mut buf = vec![0u8; (size * size * 4) as usize];
        let ok = snapshot.read_pixels(
            &info,
            &mut buf,
            (size * 4) as usize,
            skia_safe::IPoint::new(0, 0),
            skia_safe::image::CachingHint::Disallow,
        );
        assert!(ok, "pixel read should succeed");
        buf
    }

    fn buffer_alpha_at(buf: &[u8], size: i32, x: i32, y: i32) -> u8 {
        let idx = ((y * size + x) * 4 + 3) as usize;
        buf[idx]
    }

    fn vertical_ink_thickness(buf: &[u8], size: i32, x: i32) -> i32 {
        (0..size)
            .filter(|&y| buffer_alpha_at(buf, size, x, y) > 40)
            .count() as i32
    }

    fn leftmost_ink_x(buf: &[u8], size: i32, y: i32) -> Option<i32> {
        (0..size).find(|&x| buffer_alpha_at(buf, size, x, y) > 40)
    }

    #[test]
    fn draw_on_stroke_width_scales_with_the_nodes_transform_like_the_finished_render() {
        let svg = line_stroke_svg(4.0, "butt");

        let small_buf = render_alpha_buffer(&svg, &layout_for(100.0), 0.5, 100);
        let small_thickness = vertical_ink_thickness(&small_buf, 100, 30);

        let large_buf = render_alpha_buffer(&svg, &layout_for(400.0), 0.5, 400);
        let large_thickness = vertical_ink_thickness(&large_buf, 400, 120);

        assert!(
            small_thickness > 0 && large_thickness > 0,
            "both renders must show ink on the drawn segment, got small={small_thickness} large={large_thickness}"
        );
        assert!(
            (large_thickness as f32) > (small_thickness as f32) * 2.0,
            "stroke width while drawing must scale with the node's viewBox transform \
             (4x layout should read ~4x thicker), got small={small_thickness}px large={large_thickness}px"
        );
    }

    #[test]
    fn draw_on_uses_the_finished_marks_stroke_cap() {
        let svg = line_stroke_svg(16.0, "round");
        let layout = layout_for(100.0);

        let drawing_buf = render_alpha_buffer(&svg, &layout, 0.999, 100);
        let drawing_left = leftmost_ink_x(&drawing_buf, 100, 50)
            .expect("the almost-finished drawing frame must have ink on the line's row");

        let finished_buf = render_alpha_buffer(&svg, &layout, 1.0, 100);
        let finished_left = leftmost_ink_x(&finished_buf, 100, 50)
            .expect("the finished frame must have ink on the line's row");

        assert!(
            (drawing_left - finished_left).abs() <= 2,
            "the last drawing frame must reach as far left as the finished mark's round cap, \
             got drawing_left={drawing_left} finished_left={finished_left}"
        );
    }

    fn a_family_this_host_actually_has() -> Option<String> {
        shared_svg_fontdb()
            .faces()
            .next()
            .and_then(|face| face.families.first().map(|(name, _)| name.clone()))
    }

    fn text_svg(font_family: &str) -> Svg {
        Svg {
            src: None,
            data: Some(format!(
                r##"<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 300 100' width='300' height='100'><text x='10' y='70' font-size='60' font-family='{font_family}' fill='#000000'>SVG</text></svg>"##
            )),
            timing: Default::default(),
            style: Default::default(),
            timeline: Vec::new(),
            stagger: None,
            draw: false,
            draw_stroke_width: default_draw_stroke_width(),
            draw_overlap: 0.0,
            reveal: SvgReveal::Stroke,
        }
    }

    #[test]
    fn svg_text_with_a_system_font_is_rasterized() {
        let Some(family) = a_family_this_host_actually_has() else {
            return;
        };
        let svg = text_svg(&family);
        let layout = BoxLayout {
            x: 0.0,
            y: 0.0,
            width: 300.0,
            height: 100.0,
            border: Insets::default(),
            padding: Insets::default(),
        };
        let props = AnimatedProperties::default();
        let ctx = test_ctx();

        let mut surface =
            skia_safe::surfaces::raster_n32_premul((300, 100)).expect("raster surface");
        {
            let canvas = surface.canvas();
            svg.paint_content(canvas, &layout, &props, &ctx);
        }

        let snapshot = surface.image_snapshot();
        let info = skia_safe::ImageInfo::new(
            (300, 100),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Unpremul,
            None,
        );
        let mut buf = vec![0u8; (300 * 100 * 4) as usize];
        let ok = snapshot.read_pixels(
            &info,
            &mut buf,
            (300 * 4) as usize,
            skia_safe::IPoint::new(0, 0),
            skia_safe::image::CachingHint::Disallow,
        );
        assert!(ok, "pixel read should succeed");

        let dark_pixels = (0..100)
            .flat_map(|y| (0..300).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                let idx = ((y * 300 + x) * 4) as usize;
                let (r, g, b, a) = (buf[idx], buf[idx + 1], buf[idx + 2], buf[idx + 3]);
                a > 200 && r < 60 && g < 60 && b < 60
            })
            .count();

        assert!(
            dark_pixels > 20,
            "the <text> element must be rasterized into dark pixels with a system font \
             available, got {dark_pixels} matching pixels"
        );
    }

    #[test]
    fn a_host_with_no_font_at_all_says_so_instead_of_dropping_the_text() {
        let svg_data = text_svg("Helvetica").data.unwrap().into_bytes();
        let empty_db = usvg::Options::default();
        let tree = usvg::Tree::from_data(&svg_data, &empty_db).expect("valid svg");

        assert_eq!(count_svg_text_elements(&svg_data), 1);
        assert_eq!(
            count_resolved_text_nodes(tree.root()),
            0,
            "with an empty fontdb usvg drops the <text> node from the tree entirely — this \
             is the condition the warning exists for, and the one a bare CI container is in"
        );
    }

    #[test]
    fn a_resolvable_font_family_matches_declared_and_resolved_text_counts() {
        let Some(family) = a_family_this_host_actually_has() else {
            return;
        };
        let svg = text_svg(&family);
        let svg_data = svg.data.as_ref().unwrap().as_bytes().to_vec();
        let opt = svg_parse_options();
        let tree = usvg::Tree::from_data(&svg_data, &opt).expect("valid svg");

        assert_eq!(count_svg_text_elements(&svg_data), 1);
        assert_eq!(
            count_resolved_text_nodes(tree.root()),
            1,
            "a family this host actually has must resolve once system fonts are loaded"
        );
    }

    #[test]
    fn an_empty_font_database_is_detected_as_undrawn_text() {
        let svg = text_svg("Helvetica");
        let svg_data = svg.data.as_ref().unwrap().as_bytes().to_vec();
        let opt = usvg::Options::default();
        let tree = usvg::Tree::from_data(&svg_data, &opt).expect("valid svg");

        assert_eq!(count_svg_text_elements(&svg_data), 1);
        assert_eq!(
            count_resolved_text_nodes(tree.root()),
            0,
            "usvg::Options::default() carries an empty font database (the pre-fix behavior \
             behind issue #374): no <text> can resolve, which is exactly the condition that \
             must trigger the stderr warning"
        );
    }
}
