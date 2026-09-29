use std::collections::HashSet;

use rustmotion::components::box_builder::{
    build_scene_from_refs, component_kind, effective_effects, BuildAnimationCtx,
};
use rustmotion::components::connector::RoutingMode;
use rustmotion::components::intrinsic::{
    CaptionIntrinsic, GradientTextIntrinsic, RichTextIntrinsic, TableIntrinsic, TextIntrinsic,
};
use rustmotion::components::{Arrow, ChildComponent, Component, Connector, Line};
use rustmotion::core::css::style::{
    CssStyle, Position, TransformFn, TransformOrigin, WhiteSpace, MIN_LEGIBLE_FONT_RATIO,
    TEXT_AUTOFIT_MIN_FONT_PX,
};
use rustmotion::core::css::taffy_bridge::ConversionContext;
use rustmotion::core::css::units::{parse_origin_component, LengthContext, ParsedLength};
use rustmotion::core::engine::box_tree::{AvailableSpace, BoxKind, BoxNode, IntrinsicMeasure};
use rustmotion::core::engine::layout_pass::{run_layout, BoxLayout, LayoutResult};
use rustmotion::engine::animator::{resolve_props_for_effects, AnimatedProperties};
use rustmotion::engine::render;
use rustmotion::schema::{Camera, ResolvedScenario, ResolvedView, Scene, TransitionType, ViewType};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct GeometryViolation {
    pub view_index: usize,
    pub scene_index: usize,
    pub path: String,
    pub component: String,
    pub axis: Axis,
    pub kind: ViolationKind,
    pub bbox: BBox,
    pub viewport: (u32, u32),
    pub hint: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    X,
    Y,
    Both,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[allow(clippy::enum_variant_names)]
pub enum ViolationKind {
    ViewportOverflow,
    UnwrappableTextOverflow,
    ContentOverflowsBox,
    #[allow(dead_code)]
    ContentOverflowsCard,
    AnimatedTextOverflow,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct BBox {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

pub fn validate_geometry(scenario: &ResolvedScenario) -> Vec<GeometryViolation> {
    let mut violations = Vec::new();
    for (vi, view) in scenario.views.iter().enumerate() {
        for (si, scene) in view.scenes.iter().enumerate() {
            let is_world = matches!(view.view_type, ViewType::World);
            let indexed = deserialize_children_indexed(scene);
            let indexed: Vec<(usize, ChildComponent)> = if is_world {
                indexed
                    .into_iter()
                    .filter(|(_, c)| !c.is_decorative())
                    .collect()
            } else {
                indexed
            };
            let raw_indices: Vec<usize> = indexed.iter().map(|(i, _)| *i).collect();
            let children: Vec<ChildComponent> = indexed.into_iter().map(|(_, c)| c).collect();
            let viewport = (scenario.video.width, scenario.video.height);
            let viewport_f = (viewport.0 as f32, viewport.1 as f32);

            let root_css = render::root_style(scene.layout.as_ref(), view.view_type.clone());
            let built = build_scene_from_refs(children.iter(), viewport_f, root_css, None);
            let layouts = run_layout(
                &built.root,
                viewport_f,
                &ConversionContext::for_viewport(viewport_f.0, viewport_f.1),
            );

            let camera = scene
                .camera
                .as_ref()
                .filter(|_| !scene_uses_depth(&children));

            let root_bound = layouts
                .get(built.root.id)
                .map(|l| l.content_box())
                .map(|(_, _, w, h)| (w, h));

            let path_root = format!("views[{}].scenes[{}]", vi, si);
            walk(
                &children,
                &built.root.children,
                &layouts,
                viewport,
                vi,
                si,
                &path_root,
                Some(&raw_indices),
                false,
                camera,
                root_bound,
                &mut violations,
            );
        }
    }
    violations
}

fn deserialize_children_indexed(scene: &Scene) -> Vec<(usize, ChildComponent)> {
    scene
        .children
        .iter()
        .enumerate()
        .filter_map(|(i, v)| {
            serde_json::from_value::<ChildComponent>(v.clone())
                .ok()
                .map(|c| (i, c))
        })
        .collect()
}

fn scene_uses_depth(children: &[ChildComponent]) -> bool {
    children
        .iter()
        .any(|c| c.component.as_styled().style_config().depth.is_some())
}

#[allow(clippy::too_many_arguments)]
fn walk(
    children: &[ChildComponent],
    boxes: &[BoxNode],
    layouts: &LayoutResult,
    viewport: (u32, u32),
    vi: usize,
    si: usize,
    path: &str,
    path_indices: Option<&[usize]>,
    parent_clips: bool,
    camera: Option<&Camera>,
    container_bound: Option<(f32, f32)>,
    out: &mut Vec<GeometryViolation>,
) {
    let viewport_f = (viewport.0 as f32, viewport.1 as f32);
    for (i, (child, box_node)) in children.iter().zip(boxes.iter()).enumerate() {
        let json_idx = path_indices.map(|idxs| idxs[i]).unwrap_or(i);
        let child_path = format!("{}.children[{}]", path, json_idx);
        let layout = match layouts.get(box_node.id) {
            Some(l) => l,
            None => continue,
        };
        let raw_bbox = component_bbox(&child.component, layout);
        let own_bound = if box_node.css.position == Some(Position::Absolute) {
            None
        } else {
            container_bound
        };

        if !is_exempted(&child.component) {
            if !parent_clips && !bleeds(child) {
                let mut vbbox = apply_static_node_transform(&raw_bbox, &box_node.css, viewport_f);
                if let Some(cam) = camera {
                    vbbox = fold_static_camera(&vbbox, cam, viewport_f);
                }
                check_viewport(&child.component, &child_path, &vbbox, viewport, vi, si, out);
            }
            if !parent_clips && !container_clips(&child.component) {
                check_unwrappable_text(
                    &child.component,
                    &child_path,
                    layout,
                    viewport,
                    vi,
                    si,
                    out,
                );
            }
            if !parent_clips && !container_clips(&child.component) {
                check_content_overflows_box(
                    &child.component,
                    &child_path,
                    layout,
                    own_bound,
                    viewport,
                    vi,
                    si,
                    out,
                );
            }
        }

        if let Some(grandchildren) = container_children(&child.component) {
            let (_, _, cw, ch) = layout.content_box();
            walk(
                grandchildren,
                &box_node.children,
                layouts,
                viewport,
                vi,
                si,
                &child_path,
                None,
                parent_clips || container_clips(&child.component),
                camera,
                Some((cw, ch)),
                out,
            );
        }
    }
}

fn bbox_of(layout: &BoxLayout) -> BBox {
    BBox {
        x: layout.x,
        y: layout.y,
        w: layout.width,
        h: layout.height,
    }
}

const ARROW_HEAD_BBOX_PADDING: f32 = 16.0;

fn quadratic_bulge_point(x1: f32, y1: f32, x2: f32, y2: f32, curve: f32) -> (f32, f32) {
    let mid_x = (x1 + x2) / 2.0;
    let mid_y = (y1 + y2) / 2.0;
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len = (dx * dx + dy * dy).sqrt();
    if len < f32::EPSILON {
        return (mid_x, mid_y);
    }
    let perp_x = -dy / len * curve * len * 0.3;
    let perp_y = dx / len * curve * len * 0.3;
    (mid_x + perp_x, mid_y + perp_y)
}

fn arrowhead_pad(width: f32, arrow_size: f32, arrow_start: bool, arrow_end: bool) -> f32 {
    let head_pad = if arrow_start || arrow_end {
        ARROW_HEAD_BBOX_PADDING + arrow_size.max(0.0)
    } else {
        0.0
    };
    width.max(0.0) / 2.0 + head_pad
}

fn endpoint_extent(component: &Component) -> Option<(f32, f32, f32, f32, f32)> {
    match component {
        Component::Line(Line {
            x1,
            y1,
            x2,
            y2,
            width,
            ..
        }) => Some((
            x1.min(*x2),
            y1.min(*y2),
            x1.max(*x2),
            y1.max(*y2),
            width.max(0.0) / 2.0,
        )),
        Component::Arrow(Arrow {
            x1,
            y1,
            x2,
            y2,
            cp,
            cp1,
            cp2,
            curve,
            width,
            arrow_size,
            arrow_start,
            arrow_end,
            ..
        }) => {
            let mut min_x = x1.min(*x2);
            let mut max_x = x1.max(*x2);
            let mut min_y = y1.min(*y2);
            let mut max_y = y1.max(*y2);
            for p in [cp.as_ref(), cp1.as_ref(), cp2.as_ref()]
                .into_iter()
                .flatten()
            {
                min_x = min_x.min(p.x);
                max_x = max_x.max(p.x);
                min_y = min_y.min(p.y);
                max_y = max_y.max(p.y);
            }
            if cp.is_none() && cp1.is_none() && cp2.is_none() {
                if let Some(curve) = curve {
                    let (bulge_x, bulge_y) = quadratic_bulge_point(*x1, *y1, *x2, *y2, *curve);
                    min_x = min_x.min(bulge_x);
                    max_x = max_x.max(bulge_x);
                    min_y = min_y.min(bulge_y);
                    max_y = max_y.max(bulge_y);
                }
            }
            let pad = arrowhead_pad(*width, *arrow_size, *arrow_start, *arrow_end);
            Some((min_x, min_y, max_x, max_y, pad))
        }
        Component::Connector(Connector {
            from,
            to,
            routing,
            curvature,
            width,
            arrow_size,
            arrow_start,
            arrow_end,
            ..
        }) => {
            let mut min_x = from.x.min(to.x);
            let mut max_x = from.x.max(to.x);
            let mut min_y = from.y.min(to.y);
            let mut max_y = from.y.max(to.y);
            if matches!(routing, RoutingMode::Curved) {
                let (bulge_x, bulge_y) =
                    quadratic_bulge_point(from.x, from.y, to.x, to.y, *curvature);
                min_x = min_x.min(bulge_x);
                max_x = max_x.max(bulge_x);
                min_y = min_y.min(bulge_y);
                max_y = max_y.max(bulge_y);
            }
            let pad = arrowhead_pad(*width, *arrow_size, *arrow_start, *arrow_end);
            Some((min_x, min_y, max_x, max_y, pad))
        }
        _ => None,
    }
}

fn component_bbox(component: &Component, layout: &BoxLayout) -> BBox {
    match endpoint_extent(component) {
        Some((min_x, min_y, max_x, max_y, pad)) => BBox {
            x: layout.x + min_x - pad,
            y: layout.y + min_y - pad,
            w: (max_x - min_x) + pad * 2.0,
            h: (max_y - min_y) + pad * 2.0,
        },
        None => bbox_of(layout),
    }
}

fn is_exempted(c: &Component) -> bool {
    matches!(
        c,
        Component::Marquee(_) | Component::Cursor(_) | Component::Pointer(_)
    )
}

fn bleeds(child: &ChildComponent) -> bool {
    child.bleed
}

fn container_children(c: &Component) -> Option<&[ChildComponent]> {
    match c {
        Component::Container(c) => Some(&c.children),
        _ => None,
    }
}

fn container_clips(c: &Component) -> bool {
    let style = c.as_styled().style_config();
    matches!(
        style.overflow,
        Some(
            rustmotion::core::css::style::Overflow::Hidden
                | rustmotion::core::css::style::Overflow::Clip
                | rustmotion::core::css::style::Overflow::Scroll
                | rustmotion::core::css::style::Overflow::Auto
        )
    )
}

fn apply_static_node_transform(bbox: &BBox, css: &CssStyle, viewport: (f32, f32)) -> BBox {
    let transform = match css.transform.as_deref() {
        Some(t) if !t.is_empty() => t,
        _ => return *bbox,
    };
    let ctx = LengthContext {
        viewport_width: viewport.0,
        viewport_height: viewport.1,
        parent_size: bbox.w.max(bbox.h),
        font_size: css.font_size_px_or(16.0),
        root_font_size: 16.0,
    };
    let ctx_x = LengthContext {
        parent_size: bbox.w,
        ..ctx
    };
    let ctx_y = LengthContext {
        parent_size: bbox.h,
        ..ctx
    };
    let (pivot_x, pivot_y) = resolve_transform_origin_2d(css.transform_origin.as_ref(), bbox, &ctx);
    let corners = [
        (bbox.x, bbox.y),
        (bbox.x + bbox.w, bbox.y),
        (bbox.x, bbox.y + bbox.h),
        (bbox.x + bbox.w, bbox.y + bbox.h),
    ];

    let mut min_x = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for (cx, cy) in corners {
        let (tx, ty) = apply_transform_chain(transform, cx - pivot_x, cy - pivot_y, &ctx_x, &ctx_y);
        let (wx, wy) = (pivot_x + tx, pivot_y + ty);
        min_x = min_x.min(wx);
        max_x = max_x.max(wx);
        min_y = min_y.min(wy);
        max_y = max_y.max(wy);
    }

    BBox {
        x: min_x,
        y: min_y,
        w: max_x - min_x,
        h: max_y - min_y,
    }
}

fn resolve_transform_origin_2d(
    origin: Option<&TransformOrigin>,
    bbox: &BBox,
    ctx: &LengthContext,
) -> (f32, f32) {
    let Some(o) = origin else {
        return (bbox.x + bbox.w / 2.0, bbox.y + bbox.h / 2.0);
    };
    let resolve_axis = |lp: &rustmotion::core::css::units::LengthPercentage,
                        axis_size: f32,
                        axis_origin: f32|
     -> f32 {
        let parsed = match lp {
            rustmotion::core::css::units::LengthPercentage::String(s) => {
                parse_origin_component(s).unwrap_or(ParsedLength::Percent(50.0))
            }
            rustmotion::core::css::units::LengthPercentage::Px(v) => ParsedLength::Px(*v),
        };
        let local_ctx = LengthContext {
            parent_size: axis_size,
            ..*ctx
        };
        axis_origin + parsed.resolve(&local_ctx).unwrap_or(axis_size / 2.0)
    };
    let ox =
        o.x.as_ref()
            .map(|lp| resolve_axis(lp, bbox.w, bbox.x))
            .unwrap_or(bbox.x + bbox.w / 2.0);
    let oy =
        o.y.as_ref()
            .map(|lp| resolve_axis(lp, bbox.h, bbox.y))
            .unwrap_or(bbox.y + bbox.h / 2.0);
    (ox, oy)
}

fn apply_transform_chain(
    list: &[TransformFn],
    x: f32,
    y: f32,
    ctx_x: &LengthContext,
    ctx_y: &LengthContext,
) -> (f32, f32) {
    let (mut x, mut y) = (x, y);
    for f in list.iter().rev() {
        let (nx, ny) = match f {
            TransformFn::Translate { x: tx, y: ty } => {
                (x + tx.resolve(ctx_x), y + ty.resolve(ctx_y))
            }
            TransformFn::TranslateX { x: tx } => (x + tx.resolve(ctx_x), y),
            TransformFn::TranslateY { y: ty } => (x, y + ty.resolve(ctx_y)),
            TransformFn::Translate3d { x: tx, y: ty, .. } => {
                (x + tx.resolve(ctx_x), y + ty.resolve(ctx_y))
            }
            TransformFn::Scale { x: sx, y: sy } => (x * sx, y * sy),
            TransformFn::ScaleX { x: sx } => (x * sx, y),
            TransformFn::ScaleY { y: sy } => (x, y * sy),
            TransformFn::Rotate { deg } | TransformFn::RotateZ { deg } => {
                let (sin, cos) = deg.to_radians().sin_cos();
                (x * cos - y * sin, x * sin + y * cos)
            }
            TransformFn::Skew { x: sx, y: sy } => {
                (x + y * sx.to_radians().tan(), y + x * sy.to_radians().tan())
            }
            TransformFn::SkewX { x: sx } => (x + y * sx.to_radians().tan(), y),
            TransformFn::SkewY { y: sy } => (x, y + x * sy.to_radians().tan()),
            _ => (x, y),
        };
        x = nx;
        y = ny;
    }
    (x, y)
}

fn fold_static_camera(bbox: &BBox, camera: &Camera, viewport: (f32, f32)) -> BBox {
    let zoom = camera.zoom;
    let (cx, cy) = camera
        .origin
        .as_ref()
        .map(|o| (o.x, o.y))
        .unwrap_or((viewport.0 / 2.0, viewport.1 / 2.0));
    let new_x = zoom * bbox.x + (1.0 - zoom) * cx - zoom * camera.x;
    let new_y = zoom * bbox.y + (1.0 - zoom) * cy - zoom * camera.y;
    BBox {
        x: new_x,
        y: new_y,
        w: bbox.w * zoom,
        h: bbox.h * zoom,
    }
}

fn check_viewport(
    component: &Component,
    path: &str,
    bbox: &BBox,
    viewport: (u32, u32),
    vi: usize,
    si: usize,
    out: &mut Vec<GeometryViolation>,
) {
    let vw = viewport.0 as f32;
    let vh = viewport.1 as f32;
    let right = bbox.x + bbox.w;
    let bottom = bbox.y + bbox.h;
    let eps = 0.5;

    let x_over = bbox.x < -eps || right > vw + eps;
    let y_over = bbox.y < -eps || bottom > vh + eps;

    if !x_over && !y_over {
        return;
    }
    let axis = match (x_over, y_over) {
        (true, true) => Axis::Both,
        (true, false) => Axis::X,
        (false, true) => Axis::Y,
        (false, false) => return,
    };
    out.push(GeometryViolation {
        view_index: vi,
        scene_index: si,
        path: path.to_string(),
        component: component_kind(component).to_string(),
        axis,
        kind: ViolationKind::ViewportOverflow,
        bbox: *bbox,
        viewport,
        hint: hint_for_viewport(component, axis, bbox, viewport),
    });
}

fn hint_for_viewport(component: &Component, axis: Axis, bbox: &BBox, vp: (u32, u32)) -> String {
    let vw = vp.0 as f32;
    let vh = vp.1 as f32;
    match component_kind(component) {
        "text" | "rich_text" | "gradient_text" | "caption" => {
            "allow the text to wrap (remove style.white-space: nowrap) or reduce style.font-size"
                .to_string()
        }
        "counter" => format!(
            "card width must be ≥ {:.0}px (counter natural width)",
            bbox.w
        ),
        _ => {
            let eps = 0.5;
            match axis {
                Axis::X if bbox.x < -eps => {
                    format!(
                        "shift x to fit [0..{:.0}], current left edge is {:.0}",
                        vw, bbox.x
                    )
                }
                Axis::X => format!(
                    "shift x to fit [0..{:.0}], current right edge is {:.0}",
                    vw,
                    bbox.x + bbox.w
                ),
                Axis::Y if bbox.y < -eps => {
                    format!(
                        "shift y to fit [0..{:.0}], current top edge is {:.0}",
                        vh, bbox.y
                    )
                }
                Axis::Y => format!(
                    "shift y to fit [0..{:.0}], current bottom edge is {:.0}",
                    vh,
                    bbox.y + bbox.h
                ),
                Axis::Both => "reposition the component to stay inside the viewport".to_string(),
            }
        }
    }
}

fn measurer_and_nowrap(component: &Component) -> Option<(Box<dyn IntrinsicMeasure>, bool)> {
    fn is_nowrap(ws: &Option<WhiteSpace>) -> bool {
        matches!(ws, Some(WhiteSpace::Nowrap | WhiteSpace::Pre))
    }
    match component {
        Component::Text(t) => Some((
            Box::new(TextIntrinsic::from_text(t)),
            is_nowrap(&t.style.white_space),
        )),
        Component::GradientText(t) => Some((
            Box::new(GradientTextIntrinsic::from_gradient_text(t)),
            is_nowrap(&t.style.white_space),
        )),
        #[allow(deprecated)]
        Component::Caption(c) => Some((
            Box::new(CaptionIntrinsic::from_caption(c)),
            is_nowrap(&c.style.white_space),
        )),
        Component::RichText(rt) => Some((Box::new(RichTextIntrinsic::from_rich_text(rt)), false)),
        Component::Table(t) => Some((Box::new(TableIntrinsic::from_table(t)), false)),
        _ => None,
    }
}

fn check_unwrappable_text(
    component: &Component,
    path: &str,
    layout: &BoxLayout,
    viewport: (u32, u32),
    vi: usize,
    si: usize,
    out: &mut Vec<GeometryViolation>,
) {
    let Some((intrinsic, nowrap)) = measurer_and_nowrap(component) else {
        return;
    };
    if !nowrap {
        return;
    }
    let (cx, cy, cw, ch) = layout.content_box();
    let (natural_w, _) = intrinsic.measure(
        (None, None),
        (AvailableSpace::Definite(cw), AvailableSpace::MaxContent),
    );
    if natural_w > cw + 0.5 {
        let kind = component_kind(component);
        out.push(GeometryViolation {
            view_index: vi,
            scene_index: si,
            path: path.to_string(),
            component: kind.to_string(),
            axis: Axis::X,
            kind: ViolationKind::UnwrappableTextOverflow,
            bbox: BBox {
                x: cx,
                y: cy,
                w: cw,
                h: ch,
            },
            viewport,
            hint: format!(
                "{kind} natural width is {natural_w:.0}px but only {:.0}px available — remove style.white-space: nowrap (or set it to normal) so it can wrap, or reduce style.font-size",
                cw
            ),
        });
    }
}

fn check_content_overflows_box(
    component: &Component,
    path: &str,
    layout: &BoxLayout,
    container_bound: Option<(f32, f32)>,
    viewport: (u32, u32),
    vi: usize,
    si: usize,
    out: &mut Vec<GeometryViolation>,
) {
    let Some((intrinsic, nowrap)) = measurer_and_nowrap(component) else {
        return;
    };

    let (cx, cy, cw, ch) = layout.content_box();
    let (cw, ch) = match container_bound {
        Some((bw, bh)) => (cw.min(bw), ch.min(bh)),
        None => (cw, ch),
    };
    if cw <= 0.0 || ch <= 0.0 {
        return;
    }

    if nowrap {
        let (_, natural_h) = intrinsic.measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::Definite(ch)),
        );
        let eps = 0.5;
        if natural_h <= ch + eps {
            return;
        }
        let kind = component_kind(component);
        out.push(GeometryViolation {
            view_index: vi,
            scene_index: si,
            path: path.to_string(),
            component: kind.to_string(),
            axis: Axis::Y,
            kind: ViolationKind::ContentOverflowsBox,
            bbox: BBox {
                x: cx,
                y: cy,
                w: cw,
                h: ch,
            },
            viewport,
            hint: format!(
                "{kind} line is {natural_h:.0}px tall but its box is only {:.0}px tall — increase style.height (or the parent's), or reduce style.font-size",
                ch
            ),
        });
        return;
    }

    let (measured_w, measured_h) = intrinsic.measure(
        (None, None),
        (AvailableSpace::Definite(cw), AvailableSpace::Definite(ch)),
    );

    let eps = 0.5;
    let x_over = measured_w > cw + eps;
    let y_over = measured_h > ch + eps;
    if !x_over && !y_over {
        return;
    }
    let axis = match (x_over, y_over) {
        (true, true) => Axis::Both,
        (true, false) => Axis::X,
        (false, true) => Axis::Y,
        (false, false) => return,
    };
    let content_bbox = BBox {
        x: cx,
        y: cy,
        w: cw,
        h: ch,
    };
    let kind = component_kind(component);
    let hint = if y_over && !x_over {
        format!(
            "{kind} wraps to {:.0}px tall at this width but its box is only {:.0}px tall — increase style.height (or the parent's), reduce style.font-size, or shorten the content",
            measured_h, ch
        )
    } else if x_over && !y_over {
        format!(
            "{kind} content needs {:.0}px but the box is only {:.0}px wide — widen the box, reduce style.font-size, or (for text) insert a break (e.g. a space) in a long token",
            measured_w, cw
        )
    } else {
        format!(
            "{kind} content needs {:.0}×{:.0}px but its box is only {:.0}×{:.0}px — widen/heighten the box or reduce style.font-size",
            measured_w, measured_h, cw, ch
        )
    };
    out.push(GeometryViolation {
        view_index: vi,
        scene_index: si,
        path: path.to_string(),
        component: kind.to_string(),
        axis,
        kind: ViolationKind::ContentOverflowsBox,
        bbox: content_bbox,
        viewport,
        hint,
    });
}

pub fn check_legibility(scenario: &ResolvedScenario) -> Vec<String> {
    let mut warnings = Vec::new();
    let video_h = scenario.video.height as f32;
    if video_h <= 0.0 {
        return warnings;
    }
    let min_px = MIN_LEGIBLE_FONT_RATIO * video_h;

    for (vi, view) in scenario.views.iter().enumerate() {
        for (si, scene) in view.scenes.iter().enumerate() {
            let indexed = deserialize_children_indexed(scene);
            let path_root = format!("views[{}].scenes[{}]", vi, si);
            for (json_idx, child) in &indexed {
                let path = format!("{}.children[{}]", path_root, json_idx);
                walk_legibility(&child.component, &path, min_px, video_h, &mut warnings);
            }
        }
    }
    warnings
}

fn walk_legibility(
    component: &Component,
    path: &str,
    min_px: f32,
    video_h: f32,
    out: &mut Vec<String>,
) {
    for (label, effective_px) in text_sizes(component) {
        if effective_px < min_px - 0.05 {
            out.push(format!(
                "{path}: {label} renders at ~{effective_px:.0}px on a {video_h:.0}px-tall frame \
                 ({:.2}% of height) — likely illegible once the video is viewed at anything less \
                 than native resolution. Raise the effective font size to at least {min_px:.0}px \
                 (~{:.1}% of height).",
                effective_px / video_h * 100.0,
                MIN_LEGIBLE_FONT_RATIO * 100.0,
            ));
        }
    }

    if declares_text_autofit(component) && TEXT_AUTOFIT_MIN_FONT_PX < min_px - 0.05 {
        out.push(format!(
            "{path}: text-autofit may shrink this text to ~{TEXT_AUTOFIT_MIN_FONT_PX:.0}px, below \
             the {min_px:.0}px legibility floor for a {video_h:.0}px-tall frame. Give it a wider \
             or taller box so it settles above that, or check the rendered frame.",
        ));
    }

    if let Some(children) = container_children(component) {
        for (i, child) in children.iter().enumerate() {
            walk_legibility(
                &child.component,
                &format!("{path}.children[{i}]"),
                min_px,
                video_h,
                out,
            );
        }
    }
}

fn declares_text_autofit(component: &Component) -> bool {
    match component {
        Component::Text(t) => matches!(t.style.text_autofit, Some(true)),
        Component::GradientText(t) => matches!(t.style.text_autofit, Some(true)),
        _ => false,
    }
}

#[allow(deprecated)]
fn text_sizes(component: &Component) -> Vec<(&'static str, f32)> {
    match component {
        Component::Text(t) => vec![("text", t.style.font_size_px_or(48.0))],
        Component::RichText(t) => vec![("rich_text", t.style.font_size_px_or(48.0))],
        Component::GradientText(t) => vec![("gradient_text", t.style.font_size_px_or(48.0))],
        Component::Caption(t) => vec![("caption", t.style.font_size_px_or(48.0))],
        Component::Counter(c) => vec![("counter", c.style.font_size_px_or(48.0))],
        Component::Table(t) => vec![("table", t.style.font_size_px_or(14.0))],
        Component::PillNav(p) => vec![("pill_nav", p.style.font_size_px_or(14.0))],
        Component::Callout(c) => vec![("callout", c.style.font_size_px_or(16.0))],
        Component::List(l) => vec![("list", l.style.font_size_px_or(16.0))],
        Component::Kbd(k) => vec![("kbd", k.style.font_size_px_or(k.font_size))],
        Component::Tooltip(t) => vec![("tooltip", t.style.font_size_px_or(t.font_size))],
        Component::Marquee(m) => vec![("marquee", m.style.font_size_px_or(m.font_size))],
        Component::Badge(b) => {
            let default_fs = match b.badge_size {
                rustmotion::components::badge::BadgeSize::Sm => 12.0,
                rustmotion::components::badge::BadgeSize::Md => 14.0,
                rustmotion::components::badge::BadgeSize::Lg => 18.0,
            };
            vec![("badge", b.style.font_size_px_or(default_fs))]
        }
        _ => vec![],
    }
}

const ANIM_SAMPLES_PER_SECOND: f64 = 8.0;
const ANIM_MIN_SAMPLES: usize = 5;
const ANIM_MAX_SAMPLES: usize = 480;

fn anim_sample_times(scene_duration: f64) -> Vec<f64> {
    if scene_duration <= 0.0 {
        return vec![0.0];
    }
    let raw = (scene_duration * ANIM_SAMPLES_PER_SECOND).ceil() as usize;
    let n = raw.clamp(ANIM_MIN_SAMPLES, ANIM_MAX_SAMPLES);
    if n <= 1 {
        return vec![0.0];
    }
    (0..n)
        .map(|i| scene_duration * (i as f64) / ((n - 1) as f64))
        .collect()
}

pub fn validate_geometry_animated(scenario: &ResolvedScenario) -> Vec<GeometryViolation> {
    let mut violations = Vec::new();
    let mut seen: HashSet<(usize, usize, String)> = HashSet::new();
    let fps = scenario.video.fps;
    for (vi, view) in scenario.views.iter().enumerate() {
        for (si, scene) in view.scenes.iter().enumerate() {
            let (children, raw_indices) = scene_geometry_children(view, scene);
            let viewport = (scenario.video.width, scenario.video.height);

            let camera = scene
                .camera
                .as_ref()
                .filter(|_| !scene_uses_depth(&children));

            let path_root = format!("views[{}].scenes[{}]", vi, si);
            let scene_duration = scene.duration;
            let sample_until = scene
                .freeze_at
                .map_or(scene_duration, |f| f.clamp(0.0, scene_duration));

            for time in anim_sample_times(sample_until) {
                sample_scene_geometry(
                    scene,
                    &children,
                    &raw_indices,
                    view,
                    vi,
                    si,
                    viewport,
                    camera,
                    &path_root,
                    fps,
                    time,
                    time,
                    None,
                    &mut seen,
                    &mut violations,
                );
            }
        }
    }
    violations
}

fn scene_geometry_children(
    view: &ResolvedView,
    scene: &Scene,
) -> (Vec<ChildComponent>, Vec<usize>) {
    let is_world = matches!(view.view_type, ViewType::World);
    let indexed = deserialize_children_indexed(scene);
    let indexed: Vec<(usize, ChildComponent)> = if is_world {
        indexed
            .into_iter()
            .filter(|(_, c)| !c.is_decorative())
            .collect()
    } else {
        indexed
    };
    let raw_indices: Vec<usize> = indexed.iter().map(|(i, _)| *i).collect();
    let children: Vec<ChildComponent> = indexed.into_iter().map(|(_, c)| c).collect();
    (children, raw_indices)
}

#[allow(clippy::too_many_arguments)]
fn sample_scene_geometry(
    scene: &Scene,
    children: &[ChildComponent],
    raw_indices: &[usize],
    view: &ResolvedView,
    vi: usize,
    si: usize,
    viewport: (u32, u32),
    camera: Option<&Camera>,
    path_root: &str,
    fps: u32,
    time: f64,
    scenario_time: f64,
    transition_label: Option<&str>,
    seen: &mut HashSet<(usize, usize, String)>,
    out: &mut Vec<GeometryViolation>,
) {
    let viewport_f = (viewport.0 as f32, viewport.1 as f32);
    let root_css = render::root_style(scene.layout.as_ref(), view.view_type.clone());
    let anim = Some(BuildAnimationCtx {
        time,
        scenario_time,
        scene_duration: scene.duration,
        fps,
    });
    let built = build_scene_from_refs(children.iter(), viewport_f, root_css, anim);
    let layouts = run_layout(
        &built.root,
        viewport_f,
        &ConversionContext::for_viewport(viewport_f.0, viewport_f.1),
    );

    walk_anim(
        children,
        &built.root.children,
        &layouts,
        &built.stagger_delays,
        &built.time_params,
        viewport,
        vi,
        si,
        path_root,
        Some(raw_indices),
        false,
        camera,
        transition_label,
        time,
        scene.duration,
        seen,
        out,
    );
}

fn principal_boxes(boxes: &[BoxNode]) -> impl Iterator<Item = &BoxNode> {
    boxes
        .iter()
        .filter(|b| !matches!(b.kind, BoxKind::Ghost(_)))
}

#[allow(clippy::too_many_arguments)]
fn walk_anim(
    children: &[ChildComponent],
    boxes: &[BoxNode],
    layouts: &LayoutResult,
    stagger_delays: &[f64],
    time_params: &[(f64, f64)],
    viewport: (u32, u32),
    vi: usize,
    si: usize,
    path: &str,
    path_indices: Option<&[usize]>,
    parent_clips: bool,
    camera: Option<&Camera>,
    transition_label: Option<&str>,
    time: f64,
    scene_duration: f64,
    seen: &mut HashSet<(usize, usize, String)>,
    out: &mut Vec<GeometryViolation>,
) {
    let viewport_f = (viewport.0 as f32, viewport.1 as f32);
    for (i, (child, box_node)) in children.iter().zip(principal_boxes(boxes)).enumerate() {
        let json_idx = path_indices.map(|idxs| idxs[i]).unwrap_or(i);
        let child_path = format!("{}.children[{}]", path, json_idx);
        let layout = match layouts.get(box_node.id) {
            Some(l) => l,
            None => continue,
        };

        let visible = box_node.window.as_ref().is_none_or(|w| w.contains(time));
        if !visible {
            continue;
        }

        if !is_exempted(&child.component)
            && !bleeds(child)
            && !parent_clips
            && layout.width > 0.5
            && layout.height > 0.5
            && box_node.css.opacity.unwrap_or(1.0) > 0.001
        {
            let stagger_delay = stagger_delays
                .get(box_node.id as usize)
                .copied()
                .unwrap_or(0.0);
            let (scale, shift) = time_params
                .get(box_node.id as usize)
                .copied()
                .unwrap_or((1.0, 0.0));
            let local_time = scale * time + shift;
            let props = match effective_effects(&child.component, stagger_delay, local_time) {
                Some(effects) => resolve_props_for_effects(&effects, local_time, scene_duration),
                None => AnimatedProperties::default(),
            };
            let raw_bbox = component_bbox(&child.component, layout);
            let mut transformed = apply_static_node_transform(&raw_bbox, &box_node.css, viewport_f);
            if let Some(overshoot) = props
                .char_animation
                .as_ref()
                .map(|c| c.overshoot.max(0.0))
                .filter(|o| *o > 1e-4)
            {
                transformed = scale_bbox_from_own_center(&transformed, 1.0 + overshoot);
            }
            if let Some(cam) = camera {
                transformed = fold_static_camera(&transformed, cam, viewport_f);
            }
            let vw = viewport.0 as f32;
            let vh = viewport.1 as f32;
            let eps = 0.5;
            let right = transformed.x + transformed.w;
            let bottom = transformed.y + transformed.h;
            let x_over = transformed.x < -eps || right > vw + eps;
            let y_over = transformed.y < -eps || bottom > vh + eps;
            if x_over || y_over {
                let axis = match (x_over, y_over) {
                    (true, true) => Axis::Both,
                    (true, false) => Axis::X,
                    (false, true) => Axis::Y,
                    _ => unreachable!(),
                };
                let key = (vi, si, child_path.clone());
                if seen.insert(key) {
                    let component_name = component_kind(&child.component).to_string();
                    out.push(GeometryViolation {
                        view_index: vi,
                        scene_index: si,
                        path: child_path.clone(),
                        component: component_name,
                        axis,
                        kind: ViolationKind::AnimatedTextOverflow,
                        bbox: transformed,
                        viewport,
                        hint: hint_for_animated(
                            &child.component,
                            &props,
                            time,
                            scene_duration,
                            transition_label,
                        ),
                    });
                }
            }
        }

        if let Some(grandchildren) = container_children(&child.component) {
            walk_anim(
                grandchildren,
                &box_node.children,
                layouts,
                stagger_delays,
                time_params,
                viewport,
                vi,
                si,
                &child_path,
                None,
                parent_clips || container_clips(&child.component),
                camera,
                transition_label,
                time,
                scene_duration,
                seen,
                out,
            );
        }
    }
}

fn scale_bbox_from_own_center(bbox: &BBox, factor: f32) -> BBox {
    let cx = bbox.x + bbox.w / 2.0;
    let cy = bbox.y + bbox.h / 2.0;
    let w = bbox.w * factor;
    let h = bbox.h * factor;
    BBox {
        x: cx - w / 2.0,
        y: cy - h / 2.0,
        w,
        h,
    }
}

fn hint_for_animated(
    component: &Component,
    props: &AnimatedProperties,
    time: f64,
    scene_duration: f64,
    transition_label: Option<&str>,
) -> String {
    let ratio = if scene_duration > 1e-6 {
        time / scene_duration
    } else {
        0.0
    };
    let base = format!(
        "at t={:.2}s ({:.0}% of scene), animation transforms (tx={:.0}, ty={:.0}, sx={:.2}, sy={:.2}) push the bbox out of the viewport",
        time,
        ratio * 100.0,
        props.translate_x,
        props.translate_y,
        props.scale_x,
        props.scale_y,
    );
    let msg = match component_kind(component) {
        "text" | "rich_text" | "gradient_text" | "caption" | "counter" => format!(
            "{} — reduce font_size, soften the preset (e.g. fade_in instead of slide_in_left), or add max_width",
            base
        ),
        _ => format!("{} — soften the preset or pull the resting position inward", base),
    };
    match transition_label {
        Some(label) => format!("{label}: {msg}"),
        None => msg,
    }
}

fn clamp_to_scene_freeze(scene: &Scene, raw: f64) -> f64 {
    match scene.freeze_at {
        Some(freeze_at) if raw > freeze_at => freeze_at,
        _ => raw,
    }
}

pub fn validate_geometry_transitions(scenario: &ResolvedScenario) -> Vec<GeometryViolation> {
    use rustmotion::encode::video::FrameTask;

    let mut violations = Vec::new();
    let fps = scenario.video.fps;
    if fps == 0 {
        return violations;
    }
    let tasks = rustmotion::encode::build_frame_tasks(scenario);
    let mut seen: HashSet<(usize, usize, String)> = HashSet::new();

    for task in &tasks {
        match task {
            FrameTask::SlideTransition {
                global_frame,
                view_idx,
                scene_a_idx,
                scene_b_idx,
                frame_in_transition,
                scene_a_frame_offset,
                scene_a_frame_advance,
                transition_type,
                ..
            } => {
                if matches!(transition_type, TransitionType::CameraPan) {
                    continue;
                }

                let view = &scenario.views[*view_idx];
                let scene_a = &view.scenes[*scene_a_idx];
                let scene_b = &view.scenes[*scene_b_idx];
                let scenario_time = *global_frame as f64 / fps as f64;

                let frame_a_idx = if *scene_a_frame_advance {
                    scene_a_frame_offset + frame_in_transition
                } else {
                    *scene_a_frame_offset
                };
                let time_a = clamp_to_scene_freeze(scene_a, frame_a_idx as f64 / fps as f64);
                let time_b =
                    clamp_to_scene_freeze(scene_b, *frame_in_transition as f64 / fps as f64);

                let label_a = format!(
                    "SlideTransition scene {scene_a_idx}->{scene_b_idx}, frame {frame_in_transition}, \
                     outgoing side (its own local time reaches {time_a:.3}s)"
                );
                let label_b = format!(
                    "SlideTransition scene {scene_a_idx}->{scene_b_idx}, frame {frame_in_transition}, \
                     incoming side (local time {time_b:.3}s)"
                );

                let (children_a, raw_indices_a) = scene_geometry_children(view, scene_a);
                let camera_a = scene_a
                    .camera
                    .as_ref()
                    .filter(|_| !scene_uses_depth(&children_a));
                let path_root_a = format!("views[{view_idx}].scenes[{scene_a_idx}]");
                sample_scene_geometry(
                    scene_a,
                    &children_a,
                    &raw_indices_a,
                    view,
                    *view_idx,
                    *scene_a_idx,
                    (scenario.video.width, scenario.video.height),
                    camera_a,
                    &path_root_a,
                    fps,
                    time_a,
                    scenario_time,
                    Some(&label_a),
                    &mut seen,
                    &mut violations,
                );

                let (children_b, raw_indices_b) = scene_geometry_children(view, scene_b);
                let camera_b = scene_b
                    .camera
                    .as_ref()
                    .filter(|_| !scene_uses_depth(&children_b));
                let path_root_b = format!("views[{view_idx}].scenes[{scene_b_idx}]");
                sample_scene_geometry(
                    scene_b,
                    &children_b,
                    &raw_indices_b,
                    view,
                    *view_idx,
                    *scene_b_idx,
                    (scenario.video.width, scenario.video.height),
                    camera_b,
                    &path_root_b,
                    fps,
                    time_b,
                    scenario_time,
                    Some(&label_b),
                    &mut seen,
                    &mut violations,
                );
            }
            FrameTask::ViewTransition {
                global_frame,
                view_a_idx,
                view_b_idx,
                ..
            } => {
                let scenario_time = *global_frame as f64 / fps as f64;

                let view_a = &scenario.views[*view_a_idx];
                if matches!(view_a.view_type, ViewType::Slide) {
                    if let Some(last_idx) = view_a.scenes.len().checked_sub(1) {
                        let scene = &view_a.scenes[last_idx];
                        let scene_frames = (scene.duration * fps as f64).round() as u32;
                        let time = clamp_to_scene_freeze(
                            scene,
                            scene_frames.saturating_sub(1) as f64 / fps as f64,
                        );
                        let label = format!(
                            "ViewTransition view {view_a_idx}->{view_b_idx}, outgoing view's last frame"
                        );
                        let (children, raw_indices) = scene_geometry_children(view_a, scene);
                        let camera = scene
                            .camera
                            .as_ref()
                            .filter(|_| !scene_uses_depth(&children));
                        let path_root = format!("views[{view_a_idx}].scenes[{last_idx}]");
                        sample_scene_geometry(
                            scene,
                            &children,
                            &raw_indices,
                            view_a,
                            *view_a_idx,
                            last_idx,
                            (scenario.video.width, scenario.video.height),
                            camera,
                            &path_root,
                            fps,
                            time,
                            scenario_time,
                            Some(&label),
                            &mut seen,
                            &mut violations,
                        );
                    }
                }

                let view_b = &scenario.views[*view_b_idx];
                if matches!(view_b.view_type, ViewType::Slide) {
                    if let Some(scene) = view_b.scenes.first() {
                        let time = clamp_to_scene_freeze(scene, 0.0);
                        let label = format!(
                            "ViewTransition view {view_a_idx}->{view_b_idx}, incoming view's first frame"
                        );
                        let (children, raw_indices) = scene_geometry_children(view_b, scene);
                        let camera = scene
                            .camera
                            .as_ref()
                            .filter(|_| !scene_uses_depth(&children));
                        let path_root = format!("views[{view_b_idx}].scenes[0]");
                        sample_scene_geometry(
                            scene,
                            &children,
                            &raw_indices,
                            view_b,
                            *view_b_idx,
                            0,
                            (scenario.video.width, scenario.video.height),
                            camera,
                            &path_root,
                            fps,
                            time,
                            scenario_time,
                            Some(&label),
                            &mut seen,
                            &mut violations,
                        );
                    }
                }
            }
            _ => {}
        }
    }

    violations
}

pub fn format_violation(v: &GeometryViolation) -> String {
    let axis_str = match v.axis {
        Axis::X => "x",
        Axis::Y => "y",
        Axis::Both => "x+y",
    };
    let kind_str = match v.kind {
        ViolationKind::ViewportOverflow => "viewport overflow",
        ViolationKind::UnwrappableTextOverflow => "wrap=false but text too wide",
        ViolationKind::ContentOverflowsBox => "wrapped content exceeds its own box",
        ViolationKind::ContentOverflowsCard => "component extends past its containing card",
        ViolationKind::AnimatedTextOverflow => "animation pushes content outside viewport",
    };
    format!(
        "ERROR: {} ({})\n  view: {}, scene: {}\n  path: {}\n  bbox: [{:.0}, {:.0}] -> [{:.0}, {:.0}]   (viewport: {}x{})\n  axis: {}\n  hint: {}",
        v.component,
        kind_str,
        v.view_index,
        v.scene_index,
        v.path,
        v.bbox.x,
        v.bbox.y,
        v.bbox.x + v.bbox.w,
        v.bbox.y + v.bbox.h,
        v.viewport.0,
        v.viewport.1,
        axis_str,
        v.hint,
    )
}

pub fn check_off_grid_cuts(scenario: &ResolvedScenario) -> Vec<String> {
    use rustmotion::encode::video::FrameTask;
    use std::collections::HashMap;

    let mut warnings = Vec::new();
    let fps = scenario.video.fps;
    if fps == 0 {
        return warnings;
    }

    let tasks = rustmotion::encode::build_frame_tasks(scenario);
    warnings.extend(rustmotion::encode::video::v2_dropped_transition_warnings(
        scenario,
    ));

    let mut cut_frame: HashMap<(usize, usize), u32> = HashMap::new();
    for task in &tasks {
        match task {
            FrameTask::SlideTransition {
                global_frame,
                view_idx,
                scene_b_idx,
                frame_in_transition: 0,
                ..
            } => {
                cut_frame
                    .entry((*view_idx, *scene_b_idx))
                    .or_insert(*global_frame);
            }
            FrameTask::Normal {
                global_frame,
                view_idx,
                scene_idx,
                ..
            } => {
                cut_frame
                    .entry((*view_idx, *scene_idx))
                    .or_insert(*global_frame);
            }
            _ => {}
        }
    }

    for (vi, view) in scenario.views.iter().enumerate() {
        for (si, scene) in view.scenes.iter().enumerate() {
            if si == 0 {
                continue;
            }
            let Some(bpm) = scene.resolved_time_ctx.bpm else {
                continue;
            };
            if bpm <= 0.0 {
                continue;
            }
            let Some(&frame) = cut_frame.get(&(vi, si)) else {
                continue;
            };
            let time = frame as f64 / fps as f64;
            let beat_offset = scene.resolved_time_ctx.beat_offset;
            let beat_len = 60.0 / bpm;
            let nearest_beat_n = ((time - beat_offset) / beat_len).round();
            let nearest_beat = beat_offset + nearest_beat_n * beat_len;
            let drift = (time - nearest_beat).abs();
            let tolerance = 0.5 / fps as f64;
            if drift > tolerance {
                warnings.push(format!(
                    "views[{vi}].scenes[{si}]: off_grid_cut — this cut lands at {time:.3}s, \
                     {drift:.3}s off the nearest beat ({nearest_beat:.3}s at {bpm} bpm). Set \
                     `snap: \"beat\"` on the scenario, or give this scene an explicit `at` on \
                     the grid, for a rhythmic edit."
                ));
            }
        }
    }

    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmotion::loader::load_scenario_from_source;

    fn parse(json: &str) -> rustmotion::schema::ResolvedScenario {
        load_scenario_from_source(None, Some(json)).expect("scenario parses")
    }

    #[test]
    fn clean_scenario_has_no_violations() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "shape",
                    "shape": "rect",
                    "size": { "width": 100, "height": 80 },
                    "x": 10, "y": 10,
                    "fill": "#ff0000"
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        assert!(
            violations.is_empty(),
            "expected clean, got: {:?}",
            violations
        );
    }

    const V2_TAIL_CONTINUE_TRANSITION_JSON: &str = r##"{
        "video": { "width": 640, "height": 360, "fps": 30 },
        "timing": "v2",
        "scenes": [
            {
                "duration": 1.0,
                "tail": "continue",
                "children": [{
                    "type": "text",
                    "content": "EDGE",
                    "position": "absolute",
                    "x": 460, "y": 140,
                    "style": {
                        "color": "#ffffff",
                        "font-size": "40px",
                        "white-space": "nowrap",
                        "animation": [{
                            "name": "keyframes",
                            "delay": 0,
                            "duration": 2.0,
                            "keyframes": [{
                                "property": "position.x",
                                "keyframes": [
                                    { "time": 0.0, "value": 0 },
                                    { "time": 2.0, "value": -900 }
                                ],
                                "easing": "linear"
                            }]
                        }]
                    }
                }]
            },
            {
                "duration": 1.0,
                "transition": { "type": "fade", "duration": 0.5 },
                "children": []
            }
        ]
    }"##;

    #[test]
    fn validate_geometry_animated_misses_the_v2_tail_continue_transition_overflow() {
        let scenario = parse(V2_TAIL_CONTINUE_TRANSITION_JSON);
        let violations = validate_geometry_animated(&scenario);
        assert!(
            violations.is_empty(),
            "this is exactly the blind spot #334 names: `validate_geometry_animated` only \
             samples within [0, scene_duration], so it never sees this scene sliding further \
             left during the transition overlap its own `tail: \"continue\"` grants it. A \
             non-empty result here means the blind spot has already been closed some other \
             way and this demonstration needs a new repro: {:?}",
            violations
        );
    }

    #[test]
    fn validate_geometry_transitions_catches_the_v2_tail_continue_transition_overflow() {
        let scenario = parse(V2_TAIL_CONTINUE_TRANSITION_JSON);
        let violations = validate_geometry_transitions(&scenario);
        assert!(
            !violations.is_empty(),
            "expected the transition-frame sampler to catch the overflow \
             validate_geometry_animated misses"
        );
        let v = &violations[0];
        assert_eq!(v.component, "text");
        assert_eq!(v.kind, ViolationKind::AnimatedTextOverflow);
        assert!(
            matches!(v.axis, Axis::X | Axis::Both),
            "expected an X-axis (or both) overflow, got {:?}",
            v.axis
        );
        assert!(
            v.bbox.x < -0.5,
            "expected the box to have slid past the left edge, got x={}",
            v.bbox.x
        );
        assert_eq!(v.view_index, 0);
        assert_eq!(
            v.scene_index, 0,
            "the overflowing side is scene 0 (the outgoing/`tail: continue` scene), not scene 1"
        );
        assert!(
            v.hint.contains("SlideTransition"),
            "hint should name the transition frame this was sampled from, not read like an \
             ordinary in-scene sample: {}",
            v.hint
        );
    }

    #[test]
    fn camera_pan_slide_transition_sides_are_not_reported() {
        let json = r##"{
            "video": { "width": 640, "height": 360 },
            "scenes": [
                {
                    "duration": 1.0,
                    "world-position": { "x": 0, "y": 0 },
                    "children": [{
                        "type": "shape", "shape": "rect",
                        "size": { "width": 100, "height": 80 },
                        "x": 270, "y": 140, "fill": "#ff0000"
                    }]
                },
                {
                    "duration": 1.0,
                    "world-position": { "x": 500, "y": 0 },
                    "transition": { "type": "camera_pan", "duration": 0.5 },
                    "children": [{
                        "type": "shape", "shape": "rect",
                        "size": { "width": 100, "height": 80 },
                        "x": 270, "y": 140, "fill": "#00ff00"
                    }]
                }
            ]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry_transitions(&scenario);
        assert!(
            violations.is_empty(),
            "a camera_pan transition's sides must be skipped outright, not bounds-checked: {:?}",
            violations
        );
    }

    #[test]
    fn layoutless_world_scene_uses_the_centred_root_not_the_slide_default() {
        let json = r##"{
            "video": { "width": 800, "height": 600 },
            "composition": [{
                "type": "world",
                "scenes": [{
                    "duration": 1.0,
                    "children": [{
                        "type": "shape",
                        "shape": "rect",
                        "style": { "width": "1000px", "height": "100px" },
                        "fill": "#ff0000"
                    }]
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        let v = violations
            .iter()
            .find(|v| v.kind == ViolationKind::ViewportOverflow)
            .unwrap_or_else(|| panic!("expected a ViewportOverflow: {:?}", violations));
        assert_eq!(v.axis, Axis::X, "{:?}", v);
        assert!(
            (v.bbox.x - (-100.0)).abs() < 1.0,
            "expected the shape centred at x=-100 (world root), got bbox.x={}: {:?}",
            v.bbox.x,
            v
        );
    }

    #[test]
    fn shape_past_right_edge_triggers_x_overflow() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "shape",
                    "shape": "rect",
                    "style": { "width": "400px", "height": "100px" },
                    "position": "absolute",
                    "x": 1700, "y": 100,
                    "fill": "#ff0000"
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        let viewport = violations
            .iter()
            .find(|v| v.kind == ViolationKind::ViewportOverflow);
        assert!(
            viewport.is_some(),
            "missing viewport violation in {:?}",
            violations
        );
        let v = viewport.unwrap();
        assert_eq!(
            v.axis,
            Axis::X,
            "expected X-axis overflow, got {:?}",
            v.axis
        );
        assert_eq!(v.component, "shape");
        assert!(
            v.path.contains("children[0]"),
            "path missing index: {}",
            v.path
        );
    }

    #[test]
    fn unwrappable_text_in_narrow_card_is_flagged() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "card",
                    "x": 100, "y": 100,
                    "style": { "width": "200px", "height": "200px", "background": "#222244" },
                    "children": [{
                        "type": "text",
                        "content": "this string is too long to fit",
                        "style": { "color": "#ffffff", "font-size": "96px", "white-space": "nowrap" }
                    }]
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        let v = violations
            .iter()
            .find(|v| v.kind == ViolationKind::UnwrappableTextOverflow);
        assert!(
            v.is_some(),
            "missing UnwrappableTextOverflow in {:?}",
            violations
        );
        let v = v.unwrap();
        assert_eq!(v.component, "text");
        assert_eq!(v.axis, Axis::X);
    }

    #[test]
    fn unwrappable_text_is_suppressed_under_a_clipping_ancestor_card() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "card",
                    "x": 100, "y": 100,
                    "style": { "width": "200px", "height": "200px", "background": "#222244", "overflow": "hidden" },
                    "children": [{
                        "type": "text",
                        "content": "this string is too long to fit",
                        "style": { "color": "#ffffff", "font-size": "96px", "white-space": "nowrap" }
                    }]
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        assert!(
            violations
                .iter()
                .all(|v| v.kind != ViolationKind::UnwrappableTextOverflow),
            "nowrap text clipped by its own card must not be flagged: {:?}",
            violations
        );
    }

    #[test]
    fn unwrappable_text_still_fires_without_a_clipping_ancestor() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "card",
                    "x": 100, "y": 100,
                    "style": { "width": "200px", "height": "200px", "background": "#222244" },
                    "children": [{
                        "type": "text",
                        "content": "this string is too long to fit",
                        "style": { "color": "#ffffff", "font-size": "96px", "white-space": "nowrap" }
                    }]
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        assert!(
            violations
                .iter()
                .any(|v| v.kind == ViolationKind::UnwrappableTextOverflow),
            "must still fire when nothing clips: {:?}",
            violations
        );
    }

    #[test]
    fn marquee_is_exempted_from_overflow() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "marquee",
                    "content": "scrolling text that bleeds",
                    "x": 1900, "y": 100
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        assert!(
            violations.iter().all(|v| v.component != "marquee"),
            "marquee should be exempt, got: {:?}",
            violations
        );
    }

    #[test]
    fn unwrappable_text_hint_does_not_recommend_the_nonexistent_wrap_field() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "card",
                    "x": 100, "y": 100,
                    "style": { "width": "200px", "height": "200px", "background": "#222244" },
                    "children": [{
                        "type": "text",
                        "content": "this string is too long to fit",
                        "style": { "color": "#ffffff", "font-size": "96px", "white-space": "nowrap" }
                    }]
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        let v = violations
            .iter()
            .find(|v| v.kind == ViolationKind::UnwrappableTextOverflow)
            .expect("violation");
        assert!(
            !v.hint.contains("wrap: true"),
            "hint still recommends the dropped `wrap` field: {}",
            v.hint
        );
        assert!(
            !v.hint.contains(" font_size"),
            "hint still names the non-existent bare `font_size` field: {}",
            v.hint
        );
        assert!(
            v.hint.contains("white-space"),
            "hint should point at the real property: {}",
            v.hint
        );
    }

    #[test]
    fn violation_path_skips_the_raw_json_index_of_a_dropped_sibling() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [
                    { "type": "not_a_real_component_kind" },
                    {
                        "type": "card",
                        "x": 100, "y": 100,
                        "style": { "width": "200px", "height": "200px", "background": "#222244" },
                        "children": [{
                            "type": "text",
                            "content": "this string is too long to fit",
                            "style": { "color": "#ffffff", "font-size": "96px", "white-space": "nowrap" }
                        }]
                    }
                ]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        let v = violations
            .iter()
            .find(|v| v.kind == ViolationKind::UnwrappableTextOverflow)
            .expect("violation");
        assert_eq!(
            v.path, "views[0].scenes[0].children[1].children[0]",
            "path must reference the raw JSON index (1), not the post-filter index (0)"
        );
    }

    fn oversized_card_with_shape(overflow: &str) -> String {
        format!(
            r##"{{
                "video": {{ "width": 1920, "height": 1080 }},
                "scenes": [{{
                    "duration": 1.0,
                    "children": [{{
                        "type": "card",
                        "position": "absolute",
                        "x": 1700, "y": 100,
                        "style": {{ "width": "400px", "height": "200px", "overflow": "{overflow}" }},
                        "children": [{{
                            "type": "shape",
                            "shape": "rect",
                            "style": {{ "width": "100%", "height": "100%" }},
                            "fill": "#ff0000"
                        }}]
                    }}]
                }}]
            }}"##
        )
    }

    #[test]
    fn shape_overflow_suppressed_by_hidden_ancestor_but_not_by_visible_one() {
        let hidden = parse(&oversized_card_with_shape("hidden"));
        let visible = parse(&oversized_card_with_shape("visible"));

        let hidden_violations = validate_geometry(&hidden);
        assert!(
            hidden_violations
                .iter()
                .any(|v| v.component == "div" && v.kind == ViolationKind::ViewportOverflow),
            "the card itself must still be reported: {:?}",
            hidden_violations
        );
        assert!(
            hidden_violations.iter().all(|v| v.component != "shape"),
            "shape clipped by an overflow:hidden ancestor must not be reported: {:?}",
            hidden_violations
        );

        let visible_violations = validate_geometry(&visible);
        assert!(
            visible_violations
                .iter()
                .any(|v| v.component == "shape" && v.kind == ViolationKind::ViewportOverflow),
            "overflow:visible (the CSS default) must not suppress the check: {:?}",
            visible_violations
        );
    }

    #[test]
    fn oversized_type_inside_full_frame_hidden_plane_is_clean() {
        let json = r##"{
            "video": { "width": 1080, "height": 1920 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "container",
                    "style": { "width": "100%", "height": "100%", "overflow": "hidden" },
                    "children": [{
                        "type": "text",
                        "content": "BOLD",
                        "position": "absolute",
                        "x": -100, "y": 700,
                        "style": { "color": "#ffffff", "font-size": "420px", "width": "1400px" }
                    }]
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        assert!(
            violations.is_empty(),
            "type bleeding off a full-frame overflow:hidden plane should validate clean: {:?}",
            violations
        );
    }

    #[test]
    fn static_css_transform_is_folded_into_the_viewport_check() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "shape",
                    "shape": "rect",
                    "position": "absolute",
                    "x": 1700, "y": 100,
                    "style": {
                        "width": "100px", "height": "100px",
                        "transform": [{ "fn": "translate-x", "x": "200px" }]
                    },
                    "fill": "#ff0000"
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        assert!(
            violations
                .iter()
                .any(|v| v.component == "shape" && v.kind == ViolationKind::ViewportOverflow),
            "static transform should push the shape out of the viewport: {:?}",
            violations
        );
    }

    #[test]
    fn static_scene_camera_zoom_is_folded_into_the_viewport_check() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "camera": { "zoom": 2.0 },
                "children": [{
                    "type": "shape",
                    "shape": "rect",
                    "position": "absolute",
                    "x": 1800, "y": 100,
                    "style": { "width": "100px", "height": "100px" },
                    "fill": "#ff0000"
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        assert!(
            violations
                .iter()
                .any(|v| v.component == "shape" && v.kind == ViolationKind::ViewportOverflow),
            "camera zoom should push the shape out of the viewport: {:?}",
            violations
        );
    }

    #[test]
    fn camera_fold_is_skipped_when_scene_uses_per_plane_depth() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "camera": { "zoom": 2.0 },
                "children": [{
                    "type": "shape",
                    "shape": "rect",
                    "position": "absolute",
                    "x": 1800, "y": 100,
                    "style": { "width": "100px", "height": "100px", "depth": 1.0 },
                    "fill": "#ff0000"
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        assert!(
            violations.iter().all(|v| v.component != "shape"),
            "camera must not be folded in per-plane depth mode: {:?}",
            violations
        );
    }

    #[test]
    fn strict_anim_detects_slide_in_overflow() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 2.0,
                "children": [{
                    "type": "shape",
                    "shape": "rect",
                    "position": "absolute",
                    "x": 100, "y": 100,
                    "style": {
                        "width": "100px", "height": "100px",
                        "animation": [{ "name": "slide_in_left", "delay": 0, "duration": 1.0 }]
                    },
                    "fill": "#ff0000"
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry_animated(&scenario);
        let v = violations
            .iter()
            .find(|v| v.kind == ViolationKind::AnimatedTextOverflow);
        assert!(
            v.is_some(),
            "slide_in_left should push the shape past the left edge mid-animation: {:?}",
            violations
        );
        assert_eq!(v.unwrap().axis, Axis::X);
    }

    #[test]
    fn strict_anim_respects_a_containers_time_offset_remap() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 2.0,
                "children": [{
                    "type": "flex",
                    "time_offset": -5.0,
                    "style": { "width": "1920px", "height": "1080px" },
                    "children": [{
                        "type": "shape",
                        "shape": "rect",
                        "position": "absolute",
                        "x": 100, "y": 100,
                        "style": {
                            "width": "100px", "height": "100px",
                            "animation": [{ "name": "slide_in_left", "delay": 0, "duration": 1.0 }]
                        },
                        "fill": "#ff0000"
                    }]
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry_animated(&scenario);
        let overflow: Vec<_> = violations
            .iter()
            .filter(|v| v.kind == ViolationKind::AnimatedTextOverflow)
            .collect();
        assert!(
            overflow.is_empty(),
            "time_offset=-5.0 settles the slide-in 5-7s before any sampled instant; a \
             walker that honours the remap must report zero overflows, got: {:?}",
            overflow
        );
    }

    #[test]
    fn strict_anim_start_at_and_effect_delay_do_not_double_shift_the_timeline() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 2.0,
                "children": [{
                    "type": "shape",
                    "shape": "rect",
                    "position": "absolute",
                    "x": 100, "y": 100,
                    "start_at": 0.5,
                    "style": {
                        "width": "100px", "height": "100px",
                        "animation": [{ "name": "slide_in_left", "delay": 0.5, "duration": 1.0 }]
                    },
                    "fill": "#ff0000"
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry_animated(&scenario);
        let v = violations
            .iter()
            .find(|v| v.kind == ViolationKind::AnimatedTextOverflow);
        assert!(
            v.is_some(),
            "overflow shortly after start_at must be caught, not masked by the opacity guard: {:?}",
            violations
        );
    }

    #[test]
    fn strict_anim_respects_start_at_gate_before_visibility() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 2.0,
                "children": [{
                    "type": "shape",
                    "shape": "rect",
                    "position": "absolute",
                    "x": 100, "y": 100,
                    "start_at": 1.5,
                    "style": {
                        "width": "100px", "height": "100px",
                        "animation": [{ "name": "slide_in_left", "delay": 0, "duration": 0.3 }]
                    },
                    "fill": "#ff0000"
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry_animated(&scenario);
        assert!(
            violations.is_empty(),
            "component settles to rest well before start_at; nothing should be flagged: {:?}",
            violations
        );
    }

    #[test]
    fn strict_anim_catches_a_brief_excursion_a_40_sample_cap_would_miss() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 20.0,
                "children": [{
                    "type": "shape",
                    "shape": "rect",
                    "position": "absolute",
                    "x": 100, "y": 490,
                    "style": {
                        "width": "100px", "height": "100px",
                        "animation": [{ "name": "slide_in_left", "delay": 9.85, "duration": 1.0 }]
                    },
                    "fill": "#ff0000"
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry_animated(&scenario);
        let v = violations
            .iter()
            .find(|v| v.kind == ViolationKind::AnimatedTextOverflow)
            .unwrap_or_else(|| {
                panic!(
                    "expected the ~206ms slide_in_left excursion around t≈9.85-10.06s \
                     to be caught by the denser sampling: {:?}",
                    violations
                )
            });
        assert_eq!(v.component, "shape");
        assert_eq!(v.axis, Axis::X);
        assert!(
            v.bbox.x < -0.5,
            "expected a negative bbox.x (left-edge overflow), got {}",
            v.bbox.x
        );
    }

    #[test]
    fn strict_anim_detects_a_timeline_width_step_that_overflows_later_in_the_scene() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 2.0,
                "children": [{
                    "type": "shape",
                    "shape": "rect",
                    "position": "absolute",
                    "x": 100, "y": 100,
                    "style": { "width": "200px", "height": "100px" },
                    "timeline": [{ "at": 1.0, "style": { "width": "1900px" } }],
                    "fill": "#ff0000"
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry_animated(&scenario);
        let v = violations
            .iter()
            .find(|v| v.kind == ViolationKind::AnimatedTextOverflow)
            .unwrap_or_else(|| {
                panic!(
                    "expected AnimatedTextOverflow once the t=1.0s timeline step widens \
                     the box to 1900px: {:?}",
                    violations
                )
            });
        assert_eq!(v.component, "shape");
        assert_eq!(v.axis, Axis::X);
    }

    #[test]
    fn strict_anim_does_not_sample_past_freeze_at() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 2.0,
                "freeze_at": 0.05,
                "children": [{
                    "type": "shape",
                    "shape": "rect",
                    "position": "absolute",
                    "x": 1810, "y": 490,
                    "style": {
                        "width": "100px", "height": "100px",
                        "animation": [{ "name": "spin", "delay": 0, "duration": 2.0 }]
                    },
                    "fill": "#ff0000"
                }]
            }]
        }"##;
        let violations = validate_geometry_animated(&parse(json));
        assert!(
            violations
                .iter()
                .all(|v| v.kind != ViolationKind::AnimatedTextOverflow),
            "the frame that would overflow is never rendered — freeze_at is at \
             0.05s: {violations:?}"
        );
    }

    #[test]
    fn strict_anim_detects_a_spin_animation_pushing_a_square_off_screen() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 2.0,
                "children": [{
                    "type": "shape",
                    "shape": "rect",
                    "position": "absolute",
                    "x": 1810, "y": 490,
                    "style": {
                        "width": "100px", "height": "100px",
                        "animation": [{ "name": "spin", "delay": 0, "duration": 2.0 }]
                    },
                    "fill": "#ff0000"
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry_animated(&scenario);
        let v = violations
            .iter()
            .find(|v| v.kind == ViolationKind::AnimatedTextOverflow)
            .unwrap_or_else(|| {
                panic!(
                    "expected AnimatedTextOverflow from the spin preset rotating the \
                     square past the right edge at some sampled angle: {:?}",
                    violations
                )
            });
        assert_eq!(v.component, "shape");
        assert_eq!(v.axis, Axis::X);
    }

    #[test]
    fn anim_sample_times_scale_with_scene_duration() {
        let short = anim_sample_times(0.5);
        let long = anim_sample_times(4.0);
        assert!(
            long.len() > short.len(),
            "longer scenes should get more samples: {} vs {}",
            long.len(),
            short.len()
        );
        assert!(
            short.first().copied().unwrap().abs() < 1e-9,
            "must include t=0"
        );
        assert!(
            (short.last().copied().unwrap() - 0.5).abs() < 1e-9,
            "must include the scene end"
        );
        assert!(
            long.len() <= ANIM_MAX_SAMPLES,
            "sample count must stay bounded"
        );
    }

    #[test]
    fn anim_sample_times_keeps_the_8_per_second_cadence_up_to_60s() {
        for duration in [20.0, 40.0, 60.0] {
            let samples = anim_sample_times(duration);
            let step = duration / (samples.len() - 1) as f64;
            assert!(
                (step - 0.125).abs() < 0.001,
                "duration={duration}s: expected ~0.125s step, got {step}s ({} samples)",
                samples.len()
            );
        }
    }

    #[test]
    #[ignore = "manual timing probe for constat 8's cap — not run in CI"]
    fn timing_probe_for_constat_8() {
        let mut children = String::new();
        for i in 0..15 {
            children.push_str(&format!(
                r##"{{"type":"text","position":"absolute","x":{},"y":{},
                    "content":"item {}",
                    "style":{{"font-size":32,"color":"#ffffff",
                        "animation":[{{"name":"slide_in_left","delay":0.1,"duration":0.5}}]}}}},"##,
                (i % 5) * 300,
                (i / 5) * 200,
                i
            ));
        }
        children.pop();
        let json = format!(
            r##"{{"video":{{"width":1920,"height":1080}},
                "scenes":[{{"duration":60.0,"children":[{children}]}}]}}"##
        );
        let scenario = parse(&json);
        let n = anim_sample_times(60.0).len();
        let start = std::time::Instant::now();
        let violations = validate_geometry_animated(&scenario);
        let elapsed = start.elapsed();
        eprintln!(
            "timing_probe: {} samples, {:?} total, {:?}/sample, {} violations",
            n,
            elapsed,
            elapsed / n.max(1) as u32,
            violations.len()
        );
    }

    #[test]
    fn wrapped_text_taller_than_its_fixed_height_card_is_flagged() {
        let json = r##"{"video":{"width":960,"height":540,"fps":30,"background":"#0A0A12"},
 "scenes":[{"duration":1.0,"children":[
   {"type":"card","position":"absolute","x":330,"y":100,
    "style":{"width":300,"height":80,"background":"#1e2233","overflow":"visible"},
    "children":[{"type":"text",
      "content":"Ce paragraphe est beaucoup plus grand que la carte de 80px qui le contient.",
      "style":{"font-size":44,"color":"#ffffff"}}]}]}]}"##;
        let scenario = parse(json);

        let viewport_violations: Vec<_> = validate_geometry(&scenario)
            .into_iter()
            .filter(|v| v.kind == ViolationKind::ViewportOverflow)
            .collect();
        assert!(
            viewport_violations.is_empty(),
            "fixture should be viewport-clean by construction: {:?}",
            viewport_violations
        );

        let violations = validate_geometry(&scenario);
        let v = violations
            .iter()
            .find(|v| v.kind == ViolationKind::ContentOverflowsBox)
            .unwrap_or_else(|| {
                panic!(
                    "expected ContentOverflowsBox for text taller than its fixed-height card, got: {:?}",
                    violations
                )
            });
        assert_eq!(v.component, "text");
        assert_eq!(
            v.axis,
            Axis::Y,
            "card is wide enough (paragraph wraps to ~294px < 300px) — only height should overflow: {:?}",
            v
        );
        assert!(
            v.hint.contains("height"),
            "hint should point at the height mismatch: {}",
            v.hint
        );
    }

    #[test]
    fn unbreakable_long_token_wider_than_its_box_is_flagged_even_with_wrap_on() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "card",
                    "position": "absolute",
                    "x": 100, "y": 100,
                    "style": { "width": "150px", "height": "600px", "background": "#222244" },
                    "children": [{
                        "type": "text",
                        "content": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                        "style": { "color": "#ffffff", "font-size": "24px", "width": "150px" }
                    }]
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        let v = violations
            .iter()
            .find(|v| v.kind == ViolationKind::ContentOverflowsBox)
            .unwrap_or_else(|| {
                panic!(
                    "expected ContentOverflowsBox for an unbreakable long token: {:?}",
                    violations
                )
            });
        assert_eq!(
            v.axis,
            Axis::X,
            "the 600px-tall box rules out a height overflow: {:?}",
            v
        );
    }

    #[test]
    fn wrapped_text_that_fits_its_box_is_not_flagged() {
        let json = r##"{"video":{"width":960,"height":540,"fps":30,"background":"#0A0A12"},
 "scenes":[{"duration":1.0,"children":[
   {"type":"card","position":"absolute","x":330,"y":80,
    "style":{"width":300,"height":400,"background":"#1e2233","overflow":"visible"},
    "children":[{"type":"text",
      "content":"Ce paragraphe est beaucoup plus grand que la carte de 80px qui le contient.",
      "style":{"font-size":44,"color":"#ffffff"}}]}]}]}"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        assert!(
            violations
                .iter()
                .all(|v| v.kind != ViolationKind::ContentOverflowsBox),
            "content that fits its box must not be flagged: {:?}",
            violations
        );
    }

    #[test]
    fn in_flow_text_grown_past_its_cards_declared_height_is_flagged() {
        let json = r##"{"video":{"width":1920,"height":1080,"fps":30,"background":"#0A0A12"},
 "scenes":[{"duration":1.0,"children":[
   {"type":"card","position":"absolute","x":660,"y":50,
    "style":{"width":300,"height":80,"background":"#1e2233","overflow":"visible"},
    "children":[{"type":"text",
      "content":"Ce paragraphe est beaucoup plus grand que la carte de 80px qui le contient.",
      "style":{"font-size":44,"color":"#ffffff"}}]}]}]}"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        assert!(
            violations
                .iter()
                .all(|v| v.kind != ViolationKind::ViewportOverflow),
            "fixture stays inside the 1080px-tall frame by construction — this is purely a \
             content-vs-declared-box mismatch: {:?}",
            violations
        );
        let v = violations
            .iter()
            .find(|v| v.kind == ViolationKind::ContentOverflowsBox && v.component == "text")
            .unwrap_or_else(|| {
                panic!(
                    "expected ContentOverflowsBox for text taller than its fixed-height card, got: {:?}",
                    violations
                )
            });
        assert_eq!(v.axis, Axis::Y);
        assert!(
            v.hint.contains("height"),
            "hint should point at the height mismatch: {}",
            v.hint
        );
    }

    #[test]
    fn content_overflow_is_suppressed_under_a_clipping_ancestor() {
        let json = r##"{"video":{"width":960,"height":540,"fps":30,"background":"#0A0A12"},
 "scenes":[{"duration":1.0,"children":[
   {"type":"card","position":"absolute","x":330,"y":200,
    "style":{"width":300,"height":80,"background":"#1e2233","overflow":"hidden"},
    "children":[{"type":"text",
      "content":"Ce paragraphe est beaucoup plus grand que la carte de 80px qui le contient.",
      "style":{"font-size":44,"color":"#ffffff"}}]}]}]}"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        assert!(
            violations
                .iter()
                .all(|v| v.kind != ViolationKind::ContentOverflowsBox),
            "content clipped by an overflow:hidden ancestor must not be flagged: {:?}",
            violations
        );
    }

    #[test]
    fn content_overflow_is_suppressed_when_the_node_clips_itself() {
        let json = r##"{"video":{"width":960,"height":540,"fps":30,"background":"#0A0A12"},
 "scenes":[{"duration":1.0,"children":[
   {"type":"card","position":"absolute","x":330,"y":200,
    "style":{"width":300,"height":80,"background":"#1e2233","overflow":"visible"},
    "children":[{"type":"text",
      "content":"Ce paragraphe est beaucoup plus grand que la carte de 80px qui le contient.",
      "style":{"font-size":44,"color":"#ffffff","overflow":"hidden"}}]}]}]}"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        assert!(
            violations
                .iter()
                .all(|v| v.kind != ViolationKind::ContentOverflowsBox),
            "content clipped by its own overflow:hidden must not be flagged: {:?}",
            violations
        );
    }

    #[test]
    fn nowrap_text_is_not_double_reported_by_content_overflows_box() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "card",
                    "x": 100, "y": 100,
                    "style": { "width": "200px", "height": "200px", "background": "#222244" },
                    "children": [{
                        "type": "text",
                        "content": "this string is too long to fit",
                        "style": { "color": "#ffffff", "font-size": "96px", "white-space": "nowrap" }
                    }]
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        assert!(
            violations
                .iter()
                .all(|v| v.kind != ViolationKind::ContentOverflowsBox),
            "nowrap text must be reported only as UnwrappableTextOverflow, not also ContentOverflowsBox: {:?}",
            violations
        );
        assert!(violations
            .iter()
            .any(|v| v.kind == ViolationKind::UnwrappableTextOverflow));
    }

    #[test]
    fn static_rotation_is_folded_into_the_viewport_check() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "shape",
                    "shape": "rect",
                    "position": "absolute",
                    "x": 1810, "y": 490,
                    "style": {
                        "width": "100px", "height": "100px",
                        "transform": [{ "fn": "rotate", "deg": 45 }]
                    },
                    "fill": "#ff0000"
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        let v = violations
            .iter()
            .find(|v| v.component == "shape" && v.kind == ViolationKind::ViewportOverflow)
            .unwrap_or_else(|| {
                panic!(
                    "45° rotation should push the shape's AABB past the right edge: {:?}",
                    violations
                )
            });
        assert_eq!(v.axis, Axis::X, "only the x-axis should overflow: {:?}", v);
    }

    #[test]
    fn static_skew_is_folded_into_the_viewport_check() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "shape",
                    "shape": "rect",
                    "position": "absolute",
                    "x": 1850, "y": 400,
                    "style": {
                        "width": "50px", "height": "200px",
                        "transform": [{ "fn": "skew-x", "x": 45 }]
                    },
                    "fill": "#ff0000"
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        let v = violations
            .iter()
            .find(|v| v.component == "shape" && v.kind == ViolationKind::ViewportOverflow)
            .unwrap_or_else(|| {
                panic!(
                    "45° skew-x should push the shape's AABB past the right edge: {:?}",
                    violations
                )
            });
        assert_eq!(v.axis, Axis::X, "only the x-axis should overflow: {:?}", v);
    }

    #[test]
    fn transform_origin_right_edge_keeps_a_scaled_shape_inside_the_viewport() {
        let json = r##"{
            "video": { "width": 1000, "height": 1000 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "shape",
                    "shape": "rect",
                    "position": "absolute",
                    "x": 880, "y": 450,
                    "style": {
                        "width": "100px", "height": "100px",
                        "transform": [{ "fn": "scale", "x": 3, "y": 1 }],
                        "transform-origin": { "x": "right" }
                    },
                    "fill": "#ff0000"
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        assert!(
            violations
                .iter()
                .all(|v| v.kind != ViolationKind::ViewportOverflow),
            "transform-origin: right should pivot growth away from the right \
             edge, keeping the shape inside the 1000px-wide viewport: {:?}",
            violations
        );
    }

    #[test]
    fn transform_origin_50pct_is_identical_to_the_default_centre_pivot() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "shape",
                    "shape": "rect",
                    "position": "absolute",
                    "x": 1810, "y": 490,
                    "style": {
                        "width": "100px", "height": "100px",
                        "transform": [{ "fn": "rotate", "deg": 45 }],
                        "transform-origin": { "x": "50%", "y": "50%" }
                    },
                    "fill": "#ff0000"
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        let v = violations
            .iter()
            .find(|v| v.component == "shape" && v.kind == ViolationKind::ViewportOverflow)
            .unwrap_or_else(|| {
                panic!(
                    "explicit 50%/50% origin must behave like the default centre pivot: {:?}",
                    violations
                )
            });
        assert_eq!(v.axis, Axis::X);
        assert!(
            (v.bbox.x + v.bbox.w - 1930.71).abs() < 1.0,
            "right edge should land at the same ~1930.7 as the centre-pivot case: {:?}",
            v
        );
    }

    #[test]
    fn unrotated_transform_folding_is_unchanged_by_the_corner_based_rewrite() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "shape",
                    "shape": "rect",
                    "position": "absolute",
                    "x": 1700, "y": 100,
                    "style": {
                        "width": "100px", "height": "100px",
                        "transform": [{ "fn": "translate-x", "x": "200px" }]
                    },
                    "fill": "#ff0000"
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        assert!(
            violations
                .iter()
                .any(|v| v.component == "shape" && v.kind == ViolationKind::ViewportOverflow),
            "translate-only folding must still work: {:?}",
            violations
        );
    }

    #[test]
    fn gradient_text_taller_than_its_fixed_height_card_is_flagged() {
        let json = r##"{"video":{"width":960,"height":540,"fps":30,"background":"#0A0A12"},
 "scenes":[{"duration":1.0,"children":[
   {"type":"card","position":"absolute","x":330,"y":200,
    "style":{"width":300,"height":80,"background":"#1e2233","overflow":"visible"},
    "children":[{"type":"gradient_text",
      "content":"Ce paragraphe est beaucoup plus grand que la carte de 80px qui le contient.",
      "style":{"font-size":44,"color":"#ffffff"}}]}]}]}"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        let v = violations
            .iter()
            .find(|v| v.kind == ViolationKind::ContentOverflowsBox)
            .unwrap_or_else(|| {
                panic!(
                    "expected ContentOverflowsBox for gradient_text taller than its fixed-height card, got: {:?}",
                    violations
                )
            });
        assert_eq!(v.component, "gradient_text");
        assert_eq!(v.axis, Axis::Y);
    }

    #[test]
    fn absolutely_positioned_text_spilling_past_a_visible_card_is_legal() {
        let json = r##"{"video":{"width":960,"height":540,"fps":30,"background":"#0A0A12"},
 "scenes":[{"duration":1.0,"children":[
   {"type":"card","position":"absolute","x":330,"y":100,
    "style":{"width":300,"height":80,"background":"#1e2233","overflow":"visible"},
    "children":[{"type":"text","position":"absolute","x":0,"y":0,
      "content":"Ce paragraphe est beaucoup plus grand que la carte de 80px qui le contient.",
      "style":{"font-size":44,"color":"#ffffff","width":"300px"}}]}]}]}"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);

        assert!(
            violations
                .iter()
                .all(|v| v.kind != ViolationKind::ContentOverflowsBox),
            "the text's own box already matches its own (unclamped) content — must not fire ContentOverflowsBox: {:?}",
            violations
        );
        assert!(
            violations
                .iter()
                .all(|v| v.kind != ViolationKind::ViewportOverflow),
            "fixture should stay inside the 540px-tall frame by construction: {:?}",
            violations
        );
        assert!(
            violations
                .iter()
                .all(|v| v.kind != ViolationKind::ContentOverflowsCard),
            "text sticking out of a visible-overflow card is a legal, documented pattern: {:?}",
            violations
        );
    }

    #[test]
    fn spilling_past_a_visible_card_is_still_caught_when_it_leaves_the_viewport() {
        let json = r##"{"video":{"width":960,"height":540,"fps":30,"background":"#0A0A12"},
 "scenes":[{"duration":1.0,"children":[
   {"type":"card","position":"absolute","x":330,"y":460,
    "style":{"width":300,"height":80,"background":"#1e2233","overflow":"visible"},
    "children":[{"type":"text","position":"absolute","x":0,"y":0,
      "content":"Ce paragraphe est beaucoup plus grand que la carte de 80px qui le contient.",
      "style":{"font-size":44,"color":"#ffffff","width":"300px"}}]}]}]}"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);

        assert!(
            violations
                .iter()
                .any(|v| v.kind == ViolationKind::ViewportOverflow),
            "content escaping a visible card AND the 540px frame must still be reported by \
             check_viewport — retiring ContentOverflowsCard must not have removed this: {:?}",
            violations
        );
    }

    #[test]
    fn in_flow_table_taller_than_its_card_is_flagged_via_content_overflows_box() {
        let rows: String = (1..=15)
            .map(|i| format!(r#"["{i}a","{i}b","{i}c"]"#))
            .collect::<Vec<_>>()
            .join(",");
        let json = format!(
            r##"{{
                "video": {{ "width": 1920, "height": 1080 }},
                "scenes": [{{
                    "duration": 1.0,
                    "children": [{{
                        "type": "card",
                        "position": "absolute",
                        "x": 100, "y": 100,
                        "style": {{ "width": "600px", "height": "60px", "background": "#111111" }},
                        "children": [{{
                            "type": "table",
                            "headers": ["a", "b", "c"],
                            "rows": [{rows}]
                        }}]
                    }}]
                }}]
            }}"##
        );
        let scenario = parse(&json);
        let violations = validate_geometry(&scenario);
        let v = violations
            .iter()
            .find(|v| v.kind == ViolationKind::ContentOverflowsBox && v.component == "table")
            .unwrap_or_else(|| {
                panic!(
                    "expected ContentOverflowsBox for a 16-row table in a 60px card: {:?}",
                    violations
                )
            });
        assert_eq!(v.axis, Axis::Y);
    }

    #[test]
    fn absolutely_positioned_table_spilling_past_a_visible_card_is_legal() {
        let rows: String = (1..=15)
            .map(|i| format!(r#"["{i}a","{i}b","{i}c"]"#))
            .collect::<Vec<_>>()
            .join(",");
        let json = format!(
            r##"{{
                "video": {{ "width": 1920, "height": 1080 }},
                "scenes": [{{
                    "duration": 1.0,
                    "children": [{{
                        "type": "card",
                        "position": "absolute",
                        "x": 100, "y": 100,
                        "style": {{ "width": "600px", "height": "60px", "background": "#111111" }},
                        "children": [{{
                            "type": "table",
                            "position": "absolute",
                            "x": 0, "y": 0,
                            "headers": ["a", "b", "c"],
                            "rows": [{rows}]
                        }}]
                    }}]
                }}]
            }}"##
        );
        let scenario = parse(&json);
        let violations = validate_geometry(&scenario);
        assert!(
            violations
                .iter()
                .all(|v| v.kind != ViolationKind::ContentOverflowsCard),
            "table sticking out of a visible-overflow card is a legal, documented pattern: {:?}",
            violations
        );
    }

    #[test]
    fn nested_card_bigger_than_its_visible_outer_card_is_legal() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "card",
                    "position": "absolute",
                    "x": 100, "y": 100,
                    "style": { "width": "200px", "height": "200px", "background": "#111111" },
                    "children": [{
                        "type": "card",
                        "style": { "width": "500px", "height": "500px", "background": "#222222" },
                        "children": []
                    }]
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        assert!(
            violations
                .iter()
                .all(|v| v.kind != ViolationKind::ContentOverflowsCard),
            "an inner card bigger than its visible-overflow outer card is legal: {:?}",
            violations
        );
    }

    #[test]
    fn content_overflowing_a_clipping_card_is_also_not_flagged() {
        let json = r##"{"video":{"width":960,"height":540,"fps":30,"background":"#0A0A12"},
 "scenes":[{"duration":1.0,"children":[
   {"type":"card","position":"absolute","x":330,"y":100,
    "style":{"width":300,"height":80,"background":"#1e2233","overflow":"hidden"},
    "children":[{"type":"text","position":"absolute","x":0,"y":0,
      "content":"Ce paragraphe est beaucoup plus grand que la carte de 80px qui le contient.",
      "style":{"font-size":44,"color":"#ffffff","width":"300px"}}]}]}]}"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        assert!(
            violations
                .iter()
                .all(|v| v.kind != ViolationKind::ContentOverflowsCard),
            "content clipped by its own card must not be flagged: {:?}",
            violations
        );
    }

    #[test]
    fn component_that_fits_its_card_is_not_flagged() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "card",
                    "position": "absolute",
                    "x": 100, "y": 100,
                    "style": { "width": "600px", "height": "200px", "background": "#111111" },
                    "children": [{
                        "type": "text",
                        "content": "hi"
                    }]
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        assert!(
            violations
                .iter()
                .all(|v| v.kind != ViolationKind::ContentOverflowsCard),
            "a component that fits its card must not be flagged: {:?}",
            violations
        );
    }

    fn bleeding_shape_json(bleed: bool) -> String {
        format!(
            r##"{{
                "video": {{ "width": 1920, "height": 1080 }},
                "scenes": [{{
                    "duration": 1.0,
                    "children": [{{
                        "type": "shape",
                        "shape": "rect",
                        "style": {{ "width": "400px", "height": "100px" }},
                        "position": "absolute",
                        "x": 1700, "y": 100,
                        "fill": "#ff0000"{}
                    }}]
                }}]
            }}"##,
            if bleed { r#", "bleed": true"# } else { "" }
        )
    }

    #[test]
    fn bleeding_shape_with_bleed_true_validates_clean() {
        let scenario = parse(&bleeding_shape_json(true));
        let violations = validate_geometry(&scenario);
        assert!(
            violations.is_empty(),
            "bleed: true must exempt the shape from ViewportOverflow: {:?}",
            violations
        );
    }

    #[test]
    fn identical_fixture_without_bleed_still_errors() {
        let scenario = parse(&bleeding_shape_json(false));
        let violations = validate_geometry(&scenario);
        assert!(
            violations
                .iter()
                .any(|v| v.kind == ViolationKind::ViewportOverflow && v.component == "shape"),
            "without bleed, the identical shape must still be reported: {:?}",
            violations
        );
    }

    #[test]
    fn bleed_true_does_not_exempt_content_overflows_box() {
        let json = r##"{"video":{"width":960,"height":540,"fps":30,"background":"#0A0A12"},
 "scenes":[{"duration":1.0,"children":[
   {"type":"card","position":"absolute","x":330,"y":200,
    "style":{"width":300,"height":80,"background":"#1e2233","overflow":"visible"},
    "children":[{"type":"text","bleed":true,
      "content":"Ce paragraphe est beaucoup plus grand que la carte de 80px qui le contient.",
      "style":{"font-size":44,"color":"#ffffff"}}]}]}]}"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        assert!(
            violations
                .iter()
                .any(|v| v.kind == ViolationKind::ContentOverflowsBox && v.component == "text"),
            "bleed: true on the text must NOT suppress ContentOverflowsBox: {:?}",
            violations
        );
    }

    #[test]
    fn bleed_on_a_parent_does_not_suppress_a_childs_viewport_overflow() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "container",
                    "bleed": true,
                    "position": "absolute",
                    "x": 50, "y": 50,
                    "style": { "width": "200px", "height": "200px" },
                    "children": [{
                        "type": "shape",
                        "shape": "rect",
                        "position": "absolute",
                        "x": 1900, "y": 100,
                        "style": { "width": "300px", "height": "100px" },
                        "fill": "#ff0000"
                    }]
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        assert!(
            violations.iter().all(|v| v.component != "container"),
            "the container itself sits inside the frame and must not be reported: {:?}",
            violations
        );
        assert!(
            violations
                .iter()
                .any(|v| v.kind == ViolationKind::ViewportOverflow && v.component == "shape"),
            "the parent's bleed:true must not suppress the child's own genuine overflow: {:?}",
            violations
        );
    }

    #[test]
    fn bleed_true_exempts_animated_text_overflow_too() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 2.0,
                "children": [{
                    "type": "shape",
                    "shape": "rect",
                    "position": "absolute",
                    "x": 100, "y": 100,
                    "bleed": true,
                    "style": {
                        "width": "100px", "height": "100px",
                        "animation": [{ "name": "slide_in_left", "delay": 0, "duration": 1.0 }]
                    },
                    "fill": "#ff0000"
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry_animated(&scenario);
        assert!(
            violations
                .iter()
                .all(|v| v.kind != ViolationKind::AnimatedTextOverflow),
            "bleed: true must exempt the shape from AnimatedTextOverflow: {:?}",
            violations
        );
    }

    #[test]
    fn text_autofit_resolves_a_content_overflow_that_would_otherwise_fire() {
        let json = r##"{"video":{"width":960,"height":540,"fps":30,"background":"#0A0A12"},
 "scenes":[{"duration":1.0,"children":[
   {"type":"card","position":"absolute","x":330,"y":200,
    "style":{"width":300,"height":80,"background":"#1e2233","overflow":"visible"},
    "children":[{"type":"text",
      "content":"Ce paragraphe est beaucoup plus grand que la carte de 80px qui le contient.",
      "style":{"font-size":44,"color":"#ffffff","text-autofit":true}}]}]}]}"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        assert!(
            violations
                .iter()
                .all(|v| v.kind != ViolationKind::ContentOverflowsBox),
            "text-autofit: true must resolve the height overflow this exact fixture (minus the \
             flag) triggers: {:?}",
            violations
        );
    }

    #[test]
    fn without_text_autofit_the_same_fixture_still_overflows() {
        let json = r##"{"video":{"width":960,"height":540,"fps":30,"background":"#0A0A12"},
 "scenes":[{"duration":1.0,"children":[
   {"type":"card","position":"absolute","x":330,"y":200,
    "style":{"width":300,"height":80,"background":"#1e2233","overflow":"visible"},
    "children":[{"type":"text",
      "content":"Ce paragraphe est beaucoup plus grand que la carte de 80px qui le contient.",
      "style":{"font-size":44,"color":"#ffffff"}}]}]}]}"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        assert!(
            violations
                .iter()
                .any(|v| v.kind == ViolationKind::ContentOverflowsBox),
            "control fixture (no text-autofit) must still report the overflow: {:?}",
            violations
        );
    }

    #[test]
    fn text_autofit_does_not_silence_an_overflow_the_floor_cannot_fix() {
        let json = r##"{"video":{"width":960,"height":540,"fps":30,"background":"#0A0A12"},
 "scenes":[{"duration":1.0,"children":[
   {"type":"card","position":"absolute","x":330,"y":200,
    "style":{"width":300,"height":8,"background":"#1e2233","overflow":"visible"},
    "children":[{"type":"text",
      "content":"Ce paragraphe est beaucoup plus grand que la carte de 80px qui le contient.",
      "style":{"font-size":44,"color":"#ffffff","text-autofit":true}}]}]}]}"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        assert!(
            violations
                .iter()
                .any(|v| v.kind == ViolationKind::ContentOverflowsBox),
            "text-autofit must not silence an overflow the legibility floor cannot resolve: {:?}",
            violations
        );
    }

    #[test]
    fn text_autofit_resolves_an_unwrappable_nowrap_overflow() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "card",
                    "x": 100, "y": 100,
                    "style": { "width": "200px", "height": "200px", "background": "#222244" },
                    "children": [{
                        "type": "text",
                        "content": "this string is too long to fit",
                        "style": { "color": "#ffffff", "font-size": "96px", "white-space": "nowrap", "text-autofit": true }
                    }]
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        assert!(
            violations
                .iter()
                .all(|v| v.kind != ViolationKind::UnwrappableTextOverflow),
            "text-autofit: true must resolve the nowrap overflow this exact fixture (minus the \
             flag) triggers: {:?}",
            violations
        );
    }

    #[test]
    fn line_bbox_honours_x1_y1_not_just_the_node_position() {
        let json = r##"{"video":{"width":1920,"height":1080,"fps":30,"background":"#000000"},
 "scenes":[{"duration":1.0,"children":[
  {"type":"line","position":"absolute","x":0,"y":0,"x1":960,"y1":100,"x2":960,"y2":1280,"color":"#FFFFFF","width":4}]}]}"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        let v = violations
            .iter()
            .find(|v| v.component == "line" && v.kind == ViolationKind::ViewportOverflow)
            .unwrap_or_else(|| {
                panic!("expected a ViewportOverflow for the line: {:?}", violations)
            });
        assert_eq!(v.axis, Axis::Y);
        assert_eq!(
            (v.bbox.x, v.bbox.y, v.bbox.w, v.bbox.h),
            (958.0, 98.0, 4.0, 1184.0),
            "bbox must be anchored at x1/y1 (960, 100), not at the node's own x/y (0, 0): {:?}",
            v.bbox
        );
    }

    #[test]
    fn line_with_negative_x1_that_pokes_off_the_left_edge_is_caught() {
        let json = r##"{"video":{"width":1920,"height":1080,"fps":30,"background":"#000000"},
 "scenes":[{"duration":1.0,"children":[
  {"type":"line","position":"absolute","x":0,"y":0,"x1":-500,"y1":100,"x2":0,"y2":100,"color":"#FFFFFF","width":4}]}]}"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        assert!(
            violations
                .iter()
                .any(|v| v.component == "line" && v.kind == ViolationKind::ViewportOverflow && v.axis == Axis::X),
            "a line whose x1 pokes past x=0 must be reported even though its own box (x=0) does not: {:?}",
            violations
        );
    }

    #[test]
    fn an_arrow_with_no_arrowhead_near_the_edge_does_not_false_positive_on_head_padding() {
        let json = r##"{"video":{"width":1920,"height":1080,"fps":30,"background":"#000000"},
 "scenes":[{"duration":1.0,"children":[
  {"type":"arrow","position":"absolute","x":0,"y":0,"x1":2,"y1":500,"x2":100,"y2":500,
   "arrow_start":false,"arrow_end":false,"color":"#FFFFFF","width":4}]}]}"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        assert!(
            violations
                .iter()
                .all(|v| !(v.component == "arrow" && v.kind == ViolationKind::ViewportOverflow)),
            "an arrow with neither arrow_start nor arrow_end must not pay the arrowhead padding \
             it never draws: {:?}",
            violations
        );
    }

    #[test]
    fn a_curved_arrow_that_bulges_off_the_top_edge_is_caught() {
        let json = r##"{"video":{"width":1920,"height":1080,"fps":30,"background":"#000000"},
 "scenes":[{"duration":1.0,"children":[
  {"type":"arrow","position":"absolute","x":0,"y":0,"x1":1700,"y1":100,"x2":200,"y2":100,
   "curve":0.5,"color":"#FFFFFF","width":4}]}]}"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        assert!(
            violations
                .iter()
                .any(|v| v.component == "arrow" && v.kind == ViolationKind::ViewportOverflow),
            "a `curve` that bulges the arrow's implicit control point above y=0 must be caught, \
             not silently clipped at render: {:?}",
            violations
        );
    }

    #[test]
    fn overflow_hint_names_the_left_edge_not_the_right_for_a_negative_x_overflow() {
        let json = r##"{"video":{"width":1920,"height":1080,"fps":30,"background":"#000000"},
 "scenes":[{"duration":1.0,"children":[
  {"type":"shape","shape":"rect","position":"absolute","x":-102,"y":10,
   "size":{"width":50,"height":50},"fill":"#ff0000"}]}]}"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        let v = violations
            .iter()
            .find(|v| v.kind == ViolationKind::ViewportOverflow && v.axis == Axis::X)
            .unwrap_or_else(|| panic!("expected a ViewportOverflow: {:?}", violations));
        assert!(
            v.hint.contains("left edge") && v.hint.contains("-102"),
            "a shape poking off the left edge at x=-102 must name the left edge in its hint, \
             not fabricate a right edge: {:?}",
            v.hint
        );
    }

    #[test]
    fn overflow_hint_names_the_top_edge_not_the_bottom_for_a_negative_y_overflow() {
        let json = r##"{"video":{"width":1920,"height":1080,"fps":30,"background":"#000000"},
 "scenes":[{"duration":1.0,"children":[
  {"type":"shape","shape":"rect","position":"absolute","x":10,"y":-75,
   "size":{"width":50,"height":50},"fill":"#ff0000"}]}]}"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        let v = violations
            .iter()
            .find(|v| v.kind == ViolationKind::ViewportOverflow && v.axis == Axis::Y)
            .unwrap_or_else(|| panic!("expected a ViewportOverflow: {:?}", violations));
        assert!(
            v.hint.contains("top edge") && v.hint.contains("-75"),
            "a shape poking off the top edge at y=-75 must name the top edge in its hint, \
             not fabricate a bottom edge: {:?}",
            v.hint
        );
    }

    const V2_COMPOSITED_DROPPED_TRANSITION_JSON: &str = r##"{"timing":"v2",
 "video":{"width":320,"height":180,"fps":30,"background":"#000000"},
 "composition":[{"type":"slide","scenes":[
  {"at":0,"duration":2.0,"background":"#FF0000","children":[]},
  {"at":0.5,"duration":2.0,"background":"#00FF00",
   "transition":{"type":"fade","duration":0.2},"children":[]}]}]}"##;

    #[test]
    fn dropped_v2_transition_warning_is_reported_exactly_once_by_off_grid_cuts() {
        let scenario = parse(V2_COMPOSITED_DROPPED_TRANSITION_JSON);
        let warnings = check_off_grid_cuts(&scenario);
        let dropped_transition_warnings = warnings
            .iter()
            .filter(|w| w.contains("both overlaps scene") && w.contains("declares a `transition`"))
            .count();
        assert_eq!(
            dropped_transition_warnings, 1,
            "expected exactly one dropped-transition warning from the single authoritative \
             call site, got {dropped_transition_warnings}: {warnings:?}"
        );
    }
}

#[cfg(test)]
mod legibility_tests {
    use super::*;
    use rustmotion::loader::load_scenario_from_source;

    fn parse(json: &str) -> rustmotion::schema::ResolvedScenario {
        load_scenario_from_source(None, Some(json)).expect("scenario parses")
    }

    #[test]
    fn tiny_font_on_1080p_warns() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "text",
                    "content": "fine print",
                    "style": { "color": "#ffffff", "font-size": "11px" }
                }]
            }]
        }"##;
        let scenario = parse(json);
        let warnings = check_legibility(&scenario);
        assert_eq!(warnings.len(), 1, "expected one warning: {warnings:?}");
        assert!(warnings[0].contains("11px"), "got: {}", warnings[0]);
        assert!(
            warnings[0].contains("views[0].scenes[0].children[0]"),
            "got: {}",
            warnings[0]
        );
    }

    #[test]
    fn autofit_on_a_taller_than_1080_canvas_warns_that_it_may_shrink_below_legibility() {
        let json = r##"{
            "video": { "width": 3840, "height": 2160 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "text",
                    "content": "a long headline that will not fit its narrow box",
                    "style": {
                        "width": "320px", "height": "90px",
                        "color": "#ffffff", "font-size": "120px",
                        "text-autofit": true
                    }
                }]
            }]
        }"##;
        let warnings = check_legibility(&parse(json));
        assert_eq!(warnings.len(), 1, "expected one warning: {warnings:?}");
        assert!(
            warnings[0].contains("text-autofit may shrink"),
            "got: {}",
            warnings[0]
        );
        assert!(warnings[0].contains("13px"), "got: {}", warnings[0]);
        assert!(warnings[0].contains("26px"), "got: {}", warnings[0]);
    }

    #[test]
    fn autofit_on_a_1080_canvas_does_not_warn() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "text",
                    "content": "a long headline that will not fit its narrow box",
                    "style": {
                        "width": "320px", "height": "90px",
                        "color": "#ffffff", "font-size": "120px",
                        "text-autofit": true
                    }
                }]
            }]
        }"##;
        assert!(
            check_legibility(&parse(json)).is_empty(),
            "no divergence at 1080, so no warning"
        );
    }

    #[test]
    fn autofit_declared_on_a_component_that_ignores_it_does_not_warn() {
        let json = r##"{
            "video": { "width": 3840, "height": 2160 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "caption",
                    "mode": "highlight",
                    "words": [{ "text": "hello", "start": 0.0, "end": 1.0 }],
                    "style": { "font-size": "120px", "color": "#ffffff", "text-autofit": true }
                }]
            }]
        }"##;
        assert!(
            check_legibility(&parse(json)).is_empty(),
            "caption's painter never reads text-autofit"
        );
    }

    #[test]
    fn default_sized_text_on_1080p_has_no_legibility_warning() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "text",
                    "content": "headline",
                    "style": { "color": "#ffffff" }
                }]
            }]
        }"##;
        let scenario = parse(json);
        let warnings = check_legibility(&scenario);
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    }

    #[test]
    fn default_table_on_1080p_does_not_warn() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [
                    { "type": "table", "headers": ["a"], "rows": [["1"]] }
                ]
            }]
        }"##;
        let scenario = parse(json);
        let warnings = check_legibility(&scenario);
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    }

    #[test]
    fn same_absolute_px_warns_more_readily_on_a_taller_frame() {
        let json = r##"{
            "video": { "width": 1080, "height": 4000 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "text",
                    "content": "small on a huge canvas",
                    "style": { "color": "#ffffff", "font-size": "20px" }
                }]
            }]
        }"##;
        let scenario = parse(json);
        let warnings = check_legibility(&scenario);
        assert_eq!(warnings.len(), 1, "expected one warning: {warnings:?}");
    }

    #[test]
    fn small_badge_size_warns_using_its_own_default() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "badge",
                    "text": "new",
                    "badge_size": "sm"
                }]
            }]
        }"##;
        let scenario = parse(json);
        let warnings = check_legibility(&scenario);
        assert_eq!(warnings.len(), 1, "expected one warning: {warnings:?}");
        assert!(warnings[0].contains("badge"), "got: {}", warnings[0]);
    }

    #[test]
    fn legibility_never_blocks_validation() {
        use crate::cli::commands::validate_schema::validate_scenario;
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "text",
                    "content": "fine print",
                    "style": { "color": "#ffffff", "font-size": "6px" }
                }]
            }]
        }"##;
        let scenario = parse(json);
        assert!(!check_legibility(&scenario).is_empty());
        let (errors, _warnings) = validate_scenario(&scenario);
        assert!(
            errors.is_empty(),
            "legibility must never surface as a schema error: {errors:?}"
        );
    }

    #[test]
    fn nested_card_child_gets_a_nested_path() {
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "card",
                    "children": [{
                        "type": "text",
                        "content": "fine print",
                        "style": { "color": "#ffffff", "font-size": "8px" }
                    }]
                }]
            }]
        }"##;
        let scenario = parse(json);
        let warnings = check_legibility(&scenario);
        assert_eq!(warnings.len(), 1, "expected one warning: {warnings:?}");
        assert!(
            warnings[0].contains("views[0].scenes[0].children[0].children[0]"),
            "got: {}",
            warnings[0]
        );
    }
}

#[cfg(test)]
mod off_grid_cut_tests {
    use super::*;
    use rustmotion::loader::load_scenario_from_source;

    fn parse(json: &str) -> rustmotion::schema::ResolvedScenario {
        load_scenario_from_source(None, Some(json)).expect("scenario parses")
    }

    #[test]
    fn no_bpm_means_no_warnings_regardless_of_cut_placement() {
        let json = r##"{
            "video": {"width": 64, "height": 64, "fps": 20},
            "scenes": [
                {"duration": 0.62, "children": []},
                {"duration": 1.0, "children": []}
            ]
        }"##;
        assert!(check_off_grid_cuts(&parse(json)).is_empty());
    }

    #[test]
    fn cut_exactly_on_the_beat_does_not_warn() {
        let json = r##"{
            "video": {"width": 64, "height": 64, "fps": 20},
            "bpm": 120,
            "scenes": [
                {"duration": 1.0, "children": []},
                {"duration": 1.0, "children": []}
            ]
        }"##;
        assert!(
            check_off_grid_cuts(&parse(json)).is_empty(),
            "a cut exactly on a beat must not warn"
        );
    }

    #[test]
    fn off_grid_cut_is_named_and_located() {
        let json = r##"{
            "video": {"width": 64, "height": 64, "fps": 20},
            "bpm": 120,
            "scenes": [
                {"duration": 0.62, "children": []},
                {"duration": 1.0, "children": []}
            ]
        }"##;
        let warnings = check_off_grid_cuts(&parse(json));
        assert_eq!(warnings.len(), 1, "got: {warnings:?}");
        assert!(warnings[0].contains("off_grid_cut"), "got: {}", warnings[0]);
        assert!(
            warnings[0].contains("views[0].scenes[1]"),
            "must name and locate the entering scene: {}",
            warnings[0]
        );
    }

    #[test]
    fn snap_beat_on_an_explicit_at_silences_the_warning() {
        let json = r##"{
            "video": {"width": 64, "height": 64, "fps": 20},
            "timing": "v2",
            "bpm": 120,
            "snap": "beat",
            "scenes": [
                {"duration": 1.0, "children": []},
                {"duration": 1.0, "children": [], "at": "0.62s"}
            ]
        }"##;
        assert!(
            check_off_grid_cuts(&parse(json)).is_empty(),
            "snap: beat must resolve the cut onto the grid before this check sees it"
        );
    }

    #[test]
    fn a_views_first_scene_never_warns() {
        let json = r##"{
            "video": {"width": 64, "height": 64, "fps": 20},
            "bpm": 120,
            "scenes": [
                {"duration": 0.37, "children": []}
            ]
        }"##;
        assert!(check_off_grid_cuts(&parse(json)).is_empty());
    }
}
