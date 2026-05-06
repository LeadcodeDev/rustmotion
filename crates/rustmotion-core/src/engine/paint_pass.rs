//! Paint pass — walks the BoxTree post-layout and paints each node.
//!
//! Order per node (top-down):
//!   1. canvas.save()
//!   2. apply transform (CSS `transform` → Skia matrix)
//!   3. open opacity layer if `opacity < 1.0`
//!   4. clip to padding-box if `overflow != visible`
//!   5. paint outset box-shadow
//!   6. paint background
//!   7. paint border (border-radius aware)
//!   8. delegate component-specific paint via `PaintDispatcher`
//!   9. recurse children sorted by z-index
//!  10. canvas.restore()
//!
//! The dispatcher hook lets the higher-level crate plug component-specific
//! paint without coupling `rustmotion-core` to all 51 component types.

use skia_safe::{
    canvas::SaveLayerRec, Canvas, ClipOp, Color as SColor, Color4f, M44, Paint, PaintStyle,
    Path, Point, RRect, Rect, V3,
};

use crate::css::style::{
    Background, BackgroundLayer, BorderEdges, BorderRadius, BorderStyle, BoxShadow,
    Color, CssStyle, Edges, Overflow, TransformFn,
};
use crate::css::units::{LengthContext, LengthPercentage, ParsedLength};
use crate::engine::box_tree::{BoxKind, BoxNode};
use crate::engine::layout_pass::{BoxLayout, LayoutResult};

/// Frame-level paint context (timing + viewport).
#[derive(Debug, Clone, Copy)]
pub struct PaintFrame {
    pub time: f64,
    pub frame_index: u32,
    pub fps: u32,
    pub video_width: u32,
    pub video_height: u32,
    /// Total duration of the scene in seconds — used by the dispatcher to
    /// compute animation progress (`time / scene_duration`).
    pub scene_duration: f64,
}

/// Hook to delegate component-specific painting. Implemented by the
/// higher-level crate that owns the actual `Component` enum.
pub trait PaintDispatcher {
    /// Called for `BoxKind::Component(payload)`. Implementations downcast
    /// `payload` to the concrete component type and paint into `canvas`.
    /// The canvas is already translated to the content-box origin and
    /// clipped if `overflow: hidden`.
    fn dispatch(
        &self,
        canvas: &Canvas,
        payload: &(dyn std::any::Any + Send + Sync),
        css: &CssStyle,
        layout: &BoxLayout,
        frame: &PaintFrame,
    );
}

/// No-op dispatcher (useful for tests where only generic box decoration is exercised).
pub struct NoopDispatcher;

impl PaintDispatcher for NoopDispatcher {
    fn dispatch(
        &self,
        _canvas: &Canvas,
        _payload: &(dyn std::any::Any + Send + Sync),
        _css: &CssStyle,
        _layout: &BoxLayout,
        _frame: &PaintFrame,
    ) {
    }
}

/// Paint a fully-laid-out box tree onto a Skia canvas.
pub fn paint_tree(
    canvas: &Canvas,
    root: &BoxNode,
    layout: &LayoutResult,
    frame: &PaintFrame,
    dispatcher: &dyn PaintDispatcher,
) {
    let ctx = PaintContext {
        layout,
        frame,
        dispatcher,
        viewport_size: (frame.video_width as f32, frame.video_height as f32),
    };
    paint_node(canvas, root, &ctx);
}

struct PaintContext<'a> {
    layout: &'a LayoutResult,
    frame: &'a PaintFrame,
    dispatcher: &'a dyn PaintDispatcher,
    viewport_size: (f32, f32),
}

fn paint_node(canvas: &Canvas, node: &BoxNode, ctx: &PaintContext) {
    let Some(box_layout) = ctx.layout.get(node.id) else { return };
    if box_layout.width <= 0.0 || box_layout.height <= 0.0 {
        return;
    }

    let length_ctx = LengthContext {
        viewport_width: ctx.viewport_size.0,
        viewport_height: ctx.viewport_size.1,
        parent_size: box_layout.width.max(box_layout.height),
        font_size: 16.0,
        root_font_size: 16.0,
    };

    canvas.save();

    // 2. transform
    if node.css.transform.is_some() || node.css.perspective.is_some() {
        let pivot = (
            box_layout.x + box_layout.width / 2.0,
            box_layout.y + box_layout.height / 2.0,
        );
        let transform_list = node.css.transform.as_deref().unwrap_or(&[]);
        let perspective_d = node.css.perspective.as_ref().map(|l| l.resolve(&length_ctx).max(1.0));
        apply_transform(canvas, transform_list, perspective_d, pivot, &length_ctx);
    }

    // 3. opacity layer
    let opacity = node.css.opacity.unwrap_or(1.0).clamp(0.0, 1.0);
    let opened_opacity_layer = if opacity < 1.0 {
        let mut paint = Paint::default();
        paint.set_alpha((opacity * 255.0) as u8);
        let rec = SaveLayerRec::default().paint(&paint);
        canvas.save_layer(&rec);
        true
    } else {
        false
    };

    // 4. clip overflow:hidden / clip
    let overflow = node.css.overflow.unwrap_or(Overflow::Visible);
    if matches!(overflow, Overflow::Hidden | Overflow::Clip | Overflow::Scroll | Overflow::Auto) {
        let radius = node
            .css
            .border_radius
            .as_ref()
            .map(|r| resolve_border_radius(r, box_layout, &length_ctx))
            .unwrap_or([0.0; 4]);
        let rrect = padding_rrect(box_layout, radius);
        canvas.clip_rrect(&rrect, ClipOp::Intersect, true);
    }

    // 5. outset box-shadow
    if let Some(shadows) = node.css.box_shadow.as_ref() {
        for shadow in shadows {
            if shadow.inset.unwrap_or(false) {
                continue;
            }
            paint_box_shadow(canvas, box_layout, &node.css, shadow, &length_ctx, false);
        }
    }

    // 6. background
    if let Some(bg) = node.css.background.as_ref() {
        paint_background(canvas, box_layout, &node.css, bg, &length_ctx);
    }

    // 7. border
    if let Some(border) = node.css.border.as_ref() {
        paint_border(canvas, box_layout, &node.css, border, &length_ctx);
    }

    // 8. component-specific content
    if let BoxKind::Component(payload) = &node.kind {
        ctx.dispatcher.dispatch(
            canvas,
            payload.as_ref(),
            &node.css,
            box_layout,
            ctx.frame,
        );
    }

    // 9. children (z-index ordered, then source order)
    let mut indices: Vec<usize> = (0..node.children.len()).collect();
    indices.sort_by_key(|&i| node.children[i].css.z_index.unwrap_or(0));
    for &i in &indices {
        paint_node(canvas, &node.children[i], ctx);
    }

    // inset shadows (after children so they overlay content)
    if let Some(shadows) = node.css.box_shadow.as_ref() {
        for shadow in shadows {
            if shadow.inset.unwrap_or(false) {
                paint_box_shadow(canvas, box_layout, &node.css, shadow, &length_ctx, true);
            }
        }
    }

    if opened_opacity_layer {
        canvas.restore();
    }
    canvas.restore();
}

// ---- Transform ----

fn has_3d_transform(list: &[TransformFn]) -> bool {
    list.iter().any(|t| {
        matches!(
            t,
            TransformFn::RotateX { .. }
                | TransformFn::RotateY { .. }
                | TransformFn::Rotate3d { .. }
                | TransformFn::Scale3d { .. }
                | TransformFn::ScaleZ { .. }
                | TransformFn::TranslateZ { .. }
                | TransformFn::Perspective { .. }
                | TransformFn::Matrix3d { .. }
        )
    })
}

fn apply_transform(
    canvas: &Canvas,
    list: &[TransformFn],
    perspective_d: Option<f32>,
    pivot: (f32, f32),
    ctx: &LengthContext,
) {
    if perspective_d.is_none() && !has_3d_transform(list) {
        // Fast path: 2D-only, use the native Skia 2D canvas API.
        canvas.translate(Point::new(pivot.0, pivot.1));
        for tr in list {
            match tr {
                TransformFn::Translate { x, y } => {
                    canvas.translate(Point::new(x.resolve(ctx), y.resolve(ctx)));
                }
                TransformFn::TranslateX { x } => {
                    canvas.translate(Point::new(x.resolve(ctx), 0.0));
                }
                TransformFn::TranslateY { y } => {
                    canvas.translate(Point::new(0.0, y.resolve(ctx)));
                }
                TransformFn::Translate3d { x, y, .. } => {
                    canvas.translate(Point::new(x.resolve(ctx), y.resolve(ctx)));
                }
                TransformFn::Scale { x, y } => {
                    canvas.scale((*x, *y));
                }
                TransformFn::ScaleX { x } => {
                    canvas.scale((*x, 1.0));
                }
                TransformFn::ScaleY { y } => {
                    canvas.scale((1.0, *y));
                }
                TransformFn::Rotate { deg } | TransformFn::RotateZ { deg } => {
                    canvas.rotate(*deg, None);
                }
                TransformFn::Skew { x, y } => {
                    canvas.skew((x.to_radians().tan(), y.to_radians().tan()));
                }
                TransformFn::SkewX { x } => {
                    canvas.skew((x.to_radians().tan(), 0.0));
                }
                TransformFn::SkewY { y } => {
                    canvas.skew((0.0, y.to_radians().tan()));
                }
                TransformFn::Matrix { values: v } => {
                    let m = skia_safe::Matrix::new_all(
                        v[0], v[2], v[4], v[1], v[3], v[5], 0.0, 0.0, 1.0,
                    );
                    canvas.concat(&m);
                }
                _ => {}
            }
        }
        canvas.translate(Point::new(-pivot.0, -pivot.1));
    } else {
        // 3D path: compose a single M44 and apply via concat_44 for true perspective.
        let mut m = M44::new_identity();
        m.pre_concat(&M44::translate(pivot.0, pivot.1, 0.0));
        if let Some(d) = perspective_d {
            m.pre_concat(&css_perspective_m44(d));
        }
        for tr in list {
            m.pre_concat(&transform_to_m44(tr, ctx));
        }
        m.pre_concat(&M44::translate(-pivot.0, -pivot.1, 0.0));
        canvas.concat_44(&m);
    }
}

/// CSS `perspective(d)` projection matrix in row-major form.
/// Maps (x, y, z, 1) → w' = 1 - z/d; perspective divide yields depth scaling.
fn css_perspective_m44(d: f32) -> M44 {
    M44::row_major(&[
        1.0, 0.0,       0.0, 0.0,
        0.0, 1.0,       0.0, 0.0,
        0.0, 0.0,       1.0, 0.0,
        0.0, 0.0, -1.0 / d, 1.0,
    ])
}

fn transform_to_m44(tr: &TransformFn, ctx: &LengthContext) -> M44 {
    match tr {
        TransformFn::Translate { x, y } => {
            M44::translate(x.resolve(ctx), y.resolve(ctx), 0.0)
        }
        TransformFn::TranslateX { x } => M44::translate(x.resolve(ctx), 0.0, 0.0),
        TransformFn::TranslateY { y } => M44::translate(0.0, y.resolve(ctx), 0.0),
        TransformFn::TranslateZ { z } => M44::translate(0.0, 0.0, z.resolve(ctx)),
        TransformFn::Translate3d { x, y, z } => {
            M44::translate(x.resolve(ctx), y.resolve(ctx), z.resolve(ctx))
        }
        TransformFn::Scale { x, y } => M44::scale(*x, *y, 1.0),
        TransformFn::ScaleX { x } => M44::scale(*x, 1.0, 1.0),
        TransformFn::ScaleY { y } => M44::scale(1.0, *y, 1.0),
        TransformFn::ScaleZ { z } => M44::scale(1.0, 1.0, *z),
        TransformFn::Scale3d { x, y, z } => M44::scale(*x, *y, *z),
        TransformFn::Rotate { deg } | TransformFn::RotateZ { deg } => {
            M44::rotate(V3::new(0.0, 0.0, 1.0), deg.to_radians())
        }
        TransformFn::RotateX { deg } => M44::rotate(V3::new(1.0, 0.0, 0.0), deg.to_radians()),
        TransformFn::RotateY { deg } => M44::rotate(V3::new(0.0, 1.0, 0.0), deg.to_radians()),
        TransformFn::Rotate3d { x, y, z, deg } => {
            M44::rotate(V3::new(*x, *y, *z), deg.to_radians())
        }
        TransformFn::Skew { x, y } => M44::row_major(&[
            1.0, x.to_radians().tan(), 0.0, 0.0,
            y.to_radians().tan(), 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            0.0, 0.0, 0.0, 1.0,
        ]),
        TransformFn::SkewX { x } => M44::row_major(&[
            1.0, x.to_radians().tan(), 0.0, 0.0,
            0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            0.0, 0.0, 0.0, 1.0,
        ]),
        TransformFn::SkewY { y } => M44::row_major(&[
            1.0, 0.0, 0.0, 0.0,
            y.to_radians().tan(), 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0,
            0.0, 0.0, 0.0, 1.0,
        ]),
        TransformFn::Perspective { length } => css_perspective_m44(length.resolve(ctx).max(1.0)),
        TransformFn::Matrix { values: v } => M44::row_major(&[
            v[0], v[2], 0.0, v[4],
            v[1], v[3], 0.0, v[5],
            0.0,  0.0,  1.0, 0.0,
            0.0,  0.0,  0.0, 1.0,
        ]),
        TransformFn::Matrix3d { values: v } => M44::col_major(v),
    }
}

// ---- Background ----

fn paint_background(
    canvas: &Canvas,
    layout: &BoxLayout,
    css: &CssStyle,
    bg: &Background,
    ctx: &LengthContext,
) {
    let radius = css
        .border_radius
        .as_ref()
        .map(|r| resolve_border_radius(r, layout, ctx))
        .unwrap_or([0.0; 4]);
    let rrect = padding_rrect(layout, radius);

    match bg {
        Background::Color(c) => {
            let mut paint = Paint::default();
            paint.set_anti_alias(true);
            paint.set_color(parse_color(c));
            canvas.draw_rrect(&rrect, &paint);
        }
        Background::Single(layer) => paint_bg_layer(canvas, &rrect, layer),
        Background::Layers(layers) => {
            // Painted bottom-up per CSS spec (last layer = bottom).
            for layer in layers.iter().rev() {
                paint_bg_layer(canvas, &rrect, layer);
            }
        }
    }
}

fn paint_bg_layer(canvas: &Canvas, rrect: &RRect, layer: &BackgroundLayer) {
    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    match layer {
        BackgroundLayer::Color { color } => {
            paint.set_color(parse_color(color));
            canvas.draw_rrect(rrect, &paint);
        }
        BackgroundLayer::LinearGradient { angle, stops } => {
            let bounds = rrect.bounds();
            let (p0, p1) = gradient_endpoints(*bounds, angle.unwrap_or(180.0));
            let (colors, positions) = gradient_stops(stops);
            if let Some(shader) = skia_safe::gradient_shader::linear(
                (p0, p1),
                colors.as_slice(),
                positions.as_slice(),
                skia_safe::TileMode::Clamp,
                None,
                None,
            ) {
                paint.set_shader(shader);
                canvas.draw_rrect(rrect, &paint);
            }
        }
        BackgroundLayer::RadialGradient { stops, .. } => {
            let bounds = rrect.bounds();
            let center = Point::new(
                bounds.left + bounds.width() / 2.0,
                bounds.top + bounds.height() / 2.0,
            );
            let radius = bounds.width().max(bounds.height()) / 2.0;
            let (colors, positions) = gradient_stops(stops);
            if let Some(shader) = skia_safe::gradient_shader::radial(
                center,
                radius,
                colors.as_slice(),
                positions.as_slice(),
                skia_safe::TileMode::Clamp,
                None,
                None,
            ) {
                paint.set_shader(shader);
                canvas.draw_rrect(rrect, &paint);
            }
        }
        BackgroundLayer::ConicGradient { stops, .. } => {
            // Skia has SweepGradient = conic.
            let bounds = rrect.bounds();
            let center = Point::new(
                bounds.left + bounds.width() / 2.0,
                bounds.top + bounds.height() / 2.0,
            );
            let (colors, positions) = gradient_stops(stops);
            if let Some(shader) = skia_safe::gradient_shader::sweep(
                center,
                colors.as_slice(),
                positions.as_slice(),
                skia_safe::TileMode::Clamp,
                None,
                None,
                None,
            ) {
                paint.set_shader(shader);
                canvas.draw_rrect(rrect, &paint);
            }
        }
        BackgroundLayer::Image { .. } => {
            // TODO: image background — needs a resource resolver.
        }
    }
}

fn gradient_stops(stops: &[crate::css::style::GradientStop]) -> (Vec<SColor>, Vec<f32>) {
    let mut colors = Vec::with_capacity(stops.len());
    let mut positions = Vec::with_capacity(stops.len());
    let n = stops.len().max(1);
    for (i, s) in stops.iter().enumerate() {
        colors.push(parse_color(&s.color));
        let default_offset = i as f32 / (n.saturating_sub(1).max(1) as f32);
        positions.push(s.offset.unwrap_or(default_offset));
    }
    (colors, positions)
}

fn gradient_endpoints(bounds: Rect, angle_deg: f32) -> (Point, Point) {
    // CSS angle: 0deg = bottom→top, increasing clockwise.
    let cx = bounds.left + bounds.width() / 2.0;
    let cy = bounds.top + bounds.height() / 2.0;
    let rad = (angle_deg - 180.0).to_radians();
    let (sin_a, cos_a) = (rad.sin(), -rad.cos());
    let len = (bounds.width().abs() * sin_a.abs() + bounds.height().abs() * cos_a.abs()) / 2.0;
    let p0 = Point::new(cx - sin_a * len, cy - cos_a * len);
    let p1 = Point::new(cx + sin_a * len, cy + cos_a * len);
    (p0, p1)
}

// ---- Border ----

fn paint_border(
    canvas: &Canvas,
    layout: &BoxLayout,
    css: &CssStyle,
    border: &BorderEdges,
    ctx: &LengthContext,
) {
    // Uniform fast-path: same width on all sides + same color + solid style.
    let style = border.style.unwrap_or(BorderStyle::Solid);
    if matches!(style, BorderStyle::None) {
        return;
    }
    let color = border
        .color
        .as_ref()
        .map(parse_color)
        .unwrap_or(SColor::BLACK);

    // Compute per-side widths (already resolved into BoxLayout.border by taffy).
    let widths = layout.border;
    let max_w = widths.top.max(widths.right).max(widths.bottom).max(widths.left);
    if max_w <= 0.0 {
        return;
    }

    let radius = css
        .border_radius
        .as_ref()
        .map(|r| resolve_border_radius(r, layout, ctx))
        .unwrap_or([0.0; 4]);

    // Outer rrect (border box) and inner rrect (padding box).
    let outer = border_rrect(layout, radius);
    let inner = inner_rrect(layout, radius);

    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    paint.set_style(PaintStyle::Fill);
    paint.set_color(color);

    // Use DRRect = outer minus inner for an accurate stroked border with radius.
    canvas.draw_drrect(&outer, &inner, &paint);
}

fn border_rrect(layout: &BoxLayout, radius: [f32; 4]) -> RRect {
    let rect = Rect::from_xywh(layout.x, layout.y, layout.width, layout.height);
    rrect_from_corners(rect, radius)
}

fn padding_rrect(layout: &BoxLayout, radius: [f32; 4]) -> RRect {
    let (x, y, w, h) = layout.padding_box();
    let rect = Rect::from_xywh(x, y, w, h);
    // Inner radius: max(0, outer_radius - border_width).
    let r = [
        (radius[0] - layout.border.left.max(layout.border.top)).max(0.0),
        (radius[1] - layout.border.right.max(layout.border.top)).max(0.0),
        (radius[2] - layout.border.right.max(layout.border.bottom)).max(0.0),
        (radius[3] - layout.border.left.max(layout.border.bottom)).max(0.0),
    ];
    rrect_from_corners(rect, r)
}

fn inner_rrect(layout: &BoxLayout, radius: [f32; 4]) -> RRect {
    padding_rrect(layout, radius)
}

fn rrect_from_corners(rect: Rect, radius: [f32; 4]) -> RRect {
    // Order: top-left, top-right, bottom-right, bottom-left.
    let radii = [
        Point::new(radius[0], radius[0]),
        Point::new(radius[1], radius[1]),
        Point::new(radius[2], radius[2]),
        Point::new(radius[3], radius[3]),
    ];
    RRect::new_rect_radii(rect, &radii)
}

fn resolve_border_radius(r: &BorderRadius, layout: &BoxLayout, ctx: &LengthContext) -> [f32; 4] {
    let mut local_ctx = *ctx;
    local_ctx.parent_size = layout.width.min(layout.height);
    match r {
        BorderRadius::Uniform(v) => {
            let p = v.resolve(&local_ctx);
            [p, p, p, p]
        }
        BorderRadius::Corners {
            top_left,
            top_right,
            bottom_right,
            bottom_left,
        } => [
            top_left.resolve(&local_ctx),
            top_right.resolve(&local_ctx),
            bottom_right.resolve(&local_ctx),
            bottom_left.resolve(&local_ctx),
        ],
    }
}

// ---- Box-shadow ----

fn paint_box_shadow(
    canvas: &Canvas,
    layout: &BoxLayout,
    css: &CssStyle,
    shadow: &BoxShadow,
    ctx: &LengthContext,
    inset: bool,
) {
    let dx = shadow.offset_x.resolve(ctx);
    let dy = shadow.offset_y.resolve(ctx);
    let blur = shadow.blur.as_ref().map(|b| b.resolve(ctx)).unwrap_or(0.0);
    let spread = shadow.spread.as_ref().map(|b| b.resolve(ctx)).unwrap_or(0.0);
    let color = shadow.color.as_ref().map(parse_color).unwrap_or(SColor::BLACK);

    let radius = css
        .border_radius
        .as_ref()
        .map(|r| resolve_border_radius(r, layout, ctx))
        .unwrap_or([0.0; 4]);

    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    paint.set_color(color);
    if blur > 0.0 {
        if let Some(filter) = skia_safe::MaskFilter::blur(
            skia_safe::BlurStyle::Normal,
            blur / 2.0,
            None,
        ) {
            paint.set_mask_filter(filter);
        }
    }

    if !inset {
        let rect = Rect::from_xywh(
            layout.x + dx - spread,
            layout.y + dy - spread,
            layout.width + spread * 2.0,
            layout.height + spread * 2.0,
        );
        let rrect = rrect_from_corners(rect, radius);
        canvas.draw_rrect(&rrect, &paint);
    } else {
        // Inset: invert — paint the area outside the inner rect within the box.
        // Approximation: draw a stroked rrect inside the padding box.
        let (px, py, pw, ph) = layout.padding_box();
        let outer = rrect_from_corners(Rect::from_xywh(px, py, pw, ph), radius);
        canvas.save();
        canvas.clip_rrect(&outer, ClipOp::Intersect, true);
        let inner_rect = Rect::from_xywh(
            px + dx + spread,
            py + dy + spread,
            (pw - spread * 2.0).max(0.0),
            (ph - spread * 2.0).max(0.0),
        );
        let inner = rrect_from_corners(inner_rect, radius);
        let mut clear = Paint::default();
        clear.set_color(color);
        clear.set_anti_alias(true);
        if blur > 0.0 {
            if let Some(filter) = skia_safe::MaskFilter::blur(
                skia_safe::BlurStyle::Normal,
                blur / 2.0,
                None,
            ) {
                clear.set_mask_filter(filter);
            }
        }
        // Cheap approximation — TODO: proper inset shadow with subtraction path.
        let mut path = Path::new();
        path.add_rrect(&outer, None);
        path.add_rrect(&inner, None);
        path.set_fill_type(skia_safe::PathFillType::EvenOdd);
        canvas.draw_path(&path, &clear);
        canvas.restore();
    }
}

// ---- Color parsing ----

pub fn parse_color(c: &Color) -> SColor {
    match c {
        Color::Rgba { r, g, b, a } => {
            let alpha = (a.clamp(0.0, 1.0) * 255.0) as u8;
            SColor::from_argb(alpha, *r, *g, *b)
        }
        Color::String(s) => parse_color_string(s).unwrap_or(SColor::BLACK),
    }
}

fn parse_color_string(s: &str) -> Option<SColor> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex(hex);
    }
    if let Some(inner) = s.strip_prefix("rgb(").and_then(|s| s.strip_suffix(')')) {
        let parts: Vec<_> = inner.split(',').map(|s| s.trim()).collect();
        if parts.len() == 3 {
            let r = parts[0].parse::<u8>().ok()?;
            let g = parts[1].parse::<u8>().ok()?;
            let b = parts[2].parse::<u8>().ok()?;
            return Some(SColor::from_argb(255, r, g, b));
        }
    }
    if let Some(inner) = s.strip_prefix("rgba(").and_then(|s| s.strip_suffix(')')) {
        let parts: Vec<_> = inner.split(',').map(|s| s.trim()).collect();
        if parts.len() == 4 {
            let r = parts[0].parse::<u8>().ok()?;
            let g = parts[1].parse::<u8>().ok()?;
            let b = parts[2].parse::<u8>().ok()?;
            let a = parts[3].parse::<f32>().ok()?;
            return Some(SColor::from_argb(
                (a.clamp(0.0, 1.0) * 255.0) as u8,
                r,
                g,
                b,
            ));
        }
    }
    match s.to_ascii_lowercase().as_str() {
        "black" => Some(SColor::BLACK),
        "white" => Some(SColor::WHITE),
        "transparent" => Some(SColor::TRANSPARENT),
        "red" => Some(SColor::RED),
        "green" => Some(SColor::GREEN),
        "blue" => Some(SColor::BLUE),
        "yellow" => Some(SColor::YELLOW),
        "magenta" | "fuchsia" => Some(SColor::MAGENTA),
        "cyan" | "aqua" => Some(SColor::CYAN),
        "gray" | "grey" => Some(SColor::GRAY),
        _ => None,
    }
}

fn parse_hex(hex: &str) -> Option<SColor> {
    match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
            Some(SColor::from_argb(255, r, g, b))
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            Some(SColor::from_argb(255, r, g, b))
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            Some(SColor::from_argb(a, r, g, b))
        }
        _ => None,
    }
}

// Suppress unused import warnings for items only used in trait-bound paths.
#[allow(dead_code)]
fn _unused_marker(_e: &Edges, _l: &LengthPercentage, _p: &ParsedLength, _f: Color4f) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hex_3_and_6() {
        let c = parse_color_string("#fff").unwrap();
        assert_eq!(c, SColor::from_argb(255, 255, 255, 255));
        let c = parse_color_string("#102030").unwrap();
        assert_eq!(c, SColor::from_argb(255, 0x10, 0x20, 0x30));
    }

    #[test]
    fn parse_hex_with_alpha() {
        let c = parse_color_string("#80ffffff").unwrap();
        assert_eq!(c.a(), 0xff);
    }

    #[test]
    fn parse_rgb_string() {
        let c = parse_color_string("rgb(10, 20, 30)").unwrap();
        assert_eq!(c, SColor::from_argb(255, 10, 20, 30));
    }

    #[test]
    fn parse_rgba_string() {
        let c = parse_color_string("rgba(10, 20, 30, 0.5)").unwrap();
        assert_eq!(c.r(), 10);
        assert_eq!(c.a(), 127);
    }

    #[test]
    fn parse_named_colors() {
        assert_eq!(parse_color_string("red").unwrap(), SColor::RED);
        assert_eq!(parse_color_string("transparent").unwrap(), SColor::TRANSPARENT);
    }
}
