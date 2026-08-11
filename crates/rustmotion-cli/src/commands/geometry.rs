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
//!   * detect terminal/codeblock content that overflows their box when
//!     `auto_scroll: false`
//!   * exempt `marquee` and `cursor` (designed to bleed)
//!   * never report a node clipped by an `overflow: hidden`/`clip`/`scroll`/
//!     `auto` ancestor as a viewport overflow (H4) — the ancestor's own bbox
//!     is still checked independently, at its own level
//!
//! Deliberately NOT in scope: a component's box vs its nearest ancestor
//! `card`'s box, independent of the viewport (#128 item 2, briefly added
//! then retired — round 4 audit, constat 7). CLAUDE.md and
//! `geometry-safety.md` both promise the validator only complains about
//! content escaping the *viewport*, never about escaping a non-clipping
//! (`overflow: visible`, the default) container — "a badge sticking out of
//! a card is legal". A box-vs-card check can only ever fire in exactly that
//! legal case (a clipping card already suppresses it the same way it
//! suppresses every other check here, so there is nothing left for it to
//! report when the card *does* clip either) — see the retired call site's
//! comment in `walk` for the full reasoning.
//!
//! Animation handling is layered: by default we only check the resting
//! (untransformed) layout, built once with `anim: None`. With
//! `--strict-anim`, we additionally sample frames — proportionally to scene
//! duration (H6; round 4 audit, constat 8: dense enough to stay near a
//! promised 8/s up to 60s scenes) — and at EACH sample, rebuild the box
//! tree and rerun layout with a real `BuildAnimationCtx` (round 4 audit,
//! constats 2 & 9): the same engine path `render_with_new_pipeline_iter`
//! calls once per rendered frame, rather than building once at rest and
//! hand-deriving only translate/scale afterwards. This is what makes
//! `timeline` style states, audio-reactive transforms, and animated
//! rotation all visible to `--strict-anim`, not just translate/scale — see
//! `validate_geometry_animated`'s doc comment. The paint pass's
//! start_at/end_at visibility window (resolved at box-tree build time,
//! independent of `anim`) is honoured the same way in both modes.
//!
//! This walker runs the new CSS-engine pipeline (taffy + cosmic-text) so the
//! geometry it checks matches what the renderer will actually paint.

use std::collections::HashSet;

use rustmotion::components::box_builder::{
    build_scene_from_refs, effective_effects, BuildAnimationCtx,
};
use rustmotion::components::intrinsic::{
    CaptionIntrinsic, CodeblockIntrinsic, GradientTextIntrinsic, RichTextIntrinsic, TableIntrinsic,
    TerminalIntrinsic, TextIntrinsic,
};
use rustmotion::components::{ChildComponent, Component};
use rustmotion::core::css::style::{
    CssStyle, TransformFn, TransformOrigin, WhiteSpace, MIN_LEGIBLE_FONT_RATIO,
    TEXT_AUTOFIT_MIN_FONT_PX,
};
use rustmotion::core::css::taffy_bridge::ConversionContext;
use rustmotion::core::css::units::{parse_origin_component, LengthContext, ParsedLength};
use rustmotion::core::engine::box_tree::{AvailableSpace, BoxKind, BoxNode, IntrinsicMeasure};
use rustmotion::core::engine::layout_pass::{run_layout, BoxLayout, LayoutResult};
use rustmotion::engine::animator::{resolve_props_for_effects, AnimatedProperties};
use rustmotion::engine::render;
use rustmotion::schema::{Camera, ResolvedScenario, Scene, ViewType};
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
    /// Retired (round 4 audit, constat 7) — no longer constructed by
    /// `walk`/`walk_anim`. Was: a component's own (resolved, post-layout)
    /// box extending past its nearest ancestor `card`'s box (#128 item 2),
    /// unconditionally on any non-clipping card — exactly the "badge
    /// sticking out of a card" pattern CLAUDE.md and `geometry-safety.md`
    /// document as legal (`overflow: visible`, the default). Kept as a
    /// variant — not renamed/removed — for `--fix`'s match arm and
    /// `--report` JSON schema stability (frozen violation-kind contract);
    /// see the module doc comment's "Deliberately NOT in scope" note for
    /// the full reasoning.
    #[allow(dead_code)] // never constructed by design — see doc comment above
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
            // Round 4 audit, constat 4: a `world` scene's decorative
            // children (particles) are never fed into the flex box tree at
            // render time either — `render_world_frame_scaled` paints them
            // full-viewport via `paint_decorative_fullscreen`, filtered out
            // of `render_with_new_pipeline_iter`'s children entirely (see
            // `scene_children.iter().filter(|c| !c.is_decorative())` there).
            // Leaving them in here would let them occupy a flex slot that
            // pushes sibling positions around in a way that never happens
            // at render, so they're dropped from the walk the same way for
            // `world` views only — `slide` views never filtered them (a
            // particle IS flex-flowed there), so scoping this to `world`
            // keeps slide-view behaviour byte-identical.
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
            // Round 4 audit, constat 3: this natural-width-vs-own-box check
            // is content vs its OWN box, exactly the same category as
            // `check_content_overflows_box` below (just for the nowrap/
            // single-line case instead of the wrapped one) — so it gets the
            // identical double exemption: an ancestor that clips
            // (`parent_clips`) genuinely crops the overflowing line before
            // it can paint past the box, and a node that clips ITSELF
            // (`container_clips`) does the same to its own content. Before
            // this fix it ran unconditionally, contradicting
            // geometry-safety.md's documented promise ("A node is also
            // exempt when it clips itself, or when any ancestor clips it")
            // and `--fix` would then strip a legitimate `white-space:
            // nowrap` from a component that was never actually broken.
            if !parent_clips && !container_clips(&child.component) {
                check_unwrappable_text(
                    &child.component,
                    &child_path,
                    &raw_bbox,
                    viewport,
                    vi,
                    si,
                    out,
                );
            }
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
            // #128 item 2 (`ContentOverflowsCard`) used to live here: a
            // component's box vs its nearest ancestor `card`'s box,
            // unconditionally (as long as nothing clipped in between).
            // Round 4 audit, constat 7: that check is structurally
            // incompatible with the validator's own documented contract.
            // Its own suppression (`!parent_clips`, mirroring every other
            // check here) is reachable if and only if the nearest card AND
            // everything between it and this node is non-clipping — i.e. it
            // could only ever fire in exactly the case CLAUDE.md ("le
            // validateur ne se plaint que si le contenu sort du viewport,
            // pas d'un parent visible") and geometry-safety.md:34/77 ("a
            // badge sticking out of a card is legal" when the card's
            // `overflow` is `visible`, the default — "no change needed")
            // both promise is legal and must NOT be reported. Whenever the
            // card *does* clip (`overflow: hidden`), `parent_clips` already
            // suppresses this whole block, so the content is invisible
            // anyway and there is nothing left to warn about either way.
            // There is no configuration where firing is both reachable and
            // consistent with the documented contract, so it is retired
            // here rather than patched with a redundant escape hatch —
            // `check_overflows_card` (and the `nearest_card` tracking that
            // fed it) is deleted; the `ViolationKind::ContentOverflowsCard`
            // variant itself is kept, unconstructed, for `--fix`'s match arm
            // and `--report` JSON schema stability (frozen violation-kind
            // contract — see that variant's doc comment).
        }

        if let Some(grandchildren) = container_children(&child.component) {
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
/// in `style.transform` order, pivoted at `style.transform-origin` —
/// resolved by [`resolve_transform_origin_2d`], defaulting to the box centre
/// exactly like the paint pass does when it's absent), then takes the AABB
/// of the four transformed corners. This is what makes rotation/skew
/// contribute correctly: an exact AABB under rotation needs the four
/// corners, not a translate/scale-only shortcut.
///
/// 3D transform functions (`RotateX`/`RotateY`/`Rotate3d`/`TranslateZ`/
/// `Translate3d`'s z component/`ScaleZ`/`Scale3d`/`Perspective`/
/// `Matrix3d`) and the general 2D `Matrix` are intentionally still not
/// modeled (identity for that function) — an exact AABB there needs
/// projecting through the full 3D pipeline `apply_transform` uses for that
/// path, out of scope for this fix. Animated transform-producing presets are
/// folded separately in `walk_anim`; this only handles what a component
/// declares directly in `style.transform`.
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

/// Round 4 audit, constat 5: resolve `style.transform-origin` to an absolute
/// viewport-space pivot `(x, y)`, in the same way the paint pass's own
/// `resolve_origin` does (`crates/rustmotion-core/src/engine/paint_pass.rs`)
/// — percentages resolve against the box's own width (x) / height (y), an
/// absent axis defaults to 50%, and an absent `transform-origin` altogether
/// defaults to dead-centre.
///
/// This mirrors `resolve_origin`'s 2D resolution rather than calling it
/// directly: that function is private to `paint_pass.rs`, which sits outside
/// this workstream's file perimeter (round 4 audit, lot VALIDATION
/// GÉOMÉTRIQUE — geometry.rs/validate.rs/scene.rs only), so it cannot be
/// marked `pub`/re-exported from here without touching a file outside that
/// scope. What's duplicated is only the small resolution *orchestration*;
/// the actual unit-conversion primitives it calls (`parse_origin_component`,
/// `ParsedLength::resolve`) are `pub` in `rustmotion_core::css::units` and
/// are the exact same functions `resolve_origin` itself calls, so the two
/// can only drift on the orchestration shape, not on what a given length
/// string resolves to. Keep this in sync with `resolve_origin` if that
/// function's resolution rules change; the z component is intentionally not
/// resolved (this fold is 2D-only, see this function's caller's doc comment
/// on the 3D exemption).
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
    // Measure via the same cosmic-text–backed intrinsic the layout engine
    // uses. Width is bounded by the node's own resolved `bbox.w` (not
    // `MaxContent`) so a `text-autofit: true` node can shrink to fit it —
    // see `measurer_and_nowrap`'s `TextIntrinsic`/`GradientTextIntrinsic`
    // arms and `CssStyle::text_autofit`'s doc comment. For a non-autofit
    // node this changes nothing: `TextIntrinsic::measure` only reads the
    // width constraint at all when `text_autofit` is on (see its early
    // return), and `nowrap` already forces a single unwrapped line here
    // regardless of what width is offered — so `natural_w` below is
    // "natural" in the non-autofit case exactly as before, and "shrunk to
    // fit, if that's enough" when the author declared it.
    let (natural_w, _) = intrinsic.measure(
        (None, None),
        (AvailableSpace::Definite(bbox.w), AvailableSpace::MaxContent),
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

    // Height is bounded by the node's own resolved content-box height `ch`
    // (not `MaxContent`) for the same reason width is bounded by `cw`: a
    // `text-autofit: true` node can only try to shrink into a target it's
    // actually told about. `TextIntrinsic::measure` only reads this height
    // bound at all when `text_autofit` is on (see its early return right
    // after the base, non-autofit measurement), so a non-autofit node's
    // `measured_h` is unaffected — this is the same "safe to change
    // unconditionally" argument as `check_unwrappable_text`'s width bound
    // above.
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

/// Round 4 audit, constat 6: this used to hand-roll codeblock/terminal
/// natural-height formulas with a hardcoded 16+16=32px padding assumption
/// and (for terminal) the CSS `style.line-height` property — neither of
/// which is what actually gets painted. `CodeblockIntrinsic`/
/// `TerminalIntrinsic` are the exact measurers `component_intrinsic`
/// (`box_builder.rs`) hands to the layout pass for these two components, so
/// calling them here — instead of re-deriving the formula — keeps this
/// check byte-for-byte in sync with `compute_code_dimensions` (codeblock,
/// which DOES read `style.padding_px()`) and `terminal::line_height()`
/// (terminal, which does NOT honour `style.line-height`, always using its
/// own fixed `LINE_HEIGHT`/`FONT_SIZE` ratio). Measuring at
/// `(None, None)`/`MaxContent` yields each component's natural (unbounded)
/// size, exactly like `check_unwrappable_text`/`check_content_overflows_box`
/// already do for the text-family intrinsics.
fn check_auto_scroll(
    component: &Component,
    path: &str,
    bbox: &BBox,
    viewport: (u32, u32),
    vi: usize,
    si: usize,
    out: &mut Vec<GeometryViolation>,
) {
    let max_content = (AvailableSpace::MaxContent, AvailableSpace::MaxContent);
    match component {
        Component::Codeblock(cb) if !cb.auto_scroll => {
            let (_, natural_h) =
                CodeblockIntrinsic::from_codeblock(cb).measure((None, None), max_content);
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
            let (_, natural_h) =
                TerminalIntrinsic::from_terminal(t).measure((None, None), max_content);
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
// The calibration and its threshold now live on `MIN_LEGIBLE_FONT_RATIO`
// itself, in `rustmotion_core::css::style` — relocated there (not
// duplicated) so `CssStyle::text_autofit`'s shrink floor can reuse the exact
// same calibrated ratio instead of inventing a second one; `rustmotion-core`
// is a dependency of this crate, never the other way around, so that is the
// only direction the constant can live in for both sides to share it.

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

    // The check above reads the *declared* size, which is the rendered size
    // for every component except an autofitting one: `text-autofit` shrinks
    // toward `TEXT_AUTOFIT_MIN_FONT_PX`, a constant pinned to a 1080-tall
    // reference so that measure and paint cannot disagree about it (see that
    // constant's doc comment). `min_px` here is relative to the *real* frame
    // height, so on any canvas taller than 1080 the floor sits below the
    // legibility threshold — and a declared 120px that shrinks to ~13px on a
    // 2160-tall frame would otherwise pass this check in silence, which is
    // the exact failure mode autofit exists to remove rather than relocate.
    //
    // Advisory and conditional: it fires only when the two genuinely diverge
    // (taller-than-1080 canvases), and says "may" because resolving the
    // actual shrunk size needs layout, which this pass does not run. The
    // precise fix is a canvas-relative floor on both sides, which requires
    // plumbing the frame height into `TextIntrinsic` — tracked separately.
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

/// Effective rendered font size(s) for a component, mirroring exactly the
/// default each `Painter` falls back to when `style.font-size` is unset
/// (see the file/line citations below — kept in sync by hand since these
/// defaults live in `rustmotion-components`, out of this workstream's
/// scope). A component can report more than one size (e.g. a notification's
/// title and message use different sizes).
/// Whether this component's painter actually honours `style.text-autofit`.
/// Deliberately the same two variants `TextIntrinsic::with_autofit` is called
/// for — every other component ignores the field, so warning about them would
/// be a false positive about a shrink that cannot happen.
fn declares_text_autofit(component: &Component) -> bool {
    match component {
        Component::Text(t) => matches!(t.style.text_autofit, Some(true)),
        Component::GradientText(t) => matches!(t.style.text_autofit, Some(true)),
        _ => false,
    }
}

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
/// Round 4 audit, constat 8: raised from 40 (a ~5s ceiling on the promised
/// 8/s cadence) to 480 - 60s worth of samples at exactly 8/s, the audit's
/// own reference duration ("pas de 0.51s a 20s, 1.0s a 40s, 1.5s a 60s").
/// Past a 5s scene, the old cap widened the step linearly with duration
/// (0.51s at 20s, 1.0s at 40s, 1.5s at 60s), so a brief transform excursion
/// shorter than that step could land entirely between two samples and never
/// get checked. Cost, measured on the box-tree-rebuild-per-sample walker
/// this cap now drives (constats 2 & 9): a 15-component animated scene at
/// 60s / 480 samples took 309ms wall-clock in a `--release` build
/// (~0.64ms/sample) and 543ms in a debug build (~1.13ms/sample) - see
/// `timing_probe_for_constat_8` (run with `--ignored`) for the harness.
/// `--strict-anim` is opt-in, and `validate`/`render`'s implicit checks
/// don't pass it, so this cost is paid only when explicitly asked for.
/// Scenes longer than 60s still degrade past this cap - CLAUDE.md's own
/// architecture favours many short scenes stitched by transitions/world
/// panning over one very long scene, so a single-scene ceiling at 60s
/// covers the documented common case.
const ANIM_MAX_SAMPLES: usize = 480;

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
///
/// Round 4 audit, constats 2 & 9: rebuilds the box tree AND reruns layout at
/// EACH sampled time, with a real `BuildAnimationCtx` — exactly the engine
/// path `render_with_new_pipeline_iter` calls once per rendered frame
/// (`build_scene_from_refs` + `run_layout`) — instead of building once at a
/// frozen resting state (`anim: None`) and hand-deriving only
/// translate/scale afterwards in `walk_anim`. This one change fixes two
/// separate blind spots at once, because both are downstream of the SAME
/// `anim: None`:
///   * `build_child` only applies `apply_style_states` (`timeline` steps)
///     and the audio-reactive CSS block at the times a REAL `local_actx` is
///     available — a `timeline` step that changes a box-model property
///     (e.g. `width`) was invisible at every sample (constat 2).
///   * `apply_animated_props` bakes the resolved transform (translate,
///     scale, AND rotation) into `css.transform` — so once the tree is
///     rebuilt with the real time, `apply_static_node_transform` (already
///     used for static `style.transform`, already handling rotation/skew
///     via a four-corner AABB) picks up animated rotation too, with no
///     separate rotation-aware fold needed (constat 9).
pub fn validate_geometry_animated(scenario: &ResolvedScenario) -> Vec<GeometryViolation> {
    let mut violations = Vec::new();
    let mut seen: HashSet<(usize, usize, String)> = HashSet::new();
    let fps = scenario.video.fps;
    for (vi, view) in scenario.views.iter().enumerate() {
        for (si, scene) in view.scenes.iter().enumerate() {
            // Constat 4: same decorative-child filtering as `validate_geometry`
            // — see that call site's comment for why.
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

            let camera = scene
                .camera
                .as_ref()
                .filter(|_| !scene_uses_depth(&children));

            let path_root = format!("views[{}].scenes[{}]", vi, si);
            let scene_duration = scene.duration;
            // A frozen scene renders nothing past `freeze_at` — every path
            // now funnels through `SceneTime`, which clamps there (#164). So
            // sampling beyond it evaluates transforms at instants the video
            // never contains, which is how `--strict-anim` reports a
            // violation that cannot happen. Bounding the sample list rather
            // than clamping each `time` afterwards also avoids generating a
            // run of identical post-freeze samples.
            //
            // `scene_duration` itself stays untouched below: duration-relative
            // effects (contract from PR #27) must keep their real window —
            // only the sampling ceiling moves.
            let sample_until = scene
                .freeze_at
                .map_or(scene_duration, |f| f.clamp(0.0, scene_duration));

            for time in anim_sample_times(sample_until) {
                let root_css = render::root_style(scene.layout.as_ref(), view.view_type.clone());
                let anim = Some(BuildAnimationCtx {
                    time,
                    scene_duration,
                    fps,
                });
                let built = build_scene_from_refs(children.iter(), viewport_f, root_css, anim);
                let layouts = run_layout(&built.root, viewport_f, &ConversionContext::default());

                walk_anim(
                    &children,
                    &built.root.children,
                    &layouts,
                    &built.stagger_delays,
                    &built.time_params,
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

/// `boxes` filtered down to principal nodes — motion-blur/trail ghosts
/// (`BoxKind::Ghost`, only ever generated when the box tree is built with a
/// real `BuildAnimationCtx`, see `build_ghosts` in `box_builder.rs`) are
/// prepended before each principal in the flat child list, so a naive
/// `children.iter().zip(boxes.iter())` would misalign as soon as any
/// earlier sibling has `motion_blur`/`trail` — the same category of bug
/// H3 fixed for raw JSON indices. `walk` never hits this (its box tree is
/// always built with `anim: None`, so `build_child` never generates ghosts
/// there — see its own module doc comment); `walk_anim` started building
/// with a real `BuildAnimationCtx` for constats 2 & 9, so it must filter.
/// Ghosts are paint-only trailing copies of the SAME component at an
/// earlier local time and are not independently meaningful overflow
/// targets — the principal's own per-sample check already covers the
/// component's position.
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
            // Round 4 audit, constats 2 & 9: `box_node.css` was rebuilt at
            // this sample's real time (see `validate_geometry_animated`),
            // so `css.opacity` already reflects `apply_animated_props` —
            // no separate `AnimatedProperties` re-derivation needed for
            // the visibility short-circuit any more.
            && box_node.css.opacity.unwrap_or(1.0) > 0.001
        {
            let stagger_delay = stagger_delays
                .get(box_node.id as usize)
                .copied()
                .unwrap_or(0.0);
            // `time` above is *global* scene time; the renderer never
            // resolves effects at that raw value once a `time_scale`/
            // `time_offset`-bearing container is in the ancestor chain —
            // `build_child` remaps it first (`box_builder.rs`:
            // `t_local = scale * t_global + shift`), and it already used
            // this same remap to build `box_node.css` above. `local_time`
            // is only still needed here to independently re-derive
            // `AnimatedProperties.char_animation` (char-level overshoot),
            // which `apply_animated_props` deliberately does NOT bake into
            // CSS (component-internal, painter-only property — see that
            // function's doc comment) — `built.time_params` carries the
            // exact same accumulated `(scale, shift)` the renderer used, so
            // this stays in lockstep with it.
            let (scale, shift) = time_params
                .get(box_node.id as usize)
                .copied()
                .unwrap_or((1.0, 0.0));
            let local_time = scale * time + shift;
            let props = match effective_effects(&child.component, stagger_delay) {
                Some(effects) => resolve_props_for_effects(&effects, local_time, scene_duration),
                None => AnimatedProperties::default(),
            };
            let raw_bbox = bbox_of(layout);
            // `apply_static_node_transform` — the SAME fold `walk` uses for
            // a *static* `style.transform` — now does the whole job:
            // `box_node.css.transform` already carries the resolved
            // translate/scale/rotation (`apply_animated_props`, baked in at
            // box-tree build time for this sample) composed with any
            // static `style.transform` the component also declares, and
            // the four-corner AABB it computes already accounts for
            // rotation (constat 9) the same way it does for a static
            // `transform: rotate(...)`.
            let mut transformed = apply_static_node_transform(&raw_bbox, &box_node.css, viewport_f);
            // Char-level overshoot (e.g. `char_scale_in`'s default 1.08)
            // is the one animated-transform contributor NOT baked into
            // `css.transform` — fold it in as an extra uniform scale
            // around the already-transformed box's own centre.
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
                time,
                scene_duration,
                seen,
                out,
            );
        }
    }
}

/// Scale a bbox by `factor` around its OWN centre (as opposed to
/// `apply_static_node_transform`'s pivot, which is `transform-origin`) —
/// used only for the char-animation overshoot top-up in `walk_anim`, which
/// is not a CSS transform and has no origin concept of its own.
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

    // ─── Round 4 audit, constat 4: a `world` scene without its own `layout`
    // must be validated against the SAME centred-column root layout
    // `render_world_frame_scaled` synthesizes, not the plain top-aligned
    // slide default ─────────────────────────────────────────────────────

    #[test]
    fn layoutless_world_scene_uses_the_centred_root_not_the_slide_default() {
        // A single in-flow (no `position`) 1000×100 shape, wider than the
        // 800px-wide viewport, inside a `world` scene with no `layout` of
        // its own. `render_world_frame_scaled` synthesizes a centred column
        // (`align_items: center`) for exactly this case.
        //
        // Red-phase capture (root forced back to the slide default): bbox
        // = {x: 0, y: 0, w: 1000, h: 100}, hint "current right edge is
        // 1000" — `align_items` unset resolves start-aligned for an item
        // with an explicit size, so the shape sits at x=0, right edge=1000.
        //
        // Under the CORRECT (world-default, centred) root, a 1000px item in
        // an 800px-wide container centres at x=(800-1000)/2=-100: bbox
        // x=[-100,900]. Still a single-axis (X) overflow — both edges are
        // crossed, but `Axis::Both` means "X and Y both overflow", not "X
        // overflows on both sides" — but the reported bbox.x is materially
        // different (-100 vs 0) and, before this fix, wrong.
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

    // ─── Round 4 audit, constat 3: unwrappable_text_overflow must respect a
    // clipping ancestor exactly like check_viewport/check_content_overflows_box
    // already do ───────────────────────────────────────────────────────────

    #[test]
    fn unwrappable_text_is_suppressed_under_a_clipping_ancestor_card() {
        // Same headline fixture as `unwrappable_text_in_narrow_card_is_flagged`
        // (a 200px card, 96px nowrap text, natural width far exceeding 200px)
        // but the card now clips (`overflow: hidden`): the text genuinely
        // gets cropped to the card's edge at paint time, so nothing overflows
        // on screen — geometry-safety.md promises this is exempt ("A node is
        // also exempt when it clips itself, or when any ancestor clips it"),
        // and `check_viewport`/`check_content_overflows_box` already honour
        // it. This is a CORRECT scenario (the clip makes the excess
        // invisible) that the validator wrongly rejected before this fix.
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
        // Regression guard: the exact pre-existing fixture from
        // `unwrappable_text_in_narrow_card_is_flagged` (card overflow left
        // at the default `visible`) must keep firing — the fix must only add
        // a clip-aware exemption, not silence the check generally.
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

    // ─── Round 4 audit, constat 6: check_auto_scroll must use the real
    // painter's dimension formula (CodeblockIntrinsic/TerminalIntrinsic),
    // not a hardcoded 16+16=32px padding assumption ──────────────────────

    #[test]
    fn codeblock_auto_scroll_check_honours_explicit_padding_not_a_hardcoded_16px() {
        // 10 lines, font-size defaults to 14px (line-height 1.3 -> 18.2px/line
        // -> 182px of text), auto_scroll: false, box height fixed at 250px.
        // style.padding is *explicitly* 60px on every side (120px vertical
        // budget) — nothing close to the hardcoded "16 top + 16 bottom" the
        // old formula assumed. Real natural height (chrome disabled):
        // 120 (padding) + 182 (text) = 302px, ~52px past the 250px box —
        // a genuine overflow. The hardcoded-32px formula computed
        // 32 + 182 = 214px, comfortably under 250px, and stayed silent.
        let code_lines: String = (1..=10)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("\\n");
        let json = format!(
            r##"{{
                "video": {{ "width": 1920, "height": 1080 }},
                "scenes": [{{
                    "duration": 1.0,
                    "children": [{{
                        "type": "codeblock",
                        "code": "{code_lines}",
                        "auto_scroll": false,
                        "style": {{ "width": "600px", "height": "250px", "padding": "60px" }}
                    }}]
                }}]
            }}"##
        );
        let scenario = parse(&json);
        let violations = validate_geometry(&scenario);
        let v = violations
            .iter()
            .find(|v| v.kind == ViolationKind::AutoScrollDisabledOverflow);
        assert!(
            v.is_some(),
            "expected AutoScrollDisabledOverflow for a 60px-padded codeblock the \
             hardcoded-16px formula wrongly cleared (real natural height ~302px > \
             250px box): {:?}",
            violations
        );
    }

    #[test]
    fn codeblock_auto_scroll_check_does_not_false_positive_on_tight_default_padding() {
        // Complementary false-positive guard: 10 lines, DEFAULT padding
        // (16px each side -> 32px vertical budget, matching
        // CodeblockIntrinsic's own fallback for an all-zero/unset padding —
        // see `CodeblockIntrinsic::from_codeblock`'s (16,16,16,16) default).
        // Natural height: 32 + 182 = 214px. Box height 220px comfortably
        // holds it — must NOT be flagged.
        let code_lines: String = (1..=10)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("\\n");
        let json = format!(
            r##"{{
                "video": {{ "width": 1920, "height": 1080 }},
                "scenes": [{{
                    "duration": 1.0,
                    "children": [{{
                        "type": "codeblock",
                        "code": "{code_lines}",
                        "auto_scroll": false,
                        "style": {{ "width": "600px", "height": "220px" }}
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
            "a codeblock that genuinely fits its box must not be flagged: {:?}",
            violations
        );
    }

    #[test]
    fn terminal_auto_scroll_check_uses_the_painters_fixed_line_height_ratio() {
        // Terminal (unlike codeblock) does NOT honour `style.line-height` at
        // paint time — `terminal.rs`'s own `line_height()` method always
        // computes `(font_size * 22.0 / 14.0).ceil()` (a fixed ratio baked
        // into the component, `terminal::LINE_HEIGHT`/`FONT_SIZE`), ignoring
        // any CSS `line-height` override entirely. The old hand-rolled check
        // used `t.style.line_height_for(font_size)` (the CSS property,
        // honouring `style.line-height`) instead — so a `line-height: 3`
        // override (unitless -> 3 * 14px = 42px/line) inflated the OLD
        // formula's estimate even though the real painter still renders
        // 22px lines and ignores the override.
        //
        // 8 lines, chrome disabled, font-size defaults to 14:
        //   real (TerminalIntrinsic/painter): 2*16 (fixed padding) +
        //     8 * 22 (fixed ratio, ignores the override) = 32 + 176 = 208px
        //   old hand-rolled (CSS line-height, AND its own wrong default
        //     font-size of 16px instead of the real 14px):
        //     32 + 8 * line_height_for(16) = 32 + 8 * 48 = 32 + 384 = 416px
        //     (captured red-phase output: "terminal content needs ~416px")
        // Box height fixed at 300px sits strictly between the two: the real
        // content fits (208 < 300), but the old formula's inflated 416px
        // wrongly reported an overflow — a false positive this fix removes.
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "terminal",
                    "lines": [
                        { "text": "one" }, { "text": "two" }, { "text": "three" },
                        { "text": "four" }, { "text": "five" }, { "text": "six" },
                        { "text": "seven" }, { "text": "eight" }
                    ],
                    "show_chrome": false,
                    "auto_scroll": false,
                    "style": { "width": "600px", "height": "300px", "line-height": 3 }
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        assert!(
            violations
                .iter()
                .all(|v| v.kind != ViolationKind::AutoScrollDisabledOverflow),
            "terminal ignores style.line-height at paint time — the check must too, \
             real content (208px) fits the 300px box: {:?}",
            violations
        );
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
    fn strict_anim_respects_a_containers_time_offset_remap() {
        // Constat #3: `walk_anim` used to resolve effects at raw *global*
        // scene time, ignoring any `time_scale`/`time_offset` remap
        // accumulated from ancestor containers — even though the renderer
        // (`box_builder::build_child`) always resolves at the *local*
        // remapped time (`t_local = scale * t_global + shift`).
        //
        // Here the shape's `slide_in_left` (delay=0, duration=1.0) sits
        // inside a `flex` with `time_offset: -5.0`, which (per
        // `rustmotion/src/tests.rs`'s time-remap tests) shifts local time to
        // `t_local = t_global + 5.0`. Every sample in this 2s scene
        // (t_global in [0, 2]) therefore resolves at local time in [5, 7] —
        // 5-7s past the 1s animation window, fully settled at rest (x=100,
        // well inside the 1920px-wide viewport). A walker that ignores the
        // remap instead resolves at raw t_global in [0, 2], still inside the
        // animation's own [0, 1] window for the first half of the scene,
        // and reports a slide-in overflow that never actually happens at
        // render time.
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

    // ─── Round 4 audit, constat 2: --strict-anim must resolve `timeline`
    // style states and audio-reactive transforms — both gated on
    // `local_actx.is_some()` in `build_child`, which was always `None`
    // here ────────────────────────────────────────────────────────────────

    #[test]
    fn strict_anim_catches_a_brief_excursion_a_40_sample_cap_would_miss() {
        // A 100×100 shape resting safely at x=100 (box x=[100,200]) in a
        // 20s scene, with `slide_in_left` (delay=9.85s, duration=1.0s):
        // `position.x` eases from -200 to 0 via EaseOutCubic, so the box
        // only crosses the left edge (x < -0.5) for the FIRST ~20% of the
        // 1s window (t in [9.85, ~10.06]) — a ~206ms excursion, while
        // opacity has already ramped past its own [9.85, 10.15] fade-in
        // window's midpoint by then (so it isn't filtered as invisible).
        //
        // With the OLD `ANIM_MAX_SAMPLES=40` cap, this 20s scene sampled at
        // step 20/39 ≈ 0.513s — grid points at k·0.513s land at
        // t=9.744 (k=19) and t=10.256 (k=20), straddling the whole ~206ms
        // excursion without a single sample landing inside it. Confirmed by
        // temporarily reverting the cap to 40 during development: this
        // fixture produced ZERO violations (`violations: []`) — the exact
        // false negative constat 8 describes.
        //
        // With the new 480-sample cap (step ≈ 0.042s), a sample lands well
        // inside the excursion — this run finds one at t=9.94s with
        // bbox.x≈-52.16 (tx≈-152 relative to the resting x=100).
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
        // A 200×100 shape, safely inside a 1920×1080 viewport at rest
        // (x=[100,300]). A `timeline` step at t=1.0s grows `style.width` to
        // 1900px — box_builder's `apply_style_states` runs on the CSS
        // *before* layout, so this is a genuine box-model change, not a
        // paint-only transform: at t>=1.0s the real render lays out a
        // 1900px-wide box at x=100, right edge 2000 — 80px past the
        // 1920px-wide frame.
        //
        // The OLD `--strict-anim` walker built its box tree ONCE with
        // `anim: None` (so `apply_style_states` only ever evaluated at
        // t=0, before the step's `at`) and never rebuilt it per sample —
        // every one of the 16 samples in this 2s scene measured the
        // resting 200px-wide box, so this never got flagged.
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

    // ─── Round 4 audit, constat 9: --strict-anim must model rotation (and
    // any other transform `apply_animated_props` bakes into `css.transform`),
    // not just translate_x/y and scale_x/y ─────────────────────────────────

    /// Every render path now clamps at `scene.freeze_at` (#164, `SceneTime`),
    /// so nothing past it is ever rendered. Sampling beyond it therefore
    /// reports a violation the video cannot contain — a false positive that
    /// blocks a correct scenario and sends a generator "fixing" something
    /// that was never wrong.
    ///
    /// Same fixture as the spin test below, frozen at 0.05s: the square only
    /// leaves the frame once the rotation has turned far enough, well after
    /// the freeze.
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

    /// The mirror: without the freeze, the very same fixture must still be
    /// caught. Otherwise the bound above would be silencing real overflow
    /// rather than removing an unreachable sample.
    #[test]
    fn strict_anim_detects_a_spin_animation_pushing_a_square_off_screen() {
        // Same headline numbers as the static-transform regression
        // `static_rotation_is_folded_into_the_viewport_check`: a 100×100
        // square at (1810, 490) in a 1920×1080 viewport — resting box
        // x=[1810,1910], 10px inside the right edge. `spin` animates
        // `rotation` linearly 0deg->360deg over the 2s scene; ANY sampled
        // angle away from a multiple of 90deg grows the AABB half-width
        // beyond 50px * (|cos|+|sin|) > 50px, pushing the right edge past
        // 1920 (e.g. at 20deg: half-width ~64.1px, right edge ~1924).
        //
        // The OLD `transform_bbox` only read `translate_x/y`/`scale_x/y`
        // from `AnimatedProperties` — a pure-rotation preset leaves both at
        // their identity values (0 and 1), so every sample folded to
        // exactly the resting bbox and this never fired, at any sample,
        // for the whole 2s sweep through 360 degrees.
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
        // Round 4 audit, constat 8: with the old ANIM_MAX_SAMPLES=40 cap,
        // the step between samples grew linearly past a 5s scene —
        // 20s/39 ≈ 0.513s at 20s, 40s/39 ≈ 1.026s at 40s, 60s/39 ≈ 1.538s
        // at 60s (the exact numbers the audit cited). With the raised cap
        // (480), the step stays pinned near the promised 1/8s = 0.125s
        // resolution across the same range.
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
        children.pop(); // trailing comma
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

    // ─── Round 4 audit, constat 5: `transform-origin` must pivot the static
    // transform fold, not always the box centre ─────────────────────────────

    #[test]
    fn transform_origin_right_edge_keeps_a_scaled_shape_inside_the_viewport() {
        // 100×100 shape at (880, 450) in a 1000×1000 viewport: at rest,
        // x=[880,980] — comfortably inside, 20px margin. `scale(x: 3)`
        // pivoted at `transform-origin: { x: "right" }` (the box's own right
        // edge, 100%) grows the box purely leftward from that fixed edge:
        // left corner offset from pivot (980) is -100, ×3 = -300 -> new x =
        // 680; right corner offset is 0 -> stays at 980. Correct AABB:
        // x=[680,980], fully inside [0,1000] — this scenario is CORRECT.
        //
        // Before this fix, `apply_static_node_transform` always pivoted at
        // the box centre (930): left corner offset -50×3=-150 -> x=780;
        // right corner offset +50×3=+150 -> x=1080 — 80px past the 1000-wide
        // frame, a false positive (captured in the red-phase run below).
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
        // Sanity/regression guard: an *explicit* `transform-origin: 50% 50%`
        // must fold to exactly the same AABB as no `transform-origin` at all
        // — same fixture and expectation as
        // `static_rotation_is_folded_into_the_viewport_check`.
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

    // ─── Round 4 audit, constat 7: `ContentOverflowsCard` (#128 item 2) is
    // retired — a component escaping a non-clipping (`overflow: visible`,
    // the default) card is the exact "badge sticking out of a card is
    // legal" pattern CLAUDE.md / geometry-safety.md document as fine, so the
    // validator must accept it, not report it. See the module doc comment's
    // "Deliberately NOT in scope" note and `walk`'s retired call site for
    // the full reasoning. These fixtures are the same ones that used to
    // assert the (wrong) opposite — kept, with flipped assertions, as
    // regression coverage across component types (text/codeblock/table/
    // nested card/bleed) now that the check is gone. ─────────────────────

    #[test]
    fn absolutely_positioned_text_spilling_past_a_visible_card_is_legal() {
        // The audit's original headline repro for #128 item 2: a text with
        // no fixed height, inside a card, grows to its natural (unclamped)
        // size because it's taken out of flex flow (`position: absolute`)
        // — its OWN box already matches its OWN content exactly (so
        // `ContentOverflowsBox` must NOT fire), and that box spills past
        // the 80px-tall card it lives in. The card's `overflow` is
        // `visible` (the documented default that permits exactly this) —
        // per constat 7, `ContentOverflowsCard` must no longer fire here.
        //
        // Red-phase (before this fix): validate_geometry reported one
        // ContentOverflowsCard violation for this fixture (component:
        // "text", axis: Y, hint mentioning "extends past its containing
        // card") — captured when this test asserted the opposite.
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

    /// The guarantee that makes retiring `ContentOverflowsCard` safe rather
    /// than merely defensible: escaping a visible card is legal, but
    /// escaping the *device* never is, and that is `check_viewport`'s job —
    /// not the retired check's. Same fixture as
    /// `absolutely_positioned_text_spilling_past_a_visible_card_is_legal`,
    /// moved down the frame so the overspill leaves the viewport. If this
    /// ever stops firing, the removal has opened a real blind spot.
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
    fn absolutely_positioned_codeblock_spilling_past_a_visible_card_is_legal() {
        // #128 item 1's first repro ("a codeblock painting 578px inside a
        // 300px card"): taken out of flex flow (`position: absolute`, like
        // the analogous text/table tests above) so its own box is NOT
        // shrunk to fit the card — it stays at its natural, unscrolled
        // content height regardless of `auto_scroll`. `auto_scroll: true`
        // (the default) is used deliberately here so `check_auto_scroll`
        // stays quiet too, isolating this from every other check: the card
        // has default (`visible`) overflow, so per constat 7 this must
        // validate clean.
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
        assert!(
            violations
                .iter()
                .all(|v| v.kind != ViolationKind::ContentOverflowsCard),
            "codeblock sticking out of a visible-overflow card is a legal, documented pattern: {:?}",
            violations
        );
    }

    #[test]
    fn in_flow_table_taller_than_its_card_is_flagged_via_content_overflows_box() {
        // #128 item 1's third repro: a table with enough rows that its
        // natural (header + rows) height exceeds a small fixed-height card.
        // As an ordinary in-flow flex child, taffy shrinks the table's own
        // assigned box down to the card's 60px (same shrink-to-fit behaviour
        // already established for `text`), so this surfaces via the
        // generalized `ContentOverflowsBox` (own box too small for own
        // content).
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
        // Same table, but taken out of flex flow (`position: absolute`) so
        // its own box isn't shrunk to fit the card — the card's `overflow`
        // is `visible` (the default), so per constat 7 this must validate
        // clean.
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
        // A card nested inside another (default/`visible`-overflow) card,
        // itself bigger than the outer one it lives in — per constat 7 this
        // is the same "sticking out on purpose" pattern, now legal for any
        // component type, cards included.
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
        // Same headline repro, but the card clips (`overflow: hidden`) —
        // the text genuinely gets clipped at paint time, so
        // ContentOverflowsCard must not fire either, consistent with the
        // `parent_clips` suppression already applied to
        // check_viewport/check_content_overflows_box. Distinct from the
        // `visible` fixtures above: this is the OTHER half of the "no
        // configuration where it's both reachable and correct to fire"
        // argument (constat 7) — a clipping card suppresses it for an
        // unrelated reason (parent_clips), not because of the retirement.
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

    // ─── text-autofit: Vérification point 4 ─────────────────────────────
    //
    // "Un scénario qui déborde aujourd'hui doit valider après, avec
    // l'ajustement déclaré — et un scénario sans ajustement doit continuer
    // à déborder et à être signalé." These reuse the exact same fixtures as
    // the pre-existing `ContentOverflowsBox`/`UnwrappableTextOverflow`
    // tests above (`wrapped_text_taller_than_its_fixed_height_card_is_flagged`,
    // `unwrappable_text_in_narrow_card_is_flagged`), adding only
    // `text-autofit: true`, so the "before"/"after" pair is a controlled
    // comparison rather than two unrelated fixtures.

    #[test]
    fn text_autofit_resolves_a_content_overflow_that_would_otherwise_fire() {
        // Same fixture as `wrapped_text_taller_than_its_fixed_height_card_is_flagged`
        // (a paragraph that needs ~343px of height inside an 80px-tall
        // card), with `text-autofit: true` added.
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
        // Control for the test above: identical fixture, no `text-autofit`
        // — must still report `ContentOverflowsBox` exactly like
        // `wrapped_text_taller_than_its_fixed_height_card_is_flagged`.
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
        // The floor stops the shrink before it can ever make this fit: the
        // same paragraph crammed into an 8px-tall card. `text-autofit`
        // narrows the overflow class, it does not eliminate every
        // overflow — this must stay reported, per the brief's explicit
        // requirement that a still-too-small box remains a signalled
        // violation, not a silence.
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
        // Same fixture as `unwrappable_text_in_narrow_card_is_flagged` (a
        // 96px nowrap line in a 200px-wide card), with `text-autofit: true`
        // added — this is `check_unwrappable_text`'s territory, not
        // `check_content_overflows_box`'s.
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

    /// `text-autofit`'s shrink floor is pinned to a 1080-tall reference so
    /// measure and paint agree on it; this legibility floor is relative to
    /// the real frame. On a taller canvas the two diverge, and a declared
    /// size well above the floor can still render illegibly. Without this
    /// warning that case passes in silence — the exact failure mode autofit
    /// is supposed to remove, not relocate.
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
        // Both numbers must be named: what it can shrink to, and the floor
        // it would fall under. A warning that says neither is unactionable.
        assert!(warnings[0].contains("13px"), "got: {}", warnings[0]);
        assert!(warnings[0].contains("26px"), "got: {}", warnings[0]);
    }

    /// The mirror case, and the reason the warning is conditional rather
    /// than unconditional: at 1080 the pinned floor already sits at the
    /// legibility threshold, so there is nothing to warn about and doing so
    /// would be noise on every autofitting text in the common canvas.
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

    /// A component whose painter ignores `text-autofit` must never draw the
    /// warning: it cannot shrink, so the shrink cannot make it illegible.
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
