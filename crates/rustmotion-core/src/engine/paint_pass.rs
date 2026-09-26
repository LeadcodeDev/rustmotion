use std::cell::RefCell;

use skia_safe::gradient::{self, Colors as GradientColors, Gradient};
use skia_safe::{
    canvas::SaveLayerRec, Canvas, ClipOp, Color as SColor, Color4f, Paint, PaintStyle, PathBuilder,
    Point, RRect, Rect, M44, V3,
};

use crate::css::style::{
    Background, BackgroundLayer, BorderEdges, BorderRadius, BorderStyle, BoxShadow, ClipPath,
    Color, CssStyle, Edges, Overflow, TransformFn, TransformOrigin,
};
use crate::css::units::{parse_origin_component, LengthContext, LengthPercentage, ParsedLength};
use crate::engine::box_tree::{BoxKind, BoxNode, NodeId};
use crate::engine::layout_pass::{BoxLayout, LayoutResult};

#[derive(Debug, Clone, Copy)]
pub struct PaintFrame {
    pub time: f64,
    pub scenario_time: f64,
    pub frame_index: u32,
    pub fps: u32,
    pub video_width: u32,
    pub video_height: u32,
    pub scene_duration: f64,
    pub camera: Option<PlaneCamera>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaneCamera {
    pub pan_x: f32,
    pub pan_y: f32,
    pub zoom: f32,
    pub rotation: f32,
    pub origin_x: f32,
    pub origin_y: f32,
}

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitNode {
    pub node_id: NodeId,
    pub rect: HitRect,
}

pub type HitMap = Vec<HitNode>;

#[derive(Debug, Clone, PartialEq)]
pub struct EnrichedHit {
    pub node_id: NodeId,
    pub kind: String,
    pub rect: HitRect,
    pub pointer: Option<String>,
}

pub trait PaintDispatcher {
    fn dispatch(
        &self,
        canvas: &Canvas,
        payload: &(dyn std::any::Any + Send + Sync),
        css: &CssStyle,
        layout: &BoxLayout,
        frame: &PaintFrame,
    );
}

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

fn paint_node(canvas: &Canvas, node: &BoxNode, ctx: &PaintContext, tree_depth: usize) {
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
        font_size: node.css.font_size_px_or(16.0),
        root_font_size: 16.0,
    };
    let length_ctx_x = LengthContext {
        parent_size: box_layout.width,
        ..length_ctx
    };
    let length_ctx_y = LengthContext {
        parent_size: box_layout.height,
        ..length_ctx
    };

    canvas.save();

    if tree_depth == 1 {
        if let Some(cam) = &ctx.frame.camera {
            let depth = node.css.depth.unwrap_or(1.0);
            apply_plane_camera(canvas, cam, depth, ctx.viewport_size);
        }
    }

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
        let axes = TransformAxes {
            x: length_ctx_x,
            y: length_ctx_y,
            general: length_ctx,
        };
        apply_transform(
            canvas,
            transform_list,
            perspective_d,
            transform_pivot,
            perspective_pivot,
            &axes,
        );
    }

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

    let overflow = node.css.overflow.unwrap_or(Overflow::Visible);

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
        let filter_bleed_px = node
            .css
            .filter
            .as_deref()
            .map(|list| filter_bleed(list, &length_ctx))
            .unwrap_or(0.0);
        let shadow_bleed_px = node
            .css
            .box_shadow
            .as_deref()
            .map(|shadows| box_shadow_bleed(shadows, &length_ctx))
            .unwrap_or(0.0);
        let bleed = filter_bleed_px.max(shadow_bleed_px);
        let mut bounds = Rect::from_xywh(
            box_layout.x - bleed,
            box_layout.y - bleed,
            box_layout.width + bleed * 2.0,
            box_layout.height + bleed * 2.0,
        );
        if overflow == Overflow::Visible {
            if let Some(descendants) = subtree_layout_bounds(node, ctx.layout) {
                bounds = Rect::join2(bounds, descendants);
            }
        }
        let rec = SaveLayerRec::default().paint(&paint).bounds(&bounds);
        canvas.save_layer(&rec);
        true
    } else {
        false
    };

    let opened_clip_path = match node.css.clip_path.as_ref() {
        Some(clip) => match clip_path_to_skia(clip, box_layout, &length_ctx) {
            Some(path) => {
                canvas.save();
                canvas.clip_path(&path, ClipOp::Intersect, true);
                true
            }
            None => false,
        },
        None => false,
    };

    if let Some(shadows) = node.css.box_shadow.as_ref() {
        for shadow in shadows {
            if shadow.inset.unwrap_or(false) {
                continue;
            }
            paint_box_shadow(canvas, box_layout, &node.css, shadow, &length_ctx, false);
        }
    }
    if let Some(bg) = node.css.background.as_ref() {
        paint_background(canvas, box_layout, &node.css, bg, &length_ctx);
    }
    if let Some(gb) = node.css.gradient_border.as_ref() {
        paint_gradient_border(canvas, box_layout, &node.css, gb, &length_ctx);
    } else if let Some(border) = node.css.border.as_ref() {
        paint_border(canvas, box_layout, &node.css, border, &length_ctx);
    }

    let shimmer = active_shimmer(&node.css, ctx.frame.time);
    let opened_shimmer_layer = if shimmer.is_some() {
        let bounds = Rect::from_xywh(
            box_layout.x,
            box_layout.y,
            box_layout.width,
            box_layout.height,
        );
        let rec = SaveLayerRec::default().bounds(&bounds);
        canvas.save_layer(&rec);
        true
    } else {
        false
    };

    let opened_overflow_clip = if matches!(
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
        canvas.save();
        canvas.clip_rrect(rrect, ClipOp::Intersect, true);
        true
    } else {
        false
    };

    let payload_opt = match &node.kind {
        BoxKind::Component(p) | BoxKind::Ghost(p) => Some(p),
        BoxKind::Container => None,
    };
    if let Some(payload) = payload_opt {
        ctx.dispatcher
            .dispatch(canvas, payload.as_ref(), &node.css, box_layout, ctx.frame);
    }

    let mut indices: Vec<usize> = (0..node.children.len()).collect();
    indices.sort_by_key(|&i| node.children[i].css.z_index.unwrap_or(0));
    for &i in &indices {
        paint_node(canvas, &node.children[i], ctx, tree_depth + 1);
    }

    if opened_overflow_clip {
        canvas.restore();
    }

    if let Some(shadows) = node.css.box_shadow.as_ref() {
        for shadow in shadows {
            if shadow.inset.unwrap_or(false) {
                paint_box_shadow(canvas, box_layout, &node.css, shadow, &length_ctx, true);
            }
        }
    }

    if let Some((cfg, progress)) = shimmer {
        paint_shimmer_band(canvas, box_layout, cfg, progress);
    }
    if opened_shimmer_layer {
        canvas.restore();
    }
    if opened_clip_path {
        canvas.restore();
    }

    if opened_opacity_layer {
        canvas.restore();
    }
    canvas.restore();
}

fn active_shimmer(css: &CssStyle, time: f64) -> Option<(&crate::schema::ShimmerConfig, f32)> {
    let cfg = css.animation.iter().find_map(|e| match e {
        crate::schema::AnimationEffect::Shimmer(c) => Some(c),
        _ => None,
    })?;
    if cfg.duration <= 0.0 || cfg.intensity <= 0.0 {
        return None;
    }
    let elapsed = time - cfg.delay;
    if elapsed < 0.0 {
        return None;
    }
    let progress = if cfg.repeat {
        (elapsed / cfg.duration).rem_euclid(1.0)
    } else if elapsed > cfg.duration {
        return None;
    } else {
        elapsed / cfg.duration
    };
    Some((cfg, progress as f32))
}

fn paint_shimmer_band(
    canvas: &Canvas,
    layout: &BoxLayout,
    cfg: &crate::schema::ShimmerConfig,
    progress: f32,
) {
    if layout.width <= 0.0 || layout.height <= 0.0 {
        return;
    }
    let (r, g, b, a) = crate::engine::renderer::parse_hex_color(&cfg.color);
    let peak_alpha = (cfg.intensity.clamp(0.0, 1.0) * a as f32) as u8;
    let transparent = SColor::from_argb(0, r, g, b);
    let highlight = SColor::from_argb(peak_alpha, r, g, b);

    let theta = cfg.angle.to_radians();
    let (dx, dy) = (theta.cos(), theta.sin());
    let cx = layout.x + layout.width / 2.0;
    let cy = layout.y + layout.height / 2.0;
    let half_extent = (layout.width * dx).abs() / 2.0 + (layout.height * dy).abs() / 2.0;

    let band = (cfg.width.max(0.01) * half_extent * 2.0).max(1.0);
    let start = -half_extent - band;
    let centre = start + progress * (2.0 * half_extent + 2.0 * band);

    let p0 = Point::new(cx + dx * (centre - band), cy + dy * (centre - band));
    let p1 = Point::new(cx + dx * (centre + band), cy + dy * (centre + band));

    let colors4f = [
        Color4f::from(transparent),
        Color4f::from(highlight),
        Color4f::from(transparent),
    ];
    let gradient_colors = GradientColors::new(&colors4f, None, skia_safe::TileMode::Clamp, None);
    let grad = Gradient::new(gradient_colors, gradient::Interpolation::default());
    let Some(shader) = gradient::shaders::linear_gradient((p0, p1), &grad, None) else {
        return;
    };

    let mut paint = Paint::default();
    paint.set_style(PaintStyle::Fill);
    paint.set_anti_alias(true);
    paint.set_shader(shader);
    paint.set_blend_mode(skia_safe::BlendMode::SrcATop);
    canvas.draw_rect(
        Rect::from_xywh(layout.x, layout.y, layout.width, layout.height),
        &paint,
    );
}

fn filter_bleed(list: &[crate::css::style::FilterFn], ctx: &LengthContext) -> f32 {
    use crate::css::style::FilterFn;
    let mut bleed = 0.0f32;
    for f in list {
        let b = match f {
            FilterFn::Blur { radius } => radius.resolve(ctx).max(0.0) * 1.5,
            FilterFn::DropShadow {
                offset_x,
                offset_y,
                blur,
                ..
            } => {
                let blur_bleed = blur
                    .as_ref()
                    .map(|b| b.resolve(ctx).max(0.0) * 1.5)
                    .unwrap_or(0.0);
                offset_x.resolve(ctx).abs().max(offset_y.resolve(ctx).abs()) + blur_bleed
            }
            _ => 0.0,
        };
        bleed = bleed.max(b);
    }
    bleed
}

fn box_shadow_bleed(shadows: &[BoxShadow], ctx: &LengthContext) -> f32 {
    let mut bleed = 0.0f32;
    for shadow in shadows {
        if shadow.inset.unwrap_or(false) {
            continue;
        }
        let offset = shadow
            .offset_x
            .resolve(ctx)
            .abs()
            .max(shadow.offset_y.resolve(ctx).abs());
        let spread = shadow
            .spread
            .as_ref()
            .map(|s| s.resolve(ctx).max(0.0))
            .unwrap_or(0.0);
        let blur_bleed = shadow
            .blur
            .as_ref()
            .map(|b| b.resolve(ctx).max(0.0) * 1.5)
            .unwrap_or(0.0);
        bleed = bleed.max(offset + spread + blur_bleed);
    }
    bleed
}

fn subtree_layout_bounds(node: &BoxNode, layout: &LayoutResult) -> Option<Rect> {
    let mut bounds: Option<Rect> = None;
    for child in &node.children {
        if let Some(child_layout) = layout.get(child.id) {
            let rect = Rect::from_xywh(
                child_layout.x,
                child_layout.y,
                child_layout.width,
                child_layout.height,
            );
            bounds = Some(bounds.map_or(rect, |b| Rect::join2(b, rect)));
        }
        if let Some(child_bounds) = subtree_layout_bounds(child, layout) {
            bounds = Some(bounds.map_or(child_bounds, |b| Rect::join2(b, child_bounds)));
        }
    }
    bounds
}

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
            FilterFn::Noise { intensity, seed } => match noise_image_filter(*intensity, *seed) {
                Some(noise) => {
                    image_filters::blend(skia_safe::BlendMode::Overlay, chain, Some(noise), None)
                }
                None => chain,
            },
            other => color_matrix_for(other)
                .map(|m| skia_safe::color_filters::matrix_row_major(&m, None))
                .and_then(|cf| image_filters::color_filter(cf, chain, None)),
        };
    }
    chain
}

fn noise_image_filter(intensity: f32, seed: u64) -> Option<skia_safe::ImageFilter> {
    use skia_safe::image_filters;

    let intensity = intensity.clamp(0.0, 1.0);
    if intensity <= 0.0 {
        return None;
    }
    let noise = skia_safe::shaders::fractal_noise((0.9, 0.9), 2, seed as f32, None)?;
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

fn color_matrix_for(f: &crate::css::style::FilterFn) -> Option<[f32; 20]> {
    use crate::css::style::FilterFn;
    #[rustfmt::skip]
    fn saturation(s: f32) -> [f32; 20] {
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

fn resolve_origin(
    origin: Option<&TransformOrigin>,
    layout: &BoxLayout,
    ctx: &LengthContext,
) -> (f32, f32, f32) {
    let Some(o) = origin else {
        return (
            layout.x + layout.width / 2.0,
            layout.y + layout.height / 2.0,
            0.0,
        );
    };

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

#[derive(Clone, Copy)]
struct TransformAxes {
    x: LengthContext,
    y: LengthContext,
    general: LengthContext,
}

fn apply_transform(
    canvas: &Canvas,
    list: &[TransformFn],
    perspective_d: Option<f32>,
    transform_pivot: (f32, f32),
    perspective_pivot: (f32, f32),
    axes: &TransformAxes,
) {
    let pivots_equal = (transform_pivot.0 - perspective_pivot.0).abs() < 0.001
        && (transform_pivot.1 - perspective_pivot.1).abs() < 0.001;

    if perspective_d.is_none() && !has_3d_transform(list) {
        let pivot = transform_pivot;
        canvas.translate(Point::new(pivot.0, pivot.1));
        for tr in list {
            match tr {
                TransformFn::Translate { x, y } => {
                    canvas.translate(Point::new(x.resolve(&axes.x), y.resolve(&axes.y)));
                }
                TransformFn::TranslateX { x } => {
                    canvas.translate(Point::new(x.resolve(&axes.x), 0.0));
                }
                TransformFn::TranslateY { y } => {
                    canvas.translate(Point::new(0.0, y.resolve(&axes.y)));
                }
                TransformFn::Translate3d { x, y, .. } => {
                    canvas.translate(Point::new(x.resolve(&axes.x), y.resolve(&axes.y)));
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
        let pivot = transform_pivot;
        let mut m = M44::new_identity();
        m.pre_concat(&M44::translate(pivot.0, pivot.1, 0.0));
        if let Some(d) = perspective_d {
            m.pre_concat(&css_perspective_m44(d));
        }
        for tr in list {
            m.pre_concat(&transform_to_m44(tr, axes));
        }
        m.pre_concat(&M44::translate(-pivot.0, -pivot.1, 0.0));
        canvas.concat_44(&m);
    } else {
        let tp = transform_pivot;
        let pp = perspective_pivot;
        let mut m = M44::new_identity();

        m.pre_concat(&M44::translate(pp.0, pp.1, 0.0));
        if let Some(d) = perspective_d {
            m.pre_concat(&css_perspective_m44(d));
        }
        m.pre_concat(&M44::translate(-pp.0, -pp.1, 0.0));

        m.pre_concat(&M44::translate(tp.0, tp.1, 0.0));
        for tr in list {
            m.pre_concat(&transform_to_m44(tr, axes));
        }
        m.pre_concat(&M44::translate(-tp.0, -tp.1, 0.0));

        canvas.concat_44(&m);
    }
}

pub fn animated_transform(
    css: &CssStyle,
    layout: &BoxLayout,
    viewport: (f32, f32),
) -> (f32, f32, f32, f32, f32) {
    let length_ctx = LengthContext {
        viewport_width: viewport.0,
        viewport_height: viewport.1,
        parent_size: layout.width.max(layout.height),
        font_size: css.font_size_px_or(16.0),
        root_font_size: 16.0,
    };
    let ctx_x = LengthContext {
        parent_size: layout.width,
        ..length_ctx
    };
    let ctx_y = LengthContext {
        parent_size: layout.height,
        ..length_ctx
    };

    let mut tx = 0.0f32;
    let mut ty = 0.0f32;
    let mut scale_x = 1.0f32;
    let mut scale_y = 1.0f32;
    let mut rotation = 0.0f32;
    for t in css.transform.as_deref().unwrap_or(&[]) {
        match t {
            TransformFn::Translate { x, y } => {
                tx += x.resolve(&ctx_x);
                ty += y.resolve(&ctx_y);
            }
            TransformFn::TranslateX { x } => tx += x.resolve(&ctx_x),
            TransformFn::TranslateY { y } => ty += y.resolve(&ctx_y),
            TransformFn::Translate3d { x, y, .. } => {
                tx += x.resolve(&ctx_x);
                ty += y.resolve(&ctx_y);
            }
            TransformFn::Scale { x, y } => {
                scale_x *= x;
                scale_y *= y;
            }
            TransformFn::ScaleX { x } => scale_x *= x,
            TransformFn::ScaleY { y } => scale_y *= y,
            TransformFn::Scale3d { x, y, .. } => {
                scale_x *= x;
                scale_y *= y;
            }
            TransformFn::Rotate { deg } | TransformFn::RotateZ { deg } => rotation += deg,
            TransformFn::Rotate3d { deg, .. } => rotation += deg,
            _ => {}
        }
    }
    let scale = (scale_x + scale_y) / 2.0;
    let opacity = css.opacity.unwrap_or(1.0);
    (tx, ty, scale, rotation, opacity)
}

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

fn transform_to_m44(tr: &TransformFn, axes: &TransformAxes) -> M44 {
    match tr {
        TransformFn::Translate { x, y } => {
            M44::translate(x.resolve(&axes.x), y.resolve(&axes.y), 0.0)
        }
        TransformFn::TranslateX { x } => M44::translate(x.resolve(&axes.x), 0.0, 0.0),
        TransformFn::TranslateY { y } => M44::translate(0.0, y.resolve(&axes.y), 0.0),
        TransformFn::TranslateZ { z } => M44::translate(0.0, 0.0, z.resolve(&axes.general)),
        TransformFn::Translate3d { x, y, z } => M44::translate(
            x.resolve(&axes.x),
            y.resolve(&axes.y),
            z.resolve(&axes.general),
        ),
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
        TransformFn::Perspective { length } => {
            css_perspective_m44(length.resolve(&axes.general).max(1.0))
        }
        TransformFn::Matrix { values: v } => M44::row_major(&[
            v[0], v[2], 0.0, v[4], v[1], v[3], 0.0, v[5], 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]),
        TransformFn::Matrix3d { values: v } => M44::col_major(v),
    }
}

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
            let gradient_colors =
                GradientColors::new(&colors, Some(&positions), skia_safe::TileMode::Clamp, None);
            let grad = Gradient::new(gradient_colors, gradient::Interpolation::default());
            if let Some(shader) = gradient::shaders::linear_gradient((p0, p1), &grad, None) {
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
            let gradient_colors =
                GradientColors::new(&colors, Some(&positions), skia_safe::TileMode::Clamp, None);
            let grad = Gradient::new(gradient_colors, gradient::Interpolation::default());
            if let Some(shader) = gradient::shaders::radial_gradient((center, radius), &grad, None)
            {
                paint.set_shader(shader);
                canvas.draw_rrect(rrect, &paint);
            }
        }
        BackgroundLayer::ConicGradient { stops, .. } => {
            let bounds = rrect.bounds();
            let center = Point::new(
                bounds.left + bounds.width() / 2.0,
                bounds.top + bounds.height() / 2.0,
            );
            let (colors, positions) = gradient_stops(stops);
            let gradient_colors =
                GradientColors::new(&colors, Some(&positions), skia_safe::TileMode::Clamp, None);
            let grad = Gradient::new(gradient_colors, gradient::Interpolation::default());
            if let Some(shader) =
                gradient::shaders::sweep_gradient(center, (0.0, 360.0), &grad, None)
            {
                paint.set_shader(shader);
                canvas.draw_rrect(rrect, &paint);
            }
        }
        BackgroundLayer::Image { .. } => {}
    }
}

fn gradient_stops(stops: &[crate::css::style::GradientStop]) -> (Vec<Color4f>, Vec<f32>) {
    let mut colors = Vec::with_capacity(stops.len());
    let mut positions = Vec::with_capacity(stops.len());
    let n = stops.len().max(1);
    for (i, s) in stops.iter().enumerate() {
        colors.push(Color4f::from(parse_color(&s.color)));
        let default_offset = i as f32 / (n.saturating_sub(1).max(1) as f32);
        positions.push(s.offset.unwrap_or(default_offset));
    }
    (colors, positions)
}

fn gradient_endpoints(bounds: Rect, angle_deg: f32) -> (Point, Point) {
    let cx = bounds.left + bounds.width() / 2.0;
    let cy = bounds.top + bounds.height() / 2.0;
    let rad = angle_deg.to_radians();
    let (sin_a, cos_a) = (rad.sin(), -rad.cos());
    let len = (bounds.width().abs() * sin_a.abs() + bounds.height().abs() * cos_a.abs()) / 2.0;
    let p0 = Point::new(cx - sin_a * len, cy - cos_a * len);
    let p1 = Point::new(cx + sin_a * len, cy + cos_a * len);
    (p0, p1)
}

fn paint_border(
    canvas: &Canvas,
    layout: &BoxLayout,
    css: &CssStyle,
    border: &BorderEdges,
    ctx: &LengthContext,
) {
    let style = border.style.unwrap_or(BorderStyle::Solid);
    if matches!(style, BorderStyle::None) {
        return;
    }
    let color = border
        .color
        .as_ref()
        .map(parse_color)
        .unwrap_or(SColor::BLACK);

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

    let outer = border_rrect(layout, radius);
    let inner = inner_rrect(layout, radius);

    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    paint.set_style(PaintStyle::Fill);
    paint.set_color(color);

    canvas.draw_drrect(outer, inner, &paint);
}

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

    let colors: Vec<Color4f> = gb
        .colors
        .iter()
        .map(|c| Color4f::from(parse_color_string(c).unwrap_or_else(|| unresolved_color(c))))
        .collect();
    let n = colors.len();
    let positions: Vec<f32> = (0..n)
        .map(|i| i as f32 / (n.saturating_sub(1).max(1) as f32))
        .collect();

    let bounds = outer.bounds();
    let (p0, p1) = gradient_endpoints(*bounds, gb.angle);
    let gradient_colors =
        GradientColors::new(&colors, Some(&positions), skia_safe::TileMode::Clamp, None);
    let grad = Gradient::new(gradient_colors, gradient::Interpolation::default());
    let Some(shader) = gradient::shaders::linear_gradient((p0, p1), &grad, None) else {
        return;
    };

    let mut paint = Paint::default();
    paint.set_anti_alias(true);
    paint.set_style(PaintStyle::Fill);
    paint.set_shader(shader);
    canvas.draw_drrect(outer, inner, &paint);
}

fn clip_path_to_skia(
    clip: &ClipPath,
    layout: &BoxLayout,
    ctx: &LengthContext,
) -> Option<skia_safe::Path> {
    let ctx_w = LengthContext {
        parent_size: layout.width,
        ..*ctx
    };
    let ctx_h = LengthContext {
        parent_size: layout.height,
        ..*ctx
    };

    match clip {
        ClipPath::None => None,

        ClipPath::Inset {
            top,
            right,
            bottom,
            left,
            radius,
        } => {
            let t = top.resolve(&ctx_h);
            let r = right.resolve(&ctx_w);
            let b = bottom.resolve(&ctx_h);
            let l = left.resolve(&ctx_w);
            let width = (layout.width - l - r).max(0.0);
            let height = (layout.height - t - b).max(0.0);
            let rect = Rect::from_xywh(layout.x + l, layout.y + t, width, height);
            let corners = radius
                .as_ref()
                .map(|br| resolve_border_radius(br, layout, ctx))
                .unwrap_or([0.0; 4]);
            let mut builder = PathBuilder::new();
            builder.add_rrect(rrect_from_corners(rect, corners), None, None);
            Some(builder.detach())
        }

        ClipPath::Circle { radius, origin } => {
            let (cx, cy, _) = resolve_origin(origin.as_ref(), layout, ctx);
            let reference = LengthContext {
                parent_size: (layout.width.powi(2) + layout.height.powi(2)).sqrt()
                    / std::f32::consts::SQRT_2,
                ..*ctx
            };
            let r = radius.resolve(&reference);
            if r <= 0.0 {
                return Some(PathBuilder::new().detach());
            }
            let mut builder = PathBuilder::new();
            builder.add_circle((cx, cy), r, None);
            Some(builder.detach())
        }

        ClipPath::Ellipse { rx, ry, origin } => {
            let (cx, cy, _) = resolve_origin(origin.as_ref(), layout, ctx);
            let a = rx.resolve(&ctx_w);
            let b = ry.resolve(&ctx_h);
            if a <= 0.0 || b <= 0.0 {
                return Some(PathBuilder::new().detach());
            }
            let mut builder = PathBuilder::new();
            builder.add_oval(
                Rect::from_xywh(cx - a, cy - b, a * 2.0, b * 2.0),
                None,
                None,
            );
            Some(builder.detach())
        }

        ClipPath::Polygon { points } => {
            if points.len() < 3 {
                return Some(PathBuilder::new().detach());
            }
            let mut builder = PathBuilder::new();
            for (i, (px, py)) in points.iter().enumerate() {
                let x = layout.x + px.resolve(&ctx_w);
                let y = layout.y + py.resolve(&ctx_h);
                if i == 0 {
                    builder.move_to((x, y));
                } else {
                    builder.line_to((x, y));
                }
            }
            builder.close();
            Some(builder.detach())
        }

        ClipPath::Path { d } => {
            let parsed = skia_safe::Path::from_svg(d)?;
            Some(parsed.with_offset((layout.x, layout.y)))
        }

        ClipPath::NodePath { id } => {
            eprintln!(
                "rustmotion: clip-path {{ kind: node-path, id: \"{id}\" }} is not implemented \
                 yet — reading another node's geometry needs a resolved-path lookup the paint \
                 pass does not have. Nothing is clipped. Use kind: path with the same data, or \
                 follow the tracking issue."
            );
            None
        }
    }
}

fn border_rrect(layout: &BoxLayout, radius: [f32; 4]) -> RRect {
    let rect = Rect::from_xywh(layout.x, layout.y, layout.width, layout.height);
    rrect_from_corners(rect, radius)
}

fn padding_rrect(layout: &BoxLayout, radius: [f32; 4]) -> RRect {
    let (x, y, w, h) = layout.padding_box();
    let rect = Rect::from_xywh(x, y, w, h);
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
        let mut path = PathBuilder::new();
        path.add_rrect(outer, None, None);
        path.add_rrect(inner, None, None);
        path.set_fill_type(skia_safe::PathFillType::EvenOdd);
        canvas.draw_path(&path.detach(), &clear);
        canvas.restore();
    }
}

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

fn unresolved_color(original: &str) -> SColor {
    eprintln!(
        "Warning: unrecognized color '{original}' — rendering as opaque magenta instead of \
         silently falling back to black"
    );
    let (r, g, b, a) = crate::engine::renderer::UNRESOLVED_COLOR;
    SColor::from_argb(a, r, g, b)
}

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
            scenario_time: 0.0,
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

        assert!(red(10, 97) < 10, "outside/above must stay black");
        assert!(red(10, 103) > 245, "outside/below must stay white");
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
    fn backdrop_filter_survives_sibling_opacity_below_one() {
        use crate::css::style::{Background, Color as CssColor, FilterFn};
        use crate::css::units::Length;

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
                opacity: Some(0.99),
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

        assert!(red(10, 97) < 10, "outside/above must stay black");
        assert!(red(10, 103) > 245, "outside/below must stay white");
        let above = red(100, 97);
        let below = red(100, 103);
        assert!(
            above > 30,
            "backdrop not blurred above boundary with opacity:0.99 (r={above})"
        );
        assert!(
            below < 225,
            "backdrop not blurred below boundary with opacity:0.99 (r={below})"
        );
    }

    #[test]
    fn hitmap_reflects_node_transform() {
        use crate::css::style::TransformFn;

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
        assert!((h.rect.w - 200.0).abs() < 1.0, "w = {}", h.rect.w);
        assert!((h.rect.h - 160.0).abs() < 1.0, "h = {}", h.rect.h);
    }
}

#[cfg(test)]
mod transform_origin_tests {

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
            scenario_time: 0.0,
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

    fn r(buf: &[u8], w: u32, x: u32, y: u32) -> u8 {
        buf[((y * w + x) * 4) as usize]
    }

    fn count_red(buf: &[u8]) -> usize {
        buf.chunks(4)
            .filter(|px| px[0] > 200 && px[1] < 50 && px[2] < 50)
            .count()
    }

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

    #[test]
    fn no_transform_origin_unchanged_non_regression() {
        let mut root = root_node(
            300.0,
            300.0,
            vec![{
                let mut n = red_box(Position::Absolute, 50.0, 50.0, 100.0, 100.0);
                n.css.transform = Some(vec![TransformFn::Scale { x: 1.0, y: 1.0 }]);
                n
            }],
        );
        let buf = render_pixels(&mut root, 300, 300);

        let cx = red_centroid_x(&buf, 300, 300);
        let cy = red_centroid_y(&buf, 300, 300);
        assert!((cx - 99.5).abs() < 2.0, "cx={cx}");
        assert!((cy - 99.5).abs() < 2.0, "cy={cy}");
    }

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

    #[test]
    fn rotate_90_left_top_vs_center_occupy_different_quadrants() {
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

        let buf_center = make_root_with_origin(None, None);
        let buf_left_top = make_root_with_origin(
            Some(CLP::String("0%".into())),
            Some(CLP::String("0%".into())),
        );

        let cx_center = red_centroid_x(&buf_center, 400, 400);
        let cy_center = red_centroid_y(&buf_center, 400, 400);
        let cx_lt = red_centroid_x(&buf_left_top, 400, 400);
        let cy_lt = red_centroid_y(&buf_left_top, 400, 400);

        assert!(
            (cx_center - 150.0).abs() < 5.0,
            "center-pivot cx should be ~150, got {cx_center}"
        );
        assert!(
            (cy_center - 150.0).abs() < 5.0,
            "center-pivot cy should be ~150, got {cy_center}"
        );

        assert!(
            cx_lt < cx_center - 50.0,
            "left-top pivot cx ({cx_lt}) should be well left of center-pivot cx ({cx_center})"
        );
        let dist = ((cx_lt - cx_center).powi(2) + (cy_lt - cy_center).powi(2)).sqrt();
        assert!(
            dist > 50.0,
            "pivots should produce clearly distinct positions (dist={dist})"
        );
    }

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

    #[test]
    fn rotate_y_left_origin_left_edge_is_stable() {
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

        assert!(
            left_edge_left > 90.0 && left_edge_left < 115.0,
            "left-origin left edge should be near 100, got {left_edge_left}"
        );

        assert!(
            left_edge_center > left_edge_left + 20.0,
            "center-origin left edge ({left_edge_center}) should be further right than left-origin ({left_edge_left})"
        );
    }

    #[test]
    fn different_perspective_and_transform_origins_no_panic() {
        use crate::css::units::Length;
        let mut n = red_box(Position::Absolute, 100.0, 100.0, 200.0, 200.0);
        n.css.transform = Some(vec![TransformFn::RotateY { deg: 30.0 }]);
        n.css.perspective = Some(Length::Px(800.0));
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
        let buf = render_pixels(&mut root, 500, 500);
        let red_count = count_red(&buf);
        assert!(
            red_count > 0,
            "expected some red pixels with distinct origins"
        );
    }

    #[test]
    fn translate_percent_resolves_against_own_axis_not_max_dimension() {
        let mut n = red_box(Position::Absolute, 0.0, 0.0, 200.0, 100.0);
        n.css.transform = Some(vec![TransformFn::Translate {
            x: CLP::String("50%".into()),
            y: CLP::String("50%".into()),
        }]);
        let mut root = root_node(400.0, 400.0, vec![n]);
        let buf = render_pixels(&mut root, 400, 400);

        let mut min_x = u32::MAX;
        let mut max_x = 0u32;
        let mut min_y = u32::MAX;
        let mut max_y = 0u32;
        for y in 0..400u32 {
            for x in 0..400u32 {
                let i = ((y * 400 + x) * 4) as usize;
                if buf[i] > 200 && buf[i + 1] < 50 && buf[i + 2] < 50 {
                    min_x = min_x.min(x);
                    max_x = max_x.max(x);
                    min_y = min_y.min(y);
                    max_y = max_y.max(y);
                }
            }
        }
        assert_ne!(min_x, u32::MAX, "expected some red pixels");

        assert!((min_x as i32 - 100).abs() <= 2, "min_x={min_x}");
        assert!((max_x as i32 - 299).abs() <= 2, "max_x={max_x}");
        assert!(
            (min_y as i32 - 50).abs() <= 2,
            "min_y={min_y} (expected ~50; the max(w,h) bug would give ~100)"
        );
        assert!(
            (max_y as i32 - 149).abs() <= 2,
            "max_y={max_y} (expected ~149; the max(w,h) bug would give ~199)"
        );
    }
}

#[cfg(test)]
mod glassmorphism_tests {

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
            scenario_time: 0.0,
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

    #[test]
    fn gradient_border_paints_both_colors_on_perimeter_center_intact() {
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

        let left = px(&buf, 400, 106, 200);
        let right = px(&buf, 400, 294, 200);

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
        assert_ne!(
            left, right,
            "border edges must show different gradient stops"
        );

        let center = px(&buf, 400, 200, 200);
        assert_eq!(
            center,
            (255, 255, 255, 255),
            "box centre must not be painted by the gradient border"
        );

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

        let corner = px(&buf, 400, 104, 104);
        assert_eq!(
            corner,
            (255, 255, 255, 255),
            "square corner must stay background with border-radius"
        );
        let left_mid = px(&buf, 400, 104, 200);
        let top_mid = px(&buf, 400, 200, 104);
        assert_ne!(left_mid, (255, 255, 255, 255), "left edge must be painted");
        assert_ne!(top_mid, (255, 255, 255, 255), "top edge must be painted");
    }

    #[test]
    fn gradient_border_replaces_standard_border() {
        use crate::css::style::{BorderEdges, BorderStyle, Edges};
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
mod paint_order_tests {

    use super::*;

    use crate::css::style::{
        Background, BoxShadow, ClipPath, Color as CssColor, CssStyle, Display, FilterFn,
        FlexDirection, Overflow, Position, Size as CSize,
    };
    use crate::css::taffy_bridge::ConversionContext;
    use crate::css::units::{Length, LengthPercentage as CLP};
    use crate::engine::box_tree::{BoxKind, BoxNode};
    use crate::engine::layout_pass::run_layout;

    fn test_frame(w: u32, h: u32) -> PaintFrame {
        PaintFrame {
            time: 0.0,
            scenario_time: 0.0,
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

    fn root_node(w: f32, h: f32, background: &str, children: Vec<BoxNode>) -> BoxNode {
        BoxNode {
            id: 0,
            kind: BoxKind::Container,
            css: CssStyle {
                display: Some(Display::Flex),
                flex_direction: Some(FlexDirection::Column),
                width: Some(CSize::Length(CLP::Px(w))),
                height: Some(CSize::Length(CLP::Px(h))),
                background: Some(Background::Color(CssColor::String(background.to_string()))),
                ..Default::default()
            },
            children,
            intrinsic: None,
            source_path: None,
            window: None,
        }
    }

    fn count_red_in(buf: &[u8], w: u32, x0: u32, y0: u32, x1: u32, y1: u32) -> usize {
        let mut n = 0;
        for y in y0..y1 {
            for x in x0..x1 {
                let i = ((y * w + x) * 4) as usize;
                if buf[i] > 200 && buf[i + 1] < 50 && buf[i + 2] < 50 {
                    n += 1;
                }
            }
        }
        n
    }

    fn clipped_square(clip: Option<ClipPath>) -> BoxNode {
        BoxNode {
            id: 0,
            kind: BoxKind::Container,
            css: CssStyle {
                position: Some(Position::Absolute),
                left: Some(CLP::Px(0.0)),
                top: Some(CLP::Px(0.0)),
                width: Some(CSize::Length(CLP::Px(400.0))),
                height: Some(CSize::Length(CLP::Px(400.0))),
                background: Some(Background::Color(CssColor::String("#ff0000".into()))),
                clip_path: clip,
                ..Default::default()
            },
            children: vec![],
            intrinsic: None,
            source_path: None,
            window: None,
        }
    }

    fn is_red_at(clip: Option<ClipPath>, x: u32, y: u32) -> bool {
        let mut root = root_node(400.0, 400.0, "#000000", vec![clipped_square(clip)]);
        let buf = render_pixels(&mut root, 400, 400);
        let i = ((y * 400 + x) * 4) as usize;
        buf[i] > 200 && buf[i + 1] < 50 && buf[i + 2] < 50
    }

    #[test]
    fn clip_path_inset_removes_the_region_outside_it() {
        assert!(
            is_red_at(None, 350, 200),
            "sanity: unclipped, x=350 is inside the square"
        );
        let inset = ClipPath::Inset {
            top: CLP::Px(0.0),
            right: CLP::Px(120.0),
            bottom: CLP::Px(0.0),
            left: CLP::Px(0.0),
            radius: None,
        };
        assert!(
            is_red_at(Some(inset.clone()), 200, 200),
            "inside the inset box must still paint"
        );
        assert!(
            !is_red_at(Some(inset), 330, 200),
            "inset right:120 must remove x=330 — this is the sample from the issue that \
             stayed painted while clip-path was read by nothing"
        );
    }

    #[test]
    fn clip_path_circle_keeps_the_centre_and_drops_the_corner() {
        let circle = ClipPath::Circle {
            radius: CLP::Px(100.0),
            origin: None,
        };
        assert!(
            is_red_at(Some(circle.clone()), 200, 200),
            "the centre is inside a centred r=100 circle"
        );
        assert!(
            !is_red_at(Some(circle), 20, 20),
            "the top-left corner is outside a centred r=100 circle"
        );
    }

    #[test]
    fn clip_path_ellipse_is_wider_than_it_is_tall() {
        let ellipse = ClipPath::Ellipse {
            rx: CLP::Px(180.0),
            ry: CLP::Px(40.0),
            origin: None,
        };
        assert!(
            is_red_at(Some(ellipse.clone()), 360, 200),
            "x=360 is within rx=180 of the centre"
        );
        assert!(
            !is_red_at(Some(ellipse), 200, 360),
            "y=360 is outside ry=40 of the centre"
        );
    }

    #[test]
    fn clip_path_polygon_cuts_a_triangle() {
        let triangle = ClipPath::Polygon {
            points: vec![
                (CLP::Px(200.0), CLP::Px(0.0)),
                (CLP::Px(400.0), CLP::Px(400.0)),
                (CLP::Px(0.0), CLP::Px(400.0)),
            ],
        };
        assert!(
            is_red_at(Some(triangle.clone()), 200, 300),
            "low centre is inside the triangle"
        );
        assert!(
            !is_red_at(Some(triangle), 20, 20),
            "the top-left corner is outside the triangle"
        );
    }

    #[test]
    fn clip_path_path_takes_svg_data_relative_to_the_box() {
        let left_half = ClipPath::Path {
            d: "M0 0 L200 0 L200 400 L0 400 Z".to_string(),
        };
        assert!(
            is_red_at(Some(left_half.clone()), 100, 200),
            "the left half stays"
        );
        assert!(
            !is_red_at(Some(left_half), 300, 200),
            "the right half is clipped away"
        );
    }

    #[test]
    fn clip_path_none_paints_exactly_as_no_clip_path_at_all() {
        for (x, y) in [(20, 20), (200, 200), (380, 380)] {
            assert_eq!(
                is_red_at(Some(ClipPath::None), x, y),
                is_red_at(None, x, y),
                "clip-path: none must be indistinguishable from absent at ({x}, {y})"
            );
        }
    }

    #[test]
    fn clip_path_clips_the_background_and_the_outset_shadow_too() {
        let mut node = clipped_square(Some(ClipPath::Inset {
            top: CLP::Px(0.0),
            right: CLP::Px(200.0),
            bottom: CLP::Px(0.0),
            left: CLP::Px(0.0),
            radius: None,
        }));
        node.css.width = Some(CSize::Length(CLP::Px(200.0)));
        node.css.height = Some(CSize::Length(CLP::Px(200.0)));
        node.css.left = Some(CLP::Px(100.0));
        node.css.top = Some(CLP::Px(100.0));
        node.css.background = Some(Background::Color(CssColor::String("#ffffff".into())));
        node.css.box_shadow = Some(vec![BoxShadow {
            offset_x: Length::Px(0.0),
            offset_y: Length::Px(0.0),
            blur: None,
            spread: Some(Length::Px(40.0)),
            color: Some(CssColor::String("#ff0000".into())),
            inset: None,
        }]);

        let mut root = root_node(400.0, 400.0, "#000000", vec![node]);
        let buf = render_pixels(&mut root, 400, 400);

        let right_of_the_cut = count_red_in(&buf, 400, 260, 100, 400, 300);
        assert_eq!(
            right_of_the_cut, 0,
            "clip-path clips the element itself, shadow included — unlike overflow:hidden, \
             which clips only the content and deliberately spares the node's own outset shadow"
        );
    }

    fn card_with_shadow(overflow_hidden: bool) -> BoxNode {
        let mut css = CssStyle {
            position: Some(Position::Absolute),
            left: Some(CLP::Px(50.0)),
            top: Some(CLP::Px(50.0)),
            width: Some(CSize::Length(CLP::Px(100.0))),
            height: Some(CSize::Length(CLP::Px(100.0))),
            background: Some(Background::Color(CssColor::String("#ffffff".into()))),
            box_shadow: Some(vec![BoxShadow {
                offset_x: Length::Px(0.0),
                offset_y: Length::Px(0.0),
                blur: None,
                spread: Some(Length::Px(20.0)),
                color: Some(CssColor::String("#ff0000".into())),
                inset: None,
            }]),
            ..Default::default()
        };
        if overflow_hidden {
            css.overflow = Some(Overflow::Hidden);
        }
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

    #[test]
    fn overflow_hidden_does_not_clip_own_outset_box_shadow() {
        let without = {
            let mut root = root_node(200.0, 200.0, "#000000", vec![card_with_shadow(false)]);
            render_pixels(&mut root, 200, 200)
        };
        let with_hidden = {
            let mut root = root_node(200.0, 200.0, "#000000", vec![card_with_shadow(true)]);
            render_pixels(&mut root, 200, 200)
        };

        let probe = |buf: &[u8], x: usize, y: usize| -> (u8, u8, u8) {
            let i = (y * 200 + x) * 4;
            (buf[i], buf[i + 1], buf[i + 2])
        };

        let above_plain = probe(&without, 100, 45);
        let below_plain = probe(&without, 100, 155);
        assert!(
            above_plain.0 > 200 && above_plain.1 < 50,
            "sanity: shadow halo must be visible without overflow, got {above_plain:?}"
        );
        assert!(
            below_plain.0 > 200 && below_plain.1 < 50,
            "sanity: shadow halo must be visible without overflow, got {below_plain:?}"
        );

        let above_hidden = probe(&with_hidden, 100, 45);
        let below_hidden = probe(&with_hidden, 100, 155);
        assert!(
            above_hidden.0 > 200 && above_hidden.1 < 50,
            "overflow:hidden must not erase the node's own outset shadow, got {above_hidden:?}"
        );
        assert!(
            below_hidden.0 > 200 && below_hidden.1 < 50,
            "overflow:hidden must not erase the node's own outset shadow, got {below_hidden:?}"
        );

        let halo_count_plain = count_red_in(&without, 200, 25, 25, 175, 175);
        let halo_count_hidden = count_red_in(&with_hidden, 200, 25, 25, 175, 175);
        assert_eq!(
            halo_count_plain, halo_count_hidden,
            "halo pixel count must be identical with/without overflow:hidden \
             (plain={halo_count_plain}, hidden={halo_count_hidden})"
        );
    }

    #[test]
    fn filter_layer_bounds_do_not_clip_blur_bleed() {
        let n = BoxNode {
            id: 0,
            kind: BoxKind::Container,
            css: CssStyle {
                position: Some(Position::Absolute),
                left: Some(CLP::Px(120.0)),
                top: Some(CLP::Px(120.0)),
                width: Some(CSize::Length(CLP::Px(60.0))),
                height: Some(CSize::Length(CLP::Px(60.0))),
                background: Some(Background::Color(CssColor::String("#ff0000".into()))),
                opacity: Some(0.999),
                filter: Some(vec![FilterFn::Blur {
                    radius: Length::Px(24.0),
                }]),
                ..Default::default()
            },
            children: vec![],
            intrinsic: None,
            source_path: None,
            window: None,
        };
        let mut root = root_node(300.0, 300.0, "#000000", vec![n]);
        let buf = render_pixels(&mut root, 300, 300);

        let probe = |x: usize, y: usize| -> u8 {
            let i = (y * 300 + x) * 4;
            buf[i]
        };
        let bled = probe(112, 150);
        assert!(
            bled > 15,
            "blur must bleed past the box edge under bounded SaveLayerRec, got r={bled}"
        );
        let far = probe(20, 20);
        assert_eq!(far, 0, "far corner must stay untouched, got r={far}");
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

#[cfg(test)]
mod animated_transform_tests {
    use super::*;
    use crate::css::units::LengthPercentage as CLP;

    fn layout(w: f32, h: f32) -> BoxLayout {
        BoxLayout {
            x: 0.0,
            y: 0.0,
            width: w,
            height: h,
            ..Default::default()
        }
    }

    #[test]
    fn no_transform_is_identity_with_full_opacity() {
        let css = CssStyle::default();
        let (tx, ty, scale, rotation, opacity) =
            animated_transform(&css, &layout(100.0, 100.0), (1920.0, 1080.0));
        assert_eq!(
            (tx, ty, scale, rotation, opacity),
            (0.0, 0.0, 1.0, 0.0, 1.0)
        );
    }

    #[test]
    fn single_translate_and_opacity_roundtrip() {
        let css = CssStyle {
            transform: Some(vec![TransformFn::Translate {
                x: CLP::Px(42.0),
                y: CLP::Px(-7.0),
            }]),
            opacity: Some(0.5),
            ..Default::default()
        };
        let (tx, ty, _scale, _rotation, opacity) =
            animated_transform(&css, &layout(100.0, 100.0), (1920.0, 1080.0));
        assert_eq!(tx, 42.0);
        assert_eq!(ty, -7.0);
        assert_eq!(opacity, 0.5);
    }

    #[test]
    fn uniform_scale_and_rotation() {
        let css = CssStyle {
            transform: Some(vec![
                TransformFn::Scale { x: 2.0, y: 2.0 },
                TransformFn::Rotate { deg: 30.0 },
            ]),
            ..Default::default()
        };
        let (_tx, _ty, scale, rotation, _opacity) =
            animated_transform(&css, &layout(100.0, 100.0), (1920.0, 1080.0));
        assert_eq!(scale, 2.0);
        assert_eq!(rotation, 30.0);
    }

    #[test]
    fn an_orbiting_node_at_two_different_frames_reports_two_different_positions() {
        let orbit = |angle_deg: f32| {
            let mut css = CssStyle::default();
            let (s, c) = angle_deg.to_radians().sin_cos();
            css.transform = Some(vec![TransformFn::Translate {
                x: CLP::Px(c * 100.0),
                y: CLP::Px(s * 100.0),
            }]);
            css
        };
        let l = layout(10.0, 10.0);
        let (tx0, ty0, ..) = animated_transform(&orbit(0.0), &l, (1920.0, 1080.0));
        let (tx1, ty1, ..) = animated_transform(&orbit(90.0), &l, (1920.0, 1080.0));
        assert!((tx0 - tx1).abs() > 1.0 || (ty0 - ty1).abs() > 1.0);
    }
}
