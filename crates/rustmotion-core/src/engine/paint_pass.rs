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

use std::cell::RefCell;

use skia_safe::{
    canvas::SaveLayerRec, Canvas, ClipOp, Color as SColor, Color4f, Paint, PaintStyle, Path, Point,
    RRect, Rect, M44, V3,
};

use crate::css::style::{
    Background, BackgroundLayer, BorderEdges, BorderRadius, BorderStyle, BoxShadow, Color,
    CssStyle, Edges, Overflow, TransformFn, TransformOrigin,
};
use crate::css::units::{parse_origin_component, LengthContext, LengthPercentage, ParsedLength};
use crate::engine::box_tree::{BoxKind, BoxNode, NodeId};
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
    /// Resolved scene camera for per-plane parallax (issue #90). `Some` only
    /// when the scene declares a camera AND at least one top-level child has
    /// an explicit `style.depth` — the paint pass then applies the camera per
    /// plane (each direct child of the root, scaled by its depth) and the
    /// caller must NOT apply the global camera transform.
    pub camera: Option<PlaneCamera>,
}

/// Scene camera resolved at a fixed time, ready for per-plane application.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaneCamera {
    pub pan_x: f32,
    pub pan_y: f32,
    pub zoom: f32,
    pub rotation: f32,
    /// Focal point in frame pixels (already resolved; default = frame centre).
    pub origin_x: f32,
    pub origin_y: f32,
}

/// Apply the scene camera scaled by a plane `depth` (issue #90):
/// pan' = pan·d, zoom' = 1 + (zoom−1)·d, rotation' = rotation·d, around the
/// camera origin. `depth == 1` reproduces the global camera matrix exactly;
/// `depth == 0` is the identity (locked plane). The content-space viewport
/// clip mirrors the global path's clip-after-camera so plane content is cut
/// at the scene rectangle exactly like the single-transform path.
fn apply_plane_camera(canvas: &Canvas, cam: &PlaneCamera, depth: f32, viewport: (f32, f32)) {
    let zoom = 1.0 + (cam.zoom - 1.0) * depth;
    let rotation = cam.rotation * depth;
    let pan_x = cam.pan_x * depth;
    let pan_y = cam.pan_y * depth;

    canvas.translate(Point::new(cam.origin_x, cam.origin_y));
    if rotation.abs() > 0.001 {
        canvas.rotate(rotation, None);
    }
    if (zoom - 1.0).abs() > 0.001 {
        canvas.scale((zoom, zoom));
    }
    canvas.translate(Point::new(-cam.origin_x - pan_x, -cam.origin_y - pan_y));
    canvas.clip_rect(
        Rect::from_wh(viewport.0, viewport.1),
        ClipOp::Intersect,
        true,
    );
}

/// Axis-aligned bounding box of a painted node, in device (video-pixel) coords.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// One clickable node: its layout id and on-screen bounding box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitNode {
    pub node_id: NodeId,
    pub rect: HitRect,
}

/// Hit-test map for a single painted frame, in paint order (so later entries
/// are visually on top).
pub type HitMap = Vec<HitNode>;

/// A hit enriched with a component kind label, ready for the studio overlay.
/// `node_id` is stable within a single rendered frame.
#[derive(Debug, Clone, PartialEq)]
pub struct EnrichedHit {
    pub node_id: NodeId,
    pub kind: String,
    pub rect: HitRect,
    /// JSON path relative to the scene's `children`, e.g. "/children/2".
    pub pointer: Option<String>,
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
        hits: None,
    };
    paint_node(canvas, root, &ctx, 0);
}

/// Like [`paint_tree`] but also returns the per-frame hit-map: the on-screen
/// bounding box of every component-backed node, in paint order. Used by the
/// studio for click-to-select; the video render path uses [`paint_tree`].
pub fn paint_tree_with_hits(
    canvas: &Canvas,
    root: &BoxNode,
    layout: &LayoutResult,
    frame: &PaintFrame,
    dispatcher: &dyn PaintDispatcher,
) -> HitMap {
    let hits = RefCell::new(Vec::new());
    let ctx = PaintContext {
        layout,
        frame,
        dispatcher,
        viewport_size: (frame.video_width as f32, frame.video_height as f32),
        hits: Some(&hits),
    };
    paint_node(canvas, root, &ctx, 0);
    hits.into_inner()
}

struct PaintContext<'a> {
    layout: &'a LayoutResult,
    frame: &'a PaintFrame,
    dispatcher: &'a dyn PaintDispatcher,
    viewport_size: (f32, f32),
    hits: Option<&'a RefCell<HitMap>>,
}

/// `tree_depth` counts levels below the synthetic scene root (root = 0,
/// direct children = 1). Per-plane parallax cameras apply at level 1 only.
fn paint_node(canvas: &Canvas, node: &BoxNode, ctx: &PaintContext, tree_depth: usize) {
    // Visibility window (start_at/end_at): the node keeps its layout space
    // but paints nothing — subtree included — outside the window.
    if let Some(window) = &node.window {
        if !window.contains(ctx.frame.time) {
            return;
        }
    }
    let Some(box_layout) = ctx.layout.get(node.id) else {
        return;
    };
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

    // 1.5 per-plane scene camera (issue #90): every direct child of the scene
    // root is a plane; its explicit `depth` (default 1.0) scales the camera.
    // Deeper nodes inherit their plane's transform via the canvas matrix.
    if tree_depth == 1 {
        if let Some(cam) = &ctx.frame.camera {
            let depth = node.css.depth.unwrap_or(1.0);
            apply_plane_camera(canvas, cam, depth, ctx.viewport_size);
        }
    }

    // 2. transform
    if node.css.transform.is_some() || node.css.perspective.is_some() {
        let (tx, ty, _tz) =
            resolve_origin(node.css.transform_origin.as_ref(), box_layout, &length_ctx);
        let transform_pivot = (tx, ty);

        let perspective_pivot = if node.css.perspective_origin.is_some() {
            let (px, py, _pz) = resolve_origin(
                node.css.perspective_origin.as_ref(),
                box_layout,
                &length_ctx,
            );
            (px, py)
        } else {
            transform_pivot
        };

        let transform_list = node.css.transform.as_deref().unwrap_or(&[]);
        let perspective_d = node
            .css
            .perspective
            .as_ref()
            .map(|l| l.resolve(&length_ctx).max(1.0));
        apply_transform(
            canvas,
            transform_list,
            perspective_d,
            transform_pivot,
            perspective_pivot,
            &length_ctx,
        );
    }

    // Hit-map: record the on-screen bbox of component-backed nodes. The canvas
    // matrix here already includes this node's and all ancestors' transforms,
    // so mapping the (absolute) layout rect yields the device-space AABB.
    // Ghost nodes are intentionally excluded — they are temporal echoes for
    // motion-blur/trail effects and must never be selectable in the studio.
    if let (Some(hits), BoxKind::Component(_)) = (ctx.hits, &node.kind) {
        let local = Rect::from_xywh(
            box_layout.x,
            box_layout.y,
            box_layout.width,
            box_layout.height,
        );
        let dev = canvas.local_to_device_as_3x3().map_rect(local).0;
        hits.borrow_mut().push(HitNode {
            node_id: node.id,
            rect: HitRect {
                x: dev.left,
                y: dev.top,
                w: dev.width(),
                h: dev.height(),
            },
        });
    }

    // 3. opacity / filter layer — one shared layer carries both the group
    // alpha and the CSS `filter` chain (applies to the node and its subtree).
    let opacity = node.css.opacity.unwrap_or(1.0).clamp(0.0, 1.0);
    let content_filter = node
        .css
        .filter
        .as_deref()
        .and_then(|list| filters_to_image_filter(list, &length_ctx));
    let opened_opacity_layer = if opacity < 1.0 || content_filter.is_some() {
        let mut paint = Paint::default();
        if opacity < 1.0 {
            paint.set_alpha((opacity * 255.0) as u8);
        }
        if let Some(filter) = content_filter {
            paint.set_image_filter(filter);
        }
        let rec = SaveLayerRec::default().paint(&paint);
        canvas.save_layer(&rec);
        true
    } else {
        false
    };

    // 4. clip overflow:hidden / clip
    let overflow = node.css.overflow.unwrap_or(Overflow::Visible);
    if matches!(
        overflow,
        Overflow::Hidden | Overflow::Clip | Overflow::Scroll | Overflow::Auto
    ) {
        let radius = node
            .css
            .border_radius
            .as_ref()
            .map(|r| resolve_border_radius(r, box_layout, &length_ctx))
            .unwrap_or([0.0; 4]);
        let rrect = padding_rrect(box_layout, radius);
        canvas.clip_rrect(rrect, ClipOp::Intersect, true);
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

    // 5.5 backdrop-filter: filter what is already painted behind this node,
    // clipped to its (rounded) border-box, before its own background goes on
    // top — the glassmorphism pattern.
    if let Some(filters) = node.css.backdrop_filter.as_deref() {
        if let Some(backdrop) = filters_to_image_filter(filters, &length_ctx) {
            let radius = node
                .css
                .border_radius
                .as_ref()
                .map(|r| resolve_border_radius(r, box_layout, &length_ctx))
                .unwrap_or([0.0; 4]);
            canvas.save();
            canvas.clip_rrect(border_rrect(box_layout, radius), ClipOp::Intersect, true);
            let rec = SaveLayerRec::default().backdrop(&backdrop);
            canvas.save_layer(&rec);
            canvas.restore();
            canvas.restore();
        }
    }

    // 6. background
    if let Some(bg) = node.css.background.as_ref() {
        paint_background(canvas, box_layout, &node.css, bg, &length_ctx);
    }

    // 7. border — `gradient-border` replaces the standard border when present
    // (a box has one border, not two stacked ones).
    if let Some(gb) = node.css.gradient_border.as_ref() {
        paint_gradient_border(canvas, box_layout, &node.css, gb, &length_ctx);
    } else if let Some(border) = node.css.border.as_ref() {
        paint_border(canvas, box_layout, &node.css, border, &length_ctx);
    }

    // 8. component-specific content (Ghost is painted identically to Component;
    // the only difference is that Ghost is excluded from the hit-map above).
    let payload_opt = match &node.kind {
        BoxKind::Component(p) | BoxKind::Ghost(p) => Some(p),
        BoxKind::Container => None,
    };
    if let Some(payload) = payload_opt {
        ctx.dispatcher
            .dispatch(canvas, payload.as_ref(), &node.css, box_layout, ctx.frame);
    }

    // 9. children (z-index ordered, then source order)
    let mut indices: Vec<usize> = (0..node.children.len()).collect();
    indices.sort_by_key(|&i| node.children[i].css.z_index.unwrap_or(0));
    for &i in &indices {
        paint_node(canvas, &node.children[i], ctx, tree_depth + 1);
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

// ---- CSS filters ----

/// Build a Skia `ImageFilter` chain from a CSS `filter`/`backdrop-filter`
/// list. Color functions use the CSS Filter Effects spec matrices; Skia's
/// `color_filters::matrix_row_major` expects the translation column in
/// normalized 0..1 space (verified by `css_filter_invert_flips_colors`).
fn filters_to_image_filter(
    list: &[crate::css::style::FilterFn],
    ctx: &LengthContext,
) -> Option<skia_safe::ImageFilter> {
    use crate::css::style::FilterFn;
    use skia_safe::image_filters;

    let mut chain: Option<skia_safe::ImageFilter> = None;
    for f in list {
        chain = match f {
            FilterFn::Blur { radius } => {
                let r = radius.resolve(ctx).max(0.0);
                if r <= 0.0 {
                    chain
                } else {
                    image_filters::blur((r / 2.0, r / 2.0), skia_safe::TileMode::Clamp, chain, None)
                }
            }
            FilterFn::DropShadow {
                offset_x,
                offset_y,
                blur,
                color,
            } => {
                let sigma = blur.as_ref().map(|b| b.resolve(ctx) / 2.0).unwrap_or(0.0);
                let c = color.as_ref().map(parse_color).unwrap_or(SColor::BLACK);
                image_filters::drop_shadow(
                    (offset_x.resolve(ctx), offset_y.resolve(ctx)),
                    (sigma, sigma),
                    c,
                    None,
                    chain,
                    None,
                )
            }
            FilterFn::Noise { intensity, seed } => {
                match noise_image_filter(*intensity, *seed) {
                    // Sequential CSS composition: the chain so far is the
                    // background, the grain layer blends on top of it.
                    Some(noise) => image_filters::blend(
                        skia_safe::BlendMode::Overlay,
                        chain,
                        Some(noise),
                        None,
                    ),
                    None => chain,
                }
            }
            other => color_matrix_for(other)
                .map(|m| skia_safe::color_filters::matrix_row_major(&m, None))
                .and_then(|cf| image_filters::color_filter(cf, chain, None)),
        };
    }
    chain
}

/// Build the film-grain layer for `FilterFn::Noise` as an `ImageFilter`.
///
/// Composition formula:
///   1. `fractal_noise(base_frequency = (0.9, 0.9), octaves = 2, seed)` —
///      high frequency ⇒ fine per-pixel grain; the Skia Perlin shader is a
///      pure function of (x, y, seed): no implicit time, so the grain is
///      byte-identical across frames/renders for a given seed.
///   2. A 4×5 color matrix collapses RGB to luminance (0.213/0.715/0.072) for
///      monochrome grain and scales alpha by `intensity` (0..1).
///   3. The caller blends the result over the chain input with
///      `BlendMode::Overlay` — grain brightens/darkens the underlying pixels
///      proportionally to the noise-layer alpha (`intensity`).
fn noise_image_filter(intensity: f32, seed: u64) -> Option<skia_safe::ImageFilter> {
    use skia_safe::image_filters;

    let intensity = intensity.clamp(0.0, 1.0);
    if intensity <= 0.0 {
        return None;
    }
    let noise = skia_safe::shaders::fractal_noise((0.9, 0.9), 2, seed as f32, None)?;
    // Luminance conversion + alpha scaling in one matrix.
    let (r, g, b) = (0.213, 0.715, 0.072);
    #[rustfmt::skip]
    let m = [
        r,   g,   b,   0.0,       0.0,
        r,   g,   b,   0.0,       0.0,
        r,   g,   b,   0.0,       0.0,
        0.0, 0.0, 0.0, intensity, 0.0,
    ];
    let cf = skia_safe::color_filters::matrix_row_major(&m, None);
    let mono = noise.with_color_filter(cf);
    image_filters::shader(mono, None)
}

/// 4x5 row-major color matrix for a CSS color filter function (translation
/// column in normalized 0..1 space), or `None` for the non-matrix functions.
fn color_matrix_for(f: &crate::css::style::FilterFn) -> Option<[f32; 20]> {
    use crate::css::style::FilterFn;
    #[rustfmt::skip]
    fn saturation(s: f32) -> [f32; 20] {
        // Luminance weights per the CSS Filter Effects spec.
        let (r, g, b) = (0.213, 0.715, 0.072);
        [
            r + (1.0 - r) * s, g * (1.0 - s),       b * (1.0 - s),       0.0, 0.0,
            r * (1.0 - s),     g + (1.0 - g) * s,   b * (1.0 - s),       0.0, 0.0,
            r * (1.0 - s),     g * (1.0 - s),       b + (1.0 - b) * s,   0.0, 0.0,
            0.0,               0.0,                 0.0,                 1.0, 0.0,
        ]
    }
    match f {
        FilterFn::Brightness { value } => {
            let v = value.max(0.0);
            #[rustfmt::skip]
            let m = [
                v, 0.0, 0.0, 0.0, 0.0,
                0.0, v, 0.0, 0.0, 0.0,
                0.0, 0.0, v, 0.0, 0.0,
                0.0, 0.0, 0.0, 1.0, 0.0,
            ];
            Some(m)
        }
        FilterFn::Contrast { value } => {
            let v = value.max(0.0);
            let t = (1.0 - v) / 2.0;
            #[rustfmt::skip]
            let m = [
                v, 0.0, 0.0, 0.0, t,
                0.0, v, 0.0, 0.0, t,
                0.0, 0.0, v, 0.0, t,
                0.0, 0.0, 0.0, 1.0, 0.0,
            ];
            Some(m)
        }
        FilterFn::Saturate { value } => Some(saturation(value.max(0.0))),
        FilterFn::Grayscale { value } => Some(saturation(1.0 - value.clamp(0.0, 1.0))),
        FilterFn::HueRotate { deg } => {
            let (sin, cos) = deg.to_radians().sin_cos();
            let (r, g, b) = (0.213, 0.715, 0.072);
            #[rustfmt::skip]
            let m = [
                r + cos * (1.0 - r) + sin * (-r),      g + cos * (-g) + sin * (-g),       b + cos * (-b) + sin * (1.0 - b), 0.0, 0.0,
                r + cos * (-r) + sin * 0.143,          g + cos * (1.0 - g) + sin * 0.140, b + cos * (-b) + sin * (-0.283),  0.0, 0.0,
                r + cos * (-r) + sin * (-(1.0 - r)),   g + cos * (-g) + sin * g,          b + cos * (1.0 - b) + sin * b,    0.0, 0.0,
                0.0,                                   0.0,                               0.0,                              1.0, 0.0,
            ];
            Some(m)
        }
        FilterFn::Invert { value } => {
            let v = value.clamp(0.0, 1.0);
            let s = 1.0 - 2.0 * v;
            let t = v;
            #[rustfmt::skip]
            let m = [
                s, 0.0, 0.0, 0.0, t,
                0.0, s, 0.0, 0.0, t,
                0.0, 0.0, s, 0.0, t,
                0.0, 0.0, 0.0, 1.0, 0.0,
            ];
            Some(m)
        }
        FilterFn::Sepia { value } => {
            let v = value.clamp(0.0, 1.0);
            let lerp = |a: f32, b: f32| a + (b - a) * v;
            #[rustfmt::skip]
            let m = [
                lerp(1.0, 0.393), lerp(0.0, 0.769), lerp(0.0, 0.189), 0.0, 0.0,
                lerp(0.0, 0.349), lerp(1.0, 0.686), lerp(0.0, 0.168), 0.0, 0.0,
                lerp(0.0, 0.272), lerp(0.0, 0.534), lerp(1.0, 0.131), 0.0, 0.0,
                0.0,              0.0,              0.0,              1.0, 0.0,
            ];
            Some(m)
        }
        FilterFn::Opacity { value } => {
            let v = value.clamp(0.0, 1.0);
            #[rustfmt::skip]
            let m = [
                1.0, 0.0, 0.0, 0.0, 0.0,
                0.0, 1.0, 0.0, 0.0, 0.0,
                0.0, 0.0, 1.0, 0.0, 0.0,
                0.0, 0.0, 0.0, v, 0.0,
            ];
            Some(m)
        }
        FilterFn::Blur { .. } | FilterFn::DropShadow { .. } | FilterFn::Noise { .. } => None,
    }
}

// ---- Transform ----

/// Resolve a `transform-origin` / `perspective-origin` value to absolute
/// viewport coordinates `(x, y)`.
///
/// # Resolution rules
///
/// - Absent `origin` → centre of the box (the pre-existing hard-coded behaviour,
///   preserved byte-identically).
/// - `x` / `y` values follow the CSS spec: percentages are relative to the box
///   **width** (for x) and **height** (for y); px values are relative to the
///   box top-left corner. The result is in absolute viewport coordinates.
/// - Keywords (`left`, `center`, `right`, `top`, `bottom`) are handled by
///   `parse_origin_component` which maps them to 0%/50%/100%.
/// - An absent component (`None`) defaults to 50% on that axis.
/// - The `z` component is returned as `f32` for use in the 3-D path (defaults
///   to 0.0 when absent).
fn resolve_origin(
    origin: Option<&TransformOrigin>,
    layout: &BoxLayout,
    ctx: &LengthContext,
) -> (f32, f32, f32) {
    let Some(o) = origin else {
        // Default: 50% 50% 0 — dead-centre of the box.
        return (
            layout.x + layout.width / 2.0,
            layout.y + layout.height / 2.0,
            0.0,
        );
    };

    // Resolve x against box width.
    let ox = if let Some(lp) = &o.x {
        let parsed = match lp {
            LengthPercentage::String(s) => {
                parse_origin_component(s).unwrap_or(crate::css::units::ParsedLength::Percent(50.0))
            }
            LengthPercentage::Px(v) => crate::css::units::ParsedLength::Px(*v),
        };
        let local_ctx = LengthContext {
            parent_size: layout.width,
            ..*ctx
        };
        layout.x + parsed.resolve(&local_ctx).unwrap_or(layout.width / 2.0)
    } else {
        layout.x + layout.width / 2.0
    };

    // Resolve y against box height.
    let oy = if let Some(lp) = &o.y {
        let parsed = match lp {
            LengthPercentage::String(s) => {
                parse_origin_component(s).unwrap_or(crate::css::units::ParsedLength::Percent(50.0))
            }
            LengthPercentage::Px(v) => crate::css::units::ParsedLength::Px(*v),
        };
        let local_ctx = LengthContext {
            parent_size: layout.height,
            ..*ctx
        };
        layout.y + parsed.resolve(&local_ctx).unwrap_or(layout.height / 2.0)
    } else {
        layout.y + layout.height / 2.0
    };

    // Resolve z (optional; only used in 3D path).
    let oz = o.z.as_ref().map(|l| l.resolve(ctx)).unwrap_or(0.0);

    (ox, oy, oz)
}

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

/// Apply CSS transform + perspective to the canvas.
///
/// # Parameters
///
/// - `transform_pivot`: the origin for the `transform` property (resolved from
///   `transform-origin`, defaults to box centre).
/// - `perspective_pivot`: the origin for the perspective projection (resolved
///   from `perspective-origin`). When `perspective-origin` is absent this equals
///   `transform_pivot` and we use the cheaper single-translate path. When they
///   differ we bracket the perspective matrix with its own translate pair.
fn apply_transform(
    canvas: &Canvas,
    list: &[TransformFn],
    perspective_d: Option<f32>,
    transform_pivot: (f32, f32),
    perspective_pivot: (f32, f32),
    ctx: &LengthContext,
) {
    // Detect whether perspective and transform pivots differ.
    let pivots_equal = (transform_pivot.0 - perspective_pivot.0).abs() < 0.001
        && (transform_pivot.1 - perspective_pivot.1).abs() < 0.001;

    if perspective_d.is_none() && !has_3d_transform(list) {
        // Fast path: 2D-only, use the native Skia 2D canvas API.
        let pivot = transform_pivot;
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
    } else if pivots_equal {
        // 3D path, single-pivot (fast): perspective + transforms share the same pivot.
        // Structure: T(pivot) · Persp · Transforms · T(-pivot)
        let pivot = transform_pivot;
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
    } else {
        // 3D path, dual-pivot: perspective-origin ≠ transform-origin.
        // Structure: T(pp) · Persp · T(-pp) · T(tp) · Transforms · T(-tp)
        //
        // This matches the CSS spec where `perspective-origin` shifts the
        // vanishing point while `transform-origin` sets the local pivot.
        let tp = transform_pivot;
        let pp = perspective_pivot;
        let mut m = M44::new_identity();

        // Outer perspective bracket (perspective-origin).
        m.pre_concat(&M44::translate(pp.0, pp.1, 0.0));
        if let Some(d) = perspective_d {
            m.pre_concat(&css_perspective_m44(d));
        }
        m.pre_concat(&M44::translate(-pp.0, -pp.1, 0.0));

        // Inner transform bracket (transform-origin).
        m.pre_concat(&M44::translate(tp.0, tp.1, 0.0));
        for tr in list {
            m.pre_concat(&transform_to_m44(tr, ctx));
        }
        m.pre_concat(&M44::translate(-tp.0, -tp.1, 0.0));

        canvas.concat_44(&m);
    }
}

/// CSS `perspective(d)` projection matrix in row-major form.
/// Maps (x, y, z, 1) → w' = 1 - z/d; perspective divide yields depth scaling.
fn css_perspective_m44(d: f32) -> M44 {
    M44::row_major(&[
        1.0,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
        0.0,
        0.0,
        -1.0 / d,
        1.0,
    ])
}

fn transform_to_m44(tr: &TransformFn, ctx: &LengthContext) -> M44 {
    match tr {
        TransformFn::Translate { x, y } => M44::translate(x.resolve(ctx), y.resolve(ctx), 0.0),
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
            1.0,
            x.to_radians().tan(),
            0.0,
            0.0,
            y.to_radians().tan(),
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
        ]),
        TransformFn::SkewX { x } => M44::row_major(&[
            1.0,
            x.to_radians().tan(),
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
        ]),
        TransformFn::SkewY { y } => M44::row_major(&[
            1.0,
            0.0,
            0.0,
            0.0,
            y.to_radians().tan(),
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
            0.0,
            0.0,
            0.0,
            0.0,
            1.0,
        ]),
        TransformFn::Perspective { length } => css_perspective_m44(length.resolve(ctx).max(1.0)),
        TransformFn::Matrix { values: v } => M44::row_major(&[
            v[0], v[2], 0.0, v[4], v[1], v[3], 0.0, v[5], 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
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
            canvas.draw_rrect(rrect, &paint);
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
    let max_w = widths
        .top
        .max(widths.right)
        .max(widths.bottom)
        .max(widths.left);
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
    canvas.draw_drrect(outer, inner, &paint);
}

/// Paint a gradient-colored border ring (issue #87).
///
/// Painted **instead of** the standard `border` when both are set. Unlike
/// `border`, `gradient-border` is a pure paint decoration: it does not consume
/// layout space (taffy never sees it), the ring is inset from the border-box
/// edge by `gb.width`.
///
/// The gradient is linear along `gb.angle` with the **same angle convention as
/// `background` linear gradients** (see [`gradient_endpoints`]) so the two
/// stay visually consistent within one style block. Colors are evenly spaced.
fn paint_gradient_border(
    canvas: &Canvas,
    layout: &BoxLayout,
    css: &CssStyle,
    gb: &crate::schema::GradientBorder,
    ctx: &LengthContext,
) {
    if gb.colors.len() < 2 || gb.width <= 0.0 {
        return;
    }
    let width = gb.width.min(layout.width / 2.0).min(layout.height / 2.0);

    let radius = css
        .border_radius
        .as_ref()
        .map(|r| resolve_border_radius(r, layout, ctx))
        .unwrap_or([0.0; 4]);

    // Outer ring edge = border box; inner edge = inset by the border width.
    let outer = border_rrect(layout, radius);
    let inner_rect = Rect::from_xywh(
        layout.x + width,
        layout.y + width,
        (layout.width - width * 2.0).max(0.0),
        (layout.height - width * 2.0).max(0.0),
    );
    let inner_radius = [
        (radius[0] - width).max(0.0),
        (radius[1] - width).max(0.0),
        (radius[2] - width).max(0.0),
        (radius[3] - width).max(0.0),
    ];
    let inner = rrect_from_corners(inner_rect, inner_radius);

    let colors: Vec<SColor> = gb
        .colors
        .iter()
        .map(|c| parse_color_string(c).unwrap_or_else(|| unresolved_color(c)))
        .collect();
    let n = colors.len();
    let positions: Vec<f32> = (0..n)
        .map(|i| i as f32 / (n.saturating_sub(1).max(1) as f32))
        .collect();

    let bounds = outer.bounds();
    let (p0, p1) = gradient_endpoints(*bounds, gb.angle);
    let Some(shader) = skia_safe::gradient_shader::linear(
        (p0, p1),
        colors.as_slice(),
        positions.as_slice(),
        skia_safe::TileMode::Clamp,
        None,
        None,
    ) else {
        return;
    };

    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    paint.set_style(PaintStyle::Fill);
    paint.set_shader(shader);
    canvas.draw_drrect(outer, inner, &paint);
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
    let spread = shadow
        .spread
        .as_ref()
        .map(|b| b.resolve(ctx))
        .unwrap_or(0.0);
    let color = shadow
        .color
        .as_ref()
        .map(parse_color)
        .unwrap_or(SColor::BLACK);

    let radius = css
        .border_radius
        .as_ref()
        .map(|r| resolve_border_radius(r, layout, ctx))
        .unwrap_or([0.0; 4]);

    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    paint.set_color(color);
    if blur > 0.0 {
        if let Some(filter) =
            skia_safe::MaskFilter::blur(skia_safe::BlurStyle::Normal, blur / 2.0, None)
        {
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
        canvas.draw_rrect(rrect, &paint);
    } else {
        // Inset: invert — paint the area outside the inner rect within the box.
        // Approximation: draw a stroked rrect inside the padding box.
        let (px, py, pw, ph) = layout.padding_box();
        let outer = rrect_from_corners(Rect::from_xywh(px, py, pw, ph), radius);
        canvas.save();
        canvas.clip_rrect(outer, ClipOp::Intersect, true);
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
            if let Some(filter) =
                skia_safe::MaskFilter::blur(skia_safe::BlurStyle::Normal, blur / 2.0, None)
            {
                clear.set_mask_filter(filter);
            }
        }
        // Cheap approximation — TODO: proper inset shadow with subtraction path.
        let mut path = Path::new();
        path.add_rrect(outer, None);
        path.add_rrect(inner, None);
        path.set_fill_type(skia_safe::PathFillType::EvenOdd);
        canvas.draw_path(&path, &clear);
        canvas.restore();
    }
}

// ---- Color parsing ----
//
// Both functions below route through `renderer::parse_css_color` — the
// single frozen entry point (see `renderer/colors.rs`) — rather than
// duplicating hex/rgb/hsl/named-colour parsing here. That parser accepts
// every common CSS colour form (3/4/6/8-digit hex, rgb()/rgba(), hsl()/
// hsla(), the full CSS named-colour set) and returns `None` for anything
// else.
//
// A `Color::String` that fails to parse is a real authoring bug — the
// previous behaviour (`unwrap_or(SColor::BLACK)`) rendered it as invisible
// text on this tool's dark-background target style. It now falls back to
// `renderer::UNRESOLVED_COLOR` (opaque magenta) and logs a warning instead,
// so the failure is visible on screen and in the render logs. Wiring a
// hard validation-time error for this is out of scope here (sibling
// workstream); this only stops the render path from lying.

pub fn parse_color(c: &Color) -> SColor {
    match c {
        Color::Rgba { r, g, b, a } => {
            let alpha = (a.clamp(0.0, 1.0) * 255.0) as u8;
            SColor::from_argb(alpha, *r, *g, *b)
        }
        Color::String(s) => parse_color_string(s).unwrap_or_else(|| unresolved_color(s)),
    }
}

fn parse_color_string(s: &str) -> Option<SColor> {
    crate::engine::renderer::parse_css_color(s).map(|(r, g, b, a)| SColor::from_argb(a, r, g, b))
}

/// Loud, non-black fallback for a colour string that couldn't be resolved.
/// See the module note above `parse_color`.
fn unresolved_color(original: &str) -> SColor {
    eprintln!(
        "Warning: unrecognized color '{original}' — rendering as opaque magenta instead of \
         silently falling back to black"
    );
    let (r, g, b, a) = crate::engine::renderer::UNRESOLVED_COLOR;
    SColor::from_argb(a, r, g, b)
}

// Suppress unused import warnings for items only used in trait-bound paths.
#[allow(dead_code)]
fn _unused_marker(_e: &Edges, _l: &LengthPercentage, _p: &ParsedLength, _f: Color4f) {}

#[cfg(test)]
mod hit_tests {
    use super::*;
    use std::sync::Arc;

    use crate::css::style::{CssStyle, Display, FlexDirection, Position, Size as CSize};
    use crate::css::taffy_bridge::ConversionContext;
    use crate::css::units::LengthPercentage as CLP;
    use crate::engine::box_tree::{BoxKind, BoxNode};
    use crate::engine::layout_pass::run_layout;

    fn test_frame(w: u32, h: u32) -> PaintFrame {
        PaintFrame {
            time: 0.0,
            frame_index: 0,
            fps: 30,
            video_width: w,
            video_height: h,
            scene_duration: 1.0,
            camera: None,
        }
    }

    #[test]
    fn hitmap_reports_component_rect_for_untransformed_node() {
        // A component leaf, absolutely positioned at (40, 30), sized 100x80,
        // inside a flex-column root that fills a 400x400 viewport.
        let leaf = BoxNode {
            id: 0,
            kind: BoxKind::Component(Arc::new(1u32)),
            css: CssStyle {
                position: Some(Position::Absolute),
                left: Some(CLP::Px(40.0)),
                top: Some(CLP::Px(30.0)),
                width: Some(CSize::Length(CLP::Px(100.0))),
                height: Some(CSize::Length(CLP::Px(80.0))),
                ..Default::default()
            },
            children: vec![],
            intrinsic: None,
            source_path: None,
            window: None,
        };
        let mut root = BoxNode {
            id: 0,
            kind: BoxKind::Container,
            css: CssStyle {
                display: Some(Display::Flex),
                flex_direction: Some(FlexDirection::Column),
                width: Some(CSize::Length(CLP::Px(400.0))),
                height: Some(CSize::Length(CLP::Px(400.0))),
                ..Default::default()
            },
            children: vec![leaf],
            intrinsic: None,
            source_path: None,
            window: None,
        };
        root.assign_ids(0);

        let layout = run_layout(&root, (400.0, 400.0), &ConversionContext::default());
        let mut surface = skia_safe::surfaces::raster_n32_premul((400, 400)).unwrap();
        let hits = paint_tree_with_hits(
            surface.canvas(),
            &root,
            &layout,
            &test_frame(400, 400),
            &NoopDispatcher,
        );

        // Only the component leaf is reported, not the synthetic container root.
        assert_eq!(hits.len(), 1, "expected exactly one component hit");
        let h = &hits[0];
        assert_eq!(h.node_id, root.children[0].id);
        assert!((h.rect.x - 40.0).abs() < 0.5, "x = {}", h.rect.x);
        assert!((h.rect.y - 30.0).abs() < 0.5, "y = {}", h.rect.y);
        assert!((h.rect.w - 100.0).abs() < 0.5, "w = {}", h.rect.w);
        assert!((h.rect.h - 80.0).abs() < 0.5, "h = {}", h.rect.h);
    }

    #[test]
    fn backdrop_filter_blurs_content_behind() {
        use crate::css::style::{Background, Color as CssColor, FilterFn};
        use crate::css::units::Length;

        // Top half black on a white root; a backdrop-blur panel straddles
        // the boundary. Inside the panel the boundary must smear into greys;
        // outside it stays a hard black/white edge.
        let black_top = BoxNode {
            id: 0,
            kind: BoxKind::Container,
            css: CssStyle {
                position: Some(Position::Absolute),
                left: Some(CLP::Px(0.0)),
                top: Some(CLP::Px(0.0)),
                width: Some(CSize::Length(CLP::Px(200.0))),
                height: Some(CSize::Length(CLP::Px(100.0))),
                background: Some(Background::Color(CssColor::String("#000000".into()))),
                ..Default::default()
            },
            children: vec![],
            intrinsic: None,
            source_path: None,
            window: None,
        };
        let panel = BoxNode {
            id: 0,
            kind: BoxKind::Container,
            css: CssStyle {
                position: Some(Position::Absolute),
                left: Some(CLP::Px(50.0)),
                top: Some(CLP::Px(50.0)),
                width: Some(CSize::Length(CLP::Px(100.0))),
                height: Some(CSize::Length(CLP::Px(100.0))),
                backdrop_filter: Some(vec![FilterFn::Blur {
                    radius: Length::Px(10.0),
                }]),
                ..Default::default()
            },
            children: vec![],
            intrinsic: None,
            source_path: None,
            window: None,
        };
        let mut root = BoxNode {
            id: 0,
            kind: BoxKind::Container,
            css: CssStyle {
                display: Some(Display::Flex),
                width: Some(CSize::Length(CLP::Px(200.0))),
                height: Some(CSize::Length(CLP::Px(200.0))),
                background: Some(Background::Color(CssColor::String("#ffffff".into()))),
                ..Default::default()
            },
            children: vec![black_top, panel],
            intrinsic: None,
            source_path: None,
            window: None,
        };
        root.assign_ids(0);

        let layout = run_layout(&root, (200.0, 200.0), &ConversionContext::default());
        let mut surface = skia_safe::surfaces::raster_n32_premul((200, 200)).unwrap();
        paint_tree(
            surface.canvas(),
            &root,
            &layout,
            &test_frame(200, 200),
            &NoopDispatcher,
        );

        let info = skia_safe::ImageInfo::new(
            (200, 200),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Unpremul,
            None,
        );
        let mut buf = vec![0u8; 200 * 200 * 4];
        assert!(surface.read_pixels(&info, &mut buf, 200 * 4, (0, 0)));
        let red = |x: usize, y: usize| buf[(y * 200 + x) * 4] as i32;

        // Outside the panel: hard edge preserved.
        assert!(red(10, 97) < 10, "outside/above must stay black");
        assert!(red(10, 103) > 245, "outside/below must stay white");
        // Inside the panel: boundary smeared to intermediate greys.
        let above = red(100, 97);
        let below = red(100, 103);
        assert!(
            above > 30,
            "backdrop not blurred above boundary (r={above})"
        );
        assert!(
            below < 225,
            "backdrop not blurred below boundary (r={below})"
        );
    }

    #[test]
    fn hitmap_reflects_node_transform() {
        use crate::css::style::TransformFn;

        // Same leaf as the untransformed test, but with transform: scale(2).
        // The engine applies transforms around the node's center, so a 100x80
        // box scaled 2x grows to 200x160 (centered on the same point).
        let leaf = BoxNode {
            id: 0,
            kind: BoxKind::Component(Arc::new(1u32)),
            css: CssStyle {
                position: Some(Position::Absolute),
                left: Some(CLP::Px(40.0)),
                top: Some(CLP::Px(30.0)),
                width: Some(CSize::Length(CLP::Px(100.0))),
                height: Some(CSize::Length(CLP::Px(80.0))),
                transform: Some(vec![TransformFn::Scale { x: 2.0, y: 2.0 }]),
                ..Default::default()
            },
            children: vec![],
            intrinsic: None,
            source_path: None,
            window: None,
        };
        let mut root = BoxNode {
            id: 0,
            kind: BoxKind::Container,
            css: CssStyle {
                display: Some(Display::Flex),
                flex_direction: Some(FlexDirection::Column),
                width: Some(CSize::Length(CLP::Px(400.0))),
                height: Some(CSize::Length(CLP::Px(400.0))),
                ..Default::default()
            },
            children: vec![leaf],
            intrinsic: None,
            source_path: None,
            window: None,
        };
        root.assign_ids(0);

        let layout = run_layout(&root, (400.0, 400.0), &ConversionContext::default());
        let mut surface = skia_safe::surfaces::raster_n32_premul((400, 400)).unwrap();
        let hits = paint_tree_with_hits(
            surface.canvas(),
            &root,
            &layout,
            &test_frame(400, 400),
            &NoopDispatcher,
        );

        assert_eq!(hits.len(), 1);
        let h = &hits[0];
        // Scaled 2x: width 100->200, height 80->160. Assert the dimensions
        // (these prove the canvas transform is reflected in the hit rect).
        assert!((h.rect.w - 200.0).abs() < 1.0, "w = {}", h.rect.w);
        assert!((h.rect.h - 160.0).abs() < 1.0, "h = {}", h.rect.h);
    }
}

#[cfg(test)]
mod transform_origin_tests {
    //! TDD tests for `resolve_origin` and the pivot integration in `paint_node`.
    //!
    //! Strategy: paint a coloured rect, read back pixel centroids / columns, and
    //! verify that the transformed position matches the expected pivot behaviour.

    use super::*;

    use crate::css::style::{
        Background, Color as CssColor, CssStyle, Display, FlexDirection, Position, Size as CSize,
        TransformFn, TransformOrigin,
    };
    use crate::css::taffy_bridge::ConversionContext;
    use crate::css::units::LengthPercentage as CLP;
    use crate::engine::box_tree::{BoxKind, BoxNode};
    use crate::engine::layout_pass::run_layout;

    fn test_frame(w: u32, h: u32) -> PaintFrame {
        PaintFrame {
            time: 0.0,
            frame_index: 0,
            fps: 30,
            video_width: w,
            video_height: h,
            scene_duration: 1.0,
            camera: None,
        }
    }

    /// Render a tree and return the pixel buffer (RGBA8888).
    fn render_pixels(root: &mut BoxNode, w: u32, h: u32) -> Vec<u8> {
        root.assign_ids(0);
        let layout = run_layout(root, (w as f32, h as f32), &ConversionContext::default());
        let mut surface = skia_safe::surfaces::raster_n32_premul((w as i32, h as i32)).unwrap();
        paint_tree(
            surface.canvas(),
            root,
            &layout,
            &test_frame(w, h),
            &NoopDispatcher,
        );
        let info = skia_safe::ImageInfo::new(
            (w as i32, h as i32),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Unpremul,
            None,
        );
        let mut buf = vec![0u8; (w * h * 4) as usize];
        surface.read_pixels(&info, &mut buf, (w * 4) as usize, (0, 0));
        buf
    }

    /// Red channel at pixel (x, y) in a w-wide buffer.
    fn r(buf: &[u8], w: u32, x: u32, y: u32) -> u8 {
        buf[((y * w + x) * 4) as usize]
    }

    /// Count pixels in `buf` where red > 200 (i.e., predominantly red).
    fn count_red(buf: &[u8]) -> usize {
        buf.chunks(4)
            .filter(|px| px[0] > 200 && px[1] < 50 && px[2] < 50)
            .count()
    }

    /// Compute column centroid (weighted x) of pixels where red > 200.
    fn red_centroid_x(buf: &[u8], w: u32, h: u32) -> f32 {
        let mut sum_x = 0.0f64;
        let mut count = 0.0f64;
        for y in 0..h {
            for x in 0..w {
                if r(buf, w, x, y) > 200
                    && buf[((y * w + x) * 4 + 1) as usize] < 50
                    && buf[((y * w + x) * 4 + 2) as usize] < 50
                {
                    sum_x += x as f64;
                    count += 1.0;
                }
            }
        }
        if count == 0.0 {
            0.0
        } else {
            (sum_x / count) as f32
        }
    }

    /// Compute row centroid (weighted y) of pixels where red > 200.
    fn red_centroid_y(buf: &[u8], w: u32, h: u32) -> f32 {
        let mut sum_y = 0.0f64;
        let mut count = 0.0f64;
        for y in 0..h {
            for x in 0..w {
                if r(buf, w, x, y) > 200
                    && buf[((y * w + x) * 4 + 1) as usize] < 50
                    && buf[((y * w + x) * 4 + 2) as usize] < 50
                {
                    sum_y += y as f64;
                    count += 1.0;
                }
            }
        }
        if count == 0.0 {
            0.0
        } else {
            (sum_y / count) as f32
        }
    }

    fn red_box(position: Position, x: f32, y: f32, w: f32, h: f32) -> BoxNode {
        BoxNode {
            id: 0,
            kind: BoxKind::Container,
            css: CssStyle {
                position: Some(position),
                left: Some(CLP::Px(x)),
                top: Some(CLP::Px(y)),
                width: Some(CSize::Length(CLP::Px(w))),
                height: Some(CSize::Length(CLP::Px(h))),
                background: Some(Background::Color(CssColor::String("#ff0000".into()))),
                ..Default::default()
            },
            children: vec![],
            intrinsic: None,
            source_path: None,
            window: None,
        }
    }

    fn root_node(w: f32, h: f32, children: Vec<BoxNode>) -> BoxNode {
        BoxNode {
            id: 0,
            kind: BoxKind::Container,
            css: CssStyle {
                display: Some(Display::Flex),
                flex_direction: Some(FlexDirection::Column),
                width: Some(CSize::Length(CLP::Px(w))),
                height: Some(CSize::Length(CLP::Px(h))),
                ..Default::default()
            },
            children,
            intrinsic: None,
            source_path: None,
            window: None,
        }
    }

    // ---- Test 1: no transform — pixel-identical to reference baseline ----

    #[test]
    fn no_transform_origin_unchanged_non_regression() {
        // A red 100x100 box at (50, 50) with no transform: pixels must appear
        // exactly at (50..150, 50..150) — no origin logic involved.
        let mut root = root_node(
            300.0,
            300.0,
            vec![{
                let mut n = red_box(Position::Absolute, 50.0, 50.0, 100.0, 100.0);
                n.css.transform = Some(vec![TransformFn::Scale { x: 1.0, y: 1.0 }]);
                // transform-origin absent → should default to center (no change)
                n
            }],
        );
        let buf = render_pixels(&mut root, 300, 300);

        // Centroid must be ~100, ~100 (center of the 50..150 range).
        let cx = red_centroid_x(&buf, 300, 300);
        let cy = red_centroid_y(&buf, 300, 300);
        assert!((cx - 99.5).abs() < 2.0, "cx={cx}");
        assert!((cy - 99.5).abs() < 2.0, "cy={cy}");
    }

    // ---- Test 2: 50% 50% == absent (byte-identical behavior) ----

    #[test]
    fn transform_origin_50pct_is_center_identity() {
        let make_root = |with_origin: bool| -> Vec<u8> {
            let mut n = red_box(Position::Absolute, 50.0, 50.0, 100.0, 100.0);
            n.css.transform = Some(vec![TransformFn::Rotate { deg: 45.0 }]);
            if with_origin {
                n.css.transform_origin = Some(TransformOrigin {
                    x: Some(CLP::String("50%".into())),
                    y: Some(CLP::String("50%".into())),
                    z: None,
                });
            }
            let mut root = root_node(300.0, 300.0, vec![n]);
            render_pixels(&mut root, 300, 300)
        };

        let without = make_root(false);
        let with_50 = make_root(true);
        assert_eq!(
            without, with_50,
            "transform-origin: 50% 50% must be byte-identical to absent origin"
        );
    }

    // ---- Test 3: rotate 90° around left-top vs center — different quadrants ----

    #[test]
    fn rotate_90_left_top_vs_center_occupy_different_quadrants() {
        // A red 100x100 box at (100, 100) rotated 90° around:
        //   - center (150, 150): box stays centered on itself, centroid ~(150, 150)
        //   - left-top (100, 100): the box pivots around top-left; centroid moves
        let make_root_with_origin = |ox: Option<CLP>, oy: Option<CLP>| -> Vec<u8> {
            let mut n = red_box(Position::Absolute, 100.0, 100.0, 100.0, 100.0);
            n.css.transform = Some(vec![TransformFn::Rotate { deg: 90.0 }]);
            if ox.is_some() || oy.is_some() {
                n.css.transform_origin = Some(TransformOrigin {
                    x: ox,
                    y: oy,
                    z: None,
                });
            }
            let mut root = root_node(400.0, 400.0, vec![n]);
            render_pixels(&mut root, 400, 400)
        };

        // Center pivot (default).
        let buf_center = make_root_with_origin(None, None);
        // Left-top pivot: x=0px (relative to box left), y=0px.
        let buf_left_top = make_root_with_origin(
            Some(CLP::String("0%".into())),
            Some(CLP::String("0%".into())),
        );

        let cx_center = red_centroid_x(&buf_center, 400, 400);
        let cy_center = red_centroid_y(&buf_center, 400, 400);
        let cx_lt = red_centroid_x(&buf_left_top, 400, 400);
        let cy_lt = red_centroid_y(&buf_left_top, 400, 400);

        // With center pivot (150, 150), rotate 90° CW → box centroid stays at ~(150, 150).
        assert!(
            (cx_center - 150.0).abs() < 5.0,
            "center-pivot cx should be ~150, got {cx_center}"
        );
        assert!(
            (cy_center - 150.0).abs() < 5.0,
            "center-pivot cy should be ~150, got {cy_center}"
        );

        // With left-top pivot (100, 100), rotating 90° CW around that point:
        //   box center (150,150) maps to (50, 150) — centroid moves left.
        //   cx_lt ≈ 50, while cx_center ≈ 150. The x-shift is the distinguishing axis.
        assert!(
            cx_lt < cx_center - 50.0,
            "left-top pivot cx ({cx_lt}) should be well left of center-pivot cx ({cx_center})"
        );
        // The overall pixel-centroid distance should be large.
        let dist = ((cx_lt - cx_center).powi(2) + (cy_lt - cy_center).powi(2)).sqrt();
        assert!(
            dist > 50.0,
            "pivots should produce clearly distinct positions (dist={dist})"
        );
    }

    // ---- Test 4: keyword "left top" == "0% 0%" ----

    #[test]
    fn keyword_left_top_equals_zero_percent() {
        let make_root = |origin_x: CLP, origin_y: CLP| -> Vec<u8> {
            let mut n = red_box(Position::Absolute, 100.0, 100.0, 100.0, 100.0);
            n.css.transform = Some(vec![TransformFn::Rotate { deg: 45.0 }]);
            n.css.transform_origin = Some(TransformOrigin {
                x: Some(origin_x),
                y: Some(origin_y),
                z: None,
            });
            let mut root = root_node(400.0, 400.0, vec![n]);
            render_pixels(&mut root, 400, 400)
        };

        let buf_kw = make_root(CLP::String("left".into()), CLP::String("top".into()));
        let buf_pct = make_root(CLP::String("0%".into()), CLP::String("0%".into()));
        assert_eq!(
            buf_kw, buf_pct,
            "keyword 'left top' must produce same pixels as '0% 0%'"
        );
    }

    // ---- Test 5: keyword "right bottom" == "100% 100%" ----

    #[test]
    fn keyword_right_bottom_equals_100_percent() {
        let make_root = |origin_x: CLP, origin_y: CLP| -> Vec<u8> {
            let mut n = red_box(Position::Absolute, 50.0, 50.0, 100.0, 100.0);
            n.css.transform = Some(vec![TransformFn::Scale { x: 1.5, y: 1.5 }]);
            n.css.transform_origin = Some(TransformOrigin {
                x: Some(origin_x),
                y: Some(origin_y),
                z: None,
            });
            let mut root = root_node(400.0, 400.0, vec![n]);
            render_pixels(&mut root, 400, 400)
        };

        let buf_kw = make_root(CLP::String("right".into()), CLP::String("bottom".into()));
        let buf_pct = make_root(CLP::String("100%".into()), CLP::String("100%".into()));
        assert_eq!(
            buf_kw, buf_pct,
            "keyword 'right bottom' must produce same pixels as '100% 100%'"
        );
    }

    // ---- Test 6: 3D rotate_y with left-center origin — left edge stays fixed ----

    #[test]
    fn rotate_y_left_origin_left_edge_is_stable() {
        // A 200x200 red box at (100, 100). RotateY with origin "left center"
        // means the left edge (x=100) is the pivot — so the leftmost painted
        // column should always be near x=100 regardless of angle.
        // With center origin, the center (x=200) is the pivot and the left edge moves.
        let make_root = |origin_x: Option<CLP>| -> Vec<u8> {
            let mut n = red_box(Position::Absolute, 100.0, 100.0, 200.0, 200.0);
            n.css.transform = Some(vec![TransformFn::RotateY { deg: 45.0 }]);
            if let Some(ox) = origin_x {
                n.css.transform_origin = Some(TransformOrigin {
                    x: Some(ox),
                    y: Some(CLP::String("50%".into())),
                    z: None,
                });
            }
            let mut root = root_node(500.0, 500.0, vec![n]);
            render_pixels(&mut root, 500, 500)
        };

        let buf_left = make_root(Some(CLP::String("0%".into())));
        let buf_center = make_root(None);

        // Find the leftmost red pixel x for each render.
        fn leftmost_red(buf: &[u8], w: u32, h: u32) -> Option<u32> {
            for x in 0..w {
                for y in 0..h {
                    if buf[((y * w + x) * 4) as usize] > 200
                        && buf[((y * w + x) * 4 + 1) as usize] < 50
                        && buf[((y * w + x) * 4 + 2) as usize] < 50
                    {
                        return Some(x);
                    }
                }
            }
            None
        }

        let left_edge_left =
            leftmost_red(&buf_left, 500, 500).expect("no red pixels in left-origin render") as f32;
        let left_edge_center = leftmost_red(&buf_center, 500, 500)
            .expect("no red pixels in center-origin render") as f32;

        // Left-origin: left edge stays near x=100 (pivot is exactly there).
        assert!(
            left_edge_left > 90.0 && left_edge_left < 115.0,
            "left-origin left edge should be near 100, got {left_edge_left}"
        );

        // Center-origin: left edge moves away from 100.
        assert!(
            left_edge_center > left_edge_left + 20.0,
            "center-origin left edge ({left_edge_center}) should be further right than left-origin ({left_edge_left})"
        );
    }

    // ---- Test 7: perspective_origin ≠ transform_origin — must not panic ----

    #[test]
    fn different_perspective_and_transform_origins_no_panic() {
        use crate::css::units::Length;
        let mut n = red_box(Position::Absolute, 100.0, 100.0, 200.0, 200.0);
        n.css.transform = Some(vec![TransformFn::RotateY { deg: 30.0 }]);
        n.css.perspective = Some(Length::Px(800.0));
        // perspective-origin at top-left, transform-origin at bottom-right
        n.css.transform_origin = Some(TransformOrigin {
            x: Some(CLP::String("100%".into())),
            y: Some(CLP::String("100%".into())),
            z: None,
        });
        n.css.perspective_origin = Some(TransformOrigin {
            x: Some(CLP::String("0%".into())),
            y: Some(CLP::String("0%".into())),
            z: None,
        });
        let mut root = root_node(500.0, 500.0, vec![n]);
        // Must not panic and must produce at least some red pixels.
        let buf = render_pixels(&mut root, 500, 500);
        let red_count = count_red(&buf);
        assert!(
            red_count > 0,
            "expected some red pixels with distinct origins"
        );
    }
}

#[cfg(test)]
mod glassmorphism_tests {
    //! TDD tests for issue #87: gradient-border painting and the `noise`
    //! filter function (film grain, deterministic per seed).

    use super::*;

    use crate::css::style::{
        Background, Color as CssColor, CssStyle, Display, FilterFn, FlexDirection, Position,
        Size as CSize,
    };
    use crate::css::taffy_bridge::ConversionContext;
    use crate::css::units::LengthPercentage as CLP;
    use crate::engine::box_tree::{BoxKind, BoxNode};
    use crate::engine::layout_pass::run_layout;
    use crate::schema::GradientBorder;

    fn test_frame(w: u32, h: u32) -> PaintFrame {
        PaintFrame {
            time: 0.0,
            frame_index: 0,
            fps: 30,
            video_width: w,
            video_height: h,
            scene_duration: 1.0,
            camera: None,
        }
    }

    fn render_pixels(root: &mut BoxNode, w: u32, h: u32) -> Vec<u8> {
        root.assign_ids(0);
        let layout = run_layout(root, (w as f32, h as f32), &ConversionContext::default());
        let mut surface = skia_safe::surfaces::raster_n32_premul((w as i32, h as i32)).unwrap();
        paint_tree(
            surface.canvas(),
            root,
            &layout,
            &test_frame(w, h),
            &NoopDispatcher,
        );
        let info = skia_safe::ImageInfo::new(
            (w as i32, h as i32),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Unpremul,
            None,
        );
        let mut buf = vec![0u8; (w * h * 4) as usize];
        surface.read_pixels(&info, &mut buf, (w * 4) as usize, (0, 0));
        buf
    }

    fn px(buf: &[u8], w: u32, x: u32, y: u32) -> (u8, u8, u8, u8) {
        let i = ((y * w + x) * 4) as usize;
        (buf[i], buf[i + 1], buf[i + 2], buf[i + 3])
    }

    /// Unique (r,g,b,a) values within a rectangular region.
    fn unique_colors_in(
        buf: &[u8],
        w: u32,
        x0: u32,
        y0: u32,
        x1: u32,
        y1: u32,
    ) -> std::collections::HashSet<(u8, u8, u8, u8)> {
        let mut set = std::collections::HashSet::new();
        for y in y0..y1 {
            for x in x0..x1 {
                set.insert(px(buf, w, x, y));
            }
        }
        set
    }

    fn leaf(css: CssStyle) -> BoxNode {
        BoxNode {
            id: 0,
            kind: BoxKind::Container,
            css,
            children: vec![],
            intrinsic: None,
            source_path: None,
            window: None,
        }
    }

    fn root_node(w: f32, h: f32, background: Option<&str>, children: Vec<BoxNode>) -> BoxNode {
        BoxNode {
            id: 0,
            kind: BoxKind::Container,
            css: CssStyle {
                display: Some(Display::Flex),
                flex_direction: Some(FlexDirection::Column),
                width: Some(CSize::Length(CLP::Px(w))),
                height: Some(CSize::Length(CLP::Px(h))),
                background: background.map(|c| Background::Color(CssColor::String(c.to_string()))),
                ..Default::default()
            },
            children,
            intrinsic: None,
            source_path: None,
            window: None,
        }
    }

    fn abs_box(x: f32, y: f32, w: f32, h: f32, css_extra: CssStyle) -> BoxNode {
        let mut css = css_extra;
        css.position = Some(Position::Absolute);
        css.left = Some(CLP::Px(x));
        css.top = Some(CLP::Px(y));
        css.width = Some(CSize::Length(CLP::Px(w)));
        css.height = Some(CSize::Length(CLP::Px(h)));
        leaf(css)
    }

    // ---- gradient-border ----

    #[test]
    fn gradient_border_paints_both_colors_on_perimeter_center_intact() {
        // 200x200 box at (100, 100) on a white root; gradient-border 12px
        // red→blue along the horizontal axis (angle 90). Expect: one vertical
        // border edge red-dominant, the other blue-dominant, centre untouched.
        let node = abs_box(
            100.0,
            100.0,
            200.0,
            200.0,
            CssStyle {
                gradient_border: Some(GradientBorder {
                    colors: vec!["#ff0000".into(), "#0000ff".into()],
                    width: 12.0,
                    angle: 90.0,
                }),
                ..Default::default()
            },
        );
        let mut root = root_node(400.0, 400.0, Some("#ffffff"), vec![node]);
        let buf = render_pixels(&mut root, 400, 400);

        // Sample border midpoints: left edge (x=106, y=200), right edge (x=294, y=200).
        let left = px(&buf, 400, 106, 200);
        let right = px(&buf, 400, 294, 200);

        // One side red-dominant, the other blue-dominant (convention-agnostic).
        let red_side = if left.0 > left.2 { left } else { right };
        let blue_side = if left.0 > left.2 { right } else { left };
        assert!(
            red_side.0 > 150 && red_side.2 < 100,
            "expected a red-dominant border edge, got {red_side:?}"
        );
        assert!(
            blue_side.2 > 150 && blue_side.0 < 100,
            "expected a blue-dominant border edge, got {blue_side:?}"
        );
        // Both extremes must actually differ (it's a gradient, not a flat color).
        assert_ne!(
            left, right,
            "border edges must show different gradient stops"
        );

        // Centre of the box stays the root background (white).
        let center = px(&buf, 400, 200, 200);
        assert_eq!(
            center,
            (255, 255, 255, 255),
            "box centre must not be painted by the gradient border"
        );

        // Top border midpoint is painted (not background).
        let top_mid = px(&buf, 400, 200, 106);
        assert_ne!(
            top_mid,
            (255, 255, 255, 255),
            "top border edge must be painted"
        );
    }

    #[test]
    fn gradient_border_respects_border_radius() {
        use crate::css::style::BorderRadius;
        // Rounded 200x200 box: the square corner pixel must stay background,
        // while edge midpoints are painted.
        let node = abs_box(
            100.0,
            100.0,
            200.0,
            200.0,
            CssStyle {
                border_radius: Some(BorderRadius::Uniform(CLP::Px(60.0))),
                gradient_border: Some(GradientBorder {
                    colors: vec!["#ff0000".into(), "#0000ff".into()],
                    width: 10.0,
                    angle: 90.0,
                }),
                ..Default::default()
            },
        );
        let mut root = root_node(400.0, 400.0, Some("#ffffff"), vec![node]);
        let buf = render_pixels(&mut root, 400, 400);

        // Square corner (inside the box bounds but outside the rounded path).
        let corner = px(&buf, 400, 104, 104);
        assert_eq!(
            corner,
            (255, 255, 255, 255),
            "square corner must stay background with border-radius"
        );
        // Edge midpoints painted.
        let left_mid = px(&buf, 400, 104, 200);
        let top_mid = px(&buf, 400, 200, 104);
        assert_ne!(left_mid, (255, 255, 255, 255), "left edge must be painted");
        assert_ne!(top_mid, (255, 255, 255, 255), "top edge must be painted");
    }

    #[test]
    fn gradient_border_replaces_standard_border() {
        use crate::css::style::{BorderEdges, BorderStyle, Edges};
        // A node with BOTH border (solid green) and gradient-border (red/blue):
        // the gradient border wins; no green pixels appear.
        let node = abs_box(
            100.0,
            100.0,
            200.0,
            200.0,
            CssStyle {
                border: Some(BorderEdges {
                    width: Some(Edges::Uniform(CLP::Px(10.0))),
                    style: Some(BorderStyle::Solid),
                    color: Some(CssColor::String("#00ff00".into())),
                    ..Default::default()
                }),
                gradient_border: Some(GradientBorder {
                    colors: vec!["#ff0000".into(), "#0000ff".into()],
                    width: 10.0,
                    angle: 90.0,
                }),
                ..Default::default()
            },
        );
        let mut root = root_node(400.0, 400.0, Some("#ffffff"), vec![node]);
        let buf = render_pixels(&mut root, 400, 400);

        let green_pixels = buf
            .chunks(4)
            .filter(|p| p[1] > 200 && p[0] < 60 && p[2] < 60)
            .count();
        assert_eq!(
            green_pixels, 0,
            "standard border must not be painted when gradient-border is set"
        );
    }

    // ---- noise filter ----

    fn gray_box_with_filter(filter: Option<Vec<FilterFn>>) -> BoxNode {
        abs_box(
            50.0,
            50.0,
            200.0,
            200.0,
            CssStyle {
                background: Some(Background::Color(CssColor::String("#808080".into()))),
                filter,
                ..Default::default()
            },
        )
    }

    #[test]
    fn noise_filter_explodes_unique_color_count() {
        let mut root_plain = root_node(
            300.0,
            300.0,
            Some("#ffffff"),
            vec![gray_box_with_filter(None)],
        );
        let buf_plain = render_pixels(&mut root_plain, 300, 300);

        let mut root_noise = root_node(
            300.0,
            300.0,
            Some("#ffffff"),
            vec![gray_box_with_filter(Some(vec![FilterFn::Noise {
                intensity: 0.5,
                seed: 42,
            }]))],
        );
        let buf_noise = render_pixels(&mut root_noise, 300, 300);

        // Sample well inside the box to avoid AA edges.
        let plain = unique_colors_in(&buf_plain, 300, 70, 70, 230, 230);
        let noisy = unique_colors_in(&buf_noise, 300, 70, 70, 230, 230);
        assert!(
            plain.len() <= 4,
            "plain box interior should be near-uniform, got {} colors",
            plain.len()
        );
        assert!(
            noisy.len() > 50,
            "noise filter should explode the unique color count, got {}",
            noisy.len()
        );
    }

    #[test]
    fn noise_same_seed_is_byte_identical_across_renders() {
        let render = || {
            let mut root = root_node(
                300.0,
                300.0,
                Some("#ffffff"),
                vec![gray_box_with_filter(Some(vec![FilterFn::Noise {
                    intensity: 0.4,
                    seed: 7,
                }]))],
            );
            render_pixels(&mut root, 300, 300)
        };
        let a = render();
        let b = render();
        assert_eq!(a, b, "same seed must produce byte-identical grain");
    }

    #[test]
    fn noise_different_seeds_differ() {
        let render = |seed: u64| {
            let mut root = root_node(
                300.0,
                300.0,
                Some("#ffffff"),
                vec![gray_box_with_filter(Some(vec![FilterFn::Noise {
                    intensity: 0.4,
                    seed,
                }]))],
            );
            render_pixels(&mut root, 300, 300)
        };
        let a = render(1);
        let b = render(2);
        assert_ne!(a, b, "different seeds must produce different grain");
    }

    #[test]
    fn backdrop_noise_grains_only_the_panel_region() {
        // Uniform gray root; a panel at (50, 50, 100, 100) with
        // backdrop-filter noise. Inside the panel: grain (many colors).
        // Outside: untouched uniform gray.
        let panel = abs_box(
            50.0,
            50.0,
            100.0,
            100.0,
            CssStyle {
                backdrop_filter: Some(vec![FilterFn::Noise {
                    intensity: 0.5,
                    seed: 42,
                }]),
                ..Default::default()
            },
        );
        let mut root = root_node(300.0, 300.0, Some("#808080"), vec![panel]);
        let buf = render_pixels(&mut root, 300, 300);

        let inside = unique_colors_in(&buf, 300, 60, 60, 140, 140);
        let outside = unique_colors_in(&buf, 300, 180, 180, 280, 280);
        assert!(
            inside.len() > 30,
            "panel interior should be grained, got {} colors",
            inside.len()
        );
        assert_eq!(
            outside.len(),
            1,
            "outside the panel must stay uniform, got {} colors",
            outside.len()
        );
    }

    // ---- serde ----

    #[test]
    fn css_gradient_border_and_noise_deserialize() {
        let json = r##"{
            "gradient-border": { "colors": ["#ff0000", "#0000ff"], "width": 3, "angle": 45 },
            "filter": [{ "fn": "noise", "intensity": 0.3, "seed": 9 }],
            "backdrop-filter": [{ "fn": "noise" }]
        }"##;
        let s: CssStyle = serde_json::from_str(json).unwrap();
        let gb = s.gradient_border.expect("gradient-border parsed");
        assert_eq!(gb.colors.len(), 2);
        assert_eq!(gb.width, 3.0);
        assert_eq!(gb.angle, 45.0);
        assert!(matches!(
            s.filter.as_deref(),
            Some([FilterFn::Noise {
                intensity,
                seed: 9
            }]) if (intensity - 0.3).abs() < 1e-6
        ));
        // Defaults: intensity 0.15, seed 42.
        assert!(matches!(
            s.backdrop_filter.as_deref(),
            Some([FilterFn::Noise {
                intensity,
                seed: 42
            }]) if (intensity - 0.15).abs() < 1e-6
        ));
    }

    #[test]
    fn css_legacy_zombies_accepted() {
        // backdrop-blur / inner-shadow parse into CssStyle (accepted for
        // compat) — rendering is intentionally not wired; validate warns.
        let json = r##"{
            "backdrop-blur": 20,
            "inner-shadow": { "color": "#000000", "offset_x": 0, "offset_y": 2, "blur": 8 }
        }"##;
        let s: CssStyle = serde_json::from_str(json).unwrap();
        assert_eq!(s.backdrop_blur, Some(20.0));
        assert!(s.inner_shadow.is_some());
    }
}

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
        assert_eq!(c.a(), 128);
    }

    #[test]
    fn parse_named_colors() {
        assert_eq!(parse_color_string("red").unwrap(), SColor::RED);
        assert_eq!(
            parse_color_string("transparent").unwrap(),
            SColor::TRANSPARENT
        );
    }

    #[test]
    fn parse_white_forms_all_agree() {
        // Regression test for the C2 audit finding: `#fff`, `#FFF`, `white`
        // and `rgb(255,255,255)` must all resolve to the same opaque white,
        // never to black.
        let white = SColor::from_argb(255, 255, 255, 255);
        assert_eq!(parse_color_string("#fff").unwrap(), white);
        assert_eq!(parse_color_string("#FFF").unwrap(), white);
        assert_eq!(parse_color_string("white").unwrap(), white);
        assert_eq!(parse_color_string("WHITE").unwrap(), white);
        assert_eq!(parse_color_string("rgb(255,255,255)").unwrap(), white);
        assert_eq!(parse_color_string("rgb(255, 255, 255)").unwrap(), white);
    }

    #[test]
    fn parse_color_string_supports_extended_named_set() {
        // Only 11 names were hardcoded before; spot-check a few outside
        // that set to prove it now routes through the full CSS keyword
        // table in `renderer::colors`.
        assert!(parse_color_string("rebeccapurple").is_some());
        assert!(parse_color_string("cornflowerblue").is_some());
        assert!(parse_color_string("dodgerblue").is_some());
    }

    #[test]
    fn parse_color_string_supports_hsl() {
        assert_eq!(
            parse_color_string("hsl(0, 100%, 50%)").unwrap(),
            SColor::from_argb(255, 255, 0, 0)
        );
    }

    #[test]
    fn parse_color_unresolvable_string_is_not_black() {
        let c = parse_color(&Color::String("not-a-color".to_string()));
        assert_ne!(c, SColor::BLACK);
        assert_eq!(c, SColor::from_argb(255, 255, 0, 255));
    }
}
