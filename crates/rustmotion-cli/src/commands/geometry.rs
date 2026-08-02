//! Geometry validator — walks the resolved box tree of every scene and
//! reports nodes whose absolute bounding box leaves the device viewport.
//!
//! Scope:
//!   * detect absolute positions placed past the viewport edge, folding in
//!     static `style.transform` (including rotation/skew, via the four
//!     transformed corners — #128 item 3) and a static (non-keyframed)
//!     `scene.camera` pan/zoom (H5; still partial: 3D transform functions
//!     and keyframed camera motion are not modeled)
//!   * detect components whose unwrapped natural width exceeds the
//!     allocated width when `white-space: nowrap`/`pre` is set
//!     (`text`/`gradient_text`/`caption`)
//!   * detect wrapping content whose natural size exceeds its own resolved
//!     box (`text`/`gradient_text`/`caption`/`rich_text`/`table` — #128
//!     item 1: originally `text`-only)
//!   * detect a component's own box extending past the nearest ancestor
//!     `card` that contains it, for every component type (#128 item 2)
//!   * detect terminal/codeblock content that overflows their box when
//!     `auto_scroll: false`
//!   * exempt `marquee` and `cursor` (designed to bleed)
//!   * never report a node clipped by an `overflow: hidden`/`clip`/`scroll`/
//!     `auto` ancestor as a viewport overflow (H4) — the ancestor's own bbox
//!     is still checked independently, at its own level
//!
//! Animation handling is layered: by default we only check the resting
//! (untransformed) layout. With `--strict-anim`, we additionally sample
//! frames — proportionally to scene duration — and reapply the *real*
//! renderer's animation resolution (`effective_effects` +
//! `resolve_props_for_effects`) plus the paint pass's start_at/end_at
//! visibility window, rather than a hand-rolled fork of that logic (H6).
//!
//! This walker runs the new CSS-engine pipeline (taffy + cosmic-text) so the
//! geometry it checks matches what the renderer will actually paint.

use std::collections::HashSet;

use rustmotion::components::box_builder::{build_scene_from_refs, effective_effects};
use rustmotion::components::intrinsic::{
    CaptionIntrinsic, GradientTextIntrinsic, RichTextIntrinsic, TableIntrinsic, TextIntrinsic,
};
use rustmotion::components::{ChildComponent, Component};
use rustmotion::core::css::style::{CssStyle, TransformFn, WhiteSpace};
use rustmotion::core::css::taffy_bridge::ConversionContext;
use rustmotion::core::css::units::LengthContext;
use rustmotion::core::engine::box_tree::{AvailableSpace, BoxNode, IntrinsicMeasure};
use rustmotion::core::engine::layout_pass::{run_layout, BoxLayout, LayoutResult};
use rustmotion::engine::animator::{resolve_props_for_effects, AnimatedProperties};
use rustmotion::engine::render;
use rustmotion::schema::{Camera, ResolvedScenario, Scene};
use serde::Serialize;

/// One detected layout violation.
#[derive(Debug, Clone, Serialize)]
pub struct GeometryViolation {
    pub view_index: usize,
    pub scene_index: usize,
    /// JSON-style path to the offending child (e.g. `views[0].scenes[1].children[2].children[0]`).
    pub path: String,
    /// Component type name (e.g. "text", "counter").
    pub component: String,
    pub axis: Axis,
    pub kind: ViolationKind,
    /// Node bounding box, in viewport coordinates.
    pub bbox: BBox,
    /// Viewport size at validation time.
    pub viewport: (u32, u32),
    /// Human-readable hint suggesting a fix.
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
#[allow(clippy::enum_variant_names)] // "Overflow" postfix is load-bearing: serde output matches CLI docs
pub enum ViolationKind {
    /// Component bbox crosses the viewport edge.
    ViewportOverflow,
    /// `white-space: nowrap`/`pre` set but the natural width exceeds the
    /// allocated width.
    UnwrappableTextOverflow,
    /// terminal/codeblock has `auto_scroll: false` but content > box.
    AutoScrollDisabledOverflow,
    /// Wrapping text's content, measured at the width its own box was
    /// actually assigned, needs more width (an unbreakable word/token) or
    /// height (wrapped lines) than that box's `content_box()` — e.g. a
    /// paragraph whose parent has a fixed `height` too small for it. Text
    /// painters never clip themselves, so this paints outside its box
    /// regardless of where that box sits relative to the viewport.
    ContentOverflowsBox,
    /// A component's own (resolved, post-layout) box extends past the
    /// nearest ancestor `card` that contains it — distinct from
    /// [`ViolationKind::ViewportOverflow`] (box vs frame) and
    /// [`ViolationKind::ContentOverflowsBox`] (content vs its OWN box): this
    /// is box vs the panel that's supposed to contain it (#128 item 2). A
    /// panel-based style relies on every graphic element staying inside its
    /// card, so this is the blind spot that matters most for it.
    ContentOverflowsCard,
    /// Animated transform (scale/translate/wiggle/orbit) pushes the bbox out
    /// of the viewport at some sampled time. Only emitted with `--strict-anim`.
    AnimatedTextOverflow,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct BBox {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Top-level entry: validate every scene of every view.
pub fn validate_geometry(scenario: &ResolvedScenario) -> Vec<GeometryViolation> {
    let mut violations = Vec::new();
    for (vi, view) in scenario.views.iter().enumerate() {
        for (si, scene) in view.scenes.iter().enumerate() {
            let indexed = deserialize_children_indexed(scene);
            let raw_indices: Vec<usize> = indexed.iter().map(|(i, _)| *i).collect();
            let children: Vec<ChildComponent> = indexed.into_iter().map(|(_, c)| c).collect();
            let viewport = (scenario.video.width, scenario.video.height);
            let viewport_f = (viewport.0 as f32, viewport.1 as f32);

            let root_css = render::root_style(scene.layout.as_ref());
            let built = build_scene_from_refs(children.iter(), viewport_f, root_css, None);
            let layouts = run_layout(&built.root, viewport_f, &ConversionContext::default());

            let camera = scene
                .camera
                .as_ref()
                .filter(|_| !scene_uses_depth(&children));

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
                // Nothing clips top-level scene children but the viewport
                // frame itself — and that's exactly what check_viewport
                // tests, so top level must not be pre-suppressed.
                /*parent_clips=*/
                false,
                camera,
                // No ancestor card at the top of a scene.
                /*nearest_card=*/
                None,
                &mut violations,
            );
        }
    }
    violations
}

/// Deserialize a scene's raw JSON children like `render::deserialize_children`,
/// but keep each survivor's index into the RAW `scene.children` array.
///
/// `render::deserialize_children` filters failures out and returns a plain
/// `Vec`, so a plain `.enumerate()` over its output drifts from
/// `scene.children` as soon as an earlier sibling fails to deserialize.
/// `apply_fixes`/`navigate` in `validate.rs` walk the RAW JSON array, so a
/// path built from the drifted index patches the wrong sibling (H3). We
/// duplicate the (trivial) filter-and-skip here so violation paths always
/// resolve to the JSON node we actually measured.
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

/// True when any top-level child declares an explicit `style.depth` — the v1
/// parallax-plane rule (mirrors the private `scene_uses_depth` in
/// `engine/render/scene.rs`, reimplemented here since that one isn't `pub`).
/// When depth planes are in play the renderer applies a per-plane,
/// depth-scaled camera instead of the single global transform
/// `fold_static_camera` models, so callers skip camera folding entirely in
/// that case rather than risk a wrong correction.
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
    // Maps loop position -> raw JSON array index. `None` at nested levels,
    // where the container's own `Vec<ChildComponent>` field is strict serde
    // (fails the whole parent rather than skipping one bad element), so
    // loop position already matches the JSON array position.
    path_indices: Option<&[usize]>,
    parent_clips: bool,
    camera: Option<&Camera>,
    // Bbox of the nearest ancestor `card`, in the same raw layout space as
    // `raw_bbox` below — `None` when no card encloses this level yet. Only
    // `Component::Card` updates this for its own children (see the
    // recursion below); every other container (`flex`/`grid`/`positioned`/
    // `container`) is layout-only per CLAUDE.md and passes it through
    // unchanged, so the check always compares against the panel actually
    // responsible for containing the content, not an incidental layout box.
    nearest_card: Option<BBox>,
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
        let raw_bbox = bbox_of(layout);

        if !is_exempted(&child.component) {
            if !parent_clips && !bleeds(child) {
                let mut vbbox = apply_static_node_transform(&raw_bbox, &box_node.css, viewport_f);
                if let Some(cam) = camera {
                    vbbox = fold_static_camera(&vbbox, cam, viewport_f);
                }
                check_viewport(&child.component, &child_path, &vbbox, viewport, vi, si, out);
            }
            check_unwrappable_text(
                &child.component,
                &child_path,
                &raw_bbox,
                viewport,
                vi,
                si,
                out,
            );
            check_auto_scroll(
                &child.component,
                &child_path,
                &raw_bbox,
                viewport,
                vi,
                si,
                out,
            );
            // Suppressed under a clipping ancestor (parent_clips) exactly
            // like check_viewport, and when the node clips its own overflow
            // (paint_pass applies a node's own `overflow: hidden`/clip/
            // scroll/auto BEFORE painting its own content, at step 4 —
            // self-clipping is real, not just a container->children thing).
            if !parent_clips && !container_clips(&child.component) {
                check_content_overflows_box(
                    &child.component,
                    &child_path,
                    layout,
                    viewport,
                    vi,
                    si,
                    out,
                );
            }
            // #128 item 2: box vs the containing card. Suppressed under a
            // clipping ancestor exactly like check_viewport (content clipped
            // by the card itself, or by something between here and the
            // card, genuinely never paints past it). Deliberately NOT
            // suppressed by `bleed` — bleed is an assertion about crossing
            // the *frame* edge on purpose, not about disowning whatever a
            // component paints inside its own panel (see `bleeds`'s doc
            // comment; the same reasoning `check_content_overflows_box`
            // already applies).
            if !parent_clips {
                check_overflows_card(
                    &child.component,
                    &child_path,
                    &raw_bbox,
                    nearest_card,
                    viewport,
                    vi,
                    si,
                    out,
                );
            }
        }

        if let Some(grandchildren) = container_children(&child.component) {
            let child_nearest_card = if matches!(child.component, Component::Card(_)) {
                Some(raw_bbox)
            } else {
                nearest_card
            };
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
                child_nearest_card,
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

fn is_exempted(c: &Component) -> bool {
    matches!(c, Component::Marquee(_) | Component::Cursor(_))
}

/// Extends the exemption above with an *opt-in* declaration: a component
/// author who sets top-level `bleed: true` (see `ChildComponent::bleed`) is
/// asserting that extending past the frame is this component's job — a
/// radial glow used as a base layer, the same category as `marquee`/`cursor`
/// but not knowable from the component's *type* alone (a shape is very often
/// real, non-bleeding content).
///
/// Deliberately narrower than [`is_exempted`]: that one is checked once at
/// the top of `walk`/`walk_anim` and suppresses every check in the block
/// below it, `content_overflows_box` included. `bleed` must NOT do that —
/// content larger than its own box is a different defect, unrelated to
/// whether the box itself is allowed to cross the viewport edge, and staying
/// reported is the whole point of `content_overflows_box` existing. So this
/// is consulted individually at each of the two call sites it's allowed to
/// affect (`check_viewport` in `walk`, the animated-overflow check in
/// `walk_anim`) rather than folded into `is_exempted`.
///
/// `bleed` lives on `ChildComponent`, one per component instance, so a
/// parent declaring it never reaches its children: each child in a
/// container's own `children: Vec<ChildComponent>` carries its own `bleed`
/// (default `false`), untouched by the parent's.
fn bleeds(child: &ChildComponent) -> bool {
    child.bleed
}

fn container_children(c: &Component) -> Option<&[ChildComponent]> {
    match c {
        Component::Card(card) => Some(&card.children),
        Component::Flex(flex) => Some(&flex.children),
        Component::Grid(grid) => Some(&grid.children),
        Component::Positioned(pos) => Some(&pos.children),
        Component::Container(c) => Some(&c.children),
        _ => None,
    }
}

/// Whether a container clips its children to its own box — CSS `overflow`
/// semantics (H4). A node inside a clipping ancestor is bounded by that
/// ancestor before it can ever reach the viewport edge, so `check_viewport`
/// skips it entirely; the ancestor's own bbox is still checked independently,
/// at its own level in `walk`/`walk_anim`. Mirrors exactly what the paint
/// pass clips on (`Overflow::Hidden | Clip | Scroll | Auto`) — a plain
/// `background` does NOT imply clipping in CSS, so it is deliberately not a
/// trigger here.
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

/// Static (build-time, non-animated) CSS `transform` fold (H5; #128 item 3
/// closes the rotation/skew gap) — maps the box's four corners through the
/// same ordered transform-function chain the paint pass's 2D fast path
/// applies (`canvas.translate/scale/rotate/skew`, called once per function
/// in `style.transform` order, pivoted at the box centre), then takes the
/// AABB of the four transformed corners. This is what makes rotation/skew
/// contribute correctly: an exact AABB under rotation needs the four
/// corners, not a translate/scale-only shortcut.
///
/// 3D transform functions (`RotateX`/`RotateY`/`Rotate3d`/`TranslateZ`/
/// `Translate3d`'s z component/`ScaleZ`/`Scale3d`/`Perspective`/
/// `Matrix3d`) and the general 2D `Matrix` are intentionally still not
/// modeled (identity for that function) — an exact AABB there needs
/// projecting through the full 3D pipeline `apply_transform` uses for that
/// path, out of scope for this fix. Also still assumes the pivot is the box
/// centre (no `transform-origin` support), matching the prior partial
/// behaviour. Animated transform-producing presets are folded separately in
/// `walk_anim`; this only handles what a component declares directly in
/// `style.transform`.
fn apply_static_node_transform(bbox: &BBox, css: &CssStyle, viewport: (f32, f32)) -> BBox {
    let transform = match css.transform.as_deref() {
        Some(t) if !t.is_empty() => t,
        _ => return *bbox,
    };
    let ctx = LengthContext {
        viewport_width: viewport.0,
        viewport_height: viewport.1,
        parent_size: bbox.w.max(bbox.h),
        font_size: 16.0,
        root_font_size: 16.0,
    };
    let pivot_x = bbox.x + bbox.w / 2.0;
    let pivot_y = bbox.y + bbox.h / 2.0;
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
        let (tx, ty) = apply_transform_chain(transform, cx - pivot_x, cy - pivot_y, &ctx);
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

/// Apply a `style.transform` function list to a point already expressed
/// relative to the pivot, in the same order `apply_transform`'s 2D fast path
/// composes them: `canvas.translate/scale/rotate/skew` are called once per
/// function in list order, and each subsequent canvas call operates in the
/// coordinate frame the previous ones established. Concretely: the *last*
/// function in the list is the one closest to the box (applied to the point
/// first), the *first* function is outermost (applied last) — standard CSS
/// transform-list composition — so this iterates `list` in reverse.
///
/// Matches each 2D function's exact canvas semantics: `Rotate`/`RotateZ` use
/// the same signed-angle convention as `Canvas::rotate` (positive = visually
/// clockwise on a y-down canvas, i.e. `x' = x·cosθ − y·sinθ`,
/// `y' = x·sinθ + y·cosθ`); `Skew`/`SkewX`/`SkewY` match `Canvas::skew`
/// (`x' = x + y·tan(skew_x)`, `y' = y + x·tan(skew_y)`).
fn apply_transform_chain(list: &[TransformFn], x: f32, y: f32, ctx: &LengthContext) -> (f32, f32) {
    let (mut x, mut y) = (x, y);
    for f in list.iter().rev() {
        let (nx, ny) = match f {
            TransformFn::Translate { x: tx, y: ty } => (x + tx.resolve(ctx), y + ty.resolve(ctx)),
            TransformFn::TranslateX { x: tx } => (x + tx.resolve(ctx), y),
            TransformFn::TranslateY { y: ty } => (x, y + ty.resolve(ctx)),
            TransformFn::Translate3d { x: tx, y: ty, .. } => {
                (x + tx.resolve(ctx), y + ty.resolve(ctx))
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
            // 3D functions and the general Matrix: not modeled (see the
            // caller's doc comment) — identity for this point.
            _ => (x, y),
        };
        x = nx;
        y = ny;
    }
    (x, y)
}

/// Static (non-keyframed) global scene-camera fold (H5, partial) — mirrors
/// `apply_camera_transform` in `engine/render/scene.rs` for the
/// non-rotated case: `device = zoom*p + (1-zoom)*origin - zoom*pan`.
/// Rotation and keyframed camera motion are ignored. Callers only pass a
/// camera here when the scene isn't using per-plane depth parallax (see
/// `scene_uses_depth`) — that path applies a *different*, depth-scaled
/// camera per top-level plane, and folding the global formula there would
/// be wrong.
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
    // Sub-pixel tolerance for floating-point rounding. 0.5 px is well below
    // the human-visible threshold and avoids false positives from layout math
    // that produces e.g. 1080.0001 on a 1080-px viewport.
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
        _ => match axis {
            Axis::X => format!(
                "shift x to fit [0..{:.0}], current right edge is {:.0}",
                vw,
                bbox.x + bbox.w
            ),
            Axis::Y => format!(
                "shift y to fit [0..{:.0}], current bottom edge is {:.0}",
                vh,
                bbox.y + bbox.h
            ),
            Axis::Both => "reposition the component to stay inside the viewport".to_string(),
        },
    }
}

/// #128 item 1: this component's `IntrinsicMeasure`, if it has one, plus
/// whether `white-space: nowrap|pre` disables its wrapping. Shared by
/// `check_unwrappable_text` (nowrap-only: natural width vs available width)
/// and `check_content_overflows_box` (wrapped content vs the box's own
/// assigned size) — the two checks this generalizes beyond `text` alone.
///
/// `nowrap` is only meaningful for the text-family types whose *painters*
/// actually honor `white-space` the same way `text` does — `gradient_text`
/// and `caption` both mirror `text`'s wrap/nowrap rule exactly (see
/// `intrinsic.rs`'s "M1 follow-up" doc comments), so their measured size
/// agrees with what gets painted; always `false` for the rest.
///
/// Deliberately excludes `codeblock`/`terminal`: both have an `auto_scroll`
/// escape hatch (default `true`) that makes "natural content taller than
/// the assigned box" an *intentional*, painter-handled clip+scroll rather
/// than a defect — `check_auto_scroll` already covers the `auto_scroll:
/// false` case correctly. A blanket natural-vs-own-box comparison here would
/// false-positive on every ordinary `auto_scroll: true` codeblock/terminal
/// that's deliberately given a smaller-than-natural box to scroll within.
/// Also excludes atomic single-line components (`badge`/`kbd`/`counter`) —
/// out of scope for this pass, see the workstream report.
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
    bbox: &BBox,
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
    // Measure at the natural (unbounded) width via the same cosmic-text–
    // backed intrinsic the layout engine uses.
    let (natural_w, _) = intrinsic.measure(
        (None, None),
        (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
    );
    if natural_w > bbox.w + 0.5 {
        let kind = component_kind(component);
        out.push(GeometryViolation {
            view_index: vi,
            scene_index: si,
            path: path.to_string(),
            component: kind.to_string(),
            axis: Axis::X,
            kind: ViolationKind::UnwrappableTextOverflow,
            bbox: *bbox,
            viewport,
            hint: format!(
                "{kind} natural width is {natural_w:.0}px but only {:.0}px available — remove style.white-space: nowrap (or set it to normal) so it can wrap, or reduce style.font-size",
                bbox.w
            ),
        });
    }
}

/// H4 (second half) / #128 item 1: content larger than its *own* content
/// box, independent of where that box sits relative to the viewport.
/// `check_viewport` only catches a box escaping the *frame*; it says
/// nothing about a box whose declared size is simply too small for what's
/// inside it — e.g. a card with a fixed `height` shorter than the paragraph
/// it wraps. These painters never clip themselves and `overflow: visible`
/// (the CSS default) applies no clip in the paint pass, so that content
/// paints straight out of its box with zero signal from any other check.
///
/// Originally `text`-only (#128 item 1: "content overflow is checked for
/// text only"); now covers every component with an `IntrinsicMeasure` whose
/// natural size can legitimately be smaller than what layout assigned it —
/// see `measurer_and_nowrap`'s doc comment for exactly which types and why
/// (`codeblock`/`terminal` are deliberately excluded: their `auto_scroll`
/// escape hatch makes a smaller-than-natural box intentional).
///
/// Complementary to `check_unwrappable_text`, not overlapping with it:
/// that one covers `white-space: nowrap`/`pre` (single unwrapped line, width
/// only, measured at natural/unconstrained width). This one covers the
/// default wrapping case — measured at the width the box actually *has*
/// (`content_box().2`, unconstrained height) so it also catches a single
/// unbreakable word/token/URL that's wider than the box even though wrap is
/// on (wrapping can't break within a word), plus the width axis stays
/// consistent with what will actually be painted.
fn check_content_overflows_box(
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
    // nowrap/pre is check_unwrappable_text's territory: re-measuring it
    // here at a constrained width would wrap text that will actually
    // paint as one (too-wide) line, producing a height number that
    // doesn't correspond to anything that gets painted.
    if nowrap {
        return;
    }

    let (cx, cy, cw, ch) = layout.content_box();
    if cw <= 0.0 || ch <= 0.0 {
        return;
    }

    let (measured_w, measured_h) = intrinsic.measure(
        (None, None),
        (AvailableSpace::Definite(cw), AvailableSpace::MaxContent),
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

/// #128 item 2: does this component's own (resolved) box extend past the
/// nearest ancestor `card`'s own box? Complementary to `check_viewport`
/// (box vs frame) and `check_content_overflows_box` (content vs its OWN
/// box) — this is the missing third comparison: box vs the panel it's
/// supposed to live inside. Operates purely in layout space (no camera/
/// css-transform folding), matching `check_content_overflows_box`'s scope:
/// "does this content structurally fit inside its card as laid out",
/// independent of wherever the camera happens to be pointing at paint time.
fn check_overflows_card(
    component: &Component,
    path: &str,
    bbox: &BBox,
    nearest_card: Option<BBox>,
    viewport: (u32, u32),
    vi: usize,
    si: usize,
    out: &mut Vec<GeometryViolation>,
) {
    let Some(card) = nearest_card else {
        return;
    };
    let eps = 0.5;
    let card_right = card.x + card.w;
    let card_bottom = card.y + card.h;
    let right = bbox.x + bbox.w;
    let bottom = bbox.y + bbox.h;
    let x_over = bbox.x < card.x - eps || right > card_right + eps;
    let y_over = bbox.y < card.y - eps || bottom > card_bottom + eps;
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
        kind: ViolationKind::ContentOverflowsCard,
        bbox: *bbox,
        viewport,
        hint: format!(
            "{} at [{:.0},{:.0}]→[{:.0},{:.0}] extends past its containing card [{:.0},{:.0}]→[{:.0},{:.0}] — grow the card, shrink/reflow the content, or set overflow: hidden on the card if the bleed is intentional",
            component_kind(component),
            bbox.x,
            bbox.y,
            right,
            bottom,
            card.x,
            card.y,
            card_right,
            card_bottom,
        ),
    });
}

fn check_auto_scroll(
    component: &Component,
    path: &str,
    bbox: &BBox,
    viewport: (u32, u32),
    vi: usize,
    si: usize,
    out: &mut Vec<GeometryViolation>,
) {
    match component {
        Component::Codeblock(cb) if !cb.auto_scroll => {
            let font_size = cb.style.font_size_px_or(14.0);
            let actual_line_height = cb.style.line_height_for(font_size);
            let line_count = cb.code.lines().count().max(1) as f32;
            let chrome_h = if cb.chrome.as_ref().is_some_and(|c| c.enabled) {
                36.0
            } else {
                0.0
            };
            let pad = 32.0; // ~16 top + 16 bottom default
            let natural_h = chrome_h + pad + line_count * actual_line_height;
            if natural_h > bbox.h + 0.5 {
                out.push(GeometryViolation {
                    view_index: vi,
                    scene_index: si,
                    path: path.to_string(),
                    component: "codeblock".to_string(),
                    axis: Axis::Y,
                    kind: ViolationKind::AutoScrollDisabledOverflow,
                    bbox: *bbox,
                    viewport,
                    hint: format!(
                        "codeblock content needs ~{:.0}px but box is {:.0}px — enable auto_scroll or shorten code",
                        natural_h, bbox.h
                    ),
                });
            }
        }
        Component::Terminal(t) if !t.auto_scroll => {
            let font_size = t.style.font_size_px_or(16.0);
            let actual_line_height = t.style.line_height_for(font_size);
            let chrome_h = if t.show_chrome { 36.0 } else { 0.0 };
            let pad = 32.0;
            let natural_h = chrome_h + pad + t.lines.len() as f32 * actual_line_height;
            if natural_h > bbox.h + 0.5 {
                out.push(GeometryViolation {
                    view_index: vi,
                    scene_index: si,
                    path: path.to_string(),
                    component: "terminal".to_string(),
                    axis: Axis::Y,
                    kind: ViolationKind::AutoScrollDisabledOverflow,
                    bbox: *bbox,
                    viewport,
                    hint: format!(
                        "terminal content needs ~{:.0}px but box is {:.0}px — enable auto_scroll or remove lines",
                        natural_h, bbox.h
                    ),
                });
            }
        }
        _ => {}
    }
}

// ─── M4: legibility floor (issue #110 / #102) ──────────────────────────────
//
// "Fits in the frame" (checked above) is not "readable in a video". A table
// column, a badge, a codeblock line — any of them can validate perfectly
// clean while rendering at a font size nobody could read once the video is
// scaled down from its native resolution, which is how video is normally
// watched (embedded players, mobile feeds, thumbnails) unlike a web page,
// which is usually viewed close to 1:1.
//
// Threshold justification (rendered evidence, not a guess): a 1920×1080
// scenario was rendered with the same sample line at 8/10/11/12/13/14/16/18/
// 20/22/24/28px, then the frame was scaled down 50% (a realistic "not
// full-native" viewing size) to inspect. 8–13px degraded to an illegible
// grey smear at that scale; 14px was the first size that stayed readable.
// 0.012 (1.2% of output height) sits between those two bands — it equals
// ~13px on a 1080p frame — and clears every built-in component default
// already shipped (table/terminal/codeblock/pill_nav = 14px, badge `md` =
// 14px, kbd = 14px, tooltip = 13px), so it does not fire on scenarios that
// already validate clean today. Expressing it as a fraction of output
// height (rather than an absolute px count) makes the same *visual* size
// get flagged on a 4K or vertical-format canvas too.
const MIN_LEGIBLE_FONT_RATIO: f32 = 0.012;

/// Check every text-bearing component's effective font size against
/// [`MIN_LEGIBLE_FONT_RATIO`] of the output height. Always advisory (a
/// warning, never a blocking error) — this is a legibility floor, not a
/// geometry correctness check, and the "right" size is ultimately an
/// authorial call.
///
/// Coverage: every component whose `Painter` resolves its rendered font
/// size from `style.font-size` (falling back to that component's own
/// documented default when unset) — text, rich_text, gradient_text,
/// caption, counter, table, terminal, codeblock, callout, list,
/// notification (title + message), pill_nav, badge, kbd, tooltip, marquee.
/// Not covered: components whose text sizing isn't a simple
/// `style.font-size`-or-default resolution (chart axis/labels, gauge, stat,
/// sparkline, heatmap, treemap, dot_map, avatar initials, progress label,
/// rating, countdown, comparison, stepper, timeline, tag_cloud) — see the
/// workstream report for the full list.
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
        // 0.05px tolerance for float rounding; not a meaningful visual gap.
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

/// Effective rendered font size(s) for a component, mirroring exactly the
/// default each `Painter` falls back to when `style.font-size` is unset
/// (see the file/line citations below — kept in sync by hand since these
/// defaults live in `rustmotion-components`, out of this workstream's
/// scope). A component can report more than one size (e.g. a notification's
/// title and message use different sizes).
fn text_sizes(component: &Component) -> Vec<(&'static str, f32)> {
    match component {
        // text.rs, rich_text.rs, gradient_text.rs, caption.rs, counter.rs: 48.0
        Component::Text(t) => vec![("text", t.style.font_size_px_or(48.0))],
        Component::RichText(t) => vec![("rich_text", t.style.font_size_px_or(48.0))],
        Component::GradientText(t) => vec![("gradient_text", t.style.font_size_px_or(48.0))],
        Component::Caption(t) => vec![("caption", t.style.font_size_px_or(48.0))],
        Component::Counter(c) => vec![("counter", c.style.font_size_px_or(48.0))],
        // table.rs, terminal.rs, codeblock/{dimensions,render}.rs, pill_nav.rs: 14.0
        Component::Table(t) => vec![("table", t.style.font_size_px_or(14.0))],
        Component::Terminal(t) => vec![("terminal", t.style.font_size_px_or(14.0))],
        Component::Codeblock(c) => vec![("codeblock", c.style.font_size_px_or(14.0))],
        Component::PillNav(p) => vec![("pill_nav", p.style.font_size_px_or(14.0))],
        // callout.rs, list.rs, notification.rs (title): 16.0
        Component::Callout(c) => vec![("callout", c.style.font_size_px_or(16.0))],
        Component::List(l) => vec![("list", l.style.font_size_px_or(16.0))],
        Component::Notification(n) => {
            let title = n.style.font_size_px_or(16.0);
            let mut sizes = vec![("notification title", title)];
            if n.message.is_some() {
                // notification.rs: message_font_size() = title_font_size() * 0.85
                sizes.push(("notification message", title * 0.85));
            }
            sizes
        }
        // These carry their own `font_size` field (already serde-resolved
        // to its component default when absent from JSON), overridable by
        // `style.font-size` exactly like the rest — kbd.rs, tooltip.rs,
        // marquee.rs.
        Component::Kbd(k) => vec![("kbd", k.style.font_size_px_or(k.font_size))],
        Component::Tooltip(t) => vec![("tooltip", t.style.font_size_px_or(t.font_size))],
        Component::Marquee(m) => vec![("marquee", m.style.font_size_px_or(m.font_size))],
        // badge.rs: BadgeSize::{Sm,Md,Lg}.params().0 = {12.0, 14.0, 18.0}.
        // `params()` is private to badge.rs, so the table is duplicated here.
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

fn component_kind(c: &Component) -> &'static str {
    match c {
        Component::Text(_) => "text",
        Component::Shape(_) => "shape",
        Component::Image(_) => "image",
        Component::Icon(_) => "icon",
        Component::Svg(_) => "svg",
        Component::Video(_) => "video",
        Component::Gif(_) => "gif",
        Component::Counter(_) => "counter",
        Component::Cursor(_) => "cursor",
        Component::Caption(_) => "caption",
        Component::Codeblock(_) => "codeblock",
        Component::Avatar(_) => "avatar",
        Component::AvatarGroup(_) => "avatar_group",
        Component::Arrow(_) => "arrow",
        Component::Connector(_) => "connector",
        Component::Badge(_) => "badge",
        Component::Callout(_) => "callout",
        Component::Chart(_) => "chart",
        Component::Comparison(_) => "comparison",
        Component::Countdown(_) => "countdown",
        Component::Divider(_) => "divider",
        Component::DotMap(_) => "dot_map",
        Component::Gauge(_) => "gauge",
        Component::GradientText(_) => "gradient_text",
        Component::Heatmap(_) => "heatmap",
        Component::Kbd(_) => "kbd",
        Component::Line(_) => "line",
        Component::List(_) => "list",
        Component::Lottie(_) => "lottie",
        Component::Marquee(_) => "marquee",
        Component::Mockup(_) => "mockup",
        Component::Notification(_) => "notification",
        Component::Particle(_) => "particle",
        Component::PillNav(_) => "pill_nav",
        Component::Progress(_) => "progress",
        Component::QrCode(_) => "qrcode",
        Component::Rating(_) => "rating",
        Component::Skeleton(_) => "skeleton",
        Component::Slider(_) => "slider",
        Component::Sparkline(_) => "sparkline",
        Component::Stat(_) => "stat",
        Component::Stepper(_) => "stepper",
        Component::Switch(_) => "switch",
        Component::RichText(_) => "rich_text",
        Component::Table(_) => "table",
        Component::TagCloud(_) => "tag_cloud",
        Component::Terminal(_) => "terminal",
        Component::Timeline(_) => "timeline",
        Component::Tooltip(_) => "tooltip",
        Component::Treemap(_) => "treemap",
        Component::Positioned(_) => "positioned",
        Component::Flex(_) => "flex",
        Component::Grid(_) => "grid",
        Component::Card(_) => "card",
        Component::Container(_) => "container",
        Component::AudioSpectrum(_) => "audio_spectrum",
        Component::Waveform(_) => "waveform",
    }
}

// ─── Animated overflow sampling (--strict-anim) ─────────────────────────────

/// Samples per second of scene duration. ~8/s (125ms resolution) is dense
/// enough to land inside the high-risk window right after an entrance
/// preset's delay — where opacity has started ramping up but translate/scale
/// is still near its most extreme — without either a fixed sample count
/// (misses short bursts in long scenes) or a fixed interval (wastes cycles
/// on long, mostly-static scenes).
const ANIM_SAMPLES_PER_SECOND: f64 = 8.0;
const ANIM_MIN_SAMPLES: usize = 5;
const ANIM_MAX_SAMPLES: usize = 40;

/// Sample times (seconds, scene-relative) for `--strict-anim`, spaced evenly
/// across `[0, scene_duration]`. Count scales with `scene_duration` (H6) —
/// more samples for longer scenes — and is clamped to keep validation time
/// bounded.
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

/// Walk every scene at multiple sampled times, apply the *real* renderer's
/// animation resolution to each widget's bbox, and report viewport
/// overflows. Only emits `AnimatedTextOverflow` violations: the
/// resting-layout checks live in `validate_geometry`.
pub fn validate_geometry_animated(scenario: &ResolvedScenario) -> Vec<GeometryViolation> {
    let mut violations = Vec::new();
    let mut seen: HashSet<(usize, usize, String)> = HashSet::new();
    for (vi, view) in scenario.views.iter().enumerate() {
        for (si, scene) in view.scenes.iter().enumerate() {
            let indexed = deserialize_children_indexed(scene);
            let raw_indices: Vec<usize> = indexed.iter().map(|(i, _)| *i).collect();
            let children: Vec<ChildComponent> = indexed.into_iter().map(|(_, c)| c).collect();
            let viewport = (scenario.video.width, scenario.video.height);
            let viewport_f = (viewport.0 as f32, viewport.1 as f32);

            // Use the resting layout as the base bbox. Animation transforms
            // (translate/scale/wiggle/orbit) are applied analytically per
            // sample — they don't reflow taffy, matching how the paint pass
            // applies them after layout.
            let root_css = render::root_style(scene.layout.as_ref());
            let built = build_scene_from_refs(children.iter(), viewport_f, root_css, None);
            let layouts = run_layout(&built.root, viewport_f, &ConversionContext::default());

            let camera = scene
                .camera
                .as_ref()
                .filter(|_| !scene_uses_depth(&children));

            let path_root = format!("views[{}].scenes[{}]", vi, si);
            let scene_duration = scene.duration;

            for time in anim_sample_times(scene_duration) {
                walk_anim(
                    &children,
                    &built.root.children,
                    &layouts,
                    &built.stagger_delays,
                    viewport,
                    vi,
                    si,
                    &path_root,
                    Some(&raw_indices),
                    /*parent_clips=*/ false,
                    camera,
                    time,
                    scene_duration,
                    &mut seen,
                    &mut violations,
                );
            }
        }
    }
    violations
}

#[allow(clippy::too_many_arguments)]
fn walk_anim(
    children: &[ChildComponent],
    boxes: &[BoxNode],
    layouts: &LayoutResult,
    stagger_delays: &[f64],
    viewport: (u32, u32),
    vi: usize,
    si: usize,
    path: &str,
    path_indices: Option<&[usize]>,
    parent_clips: bool,
    camera: Option<&Camera>,
    time: f64,
    scene_duration: f64,
    seen: &mut HashSet<(usize, usize, String)>,
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

        // Visibility gate: identical to what `paint_node` checks before
        // painting — a start_at/end_at window, already resolved (with
        // stagger folded in) at box-tree build time. No manual re-timing:
        // per PR #27, start_at/end_at gate paint visibility only, and the
        // effects resolved below run at absolute scene time exactly like the
        // renderer does. A node outside its window is not painted — subtree
        // included — so we skip it (and its descendants) entirely, just like
        // `paint_node` does; nothing invisible can overflow.
        let visible = box_node.window.as_ref().is_none_or(|w| w.contains(time));
        if !visible {
            continue;
        }

        if !is_exempted(&child.component)
            && !bleeds(child)
            && !parent_clips
            && layout.width > 0.5
            && layout.height > 0.5
        {
            let stagger_delay = stagger_delays
                .get(box_node.id as usize)
                .copied()
                .unwrap_or(0.0);
            // Reuse the renderer's own effect resolution instead of a
            // divergent fork (H6): `effective_effects` merges timeline
            // steps/synthesized transitions/stagger exactly like
            // `legacy_dispatch.rs` does, and `resolve_props_for_effects`
            // resolves them at absolute scene `time` — no re-timing by
            // start_at, which would double-shift components that also
            // declare a matching `animation` delay.
            let props = match effective_effects(&child.component, stagger_delay) {
                Some(effects) => resolve_props_for_effects(&effects, time, scene_duration),
                None => AnimatedProperties::default(),
            };
            let raw_bbox = bbox_of(layout);
            let base_bbox = apply_static_node_transform(&raw_bbox, &box_node.css, viewport_f);
            if let Some(mut transformed) = transform_bbox(&base_bbox, &props) {
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
                    // Dedupe across samples: one violation per (view, scene, path).
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
                            hint: hint_for_animated(&child.component, &props, time, scene_duration),
                        });
                    }
                }
            }
        }

        if let Some(grandchildren) = container_children(&child.component) {
            walk_anim(
                grandchildren,
                &box_node.children,
                layouts,
                stagger_delays,
                viewport,
                vi,
                si,
                &child_path,
                None,
                parent_clips || container_clips(&child.component),
                camera,
                time,
                scene_duration,
                seen,
                out,
            );
        }
    }
}

/// Apply the canvas transforms the renderer applies (translate then scale
/// around the bbox center) to a base bbox. Returns `None` if the resulting
/// box is degenerate (fully transparent / zero size).
fn transform_bbox(base: &BBox, props: &AnimatedProperties) -> Option<BBox> {
    if props.opacity <= 0.001 {
        return None;
    }
    // Conservative: also account for char animations that overshoot the box
    // (e.g. char_scale_in defaults to 1.08). One extra factor on each axis.
    let char_overshoot = props
        .char_animation
        .as_ref()
        .map(|c| 1.0 + c.overshoot.max(0.0))
        .unwrap_or(1.0);
    let sx = props.scale_x.abs().max(0.001) * char_overshoot;
    let sy = props.scale_y.abs().max(0.001) * char_overshoot;
    let center_x = base.x + base.w / 2.0 + props.translate_x;
    let center_y = base.y + base.h / 2.0 + props.translate_y;
    let new_w = base.w * sx;
    let new_h = base.h * sy;
    Some(BBox {
        x: center_x - new_w / 2.0,
        y: center_y - new_h / 2.0,
        w: new_w,
        h: new_h,
    })
}

fn hint_for_animated(
    component: &Component,
    props: &AnimatedProperties,
    time: f64,
    scene_duration: f64,
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
    match component_kind(component) {
        "text" | "rich_text" | "gradient_text" | "caption" | "counter" => format!(
            "{} — reduce font_size, soften the preset (e.g. fade_in instead of slide_in_left), or add max_width",
            base
        ),
        _ => format!("{} — soften the preset or pull the resting position inward", base),
    }
}

/// Render a violation for human consumption (multi-line, color-free).
pub fn format_violation(v: &GeometryViolation) -> String {
    let axis_str = match v.axis {
        Axis::X => "x",
        Axis::Y => "y",
        Axis::Both => "x+y",
    };
    let kind_str = match v.kind {
        ViolationKind::ViewportOverflow => "viewport overflow",
        ViolationKind::UnwrappableTextOverflow => "wrap=false but text too wide",
        ViolationKind::AutoScrollDisabledOverflow => "auto_scroll=false but content too tall",
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

#[cfg(test)]
mod tests {
    use super::*;
    use rustmotion::loader::load_scenario_from_source;

    fn parse(json: &str) -> rustmotion::schema::ResolvedScenario {
        load_scenario_from_source(None, Some(json)).expect("scenario parses")
    }

    #[test]
    fn clean_scenario_has_no_violations() {
        // 100×80 shape at (10, 10) in a 1920×1080 viewport — well inside.
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

    #[test]
    fn shape_past_right_edge_triggers_x_overflow() {
        // A 400×100 shape positioned at x=1700 in a 1920-wide viewport spills
        // 180 px past the right edge.
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
        // A card 200 px wide with a 96 px font-size unwrapped text. Natural
        // width far exceeds 200, so we should get UnwrappableTextOverflow.
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
    fn marquee_is_exempted_from_overflow() {
        // A marquee that bleeds past the viewport: no violation should fire,
        // marquee is by-design designed to scroll content past edges.
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
    fn auto_scroll_disabled_codeblock_overflows() {
        // 20 lines × 14 px × 1.5 line-height + chrome + padding ≈ 487 px.
        // Box height capped at 200 via style.height → AutoScrollDisabledOverflow.
        // Note: "size" is a legacy field silently ignored by the schema; use
        // style.height to actually constrain the box in the layout pass.
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "codeblock",
                    "code": "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n13\n14\n15\n16\n17\n18\n19\n20",
                    "auto_scroll": false,
                    "style": { "width": "800px", "height": "200px" },
                    "x": 100, "y": 100
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        let v = violations
            .iter()
            .find(|v| v.kind == ViolationKind::AutoScrollDisabledOverflow);
        assert!(
            v.is_some(),
            "missing AutoScrollDisabledOverflow in {:?}",
            violations
        );
        assert_eq!(v.unwrap().component, "codeblock");
    }

    // ─── C1: remediation hints must never name the nonexistent `wrap` field ──

    #[test]
    fn unwrappable_text_hint_does_not_recommend_the_nonexistent_wrap_field() {
        // `wrap` is not a `CssStyle` field (the real property is
        // `white-space`); recommending it in a hint is what used to drive
        // the destructive `--fix` (C1).
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

    // ─── H3: violation path must reference the RAW JSON index ────────────────

    #[test]
    fn violation_path_skips_the_raw_json_index_of_a_dropped_sibling() {
        // Child #0 fails to deserialize (unknown component type) and is
        // dropped. Without the H3 fix, the walker would re-enumerate the
        // *filtered* vector and report this violation as `children[0]`,
        // which in the raw JSON is the broken sibling, not the card that
        // actually overflows.
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

    // ─── H4: overflow:hidden ancestor suppresses viewport checks, visible doesn't ──

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
        // Card at x=1700, width=400 → right edge 2100, past the 1920
        // viewport: the card itself is still reported either way. The shape
        // fills the card (100%/100%), so its own bbox tracks the card's.
        // With `overflow: hidden` the shape is fully inside a clipping
        // ancestor and must not ALSO be reported (H4). With the CSS default
        // `overflow: visible`, nothing suppresses it.
        let hidden = parse(&oversized_card_with_shape("hidden"));
        let visible = parse(&oversized_card_with_shape("visible"));

        let hidden_violations = validate_geometry(&hidden);
        assert!(
            hidden_violations
                .iter()
                .any(|v| v.component == "card" && v.kind == ViolationKind::ViewportOverflow),
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

    // ─── H7: deliberate frame-bleeding typography inside a clipping plane ────

    #[test]
    fn oversized_type_inside_full_frame_hidden_plane_is_clean() {
        // A full-viewport container with `overflow: hidden` — the
        // reference "1600-style brutalist" pattern of bleeding huge type off
        // frame. The text's own box spans from x=-100 to x=1300 in a
        // 1080-wide viewport (bleeds on both edges) but is clipped by the
        // plane, so it must not be reported.
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

    // ─── H5: static css.transform and scene.camera fold into the bbox ────────

    #[test]
    fn static_css_transform_is_folded_into_the_viewport_check() {
        // Shape sits safely inside the viewport at rest (right edge 1800 <
        // 1920), but a static `transform: translateX(200px)` pushes it out.
        // Before H5 this was a silent false negative: geometry only looked
        // at the taffy layout box, never at `css.transform`.
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
        // Shape sits safely inside the viewport at rest (right edge 1900 <
        // 1920), but the scene's static 2x camera zoom (around the frame
        // centre) pushes it out. Before H5, `scene.camera` was completely
        // ignored by geometry.
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
        // With a `style.depth` plane, the renderer applies a *different*,
        // depth-scaled camera per plane instead of the single global
        // transform `fold_static_camera` models. Applying the global formula
        // there would be wrong, so geometry must not fold `scene.camera` in
        // this mode.
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

    // ─── H6: --strict-anim reuses the renderer's effect resolution ───────────

    #[test]
    fn strict_anim_detects_slide_in_overflow() {
        // slide_in_left eases position.x from -200 to 0; a shape resting at
        // x=100 gets pushed to a strongly negative x during the first
        // fraction of the preset, after opacity has started ramping up (its
        // own keyframe window is only the first 30% of the duration).
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
    fn strict_anim_start_at_and_effect_delay_do_not_double_shift_the_timeline() {
        // A component with BOTH `start_at` (visibility gate) and a matching
        // animation `delay` — a common authoring pattern ("appear and
        // animate in at the same moment"). The old hand-rolled fork
        // re-based time by subtracting start_at *again* before resolving
        // the preset, which is already absolute-time-shifted by its own
        // `delay`. That double shift pushed every sample right after
        // start_at below the preset's first keyframe, resolving to the
        // untouched opacity=0 state and silently discarding a genuine
        // overflow through the opacity guard (H6). Reusing
        // `resolve_props_for_effects` at raw (absolute) scene time — like
        // the renderer does — must not reproduce that.
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
        // A short slide-in (delay=0, duration=0.3s) settles well before
        // start_at=1.5s makes the component visible in a 2s scene. Once
        // visible, effects resolve at absolute time — by t=1.5 the preset
        // finished at t=0.3, so props are already at rest. Nothing should
        // be flagged.
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

    // ─── H4 (second half): content larger than its own content box ───────────
    //
    // The first half of H4 (already fixed above) suppresses a *viewport*
    // check when a clipping ancestor is in the way. This half is a different
    // bug: a box can sit entirely inside the viewport and still have content
    // that paints outside *itself*, because text painters never clip their
    // own overflow and `overflow: visible` (the CSS default) applies no clip
    // in the paint pass. `check_viewport` alone never sees this.

    #[test]
    fn wrapped_text_taller_than_its_fixed_height_card_is_flagged() {
        // Exact repro from the audit: a card comfortably inside a 960x540
        // frame (x=330,y=200,w=300,h=80 -> right/bottom edges 630/280, both
        // well inside frame) with a paragraph that, wrapped at the card's
        // ~300px content width, needs ~343px of height — but the card is
        // fixed at 80px. No viewport check ever fires (card and text both
        // report a resting bbox inside the frame); this is purely a
        // content-vs-own-box mismatch.
        let json = r##"{"video":{"width":960,"height":540,"fps":30,"background":"#0A0A12"},
 "scenes":[{"duration":1.0,"children":[
   {"type":"card","position":"absolute","x":330,"y":200,
    "style":{"width":300,"height":80,"background":"#1e2233","overflow":"visible"},
    "children":[{"type":"text",
      "content":"Ce paragraphe est beaucoup plus grand que la carte de 80px qui le contient.",
      "style":{"font-size":44,"color":"#ffffff"}}]}]}]}"##;
        let scenario = parse(json);

        // Sanity check: this scenario must NOT already fail some other way
        // (e.g. viewport overflow) — the whole point is that it looks clean
        // to every other check.
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
        // A single unbroken run with no whitespace or punctuation break
        // opportunities (a long hash/id, not a URL — cosmic-text's wrapper
        // treats `/`/`-` as break points, which would defeat the test)
        // can't be broken by word-wrap, so even with the CSS default
        // `white-space: normal`, it paints wider than a too-narrow box.
        //
        // Note the explicit `width: 150px` directly on the *text*, not just
        // the card: empirically, an auto-sized flex child's cross-axis width
        // floors at its min-content size (the widest unbreakable run) and
        // simply escapes a non-clipping parent instead of being clamped —
        // correct CSS flex behavior (`min-width: auto` on flex items), and
        // not a bug this check is meant to catch (an element escaping a
        // *non-clipping* intermediate container, while staying inside the
        // viewport, is ordinary CSS — check_viewport is the check for hard
        // frame-edge violations). An *explicit* size is a hard author
        // constraint that IS supposed to be honored exactly regardless of
        // content — cosmic-text still overflows it, which is the real bug.
        // Card is tall enough that height is not the issue — only width
        // should fire.
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
        // Passing-case guard: same paragraph as the failing test above, but
        // the card is tall enough (400px) to hold the ~343px wrapped
        // content — must NOT fire. Proves the check compares against the
        // actual resolved box, not some fixed threshold.
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
    fn content_overflow_is_suppressed_under_a_clipping_ancestor() {
        // Same overflowing paragraph/80px-card fixture, but the card clips
        // via `overflow: hidden` — the paragraph genuinely gets clipped at
        // paint time, so reporting it would be a false positive, consistent
        // with the parent_clips suppression already applied to
        // check_viewport.
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
        // Same fixture, but this time the TEXT node itself (not the card)
        // declares `overflow: hidden`. paint_pass applies a node's own
        // overflow clip before painting its own content (step 4, before
        // step 8's component-specific paint) — self-clipping is real, so
        // this must not be flagged either.
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
        // The nowrap/pre case belongs entirely to check_unwrappable_text;
        // this check must defer to it rather than also firing (which would
        // both double-report the same real bug AND compute a height number
        // that doesn't correspond to what nowrap actually paints — a single
        // line, not a wrapped block).
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

    // ─── #128 item 3: rotation/skew fold into the bbox ────────────────────────

    #[test]
    fn static_rotation_is_folded_into_the_viewport_check() {
        // 100×100 shape at (1810, 490) in a 1920×1080 viewport: at rest the
        // box spans x=[1810,1910], comfortably inside (10px margin). A
        // static 45° rotation about the box centre grows the AABB to a
        // half-diagonal of 100/√2*√2 ≈ 70.7px on every side (a square
        // rotated 45° has an AABB side of w·√2), pushing the right edge to
        // ~1930.7 — past the 1920 frame. Before #128 item 3, Rotate/RotateZ
        // were silently dropped by `apply_static_node_transform`, so this
        // validated clean.
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
        // 50×200 shape at (1850, 400): at rest x=[1850,1900], well inside
        // 1920. `skew-x: 45deg` (tan 45° = 1) shifts each corner's x by its
        // y-offset-from-centre: the bottom-right corner (x-offset +25,
        // y-offset +100) moves to +125, landing at world x = 1875+125 = 2000
        // — past the frame. Before #128 item 3, Skew/SkewX/SkewY were
        // silently dropped, so this validated clean.
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
    fn unrotated_transform_folding_is_unchanged_by_the_corner_based_rewrite() {
        // Regression guard for the H5 rewrite: a translate-only transform
        // (no rotation/skew involved) must still behave exactly like the
        // old translate/scale-only formula — same fixture as
        // `static_css_transform_is_folded_into_the_viewport_check`.
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

    // ─── #128 item 1: content-overflows-box generalized beyond `text` ────────

    #[test]
    fn gradient_text_taller_than_its_fixed_height_card_is_flagged() {
        // Same exact repro as `wrapped_text_taller_than_its_fixed_height_card_is_flagged`
        // but for `gradient_text` — proves `check_content_overflows_box` no
        // longer only matches `Component::Text` (#128 item 1: "content
        // overflow is checked for text only").
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

    // ─── #128 item 2: component overflowing its containing card ──────────────

    #[test]
    fn absolutely_positioned_text_spilling_past_its_card_is_flagged() {
        // The audit's headline repro: a text with no fixed height, inside a
        // card, grows to its natural (unclamped) size because it's taken
        // out of flex flow (`position: absolute`) — its OWN box already
        // matches its OWN content exactly (so `ContentOverflowsBox` must NOT
        // fire), yet that box spills well past the 80px-tall card it lives
        // in. Before #128 item 2, nothing ever compared a component to the
        // card containing it, so this validated clean.
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
            "the text's own box already matches its own (unclamped) content — must not also fire ContentOverflowsBox: {:?}",
            violations
        );
        assert!(
            violations
                .iter()
                .all(|v| v.kind != ViolationKind::ViewportOverflow),
            "fixture should stay inside the 540px-tall frame by construction: {:?}",
            violations
        );
        let v = violations
            .iter()
            .find(|v| v.kind == ViolationKind::ContentOverflowsCard)
            .unwrap_or_else(|| {
                panic!(
                    "expected ContentOverflowsCard for text spilling past its 80px card: {:?}",
                    violations
                )
            });
        assert_eq!(v.component, "text");
        assert_eq!(
            v.axis,
            Axis::Y,
            "card is wide enough — only height should overflow: {:?}",
            v
        );
        assert!(
            v.hint.contains("card"),
            "hint should mention the card: {}",
            v.hint
        );
    }

    #[test]
    fn in_flow_codeblock_shrunk_by_its_card_is_caught_by_auto_scroll_check() {
        // Sanity/regression guard establishing the baseline this workstream
        // found empirically: an ordinary in-flow codeblock (single child,
        // no explicit height) inside a card with an *explicit* fixed height
        // gets its own box shrunk to that height by flex layout (same
        // shrink-to-fit behaviour already established for `text`/`table`),
        // so `check_auto_scroll`'s existing natural-vs-own-box comparison
        // already catches it correctly here. The genuinely uncaught case —
        // an *unclamped* codeblock whose own box already matches its own
        // (natural) content but still spills past its card — is the next
        // test, `absolutely_positioned_codeblock_spilling_past_its_card_is_flagged`.
        let code_lines: String = (1..=30)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("\\n");
        let json = format!(
            r##"{{
                "video": {{ "width": 1920, "height": 1080 }},
                "scenes": [{{
                    "duration": 1.0,
                    "children": [{{
                        "type": "card",
                        "position": "absolute",
                        "x": 100, "y": 100,
                        "style": {{ "width": "600px", "height": "300px", "background": "#111111" }},
                        "children": [{{
                            "type": "codeblock",
                            "code": "{code_lines}",
                            "auto_scroll": false
                        }}]
                    }}]
                }}]
            }}"##
        );
        let scenario = parse(&json);
        let violations = validate_geometry(&scenario);
        let v = violations
            .iter()
            .find(|v| {
                v.kind == ViolationKind::AutoScrollDisabledOverflow && v.component == "codeblock"
            })
            .unwrap_or_else(|| panic!("expected AutoScrollDisabledOverflow: {:?}", violations));
        assert_eq!(v.axis, Axis::Y);
    }

    #[test]
    fn absolutely_positioned_codeblock_spilling_past_its_card_is_flagged() {
        // #128 item 1's first repro ("a codeblock painting 578px inside a
        // 300px card"): taken out of flex flow (`position: absolute`, like
        // the analogous text/table tests above) so its own box is NOT
        // shrunk to fit the card — it stays at its natural, unscrolled
        // content height regardless of `auto_scroll`. `auto_scroll: true`
        // (the default) is used deliberately here: `check_auto_scroll` only
        // ever fires for `auto_scroll: false`, so this proves the
        // card-relative check is a genuinely independent, complementary
        // mechanism, not a duplicate of it.
        let code_lines: String = (1..=30)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("\\n");
        let json = format!(
            r##"{{
                "video": {{ "width": 1920, "height": 1080 }},
                "scenes": [{{
                    "duration": 1.0,
                    "children": [{{
                        "type": "card",
                        "position": "absolute",
                        "x": 100, "y": 100,
                        "style": {{ "width": "600px", "height": "300px", "background": "#111111" }},
                        "children": [{{
                            "type": "codeblock",
                            "position": "absolute",
                            "x": 0, "y": 0,
                            "code": "{code_lines}"
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
                .all(|v| v.kind != ViolationKind::AutoScrollDisabledOverflow),
            "auto_scroll defaults to true — that check must stay quiet: {:?}",
            violations
        );
        assert!(
            violations
                .iter()
                .all(|v| v.kind != ViolationKind::ViewportOverflow),
            "fixture should stay inside the 1080px-tall frame by construction: {:?}",
            violations
        );
        let v = violations
            .iter()
            .find(|v| v.kind == ViolationKind::ContentOverflowsCard && v.component == "codeblock")
            .unwrap_or_else(|| {
                panic!(
                    "expected ContentOverflowsCard for a 30-line codeblock spilling past its 300px card: {:?}",
                    violations
                )
            });
        assert_eq!(v.axis, Axis::Y);
    }

    #[test]
    fn in_flow_table_taller_than_its_card_is_flagged_via_content_overflows_box() {
        // #128 item 1's third repro: a table with enough rows that its
        // natural (header + rows) height exceeds a small fixed-height card.
        // As an ordinary in-flow flex child, taffy shrinks the table's own
        // assigned box down to the card's 60px (same shrink-to-fit behaviour
        // already established for `text`), so this surfaces via the
        // generalized `ContentOverflowsBox` (own box too small for own
        // content) rather than the card check — see
        // `absolutely_positioned_table_spilling_past_its_card_is_flagged`
        // below for the complementary (unclamped, card-relative) case.
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
    fn absolutely_positioned_table_spilling_past_its_card_is_flagged() {
        // Same table, but taken out of flex flow (`position: absolute`) so
        // its own box isn't shrunk to fit the card — mirrors
        // `absolutely_positioned_text_spilling_past_its_card_is_flagged`,
        // isolating the card-relative check (item 2) from the own-box check
        // (item 1) exercised above.
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
        let v = violations
            .iter()
            .find(|v| v.kind == ViolationKind::ContentOverflowsCard && v.component == "table")
            .unwrap_or_else(|| {
                panic!(
                    "expected ContentOverflowsCard for an absolutely positioned table spilling past its 60px card: {:?}",
                    violations
                )
            });
        assert_eq!(v.axis, Axis::Y);
    }

    #[test]
    fn nested_non_card_container_still_compares_its_descendant_to_the_outer_card() {
        // A `container` (layout-only, no decoration per CLAUDE.md) sits
        // between the card and the codeblock. `container`/`flex`/`grid`/
        // `positioned` must NOT update `nearest_card` — only `Component::Card`
        // does — so the codeblock two levels down must still be compared
        // against the *outer* card, not silently un-checked.
        let code_lines: String = (1..=30)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("\\n");
        let json = format!(
            r##"{{
                "video": {{ "width": 1920, "height": 1080 }},
                "scenes": [{{
                    "duration": 1.0,
                    "children": [{{
                        "type": "card",
                        "position": "absolute",
                        "x": 100, "y": 100,
                        "style": {{ "width": "600px", "height": "300px", "background": "#111111" }},
                        "children": [{{
                            "type": "container",
                            "children": [{{
                                "type": "codeblock",
                                "position": "absolute",
                                "x": 0, "y": 0,
                                "code": "{code_lines}"
                            }}]
                        }}]
                    }}]
                }}]
            }}"##
        );
        let scenario = parse(&json);
        let violations = validate_geometry(&scenario);
        let v = violations
            .iter()
            .find(|v| v.kind == ViolationKind::ContentOverflowsCard && v.component == "codeblock")
            .unwrap_or_else(|| {
                panic!(
                    "expected ContentOverflowsCard through the non-card `container` wrapper: {:?}",
                    violations
                )
            });
        assert!(
            v.path.contains("children[0].children[0].children[0]"),
            "path should reach through card -> container -> codeblock: {}",
            v.path
        );
    }

    #[test]
    fn nested_card_bigger_than_its_outer_card_is_flagged() {
        // A card nested inside another card, itself bigger than the outer
        // one it lives in — the inner card's own box (not just its
        // descendants') must be checked against the outer card too.
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
                .any(|v| v.kind == ViolationKind::ContentOverflowsCard && v.component == "card"),
            "the inner card's own box must be checked against the outer card: {:?}",
            violations
        );
    }

    #[test]
    fn content_overflowing_its_card_is_suppressed_under_a_clipping_card() {
        // Same headline repro as `absolutely_positioned_text_spilling_past_its_card_is_flagged`,
        // but the card clips (`overflow: hidden`) — the text genuinely gets
        // clipped at paint time, so ContentOverflowsCard must not fire,
        // consistent with the `parent_clips` suppression already applied to
        // check_viewport/check_content_overflows_box.
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
    fn bleed_true_does_not_exempt_content_overflows_card() {
        // Same headline repro, `bleed: true` added to the text. Per the
        // frozen `bleed` contract (see `bleeds`'s doc comment): a component
        // may leave the *frame* on purpose and still be responsible for its
        // own contents relative to its card — `bleed` must not exempt this
        // check any more than it exempts ContentOverflowsBox.
        let json = r##"{"video":{"width":960,"height":540,"fps":30,"background":"#0A0A12"},
 "scenes":[{"duration":1.0,"children":[
   {"type":"card","position":"absolute","x":330,"y":100,
    "style":{"width":300,"height":80,"background":"#1e2233","overflow":"visible"},
    "children":[{"type":"text","position":"absolute","x":0,"y":0,"bleed":true,
      "content":"Ce paragraphe est beaucoup plus grand que la carte de 80px qui le contient.",
      "style":{"font-size":44,"color":"#ffffff","width":"300px"}}]}]}]}"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        assert!(
            violations
                .iter()
                .any(|v| v.kind == ViolationKind::ContentOverflowsCard && v.component == "text"),
            "bleed: true must NOT suppress ContentOverflowsCard: {:?}",
            violations
        );
    }

    #[test]
    fn component_that_fits_its_card_is_not_flagged() {
        // Passing-case guard: same shape as the codeblock repro, but the
        // card is tall enough to hold it — must not fire.
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
                        "type": "codeblock",
                        "code": "fn main() {\n    println!(\"hi\");\n}",
                        "auto_scroll": false
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
            "a codeblock that fits its card must not be flagged: {:?}",
            violations
        );
    }

    // ─── #120: opt-in `bleed: true` exempts viewport/animated overflow only ──

    fn bleeding_shape_json(bleed: bool) -> String {
        // Identical to `shape_past_right_edge_triggers_x_overflow`'s fixture
        // (a 400×100 shape at x=1700, spilling 180px past the 1920-wide
        // viewport) plus the top-level `bleed` field under test — mirrors
        // the reference films' radial-glow base layer.
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
        // Same fixture, `bleed` omitted (defaults to false) — proves the
        // default doesn't change existing behaviour and that the exemption
        // above is actually driven by the field, not something else in the
        // fixture.
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
        // Exact fixture from `wrapped_text_taller_than_its_fixed_height_card_is_flagged`
        // (a card comfortably inside frame, wrapping a paragraph that needs
        // ~343px but the card is fixed at 80px tall) with `bleed: true`
        // added to the text — content larger than its own box is a
        // different defect than crossing the viewport edge, and must stay
        // reported regardless of `bleed` (per #120's explicit non-goal).
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
        // A container declares `bleed: true` and sits entirely inside the
        // frame itself (x=50,y=50, 200×200 — no overflow of its own). Its
        // child shape is absolutely positioned far enough (relative to the
        // container's own box) to spill past the 1920-wide viewport on its
        // own merits. `bleed` lives on the child's own `ChildComponent`, one
        // per component instance — the parent's `bleed: true` must not reach
        // down into the child's, which was never set.
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
        // Same animation-overflow fixture as `strict_anim_detects_slide_in_overflow`
        // (slide_in_left pushes a resting shape at x=100 strongly negative
        // during the first fraction of the preset) with `bleed: true` added
        // — `--strict-anim`'s AnimatedTextOverflow must be exempted exactly
        // like the resting-layout ViewportOverflow check.
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
}

/// M4 (issue #110 / #102): legibility floor tests.
#[cfg(test)]
mod legibility_tests {
    use super::*;
    use rustmotion::loader::load_scenario_from_source;

    fn parse(json: &str) -> rustmotion::schema::ResolvedScenario {
        load_scenario_from_source(None, Some(json)).expect("scenario parses")
    }

    #[test]
    fn tiny_font_on_1080p_warns() {
        // 11px on a 1080p frame is the audit's own worked example of
        // unreadable text (~1.0% of height, well under the 1.2% floor).
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
    fn default_sized_text_on_1080p_has_no_legibility_warning() {
        // No style.font-size override: falls back to text's own 48px
        // default, comfortably above the floor.
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
    fn default_table_terminal_codeblock_on_1080p_do_not_warn() {
        // 14px defaults must clear the floor so this check doesn't spam
        // every scenario that never touched style.font-size.
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [
                    { "type": "table", "headers": ["a"], "rows": [["1"]] },
                    { "type": "terminal", "lines": [{ "text": "$ ok", "type": "input" }] },
                    { "type": "codeblock", "code": "fn main() {}" }
                ]
            }]
        }"##;
        let scenario = parse(json);
        let warnings = check_legibility(&scenario);
        assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
    }

    #[test]
    fn same_absolute_px_warns_more_readily_on_a_taller_frame() {
        // The floor is a fraction of output height, so the same 20px text
        // that's fine on 1080p (1.85%) should warn on a much taller canvas
        // where 20px is proportionally tiny.
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
        use crate::commands::validate_schema::validate_scenario;
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
