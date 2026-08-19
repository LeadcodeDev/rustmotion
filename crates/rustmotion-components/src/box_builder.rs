//! Bridge from the `Component` tree to the new `BoxNode` tree.
//!
//! Each `ChildComponent` becomes one `BoxNode`. The component's
//! `style: CssStyle` is augmented with:
//! - `position: absolute` + `top` / `left` when `child.position` is set
//! - `width` / `height` from the component's `size` field (if any)
//! - `z-index` from the child's `z_index` field
//!
//! Container components (Card / Flex / Grid / Container / Positioned)
//! recursively build child boxes. Leaf components produce an empty-children
//! box that the dispatcher will paint.
//!
//! The builder also returns a flat `Vec<&Component>` indexed by NodeId so
//! the painter can resolve a node back to its component.

use std::sync::Arc;

use rustmotion_core::css::style::{AlignSelf, CssStyle, Position, Size as CSize};
use rustmotion_core::css::{apply_animated_props, LengthPercentage as CLP};
use rustmotion_core::engine::animator::{resolve_props_for_effects, AnimatedProperties};
use rustmotion_core::engine::box_tree::{BoxKind, BoxNode, NodeId};
use rustmotion_core::schema::video::{AnimationEffect, MotionBlurConfig, TrailConfig};

use crate::callout::ArrowDirection as CalloutArrowDirection;
use crate::chart::ChartType;
use crate::divider::DividerDirection;
use crate::mockup::MockupDevice;
use crate::skeleton::SkeletonVariant;
use crate::stepper::StepperOrientation;
use crate::timeline::TimelineDirection;
use crate::tooltip::TooltipArrow;
use crate::{ChildComponent, Component};

/// Frame-level context passed into the builder so animations can be resolved
/// per-node and merged into the resulting `CssStyle`. When `None`, the box
/// tree is built without any animation overrides (resting state).
#[derive(Debug, Clone, Copy)]
pub struct BuildAnimationCtx {
    pub time: f64,
    /// Seconds since the scenario started, as opposed to `time`, which
    /// restarts at every scene. Only the `audio-reactive` binding reads it:
    /// the audio analysis is indexed on the scenario's timeline, so using
    /// `time` gave a scene starting at t=73 s the analysis at 73 s *into that
    /// scene*. Every animation stays on `time`, which is what a delay, a
    /// stagger or a preset is written against.
    pub scenario_time: f64,
    pub scene_duration: f64,
    /// Frames per second of the output video. Required to convert the
    /// `shutter` fraction (in `MotionBlurConfig`) into an absolute temporal
    /// offset: `shutter_window = shutter / fps` seconds.
    pub fps: u32,
}

/// Padding allowance to keep arrow/connector heads inside the box.
const ARROW_BBOX_PADDING: f32 = 16.0;

/// Result of building a box tree from a scene description.
pub struct BuiltScene<'a> {
    /// Root box (a flex column container at viewport dimensions).
    pub root: BoxNode,
    /// Lookup table — `components[id as usize]` is the component for `id`.
    /// `None` for synthetic boxes (the root scene wrapper).
    pub components: Vec<Option<&'a ChildComponent>>,
    /// Per-node animation delay accumulated from ancestor containers'
    /// `stagger` (indexed like `components`). Consumed by the paint
    /// dispatcher so internal animations shift by the same amount as the
    /// CSS overrides resolved at build time.
    pub stagger_delays: Vec<f64>,
    /// Per-node affine time remap accumulated from ancestor containers'
    /// `time_scale`/`time_offset`. Entry `i` is `(scale, shift)` where
    /// `t_local = scale * t_global + shift`. Default `(1.0, 0.0)` = identity.
    /// Indexed like `components` and `stagger_delays`.
    pub time_params: Vec<(f64, f64)>,
}

/// Build a box tree for a flat list of scene-level children at a given
/// viewport size.
///
/// The implicit scene root is a `display: flex; flex-direction: column;
/// width/height: 100%`; children flow vertically unless they specify
/// `position: { x, y }`, in which case they become `position: absolute`.
pub fn build_scene<'a>(children: &'a [ChildComponent], viewport: (f32, f32)) -> BuiltScene<'a> {
    build_scene_with_root(children, viewport, default_root_css(viewport))
}

/// Same as [`build_scene`] but lets the caller supply the root container's
/// `CssStyle`. Width/height are forced to the viewport regardless.
pub fn build_scene_with_root<'a>(
    children: &'a [ChildComponent],
    viewport: (f32, f32),
    root_css: CssStyle,
) -> BuiltScene<'a> {
    build_scene_from_refs(children.iter(), viewport, root_css, None)
}

/// Like [`build_scene_with_root`] but resolves animations at `time` (seconds)
/// for each node and merges the result into its `CssStyle`. Use this when
/// rendering an animated frame.
pub fn build_scene_at_time<'a>(
    children: &'a [ChildComponent],
    viewport: (f32, f32),
    root_css: CssStyle,
    anim: BuildAnimationCtx,
) -> BuiltScene<'a> {
    build_scene_from_refs(children.iter(), viewport, root_css, Some(anim))
}

/// Like [`build_scene`] but with an animation context. Convenience wrapper
/// that uses the default root CSS (full-viewport flex column).
pub fn build_scene_with_anim<'a>(
    children: &'a [ChildComponent],
    viewport: (f32, f32),
    anim: BuildAnimationCtx,
) -> BuiltScene<'a> {
    build_scene_from_refs(
        children.iter(),
        viewport,
        default_root_css(viewport),
        Some(anim),
    )
}

/// Same as [`build_scene_with_root`] but accepts an iterator over
/// `&ChildComponent` references. Useful when the caller has filtered or
/// re-ordered the scene's children and doesn't want to clone.
pub fn build_scene_from_refs<'a, I>(
    children: I,
    viewport: (f32, f32),
    mut root_css: CssStyle,
    anim: Option<BuildAnimationCtx>,
) -> BuiltScene<'a>
where
    I: IntoIterator<Item = &'a ChildComponent>,
{
    let mut components: Vec<Option<&'a ChildComponent>> = vec![None];
    let mut stagger_delays: Vec<f64> = vec![0.0];
    let mut time_params: Vec<(f64, f64)> = vec![(1.0, 0.0)]; // slot 0 = root (identity)
    let mut next_id: NodeId = 1;

    let mut child_boxes = Vec::new();
    for (i, c) in children.into_iter().enumerate() {
        child_boxes.extend(build_child(
            c,
            &mut components,
            &mut stagger_delays,
            &mut time_params,
            &mut next_id,
            anim,
            format!("/children/{i}"),
            0.0,
            (1.0, 0.0),
            &root_css,
        ));
    }

    // Force the root to viewport dimensions even if the caller didn't set them.
    root_css.width = Some(CSize::Length(CLP::Px(viewport.0)));
    root_css.height = Some(CSize::Length(CLP::Px(viewport.1)));

    let root = BoxNode {
        id: 0,
        kind: BoxKind::Container,
        css: root_css,
        children: child_boxes,
        intrinsic: None,
        source_path: None,
        window: None,
    };

    BuiltScene {
        root,
        components,
        stagger_delays,
        time_params,
    }
}

fn default_root_css(viewport: (f32, f32)) -> CssStyle {
    CssStyle {
        display: Some(rustmotion_core::css::style::Display::Flex),
        flex_direction: Some(rustmotion_core::css::style::FlexDirection::Column),
        width: Some(CSize::Length(CLP::Px(viewport.0))),
        height: Some(CSize::Length(CLP::Px(viewport.1))),
        ..Default::default()
    }
}

/// Detect motion_blur and trail configs from a merged effect list.
///
/// Returns `(motion_blur, trail)`. Only the first occurrence of each type is
/// used; duplicate effects of the same kind are ignored.
fn detect_ghost_effects(
    effects: &[AnimationEffect],
) -> (Option<MotionBlurConfig>, Option<TrailConfig>) {
    let mut mb: Option<MotionBlurConfig> = None;
    let mut tr: Option<TrailConfig> = None;
    for e in effects {
        match e {
            AnimationEffect::MotionBlur(c) if mb.is_none() => mb = Some(c.clone()),
            AnimationEffect::Trail(c) if tr.is_none() => tr = Some(c.clone()),
            _ => {}
        }
    }
    (mb, tr)
}

/// Build ghost `BoxNode`s for motion-blur or trail effects.
///
/// Returns a `Vec<BoxNode>` of ghosts to be prepended (painted underneath)
/// the principal node. Each ghost gets a fresh `NodeId` and its own slot in
/// `components`/`stagger_delays`, both pointing to the same `ChildComponent`
/// as the principal (v1 approximation: internal content uses frame time, not
/// ghost time).
///
/// # Ghost CSS
///
/// For **motion_blur**: the CSS is resolved at `t_ghost = t - i * (shutter/fps)/samples`
/// and opacity is `base_opacity / (samples + 1)`. The principal's opacity is
/// kept at its base value so the static (no-motion) case stays visually full.
///
/// For **trail**: the CSS is resolved at `t_ghost = t - i * spacing` and
/// opacity is `base_opacity * falloff^i`. The principal is unchanged.
///
/// Only one of `mb` / `tr` is used at a time; if both are present, motion_blur
/// takes priority.
#[allow(clippy::too_many_arguments)]
fn build_ghosts<'a>(
    child: &'a ChildComponent,
    components: &mut Vec<Option<&'a ChildComponent>>,
    stagger_delays: &mut Vec<f64>,
    time_params: &mut Vec<(f64, f64)>,
    next_id: &mut NodeId,
    actx: BuildAnimationCtx,
    stagger_delay: f64,
    time_remap: (f64, f64),
    effects: &[AnimationEffect],
    parent_css: &CssStyle,
) -> Vec<BoxNode> {
    let (mb, tr) = detect_ghost_effects(effects);

    // Choose the ghost generation strategy.
    enum Strategy {
        MotionBlur {
            samples: u32,
            shutter_window: f64,
        },
        Trail {
            copies: u32,
            spacing: f64,
            falloff: f32,
        },
    }
    let strategy = if let Some(mc) = mb {
        let samples = mc.samples.clamp(1, 16);
        if samples <= 1 {
            // Degenerate: no ghosts needed (ghost = principal position).
            return Vec::new();
        }
        let shutter_window = mc.shutter / actx.fps.max(1) as f64;
        Strategy::MotionBlur {
            samples,
            shutter_window,
        }
    } else if let Some(tc) = tr {
        let copies = tc.copies.clamp(1, 12);
        Strategy::Trail {
            copies,
            spacing: tc.spacing,
            falloff: tc.falloff,
        }
    } else {
        return Vec::new();
    };

    let base_css_for_ghost = |ghost_time: f64, ghost_opacity_scale: f32| -> CssStyle {
        // Start from the same base CSS as the principal.
        let mut css = component_css(&child.component);
        if let Some((x, y)) = child.absolute_position() {
            css.position = Some(Position::Absolute);
            css.left = Some(CLP::Px(x));
            css.top = Some(CLP::Px(y));
        }
        if let Some(z) = child.z_index {
            css.z_index = Some(z);
        }
        // Cascade: a ghost is the same component as the principal, painted
        // at a different sampled time, so it inherits from the same parent.
        rustmotion_core::css::cascade::inherit_from(parent_css, &mut css);
        // Apply timeline style states at the ghost time.
        if let Some(animatable) = child.component.as_animatable() {
            let steps = animatable.timeline_steps();
            if steps.iter().any(|s| s.style.is_some()) {
                let skip_opacity = css.transition.is_some();
                apply_style_states(&mut css, steps, ghost_time - stagger_delay, skip_opacity);
                // Same `border-radius`/`background` smoothing as the
                // principal path in `build_child`, sampled at `ghost_time`
                // so a motion-blur/trail ghost mid-transition matches what
                // the principal will look like at that same instant.
                let overrides = resolve_transition_css_overrides(
                    child.component.as_styled().style_config(),
                    steps,
                    ghost_time - stagger_delay,
                );
                if let Some(br) = overrides.border_radius {
                    css.border_radius = Some(br);
                }
                if let Some(bg) = overrides.background {
                    css.background = Some(bg);
                }
            }
        }
        // Resolve animation props at the *ghost* time.
        let ghost_actx = BuildAnimationCtx {
            time: ghost_time,
            scenario_time: actx.scenario_time,
            scene_duration: actx.scene_duration,
            fps: actx.fps,
        };
        if let Some(ghost_effects) = effective_effects(&child.component, stagger_delay) {
            let props = resolve_props_for_effects(
                &ghost_effects,
                ghost_actx.time,
                ghost_actx.scene_duration,
            );
            if props_has_paint_overrides(&props) {
                apply_animated_props(&mut css, &props);
            }
            apply_glow_effect(&mut css, &ghost_effects);
            carry_paint_pass_effects(&mut css, &ghost_effects);
        }
        // Scale opacity: multiply the base opacity by the ghost opacity factor.
        let base_opacity = css.opacity.unwrap_or(1.0);
        css.opacity = Some((base_opacity * ghost_opacity_scale).clamp(0.0, 1.0));
        css
    };

    let mut ghosts = Vec::new();

    match strategy {
        Strategy::MotionBlur {
            samples,
            shutter_window,
        } => {
            // Ghost opacity: 1 / (samples + 1) of base opacity.
            // Principal stays at full base opacity (handled in build_child).
            let ghost_opacity_scale = 1.0 / (samples + 1) as f32;
            for i in 1..=samples {
                let ghost_time = actx.time - (i as f64 * shutter_window / samples as f64);
                let ghost_css = base_css_for_ghost(ghost_time, ghost_opacity_scale);

                let ghost_id = *next_id;
                *next_id += 1;
                // Register a slot so the dispatcher can look up the component.
                components.push(Some(child));
                stagger_delays.push(stagger_delay);
                time_params.push(time_remap);

                ghosts.push(BoxNode {
                    id: ghost_id,
                    kind: BoxKind::Ghost(Arc::new(ghost_id)),
                    css: ghost_css,
                    children: Vec::new(), // v1: no child recursion in ghosts
                    intrinsic: None,      // v1: ghosts have no layout-measured content
                    source_path: None,
                    window: None,
                });
            }
        }
        Strategy::Trail {
            copies,
            spacing,
            falloff,
        } => {
            // Ghosts painted from oldest (most-trailing) to newest (closest to principal).
            // We build them in reverse order (copies → 1) so that index i=copies
            // is the oldest ghost (lowest opacity), and we then reverse to get the
            // correct under-to-over paint order.
            let mut trail_nodes = Vec::with_capacity(copies as usize);
            for i in 1..=copies {
                let ghost_time = actx.time - i as f64 * spacing;
                let ghost_opacity_scale = falloff.powi(i as i32);
                let ghost_css = base_css_for_ghost(ghost_time, ghost_opacity_scale);

                let ghost_id = *next_id;
                *next_id += 1;
                components.push(Some(child));
                stagger_delays.push(stagger_delay);
                time_params.push(time_remap);

                trail_nodes.push(BoxNode {
                    id: ghost_id,
                    kind: BoxKind::Ghost(Arc::new(ghost_id)),
                    css: ghost_css,
                    children: Vec::new(),
                    intrinsic: None,
                    source_path: None,
                    window: None,
                });
            }
            // Oldest ghost (most-trailing) painted first → prepend in reverse.
            trail_nodes.reverse();
            ghosts = trail_nodes;
        }
    }

    ghosts
}

/// Convert a single `ChildComponent` into one or more `BoxNode`s.
///
/// Returns a `Vec` whose elements are inserted in order into the parent's
/// `children`. The last element is the principal node; any preceding elements
/// are `BoxKind::Ghost` nodes inserted *before* (painted underneath) the
/// principal for motion-blur or trail effects.
///
/// `stagger_delay` is the animation delay accumulated from ancestor
/// containers' `stagger` fields.
/// `time_remap` is the accumulated affine time transform `(scale, shift)` where
/// `t_local = scale * t_global + shift`. Default is `(1.0, 0.0)` (identity).
#[allow(clippy::too_many_arguments)]
fn build_child<'a>(
    child: &'a ChildComponent,
    components: &mut Vec<Option<&'a ChildComponent>>,
    stagger_delays: &mut Vec<f64>,
    time_params: &mut Vec<(f64, f64)>,
    next_id: &mut NodeId,
    anim: Option<BuildAnimationCtx>,
    path: String,
    stagger_delay: f64,
    time_remap: (f64, f64),
    parent_css: &CssStyle,
) -> Vec<BoxNode> {
    // Compute the local animation context for this node — remapped by the
    // accumulated affine time transform from ancestor containers.
    // `t_local = scale * t_global + shift`
    let local_actx = anim.map(|a| {
        let (scale, shift) = time_remap;
        BuildAnimationCtx {
            time: a.time * scale + shift,
            // A container's `time_scale`/`time_offset` remaps the *animation*
            // clock of its subtree. The audio is not part of that subtree —
            // it plays at wall-clock speed regardless — so the scenario clock
            // passes through unremapped.
            scenario_time: a.scenario_time,
            scene_duration: a.scene_duration,
            fps: a.fps,
        }
    });

    // ── Ghost generation (motion_blur / trail) ───────────────────────────────
    // Must happen before allocating the principal's id so that ghost ids are
    // lower (earlier in the slot table). The principal's id is allocated below.
    let mut ghosts: Vec<BoxNode> = Vec::new();
    if let Some(actx) = local_actx {
        if let Some(effects) = effective_effects(&child.component, stagger_delay) {
            ghosts = build_ghosts(
                child,
                components,
                stagger_delays,
                time_params,
                next_id,
                actx,
                stagger_delay,
                time_remap,
                &effects,
                parent_css,
            );
        }
    }

    let id = *next_id;
    *next_id += 1;
    components.push(Some(child));
    stagger_delays.push(stagger_delay);
    time_params.push(time_remap);

    let mut css = component_css(&child.component);

    // Apply per-child position/z-index from the wrapper.
    if let Some((x, y)) = child.absolute_position() {
        css.position = Some(Position::Absolute);
        css.left = Some(CLP::Px(x));
        css.top = Some(CLP::Px(y));
    }
    if let Some(z) = child.z_index {
        css.z_index = Some(z);
    }

    // CSS cascade (round 4 audit, lot LAYOUT, constat 2): propagate
    // inheritable properties (color, font-*, text-align, white-space, ...)
    // from the parent's already-cascaded style into any of this node's own
    // unset properties — mirrors CSS's "specified value" resolution, which
    // happens before state/animation overrides compute the final value.
    // `crates/rustmotion-core/src/css/cascade.rs::inherit_from` existed but
    // nothing called it until this fix.
    rustmotion_core::css::cascade::inherit_from(parent_css, &mut css);

    // Timeline style states: merge every state whose (at + stagger) <= t
    // into the box CSS. Opacity is excluded when a `transition` smooths it
    // (the synthesized keyframes then own its whole history). States affect
    // box-model properties (layout, background, border, opacity, transform,
    // filter); painter-internal properties like text color flow through the
    // keyframes path below instead.
    if let Some(animatable) = child.component.as_animatable() {
        let steps = animatable.timeline_steps();
        if steps.iter().any(|s| s.style.is_some()) {
            let t = local_actx.map(|a| a.time).unwrap_or(0.0);
            let skip_opacity = css.transition.is_some();
            apply_style_states(&mut css, steps, t - stagger_delay, skip_opacity);
            // `border-radius`/`background` (solid colour, uniform absolute
            // px only — see `resolve_transition_css_overrides`'s doc
            // comment) smooth the same way opacity does above, but land
            // directly on `css` instead of through the generic effects
            // pipeline: no `AnimatedProperties` field for them is ever read
            // by a painter, so that pipeline is a dead end for these two.
            let overrides = resolve_transition_css_overrides(
                child.component.as_styled().style_config(),
                steps,
                t - stagger_delay,
            );
            if let Some(br) = overrides.border_radius {
                css.border_radius = Some(br);
            }
            if let Some(bg) = overrides.background {
                css.background = Some(bg);
            }
        }
    }

    // Resolve animations and apply transform/opacity/filter overrides on the
    // box's CSS. Paint-time properties (transform, opacity, filter,
    // perspective) plus the box size (`width`/`height`, which taffy needs so a
    // resize reflows its children instead of stretching pixels) flow into CSS
    // — internal animations like draw_progress or char_animation remain on the
    // `AnimatedProperties` legacy path.
    if let Some(actx) = local_actx {
        if let Some(effects) = effective_effects(&child.component, stagger_delay) {
            let props = resolve_props_for_effects(&effects, actx.time, actx.scene_duration);
            if props_has_paint_overrides(&props) {
                apply_animated_props(&mut css, &props);
            }
            apply_glow_effect(&mut css, &effects);
            carry_paint_pass_effects(&mut css, &effects);
        }
    }

    // ── Audio-reactive binding ────────────────────────────────────────────────
    // Reads the audio analysis cache and lerps the target CSS property between
    // min and max. Cache-miss → value = min (deterministic fallback).
    if let Some(ar) = css.audio_reactive.take() {
        use rustmotion_core::css::style::{AudioReactiveProperty, AudioSource, AudioSourceTag};
        use rustmotion_core::engine::renderer::audio_analysis::audio_analysis_cache;

        if let Some(actx) = local_actx {
            let cache = audio_analysis_cache();
            let analysis_opt = if let Some(ref src) = ar.track {
                cache.get(src).map(|r| r.clone())
            } else {
                cache.iter().next().map(|r| r.value().clone())
            };

            let raw = if let Some(analysis) = analysis_opt {
                match &ar.source {
                    AudioSource::Amplitude(AudioSourceTag::Amplitude) => {
                        analysis.amplitude_smoothed(actx.scenario_time, ar.smoothing_frames)
                    }
                    AudioSource::Band { band } => {
                        analysis.band_smoothed(actx.scenario_time, *band, ar.smoothing_frames)
                    }
                }
            } else {
                0.0 // cache empty → use min
            };

            let lerped = ar.min as f32 + raw * (ar.max - ar.min) as f32;

            match ar.property {
                AudioReactiveProperty::Opacity => {
                    let base = css.opacity.unwrap_or(1.0);
                    css.opacity = Some(base * lerped.clamp(0.0, 1.0));
                }
                AudioReactiveProperty::Scale => {
                    let s = lerped.max(0.0);
                    let tx = css.transform.get_or_insert_with(Vec::new);
                    tx.push(rustmotion_core::css::style::TransformFn::Scale { x: s, y: s });
                }
                AudioReactiveProperty::TranslateY => {
                    use rustmotion_core::css::units::LengthPercentage;
                    let tx = css.transform.get_or_insert_with(Vec::new);
                    tx.push(rustmotion_core::css::style::TransformFn::TranslateY {
                        y: LengthPercentage::Px(lerped),
                    });
                }
                AudioReactiveProperty::Rotation => {
                    let tx = css.transform.get_or_insert_with(Vec::new);
                    tx.push(rustmotion_core::css::style::TransformFn::Rotate { deg: lerped });
                }
            }
        }
    }

    // Visibility window (start_at/end_at) — enforced by the paint pass.
    // The stagger delay shifts the window too, so a hard-cut child appears
    // in step with its staggered siblings.
    // When there is an accumulated time remap, the window times (which are in
    // local/remapped time) must be converted back to global time so the paint
    // pass (which operates on global time) can apply them correctly.
    // If `t_local = scale * t_global + shift`, then `t_global = (t_local - shift) / scale`.
    let window = child.component.as_timed().and_then(|t| {
        let (start, end) = t.timing();
        (start.is_some() || end.is_some()).then_some({
            let (scale, shift) = time_remap;
            let to_global = |t_local: f64| -> f64 {
                if scale.abs() < 1e-10 {
                    t_local
                } else {
                    (t_local - shift) / scale
                }
            };
            rustmotion_core::engine::box_tree::PaintWindow {
                start: start.map(|s| to_global(s + stagger_delay)),
                end: end.map(|e| to_global(e + stagger_delay)),
            }
        })
    });

    let children_boxes = container_children(
        &child.component,
        components,
        stagger_delays,
        time_params,
        next_id,
        anim,
        &path,
        stagger_delay,
        time_remap,
        &css,
    );
    let intrinsic = component_intrinsic(&child.component);

    let principal = BoxNode {
        id,
        kind: BoxKind::Component(Arc::new(id)),
        css,
        children: children_boxes,
        intrinsic,
        source_path: Some(path),
        window,
    };

    // Ghosts prepended (painted underneath the principal).
    let mut result = ghosts;
    result.push(principal);
    result
}

/// The full effect list for a component at paint time: `style.animation`,
/// plus `timeline` steps shifted by their `at`, plus keyframes synthesized
/// from timeline style-state changes (`style.transition`), plus the
/// container-stagger delay applied to everything. Returns `None` when there
/// is nothing to resolve, `Some(Cow::Borrowed)` on the no-merge fast path.
pub fn effective_effects(
    component: &Component,
    extra_delay: f64,
) -> Option<std::borrow::Cow<'_, [rustmotion_core::schema::AnimationEffect]>> {
    let animatable = component.as_animatable()?;
    let effects = animatable.animation_effects();
    let steps = animatable.timeline_steps();
    // `color` keyframes drive AnimatedProperties.color, consumed by the
    // text-like painters only.
    let smooth_color = matches!(component, Component::Text(_) | Component::Counter(_));
    let synthesized =
        transition_keyframes(component.as_styled().style_config(), steps, smooth_color);
    if steps.is_empty() && synthesized.is_empty() && extra_delay == 0.0 {
        return (!effects.is_empty()).then_some(std::borrow::Cow::Borrowed(effects));
    }
    let mut merged = effects.to_vec();
    for step in steps {
        for effect in &step.animation {
            let mut e = effect.clone();
            e.shift_delay(step.at);
            merged.push(e);
        }
    }
    merged.extend(synthesized);
    if extra_delay != 0.0 {
        for e in &mut merged {
            e.shift_delay(extra_delay);
        }
    }
    (!merged.is_empty()).then_some(std::borrow::Cow::Owned(merged))
}

/// Merge every timeline style state whose `at <= t` into `css`, in `at`
/// order. Serialize-merge keeps this schema-complete; `null`s and the empty
/// `animation` array never erase existing values. `skip_opacity` leaves
/// opacity to the synthesized transition keyframes.
pub(crate) fn apply_style_states(
    css: &mut CssStyle,
    steps: &[rustmotion_core::schema::TimelineStep],
    t: f64,
    skip_opacity: bool,
) {
    let mut due: Vec<&rustmotion_core::schema::TimelineStep> = steps
        .iter()
        .filter(|s| s.style.is_some() && s.at <= t)
        .collect();
    if due.is_empty() {
        return;
    }
    due.sort_by(|a, b| a.at.total_cmp(&b.at));

    let Ok(serde_json::Value::Object(mut base)) = serde_json::to_value(&*css) else {
        return;
    };
    for step in due {
        let Some(style) = step.style.as_deref() else {
            continue;
        };
        let Ok(serde_json::Value::Object(state)) = serde_json::to_value(style) else {
            continue;
        };
        for (k, v) in state {
            if v.is_null() {
                continue;
            }
            if k == "animation" && v.as_array().is_some_and(|a| a.is_empty()) {
                continue;
            }
            if skip_opacity && k == "opacity" {
                continue;
            }
            base.insert(k, v);
        }
    }
    if let Ok(merged) = serde_json::from_value::<CssStyle>(serde_json::Value::Object(base)) {
        *css = merged;
    }
}

/// Synthesize `Keyframes` effects smoothing timeline style-state changes per
/// the component's `style.transition`. Supported: `opacity` (as ratios of the
/// base opacity — `apply_animated_props` multiplies) and, on text-like
/// components, `color` (absolute, via `AnimatedProperties.color`). Color
/// states without a transition still synthesize a near-instant ramp because
/// text painters only see color through this path.
pub(crate) fn transition_keyframes(
    base: &CssStyle,
    steps: &[rustmotion_core::schema::TimelineStep],
    smooth_color: bool,
) -> Vec<rustmotion_core::schema::AnimationEffect> {
    use rustmotion_core::schema::{
        Animation, AnimationEffect, Keyframe, KeyframeValue, KeyframesConfig,
    };

    let has_states = steps.iter().any(|s| s.style.is_some());
    if !has_states {
        return Vec::new();
    }
    let (duration, easing) = match base.transition.as_ref() {
        Some(tr) if tr.duration() > 0.0 => (tr.duration(), tr.easing()),
        // Color snaps still need the keyframes path (see doc above); a 1ms
        // ramp is visually a hard cut.
        _ => (0.001, rustmotion_core::schema::EasingType::Linear),
    };
    let smooth_opacity = base.transition.is_some();

    let mut sorted: Vec<&rustmotion_core::schema::TimelineStep> =
        steps.iter().filter(|s| s.style.is_some()).collect();
    sorted.sort_by(|a, b| a.at.total_cmp(&b.at));

    let base_opacity = base.opacity.unwrap_or(1.0) as f64;
    let mut opacity_kfs: Vec<Keyframe> = Vec::new();
    let mut color_kfs: Vec<Keyframe> = Vec::new();
    let mut prev_opacity = base_opacity;
    let mut prev_color = base.color.as_ref().map(|c| c.to_css_string());

    let kf_num = |time: f64, v: f64| Keyframe {
        time,
        value: KeyframeValue::Number(v),
        easing: None,
    };
    let kf_color = |time: f64, c: String| Keyframe {
        time,
        value: KeyframeValue::Color(c),
        easing: None,
    };
    // Keep keyframe times strictly ascending even when states overlap a
    // still-running transition (the resolver walks ordered segments).
    let push_pair = |kfs: &mut Vec<Keyframe>, at: f64, from: Keyframe, to: Keyframe| {
        let floor = kfs.last().map(|k| k.time + 1e-6).unwrap_or(f64::MIN);
        let start = at.max(floor);
        let mut from = from;
        let mut to = to;
        from.time = start;
        to.time = to.time.max(start + 1e-6);
        kfs.push(from);
        kfs.push(to);
    };

    for step in sorted {
        let style = step.style.as_deref().unwrap();
        if smooth_opacity && base_opacity > 1e-6 {
            if let Some(o) = style.opacity {
                let target = o as f64;
                if (target - prev_opacity).abs() > 1e-6 {
                    push_pair(
                        &mut opacity_kfs,
                        step.at,
                        kf_num(step.at, prev_opacity / base_opacity),
                        kf_num(step.at + duration, target / base_opacity),
                    );
                    prev_opacity = target;
                }
            }
        }
        if smooth_color {
            if let Some(c) = style.color.as_ref() {
                let target = c.to_css_string();
                if prev_color.as_ref() != Some(&target) {
                    if let Some(from) = prev_color.clone() {
                        push_pair(
                            &mut color_kfs,
                            step.at,
                            kf_color(step.at, from),
                            kf_color(step.at + duration, target.clone()),
                        );
                    }
                    prev_color = Some(target);
                }
            }
        }
    }

    let mut out = Vec::new();
    let mut push_effect = |property: &str, keyframes: Vec<Keyframe>| {
        if keyframes.is_empty() {
            return;
        }
        out.push(AnimationEffect::Keyframes(KeyframesConfig {
            keyframes: vec![Animation {
                property: property.to_string(),
                keyframes,
                easing: easing.clone(),
                spring: None,
            }],
            delay: 0.0,
            duration: 0.0,
            repeat: false,
        }));
    };
    push_effect("opacity", opacity_kfs);
    push_effect("color", color_kfs);
    out
}

/// Resolved `style.transition` smoothing for `border-radius`/`background`,
/// ready to be written straight onto a `CssStyle`.
pub(crate) struct TransitionCssOverrides {
    pub border_radius: Option<rustmotion_core::css::style::BorderRadius>,
    pub background: Option<rustmotion_core::css::style::Background>,
}

/// CSS-native smoothing for `border-radius` (uniform, absolute-px only) and
/// `background` (solid colour only) timeline style-state changes.
///
/// Unlike `opacity`/`color` in `transition_keyframes` above, neither
/// property has anywhere to land in `AnimatedProperties` that any painter or
/// the CSS bridge (`css/animation.rs::apply_animated_props`) actually reads
/// (see `KNOWN_ANIMATABLE_PROPERTIES`'s doc comment in `animator.rs`) —
/// every painter reads `css.border_radius`/`css.background` straight off
/// the node's own `CssStyle` (`paint_pass.rs`, frozen, already does this for
/// the static case). So instead of synthesizing an `AnimationEffect` for the
/// generic effects pipeline (a dead end for these two), this resolves the
/// interpolated value directly and returns it for the caller to write onto
/// the box's `CssStyle` by hand — reusing `animator::resolve_keyframe_track`
/// for the actual segment/easing/spring math rather than reinventing it.
///
/// Gated on `style.transition` being set, mirroring `opacity`'s gate above
/// (not `color`'s forced near-instant ramp — nothing downstream *requires*
/// these two to smooth the way text painters require `color` to). Absent an
/// explicit `style.transition`, this returns an all-`None` result, so every
/// existing scenario without one renders byte-identical to before this
/// workstream.
///
/// **"Unités mixtes" decision** (see workstream report): resolves only when
/// *both* the origin and the target value are the exact shape
/// `BorderRadius::absolute_px`/`Background::solid_hex` can resolve without a
/// `LengthContext` — uniform absolute px, solid colour. Anything else
/// (per-corner radii, `%`/`em`/`rem`/`vw`/`vh`, gradients, image layers) is
/// refused rather than guessed: that property just falls back to
/// `apply_style_states`'s existing snap. `validate_schema.rs` calls the
/// exact same two predicates so the diagnostic and the runtime can never
/// disagree about what's interpolable.
pub(crate) fn resolve_transition_css_overrides(
    base: &CssStyle,
    steps: &[rustmotion_core::schema::TimelineStep],
    t: f64,
) -> TransitionCssOverrides {
    use rustmotion_core::css::style::{Background, BorderRadius, Color};
    use rustmotion_core::css::units::LengthPercentage as CssLP;
    use rustmotion_core::engine::animator::resolve_keyframe_track;
    use rustmotion_core::schema::{Animation, Keyframe, KeyframeValue};

    let mut out = TransitionCssOverrides {
        border_radius: None,
        background: None,
    };
    let Some(tr) = base.transition.as_ref() else {
        return out;
    };
    if tr.duration() <= 0.0 {
        return out;
    }
    let duration = tr.duration();
    let easing = tr.easing();

    let mut sorted: Vec<&rustmotion_core::schema::TimelineStep> =
        steps.iter().filter(|s| s.style.is_some()).collect();
    sorted.sort_by(|a, b| a.at.total_cmp(&b.at));

    let mut prev_radius = base
        .border_radius
        .as_ref()
        .and_then(BorderRadius::absolute_px);
    let mut prev_bg = base.background.as_ref().and_then(Background::solid_hex);
    let mut radius_kfs: Vec<Keyframe> = Vec::new();
    let mut bg_kfs: Vec<Keyframe> = Vec::new();

    // Same ascending-time bookkeeping as `transition_keyframes`'s
    // `push_pair` above (kept local — a shared closure can't easily borrow
    // two different `Vec`s across both loops below without upsetting the
    // borrow checker for no real benefit at this size).
    let push_pair = |kfs: &mut Vec<Keyframe>, at: f64, from: Keyframe, to: Keyframe| {
        let floor = kfs.last().map(|k| k.time + 1e-6).unwrap_or(f64::MIN);
        let start = at.max(floor);
        let mut from = from;
        let mut to = to;
        from.time = start;
        to.time = to.time.max(start + 1e-6);
        kfs.push(from);
        kfs.push(to);
    };

    for step in sorted {
        let style = step.style.as_deref().unwrap();
        if let Some(br) = style.border_radius.as_ref() {
            match br.absolute_px() {
                Some(target) => {
                    if prev_radius != Some(target) {
                        if let Some(from) = prev_radius {
                            push_pair(
                                &mut radius_kfs,
                                step.at,
                                Keyframe {
                                    time: step.at,
                                    value: KeyframeValue::Number(from as f64),
                                    easing: None,
                                },
                                Keyframe {
                                    time: step.at + duration,
                                    value: KeyframeValue::Number(target as f64),
                                    easing: None,
                                },
                            );
                        }
                        prev_radius = Some(target);
                    }
                }
                // Unresolvable shape (per-corner, %/em/rem/vw/vh) — lose the
                // interpolation origin for *this* transition only;
                // `validate_schema.rs` diagnoses this exact step, and a
                // later resolvable value simply resumes smoothing from
                // itself onward (see the doc comment above).
                None => prev_radius = None,
            }
        }
        if let Some(bg) = style.background.as_ref() {
            match bg.solid_hex() {
                Some(target) => {
                    if prev_bg.as_deref() != Some(target.as_str()) {
                        if let Some(from) = prev_bg.clone() {
                            push_pair(
                                &mut bg_kfs,
                                step.at,
                                Keyframe {
                                    time: step.at,
                                    value: KeyframeValue::Color(from),
                                    easing: None,
                                },
                                Keyframe {
                                    time: step.at + duration,
                                    value: KeyframeValue::Color(target.clone()),
                                    easing: None,
                                },
                            );
                        }
                        prev_bg = Some(target);
                    }
                }
                None => prev_bg = None,
            }
        }
    }

    if !radius_kfs.is_empty() {
        let anim = Animation {
            property: "border_radius".to_string(),
            keyframes: radius_kfs,
            easing: easing.clone(),
            spring: None,
        };
        if let KeyframeValue::Number(v) = resolve_keyframe_track(&anim, t) {
            out.border_radius = Some(BorderRadius::Uniform(CssLP::Px(v as f32)));
        }
    }
    if !bg_kfs.is_empty() {
        let anim = Animation {
            property: "background".to_string(),
            keyframes: bg_kfs,
            easing,
            spring: None,
        };
        if let KeyframeValue::Color(c) = resolve_keyframe_track(&anim, t) {
            out.background = Some(Background::Color(Color::String(c)));
        }
    }
    out
}

/// Quick gate: does this resolved `AnimatedProperties` carry any property
/// that we know how to translate to CSS? Avoids allocating a transform Vec
/// when there's nothing to apply.
fn props_has_paint_overrides(p: &AnimatedProperties) -> bool {
    p.translate_x != 0.0
        || p.translate_y != 0.0
        || (p.scale_x - 1.0).abs() > 1e-4
        || (p.scale_y - 1.0).abs() > 1e-4
        || p.rotation.abs() > 1e-3
        || p.rotate_x.abs() > 1e-3
        || p.rotate_y.abs() > 1e-3
        || (p.opacity - 1.0).abs() > 1e-4
        || p.blur > 0.0
        || (p.glow_radius > 0.0 && p.glow_intensity > 0.0)
        || p.perspective > 0.0
        // An animated box size is a *layout* override rather than a paint one,
        // but it travels through the same bridge, so the gate has to let it
        // through or `apply_animated_props` never runs for a scenario whose
        // only animated property is `width`/`height` (the card-resize case).
        // -1.0 is the animator's "never animated" sentinel.
        || p.width >= 0.0
        || p.height >= 0.0
}

/// Hand the effects that the *paint pass* resolves for itself down to the box
/// node, in their delay-shifted form.
///
/// Everything else on this path is resolved here and lands on `css` as a
/// finished value. `shimmer` cannot be: it composites against the pixels the
/// node paints, which do not exist until the paint pass has run. So the paint
/// pass reads it off `css.animation` — and it has to read the *shifted* copy
/// (container stagger, `timeline` step `at`) rather than the author's raw
/// `style.animation`, or a shimmer inside a staggered list would sweep in step
/// with the list's first item instead of its own.
fn carry_paint_pass_effects(
    css: &mut CssStyle,
    effects: &[rustmotion_core::schema::AnimationEffect],
) {
    use rustmotion_core::schema::AnimationEffect;
    if effects
        .iter()
        .any(|e| matches!(e, AnimationEffect::Shimmer(_)))
    {
        css.animation = effects
            .iter()
            .filter(|e| matches!(e, AnimationEffect::Shimmer(_)))
            .cloned()
            .collect();
    }
}

/// M3: apply the static `glow` animation effect (a coloured halo — not
/// time-varying, see `AnimationEffect::shift_delay`'s doc comment) as a CSS
/// `filter: drop-shadow(...)`.
///
/// This is deliberately *not* routed through `resolve_props_for_effects` /
/// `AnimatedProperties::glow_radius`+`glow_intensity` even though those
/// fields exist: `AnimatedProperties`'s public shape is frozen for this
/// workstream (can't add a `glow_color` field), and the existing bridge that
/// *does* consume those two fields — `css::animation::apply_animated_props`,
/// in a file outside this workstream's scope — hardcodes `color: None` on
/// the `DropShadow` it builds, which resolves to black. Piping the `glow`
/// effect through that path would either still render black, or — if we did
/// find a way to set the color fields too — double up into two stacked
/// drop-shadows (one colourless from the frozen bridge, one coloured from
/// here). Building the filter directly here, from the raw `GlowConfig`
/// (found via `animator::find_glow_effect`), keeps it a single, correctly
/// coloured shadow and needs no change to any frozen file.
///
/// The pre-existing `glow_radius`/`glow_intensity` numeric-property path
/// (animating those as arbitrary `keyframes` targets, unrelated to this
/// named `glow` effect) is untouched and still produces a colourless halo —
/// that is `css::animation.rs`'s DropShadow-with-`color:None` bug, out of
/// this workstream's file ownership.
fn apply_glow_effect(css: &mut CssStyle, effects: &[rustmotion_core::schema::AnimationEffect]) {
    use rustmotion_core::css::style::{Color, FilterFn};
    use rustmotion_core::css::units::Length;
    use rustmotion_core::engine::animator::find_glow_effect;

    let Some(glow) = find_glow_effect(effects) else {
        return;
    };

    let (r, g, b, a) = rustmotion_core::engine::renderer::parse_css_color(&glow.color)
        .unwrap_or((255, 255, 255, 255));
    let alpha = ((a as f32 / 255.0) * glow.intensity.max(0.0)).clamp(0.0, 1.0);
    let radius = glow.radius.max(0.0);
    if radius <= 0.0 || alpha <= 0.0 {
        return;
    }

    let shadow = FilterFn::DropShadow {
        offset_x: Length::Px(0.0),
        offset_y: Length::Px(0.0),
        blur: Some(Length::Px(radius)),
        color: Some(Color::Rgba { r, g, b, a: alpha }),
    };
    css.filter.get_or_insert_with(Vec::new).push(shadow);
}

/// Build an [`IntrinsicMeasure`] for components whose box size depends on
/// their content (text, codeblock, terminal, etc.). Returns `None` for
/// components with explicit dimensions or pure containers.
fn component_intrinsic(
    component: &Component,
) -> Option<Arc<dyn rustmotion_core::engine::box_tree::IntrinsicMeasure>> {
    use Component::*;
    match component {
        Text(t) => Some(Arc::new(crate::intrinsic::TextIntrinsic::from_text(t))),
        GradientText(t) => Some(Arc::new(
            crate::intrinsic::GradientTextIntrinsic::from_gradient_text(t),
        )),
        Caption(c) => Some(Arc::new(crate::intrinsic::CaptionIntrinsic::from_caption(
            c,
        ))),
        Kbd(k) => Some(Arc::new(crate::intrinsic::KbdIntrinsic::from_kbd(k))),
        Counter(c) => Some(Arc::new(crate::intrinsic::CounterIntrinsic::from_counter(
            c,
        ))),
        NumberWheel(w) => Some(Arc::new(
            crate::intrinsic::NumberWheelIntrinsic::from_number_wheel(w),
        )),
        Badge(b) => Some(Arc::new(crate::intrinsic::BadgeIntrinsic::from_badge(b))),
        Terminal(t) => Some(Arc::new(
            crate::intrinsic::TerminalIntrinsic::from_terminal(t),
        )),
        Table(t) => Some(Arc::new(crate::intrinsic::TableIntrinsic::from_table(t))),
        Codeblock(c) => Some(Arc::new(
            crate::intrinsic::CodeblockIntrinsic::from_codeblock(c),
        )),
        // M2: rich_text had no intrinsic measurer at all, so it laid out
        // 0×0 and rendered nothing unless the author guessed an explicit
        // width/height.
        RichText(rt) => Some(Arc::new(
            crate::intrinsic::RichTextIntrinsic::from_rich_text(rt),
        )),
        _ => None,
    }
}

/// If the component is a container, recurse into its children. Otherwise
/// return an empty Vec. A container's `stagger` adds `index * stagger`
/// to each child's inherited animation delay (cumulative across nesting).
/// `time_remap` is the accumulated affine time transform `(scale, shift)` for
/// this container node; the container's own `time_scale`/`time_offset` are
/// composed in to produce the remap for children.
#[allow(clippy::too_many_arguments)]
fn container_children<'a>(
    component: &'a Component,
    components: &mut Vec<Option<&'a ChildComponent>>,
    stagger_delays: &mut Vec<f64>,
    time_params: &mut Vec<(f64, f64)>,
    next_id: &mut NodeId,
    anim: Option<BuildAnimationCtx>,
    parent_path: &str,
    inherited_delay: f64,
    time_remap: (f64, f64),
    parent_css: &CssStyle,
) -> Vec<BoxNode> {
    let (children, stagger, child_scale, child_offset): (&[ChildComponent], Option<f32>, f64, f64) =
        match component {
            Component::Card(c) => (
                &c.children,
                c.stagger,
                c.time_scale.unwrap_or(1.0),
                c.time_offset.unwrap_or(0.0),
            ),
            Component::Flex(c) => (
                &c.children,
                c.stagger,
                c.time_scale.unwrap_or(1.0),
                c.time_offset.unwrap_or(0.0),
            ),
            Component::Grid(c) => (
                &c.children,
                c.stagger,
                c.time_scale.unwrap_or(1.0),
                c.time_offset.unwrap_or(0.0),
            ),
            Component::Container(c) => (
                &c.children,
                c.stagger,
                c.time_scale.unwrap_or(1.0),
                c.time_offset.unwrap_or(0.0),
            ),
            Component::Positioned(c) => (
                &c.children,
                None,
                c.time_scale.unwrap_or(1.0),
                c.time_offset.unwrap_or(0.0),
            ),
            _ => return Vec::new(),
        };

    // Clamp scale defensively to avoid division-by-zero downstream.
    let child_scale = child_scale.max(1e-6);

    // Compose the container's time remap with the inherited (accumulated) remap.
    // Accumulated remap: `t_parent = scale_acc * t_global + shift_acc`
    // Container formula: `t_child = (t_parent - child_offset) * child_scale`
    //   = child_scale * (scale_acc * t_global + shift_acc) - child_scale * child_offset
    //   = (child_scale * scale_acc) * t_global + child_scale * (shift_acc - child_offset)
    let (scale_acc, shift_acc) = time_remap;
    let new_scale = child_scale * scale_acc;
    let new_shift = child_scale * (shift_acc - child_offset);
    let child_remap = (new_scale, new_shift);

    // `anim` stays GLOBAL all the way down the recursion — each `build_child`
    // derives its node-local time from the accumulated `child_remap`. Passing
    // a pre-remapped ctx here would double-apply the transform.
    let step = stagger.unwrap_or(0.0) as f64;
    let mut result = Vec::new();
    for (j, c) in children.iter().enumerate() {
        result.extend(build_child(
            c,
            components,
            stagger_delays,
            time_params,
            next_id,
            anim,
            format!("{parent_path}/children/{j}"),
            inherited_delay + j as f64 * step,
            child_remap,
            parent_css,
        ));
    }
    result
}

/// Pull the component's `CssStyle`, augmented with intrinsic `width`/`height`
/// for components that carry a fixed size.
fn component_css(component: &Component) -> CssStyle {
    let mut css = component_style(component).clone();
    apply_default_display(component, &mut css);
    apply_intrinsic_overrides(component, &mut css);
    css
}

/// Set `display` from the component kind when the user didn't specify one.
/// `card` / `flex` → `flex`, `grid` → `grid`. The taffy bridge defaults to
/// `block` otherwise, which would silently ignore `flex-direction` & friends.
fn apply_default_display(component: &Component, css: &mut CssStyle) {
    use rustmotion_core::css::style::Display;
    if css.display.is_some() {
        return;
    }
    css.display = match component {
        Component::Card(_) | Component::Flex(_) | Component::Container(_) => Some(Display::Flex),
        Component::Grid(_) => Some(Display::Grid),
        _ => return,
    };
}

/// Measure a single line of text with the exact same Skia font metrics the
/// affected painters (`callout`, `tooltip`, `pill_nav`, `stepper`) already use
/// to draw it (`measure_text_with_fallback`), so a size computed here matches
/// the pixels those painters actually paint instead of guessing at an average
/// character width. Returns `0.0` if the font family can't be resolved —
/// matches those painters' own silent-return-on-font-load-failure behaviour.
fn measure_text_line_width(text: &str, font_size: f32, family: &str, bold: bool) -> f32 {
    use rustmotion_core::engine::renderer::{
        emoji_typeface, measure_text_with_fallback, typeface_with_fallback,
    };
    let style = if bold {
        skia_safe::FontStyle::bold()
    } else {
        skia_safe::FontStyle::normal()
    };
    let Ok(typeface) = typeface_with_fallback(family, style) else {
        return 0.0;
    };
    let font = skia_safe::Font::from_typeface(typeface, font_size);
    let emoji_font = emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, font_size));
    measure_text_with_fallback(text, &font, &emoji_font, 0.0)
}

/// Apply per-component CSS overrides for things that the legacy
/// `Widget::measure` derived from constraints (e.g. divider stretching to its
/// parent, line bounding box from its endpoints).
fn apply_intrinsic_overrides(component: &Component, css: &mut CssStyle) {
    use Component::*;
    match component {
        Text(t) => {
            // M1: `white-space: nowrap|pre` must style the *content*, not
            // silently resize the *box*. Without this, CSS's "automatic
            // minimum size" (min-width: auto + the default overflow:
            // visible) lets a nowrap text's own auto-width box grow to its
            // full natural width inside a flex container — which can push
            // or resize flex siblings the author never touched, a
            // surprising failure mode for a tool whose whole model is
            // authors declaring explicit positions/sizes. Forcing
            // `min-width: 0` (only when the author hasn't set their own)
            // keeps the box within whatever space its container gives it;
            // the painter (`text.rs`) still draws the full unwrapped line
            // regardless of that box width, so the text visibly bleeds past
            // it exactly as `white-space: nowrap` should — it's the box
            // that stays put, not the content.
            let nowrap = matches!(
                t.style.white_space,
                Some(
                    rustmotion_core::css::style::WhiteSpace::Nowrap
                        | rustmotion_core::css::style::WhiteSpace::Pre
                )
            );
            if nowrap && css.min_width.is_none() {
                css.min_width = Some(CSize::Length(CLP::Px(0.0)));
            }
        }
        // M1 follow-up: same reasoning as the `Text` arm above — now that
        // `gradient_text.rs` and `caption.rs` word-wrap and respect
        // `white-space: nowrap|pre` too (see `intrinsic.rs`'s
        // `GradientTextIntrinsic`/`CaptionIntrinsic`), their auto-width
        // boxes can hit the same CSS automatic-minimum-size growth when
        // nowrap/pre is set with no explicit width.
        GradientText(t) => {
            let nowrap = matches!(
                t.style.white_space,
                Some(
                    rustmotion_core::css::style::WhiteSpace::Nowrap
                        | rustmotion_core::css::style::WhiteSpace::Pre
                )
            );
            if nowrap && css.min_width.is_none() {
                css.min_width = Some(CSize::Length(CLP::Px(0.0)));
            }
        }
        Caption(c) => {
            let nowrap = matches!(
                c.style.white_space,
                Some(
                    rustmotion_core::css::style::WhiteSpace::Nowrap
                        | rustmotion_core::css::style::WhiteSpace::Pre
                )
            );
            if nowrap && css.min_width.is_none() {
                css.min_width = Some(CSize::Length(CLP::Px(0.0)));
            }
        }
        Divider(d) => match d.direction {
            DividerDirection::Horizontal => {
                // Stretch horizontally in flex row/column parents (cross-axis
                // for column = horizontal). Width stays auto.
                if css.height.is_none() {
                    css.height = Some(CSize::Length(CLP::Px(d.thickness)));
                }
                if css.width.is_none() {
                    css.width = match d.length {
                        Some(l) => Some(CSize::Length(CLP::Px(l))),
                        None => Some(CSize::Length(CLP::String("100%".into()))),
                    };
                }
                if css.align_self.is_none() {
                    css.align_self = Some(AlignSelf::Stretch);
                }
            }
            DividerDirection::Vertical => {
                if css.width.is_none() {
                    css.width = Some(CSize::Length(CLP::Px(d.thickness)));
                }
                if css.height.is_none() {
                    css.height = match d.length {
                        Some(l) => Some(CSize::Length(CLP::Px(l))),
                        None => Some(CSize::Length(CLP::String("100%".into()))),
                    };
                }
            }
        },
        Line(l) => {
            // Line draws inside its bounding box at (x1,y1)→(x2,y2). Use the
            // bounding box as the intrinsic size so taffy reserves enough room.
            let w = (l.x2 - l.x1).abs().max(1.0);
            let h = (l.y2 - l.y1).abs().max(1.0);
            if css.width.is_none() {
                css.width = Some(CSize::Length(CLP::Px(w)));
            }
            if css.height.is_none() {
                css.height = Some(CSize::Length(CLP::Px(h)));
            }
        }
        Arrow(a) => {
            // Endpoint bounding box + padding for the arrowhead/curve overshoot.
            let pad = ARROW_BBOX_PADDING + a.arrow_size.max(0.0);
            let w = (a.x2 - a.x1).abs().max(1.0) + pad;
            let h = (a.y2 - a.y1).abs().max(1.0) + pad;
            if css.width.is_none() {
                css.width = Some(CSize::Length(CLP::Px(w)));
            }
            if css.height.is_none() {
                css.height = Some(CSize::Length(CLP::Px(h)));
            }
        }
        Connector(c) => {
            let pad = ARROW_BBOX_PADDING + c.arrow_size.max(0.0);
            let w = (c.to.x - c.from.x).abs().max(1.0) + pad;
            let h = (c.to.y - c.from.y).abs().max(1.0) + pad;
            if css.width.is_none() {
                css.width = Some(CSize::Length(CLP::Px(w)));
            }
            if css.height.is_none() {
                css.height = Some(CSize::Length(CLP::Px(h)));
            }
        }
        Cursor(cur) => {
            // Fixed-size pointer; legacy measure returns (width, height).
            if css.width.is_none() {
                css.width = Some(CSize::Length(CLP::Px(cur.width)));
            }
            if css.height.is_none() {
                css.height = Some(CSize::Length(CLP::Px(cur.height)));
            }
        }
        SuccessCheck(c) => {
            // The halo is the box: the entrance scales *within* it (0.72→1),
            // so the mark never needs more room than its own diameter.
            apply_default_size(css, c.size, c.size);
        }
        Pointer(p) => {
            // The box is the arrow glyph, not the area it travels over: the
            // waypoints translate the glyph away from this box, and the
            // geometry checker exempts `pointer` for exactly that reason.
            // Sizing the box to the travel instead would make the pointer
            // shove its flex siblings around.
            if css.width.is_none() {
                css.width = Some(CSize::Length(CLP::Px(p.size * 0.6)));
            }
            if css.height.is_none() {
                css.height = Some(CSize::Length(CLP::Px(p.size)));
            }
        }
        Particle(_) => {
            // Particles fill their parent (legacy returned the max constraints).
            if css.width.is_none() {
                css.width = Some(CSize::Length(CLP::String("100%".into())));
            }
            if css.height.is_none() {
                css.height = Some(CSize::Length(CLP::String("100%".into())));
            }
        }
        Switch(c) => {
            if css.width.is_none() {
                css.width = Some(CSize::Length(CLP::Px(c.width)));
            }
            if css.height.is_none() {
                css.height = Some(CSize::Length(CLP::Px(c.height)));
            }
        }
        Slider(c) => {
            if css.width.is_none() {
                css.width = Some(CSize::Length(CLP::Px(c.width)));
            }
            if css.height.is_none() {
                css.height = Some(CSize::Length(CLP::Px(c.height)));
            }
        }
        Progress(c) => {
            if css.width.is_none() {
                css.width = Some(CSize::Length(CLP::Px(c.width)));
            }
            if css.height.is_none() {
                css.height = Some(CSize::Length(CLP::Px(c.height)));
            }
        }
        List(c) => {
            if css.width.is_none() {
                css.width = Some(CSize::Length(CLP::Px(c.width)));
            }
            if css.height.is_none() {
                let font_size = c.style.font_size_px_or(16.0);
                let line_height = font_size * 1.3;
                let n = c.items.len() as f32;
                let h = n * line_height + (n - 1.0).max(0.0) * c.gap;
                css.height = Some(CSize::Length(CLP::Px(h)));
            }
        }
        Timeline(c) => {
            if css.width.is_none() {
                css.width = Some(CSize::Length(CLP::Px(c.width)));
            }
            if css.height.is_none() {
                let r = c.node_radius;
                let h = match c.direction {
                    TimelineDirection::Horizontal => r * 2.0 + c.font_size * 2.5 + 24.0,
                    TimelineDirection::Vertical => {
                        let n = c.steps.len().max(1) as f32;
                        n * (r * 2.0 + 64.0)
                    }
                };
                css.height = Some(CSize::Length(CLP::Px(h)));
            }
        }
        Notification(c) => {
            if css.width.is_none() {
                css.width = Some(CSize::Length(CLP::Px(c.width)));
            }
            if css.height.is_none() {
                let h = if c.message.is_some() { 96.0 } else { 64.0 };
                css.height = Some(CSize::Length(CLP::Px(h)));
            }
        }
        Rating(c) => {
            if css.width.is_none() {
                let count = c.max as f32;
                let w = count * c.size + (count - 1.0).max(0.0) * c.gap;
                css.width = Some(CSize::Length(CLP::Px(w)));
            }
            if css.height.is_none() {
                css.height = Some(CSize::Length(CLP::Px(c.size)));
            }
        }
        Avatar(c) => {
            if css.width.is_none() {
                css.width = Some(CSize::Length(CLP::Px(c.size)));
            }
            if css.height.is_none() {
                css.height = Some(CSize::Length(CLP::Px(c.size)));
            }
        }
        AvatarGroup(c) => {
            if css.width.is_none() {
                let visible = c.visible_count() as f32;
                let extra = if c.overflow_count() > 0 { 1.0 } else { 0.0 };
                let total = visible + extra;
                let step = (c.size - c.overlap).max(0.0);
                let w = if total <= 0.0 {
                    0.0
                } else {
                    c.size + (total - 1.0) * step
                };
                css.width = Some(CSize::Length(CLP::Px(w)));
            }
            if css.height.is_none() {
                css.height = Some(CSize::Length(CLP::Px(c.size)));
            }
        }
        QrCode(c) => {
            if css.width.is_none() {
                css.width = Some(CSize::Length(CLP::Px(c.size)));
            }
            if css.height.is_none() {
                css.height = Some(CSize::Length(CLP::Px(c.size)));
            }
        }
        Countdown(c) if (css.width.is_none() || css.height.is_none()) => {
            let visible = [c.show_hours, c.show_minutes, c.show_seconds]
                .iter()
                .filter(|v| **v)
                .count() as f32;
            let box_w = c.digit_size * 0.75;
            let box_h = c.digit_size * 1.2;
            let w = (visible * 2.0 * box_w) + ((visible - 1.0).max(0.0) * c.gap);
            if css.width.is_none() {
                css.width = Some(CSize::Length(CLP::Px(w)));
            }
            if css.height.is_none() {
                css.height = Some(CSize::Length(CLP::Px(box_h)));
            }
        }
        AudioSpectrum(_) => {
            if css.width.is_none() {
                css.width = Some(CSize::Length(CLP::Px(400.0)));
            }
            if css.height.is_none() {
                css.height = Some(CSize::Length(CLP::Px(120.0)));
            }
        }
        Waveform(_) => {
            if css.width.is_none() {
                css.width = Some(CSize::Length(CLP::Px(400.0)));
            }
            if css.height.is_none() {
                css.height = Some(CSize::Length(CLP::Px(80.0)));
            }
        }

        // ── Round 4 audit, lot LAYOUT, constat 4: the 23-components block
        // below (`Callout` through `Lottie`) now routes every default size
        // through `apply_default_size`, which honours an explicit
        // `aspect-ratio` (see its own doc comment) instead of the two
        // guards below reaching separate, aspect-ratio-blind defaults —
        // `width: 400` + `aspect-ratio: 16/9` used to still get the
        // component's unrelated hardcoded default height (e.g. `shape`'s
        // 80px) instead of the 225px the ratio implies.
        // ── #126 / W3: the 23 components with no size source ─────────────
        //
        // A card's default flex column gives every child its width via
        // `align-items: stretch`, but height stays at the CSS auto-height
        // default (0 for a leaf with no intrinsic measurer) — these 23 paint
        // nothing. Like the arms above, every default here fires only when
        // the author hasn't set the corresponding `style` property, so a
        // component that already declares `width`/`height` keeps rendering
        // exactly as before. Both width *and* height are always set (not
        // just height) so the same defaults also work in a flex *row* (e.g.
        // `stat` cards side by side), where width — not height — is the one
        // that would otherwise collapse to 0.
        //
        // Text-bearing "bubble" components (callout/tooltip) and label-flow
        // components (pill_nav/stepper) are measured with the exact same
        // Skia metrics their own painters use (`measure_text_line_width`),
        // so the box matches the ink. Everything else uses either a formula
        // derived from the component's own fields (heatmap/skeleton/gauge/
        // marquee) or a fixed size justified by a documented convention
        // already established in this project's own skill docs
        // (`.claude/skills/rustmotion/rules/*.md`) or its own example
        // scenarios (`examples/*.json`) — never a number picked by feel.
        Callout(t) => {
            // Mirrors callout.rs's own `paint()`: 12px text padding, a
            // `font_size * 1.4` line height, and the arrow eating into
            // whichever axis it points along (width for Left/Right, height
            // for Top/Bottom/default). Sized to a single line — since the
            // box is fit exactly to the unwrapped text width, the painter's
            // own `wrap_text(text, font, Some(text_area_w))` never has a
            // reason to wrap, so painted output matches this box exactly.
            let font_size = t.style.font_size_px_or(16.0);
            let family = t.style.font_family_or("Inter");
            let text_w = measure_text_line_width(&t.text, font_size, family, false);
            let h_pad = 12.0; // callout.rs's own `let padding = 12.0;`
            let v_pad = 16.0; // breathing room around the line, same order of magnitude as h_pad
            let line_h = font_size * 1.4; // callout.rs's own `line_height = font_size * 1.4`
            let (extra_w, extra_h) = match t.arrow_direction {
                CalloutArrowDirection::Left | CalloutArrowDirection::Right => (t.arrow_size, 0.0),
                CalloutArrowDirection::Top | CalloutArrowDirection::Bottom => (0.0, t.arrow_size),
            };
            apply_default_size(
                css,
                text_w + h_pad * 2.0 + extra_w,
                line_h + v_pad + extra_h,
            );
        }
        Tooltip(t) => {
            // Same shape as Callout above; padding value borrowed from
            // callout.rs since tooltip.rs's own paint() centers text in the
            // body with no defined constant of its own.
            let font_size = t.style.font_size_px_or(t.font_size);
            let family = t.style.font_family_or("Inter");
            let text_w = measure_text_line_width(&t.text, font_size, family, false);
            let h_pad = 12.0;
            let v_pad = 16.0;
            let line_h = font_size * 1.4;
            let (extra_w, extra_h) = match t.arrow {
                TooltipArrow::Left | TooltipArrow::Right => (t.arrow_size, 0.0),
                TooltipArrow::Top | TooltipArrow::Bottom | TooltipArrow::None => {
                    (0.0, t.arrow_size)
                }
            };
            apply_default_size(
                css,
                text_w + h_pad * 2.0 + extra_w,
                line_h + v_pad + extra_h,
            );
        }
        PillNav(p) => {
            // `height` is already a declared field on the component (like
            // Progress/Switch/Slider above) — just promote it to CSS. Width
            // replicates pill_nav.rs's own private `compute_tab_layout()`
            // formula (h_pad = font_size*1.2 per side, `gap` before/after/
            // between every pill) using the same public fields and the same
            // `measure_text_with_fallback` call it makes internally.
            let font_size = p.style.font_size_px_or(14.0);
            let family = p.style.font_family_or("Inter");
            let h_pad = font_size * 1.2;
            let n = p.items.len() as f32;
            let labels_w: f32 = p
                .items
                .iter()
                .map(|label| measure_text_line_width(label, font_size, family, false) + h_pad * 2.0)
                .sum();
            let total_w = labels_w + p.gap * (n + 1.0).max(1.0);
            apply_default_size(css, total_w, p.height);
        }
        Marquee(m) => {
            // Marquee's whole purpose is to scroll unbounded content, so
            // there's no natural content width. A percentage (mirroring
            // `Particle`'s "fills its parent" default above) would be the
            // obvious choice, but it resolves to 0 against an indefinite
            // parent (a card that itself has no explicit width) — the same
            // "card with height: auto" case this issue asks to fix, so a
            // percentage default would still paint nothing in that case. A
            // fixed width sidesteps that: 800px matches this project's own
            // marquee usage (examples/mega-showcase.json and SKILL.md's own
            // example both use `style.width: 800`, or scale up from there —
            // mega-showcase's 1700px is that same scene's marquee spanning a
            // much wider bleed banner). Height follows the font-size-to-
            // height ratio both of those same real usages share:
            // `font_size: 24` paired with `style.height: 48`, i.e.
            // `2 × font_size`.
            let font_size = m.style.font_size_px_or(m.font_size);
            apply_default_size(css, 800.0, font_size * 2.0);
        }
        Stepper(s) => {
            // Same shape as `Timeline`'s formula above (r*2 + label metrics),
            // adapted to stepper.rs's own layout constants: `cy = r + 4.0`,
            // label offset `r + 12.0`, description offset
            // `label_font_size + 4.0`, and its hardcoded label/description
            // font sizes (14px / 11px — not fields, copied from paint()).
            let n = (s.steps.len().max(1)) as f32;
            let has_desc = s.steps.iter().any(|st| st.description.is_some());
            const LABEL_FS: f32 = 14.0;
            const DESC_FS: f32 = 11.0;
            let max_label_w = s
                .steps
                .iter()
                .map(|st| measure_text_line_width(&st.label, LABEL_FS, "Inter", false))
                .fold(0.0_f32, f32::max);
            let max_desc_w = s
                .steps
                .iter()
                .filter_map(|st| st.description.as_deref())
                .map(|d| measure_text_line_width(d, DESC_FS, "Inter", false))
                .fold(0.0_f32, f32::max);
            match s.orientation {
                StepperOrientation::Horizontal => {
                    // Per-step allocation: the node needs ~3 diameters of
                    // breathing room (a common stepper-UI spacing
                    // convention), or enough for its longest label/desc,
                    // whichever is larger.
                    let per_step = (s.node_size * 3.0).max(max_label_w.max(max_desc_w) + 24.0);
                    let label_h = LABEL_FS * 1.3;
                    let desc_h = if has_desc { DESC_FS * 1.3 + 4.0 } else { 0.0 };
                    let h = s.node_size + 4.0 + 12.0 + label_h + desc_h;
                    apply_default_size(css, per_step * n, h);
                }
                StepperOrientation::Vertical => {
                    let label_w = max_label_w.max(max_desc_w);
                    let w = s.node_size + 12.0 + label_w + 24.0;
                    let label_block = if has_desc {
                        LABEL_FS * 1.3 + DESC_FS * 1.3 + 8.0
                    } else {
                        LABEL_FS * 1.3 + 8.0
                    };
                    let per_step = (s.node_size * 2.0).max(label_block);
                    apply_default_size(css, w, per_step * n);
                }
            }
        }
        TagCloud(tc) => {
            // Replicates tag_cloud.rs's own per-tag metrics (bold Inter,
            // weight-normalized font size between min/max_font_size, its
            // hardcoded h_gap=12/v_gap=8) to sum a single-line content width,
            // then wraps that at a conventional card-content width (matching
            // this project's own tag_cloud usage in
            // examples/mega-showcase.json) to estimate a line count and
            // hence a height — an approximation of the real flow-wrap
            // algorithm, not a re-implementation of it.
            let n = tc.tags.len();
            if n > 0 {
                let min_w = tc.tags.iter().map(|t| t.weight).fold(f64::MAX, f64::min);
                let max_w = tc.tags.iter().map(|t| t.weight).fold(f64::MIN, f64::max);
                let range = (max_w - min_w).max(0.001);
                const H_GAP: f32 = 12.0;
                const V_GAP: f32 = 8.0;
                let total_w: f32 = tc
                    .tags
                    .iter()
                    .map(|t| {
                        let normalized = ((t.weight - min_w) / range) as f32;
                        let fs =
                            tc.min_font_size + normalized * (tc.max_font_size - tc.min_font_size);
                        measure_text_line_width(&t.text, fs, "Inter", true) + H_GAP
                    })
                    .sum();
                const CAP_W: f32 = 600.0;
                let box_w = total_w.clamp(CAP_W * 0.3, CAP_W);
                let lines = (total_w / box_w).ceil().max(1.0);
                let line_h = tc.max_font_size * 1.3;
                let box_h = lines * line_h + (lines - 1.0).max(0.0) * V_GAP;
                apply_default_size(css, box_w, box_h);
            }
        }
        Heatmap(h) => {
            // Fully content-derived from heatmap.rs's own paint() formula:
            // `step = cell_size + cell_gap`, cell (col,row) drawn at
            // `(col*step, row*step)` sized `cell_size` — so the painted
            // extent is exactly `(cols-1)*step + cell_size` per axis.
            let rows = h.data.len();
            let cols = h.data.iter().map(|r| r.len()).max().unwrap_or(0);
            let step = h.cell_size + h.cell_gap;
            let w = (cols.max(1) as f32 - 1.0).max(0.0) * step + h.cell_size;
            let hh = (rows.max(1) as f32 - 1.0).max(0.0) * step + h.cell_size;
            apply_default_size(css, w, hh);
        }
        Sparkline(_) => {
            // "Sparkline: no axes, no labels, compact (120x40 default),
            // inline use" — documented in
            // .claude/skills/rustmotion/rules/data-viz-components.md.
            apply_default_size(css, 120.0, 40.0);
        }
        Stat(_) => {
            // Documented default from
            // .claude/skills/rustmotion/rules/stat-cards.md's own "GOOD"
            // example: `style: { width: 280, height: 180 }`. This is also
            // the fix for the issue's second bug: three `stat`s in a flex
            // row with no explicit size rendered zero pixels because width
            // (not just height) collapsed to 0 in a row context.
            apply_default_size(css, 280.0, 180.0);
        }
        Gauge(g) => {
            // Square — gauge.rs's own paint() derives its ring radius from
            // `min(w, h)/2 - track_width/2 - 4`. Solved backwards for a
            // target radius of 88px (chosen so the value-text font-size —
            // this same file's own `radius * 0.45` — comes out to ~40px,
            // comfortably legible), so the box scales with the component's
            // own `track_width` field rather than a size picked independent
            // of it. At the default `track_width` (16px) this lands on
            // exactly 200×200, which also matches the midpoint of the
            // documented "hero icon" desktop range (160–200px) in
            // icon-sizing-hierarchy.md.
            const TARGET_RADIUS: f32 = 88.0;
            let size = 2.0 * (TARGET_RADIUS + g.track_width / 2.0 + 4.0);
            apply_default_size(css, size, size);
        }
        DotMap(_) => {
            // 2:1 — the standard aspect ratio for an equirectangular world
            // map (360° longitude : 180° latitude), the projection
            // dot_map.rs's own `geo_to_screen` implements. dot_map.rs always
            // paints a full-box background rect first, so any positive size
            // shows ink even with zero points.
            apply_default_size(css, 640.0, 320.0);
        }
        Comparison(_) => {
            // No natural intrinsic size (the painter just splits whatever
            // box it's given at the divider) — matches this project's own
            // reference usage in examples/mega-showcase.json's `comparison`
            // block.
            apply_default_size(css, 520.0, 280.0);
        }
        Treemap(_) => {
            // Slice-and-dice treemap fills whatever box it's given — matches
            // this project's own reference usage in
            // examples/mega-showcase.json's `treemap` block (near-square,
            // the conventional treemap aspect since its rectangles are area-
            // proportional in both axes).
            apply_default_size(css, 416.0, 368.0);
        }
        Chart(c) => {
            // Pie/donut/radar/radial_bar are inherently circular — a square
            // box avoids wasting space on one axis or clipping into an
            // ellipse. The other 8 chart types (bar/line/area/scatter/
            // funnel/waterfall/stacked_bar/horizontal_bar) read axis labels
            // best in a landscape 4:3, per data-viz-components.md's guidance
            // that charts are "larger, standalone" than a sparkline.
            let round = matches!(
                c.chart_type,
                ChartType::Pie | ChartType::Donut | ChartType::Radar | ChartType::RadialBar
            );
            let (dw, dh) = if round {
                (320.0, 320.0)
            } else {
                (400.0, 300.0)
            };
            apply_default_size(css, dw, dh);
        }
        Skeleton(s) => {
            // `rectangle`: documented default from data-viz-components.md's
            // own "GOOD" example (`{ "width": 400, "height": 200 }`).
            // `circle`: matches this file's own Icon/Avatar-adjacent 64px
            // convention (skeleton circles most commonly stand in for an
            // avatar). `text`: fully derived from the component's own
            // `lines`/`line_height`/`line_gap` fields, mirroring `List`'s
            // formula above — matches skeleton.rs's own per-line paint loop
            // (`y = i * (line_height + line_gap)`) exactly.
            match s.variant {
                SkeletonVariant::Rectangle => apply_default_size(css, 400.0, 200.0),
                SkeletonVariant::Circle => apply_default_size(css, 64.0, 64.0),
                SkeletonVariant::Text => {
                    let n = s.lines.max(1) as f32;
                    let h = n * s.line_height + (n - 1.0).max(0.0) * s.line_gap;
                    apply_default_size(css, 240.0, h);
                }
            }
        }
        Mockup(m) => {
            // Per-device aspect matches each device's real-world screen
            // proportions: phones (iPhone/Android) ~9:19.5 (modern
            // flagship aspect), laptop 16:10 (the common MacBook/ultrabook
            // ratio), browser 16:9 (the standard desktop viewport ratio).
            let (dw, dh) = match m.device {
                MockupDevice::Iphone | MockupDevice::Android => (320.0, 690.0),
                MockupDevice::Laptop => (640.0, 400.0),
                MockupDevice::Browser => (640.0, 360.0),
            };
            apply_default_size(css, dw, dh);
        }
        Icon(_) => {
            // 64×64 — the midpoint of the documented "card / feature icon"
            // role across all three device classes in
            // icon-sizing-hierarchy.md (desktop 40–56px, mobile 72–96px,
            // square 60–80px), and a size icon asset systems near-universally
            // ship as a default export (24/32/48/64 being the common family).
            apply_default_size(css, 64.0, 64.0);
        }
        Svg(_) => {
            // 200×200 — square, since an arbitrary vector graphic (icon,
            // diagram, or logo) has no single natural aspect; matches the
            // common equal-aspect SVG viewBox convention and sits above
            // Icon's 64px "card icon" role for the more elaborate content
            // `svg` typically carries (illustrations/diagrams, not glyphs).
            apply_default_size(css, 200.0, 200.0);
        }
        Shape(_) => {
            // 80×80 — matches the median of this project's own decorative
            // (non full-bleed-background, non-divider-line) shape usages in
            // examples/*.json, which cluster at 44–70px for accent shapes
            // (26, 36, 44, 60, 70, 140 — median ~55, rounded up for
            // visibility as a standalone default rather than a same-scene
            // accent tuned against neighbours).
            apply_default_size(css, 80.0, 80.0);
        }
        Image(_) => {
            // 4:3 (400×300) — the traditional default photo aspect ratio,
            // distinct from Video/Gif's 16:9 below so a generic still image
            // doesn't presume widescreen framing.
            apply_default_size(css, 400.0, 300.0);
        }
        Video(_) | Gif(_) => {
            // 16:9 (400×225) — the industry-standard video aspect ratio
            // (matches every render resolution this project documents:
            // 1920×1080, 1280×720), scaled down to a card-sized default.
            apply_default_size(css, 400.0, 225.0);
        }
        Lottie(_) => {
            // 300×300 — square, matching the aspect the vast majority of
            // Lottie animation assets ship at (LottieFiles' own marketplace
            // preview convention is a 1:1 canvas).
            apply_default_size(css, 300.0, 300.0);
        }

        _ => {}
    }
}

/// Apply a component's natural default size (`dw` × `dh`) to `css`, honouring
/// an explicit `aspect-ratio` instead of always falling back to `dw`/`dh`
/// independently (round 4 audit, lot LAYOUT, constat 4 — the previous code
/// guarded each axis with its own `is_none()` check and never looked at
/// `aspect-ratio`, so `width: 400` + `aspect-ratio: 16/9` still got the
/// component's unrelated hardcoded default height instead of 225).
///
/// - Both axes already set: untouched (the author fully specified the box).
/// - One axis set to a fixed pixel length, `aspect-ratio` present: the other
///   axis is derived from it (`h = w / ratio` or `w = h * ratio`) — the CSS
///   replaced-element sizing rule for a single definite axis plus a
///   preferred aspect ratio.
/// - Neither axis set: the natural default width is kept (there is no
///   author-declared axis to derive from), and height is derived from
///   `aspect-ratio` when present, the natural default height otherwise.
///
/// `min-*`/`max-*` need no equivalent guard here: taffy clamps the final
/// used size against them at layout time regardless of what `size` resolves
/// to (`style.min_size`/`max_size` in `taffy_bridge::to_taffy_style`), so a
/// default below `min-width` is corrected downstream, not silently wrong.
fn apply_default_size(css: &mut CssStyle, dw: f32, dh: f32) {
    let ratio = css.aspect_ratio.filter(|r| *r > 0.0);
    match (css.width.is_some(), css.height.is_some()) {
        (true, true) => {}
        (true, false) => {
            let h = fixed_px(css.width.as_ref())
                .zip(ratio)
                .map(|(w, r)| w / r)
                .unwrap_or(dh);
            css.height = Some(CSize::Length(CLP::Px(h)));
        }
        (false, true) => {
            let w = fixed_px(css.height.as_ref())
                .zip(ratio)
                .map(|(h, r)| h * r)
                .unwrap_or(dw);
            css.width = Some(CSize::Length(CLP::Px(w)));
        }
        (false, false) => {
            css.width = Some(CSize::Length(CLP::Px(dw)));
            let h = ratio.map(|r| dw / r).unwrap_or(dh);
            css.height = Some(CSize::Length(CLP::Px(h)));
        }
    }
}

/// Extract a fixed pixel value from a `Size`, if it resolves to one without a
/// `LengthContext` (only `Size::Length(LengthPercentage::Px(_))` — a bare
/// number or `"NNpx"`). `%`/`vw`/`vh`/`em`/`rem` and `auto` return `None`:
/// `apply_default_size` can't derive a ratio from a length it can't resolve
/// at build time, so it falls back to the component's hardcoded default.
fn fixed_px(size: Option<&CSize>) -> Option<f32> {
    match size? {
        CSize::Length(lp) => match lp.try_parse()? {
            rustmotion_core::css::units::ParsedLength::Px(v) => Some(v),
            _ => None,
        },
        _ => None,
    }
}

/// Borrow the `CssStyle` from any component.
fn component_style(c: &Component) -> &CssStyle {
    use Component::*;
    match c {
        Text(c) => &c.style,
        Shape(c) => &c.style,
        Image(c) => &c.style,
        Icon(c) => &c.style,
        Svg(c) => &c.style,
        Video(c) => &c.style,
        Gif(c) => &c.style,
        Counter(c) => &c.style,
        Cursor(c) => &c.style,
        Caption(c) => &c.style,
        Codeblock(c) => &c.style,
        Connector(c) => &c.style,
        Avatar(c) => &c.style,
        AvatarGroup(c) => &c.style,
        Arrow(c) => &c.style,
        Badge(c) => &c.style,
        Callout(c) => &c.style,
        Chart(c) => &c.style,
        Comparison(c) => &c.style,
        Countdown(c) => &c.style,
        Divider(c) => &c.style,
        DotMap(c) => &c.style,
        Gauge(c) => &c.style,
        GradientText(c) => &c.style,
        Heatmap(c) => &c.style,
        Kbd(c) => &c.style,
        Line(c) => &c.style,
        List(c) => &c.style,
        Lottie(c) => &c.style,
        Marquee(c) => &c.style,
        Mockup(c) => &c.style,
        Notification(c) => &c.style,
        Particle(c) => &c.style,
        PillNav(c) => &c.style,
        Progress(c) => &c.style,
        QrCode(c) => &c.style,
        NumberWheel(c) => &c.style,
        SuccessCheck(c) => &c.style,
        Pointer(c) => &c.style,
        Rating(c) => &c.style,
        Skeleton(c) => &c.style,
        Slider(c) => &c.style,
        Sparkline(c) => &c.style,
        Stat(c) => &c.style,
        Stepper(c) => &c.style,
        Switch(c) => &c.style,
        RichText(c) => &c.style,
        Table(c) => &c.style,
        TagCloud(c) => &c.style,
        Terminal(c) => &c.style,
        Timeline(c) => &c.style,
        Tooltip(c) => &c.style,
        Treemap(c) => &c.style,
        Positioned(c) => &c.style,
        Flex(c) => &c.style,
        Grid(c) => &c.style,
        Card(c) => &c.style,
        Container(c) => &c.style,
        AudioSpectrum(c) => &c.style,
        Waveform(c) => &c.style,
    }
}

/// Short kind label for a component (for studio selection / inspector display).
pub fn component_kind(c: &Component) -> &'static str {
    use Component::*;
    match c {
        Text(_) => "text",
        Shape(_) => "shape",
        Image(_) => "image",
        Icon(_) => "icon",
        Svg(_) => "svg",
        Video(_) => "video",
        Gif(_) => "gif",
        Counter(_) => "counter",
        Cursor(_) => "cursor",
        Caption(_) => "caption",
        Codeblock(_) => "codeblock",
        Connector(_) => "connector",
        Avatar(_) => "avatar",
        AvatarGroup(_) => "avatar_group",
        Arrow(_) => "arrow",
        Badge(_) => "badge",
        Callout(_) => "callout",
        Chart(_) => "chart",
        Comparison(_) => "comparison",
        Countdown(_) => "countdown",
        Divider(_) => "divider",
        DotMap(_) => "dot_map",
        Gauge(_) => "gauge",
        GradientText(_) => "gradient_text",
        Heatmap(_) => "heatmap",
        Kbd(_) => "kbd",
        Line(_) => "line",
        List(_) => "list",
        Lottie(_) => "lottie",
        Marquee(_) => "marquee",
        Mockup(_) => "mockup",
        Notification(_) => "notification",
        Particle(_) => "particle",
        PillNav(_) => "pill_nav",
        Progress(_) => "progress",
        QrCode(_) => "qrcode",
        NumberWheel(_) => "number_wheel",
        SuccessCheck(_) => "success_check",
        Pointer(_) => "pointer",
        Rating(_) => "rating",
        Skeleton(_) => "skeleton",
        Slider(_) => "slider",
        Sparkline(_) => "sparkline",
        Stat(_) => "stat",
        Stepper(_) => "stepper",
        Switch(_) => "switch",
        RichText(_) => "rich_text",
        Table(_) => "table",
        TagCloud(_) => "tag_cloud",
        Terminal(_) => "terminal",
        Timeline(_) => "timeline",
        Tooltip(_) => "tooltip",
        Treemap(_) => "treemap",
        Positioned(_) => "positioned",
        Flex(_) => "flex",
        Grid(_) => "grid",
        Card(_) => "card",
        Container(_) => "container",
        AudioSpectrum(_) => "audio_spectrum",
        Waveform(_) => "waveform",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmotion_core::css::style::{
        CssStyle, Display, Edges, FlexDirection, Gap, Size as CSize,
    };
    use rustmotion_core::css::taffy_bridge::ConversionContext;
    use rustmotion_core::css::units::LengthPercentage;
    use rustmotion_core::css::units::LengthPercentage as CLP;
    use rustmotion_core::engine::layout_pass::run_layout;
    use serde_json::json;

    fn make_card(children: Vec<ChildComponent>, style: CssStyle) -> Component {
        Component::Card(crate::card::Card {
            children,
            timing: Default::default(),
            style,
            timeline: Vec::new(),
            stagger: None,
            time_scale: None,
            time_offset: None,
        })
    }

    fn make_shape(width: f32, height: f32) -> ChildComponent {
        ChildComponent {
            component: Component::Shape(crate::shape::Shape {
                shape: rustmotion_core::schema::ShapeType::Rect,
                text: None,
                timing: Default::default(),
                style: CssStyle {
                    width: Some(CSize::Length(CLP::Px(width))),
                    height: Some(CSize::Length(CLP::Px(height))),
                    ..Default::default()
                },
                timeline: Vec::new(),
                stagger: None,
                fill: None,
                stroke: None,
            }),
            position: None,
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        }
    }

    fn make_text(content: &str, style: CssStyle) -> ChildComponent {
        ChildComponent {
            component: Component::Text(crate::text::Text {
                content: content.to_string(),
                max_width: None,
                timing: Default::default(),
                style,
                timeline: Vec::new(),
                stagger: None,
                text_shadow: None,
                stroke: None,
                text_background: None,
                caret: None,
                states: Vec::new(),
                swap: None,
            }),
            position: None,
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        }
    }

    #[test]
    fn empty_scene_has_only_root() {
        let built = build_scene(&[], (1920.0, 1080.0));
        assert_eq!(built.root.children.len(), 0);
        assert_eq!(built.components.len(), 1); // synthetic root slot
    }

    #[test]
    fn component_kind_labels() {
        assert_eq!(component_kind(&make_shape(100.0, 50.0).component), "shape");
    }

    #[test]
    fn build_child_records_source_path() {
        let card = make_card(
            vec![make_shape(10.0, 10.0), make_shape(10.0, 10.0)],
            CssStyle::default(),
        );
        let scene = vec![ChildComponent {
            component: card,
            position: Some(crate::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        }];
        let built = build_scene(&scene, (800.0, 600.0));
        let card_box = &built.root.children[0];
        assert_eq!(card_box.source_path.as_deref(), Some("/children/0"));
        assert_eq!(
            card_box.children[1].source_path.as_deref(),
            Some("/children/0/children/1")
        );
    }

    #[test]
    fn flex_card_with_two_shapes_lays_out_vertically() {
        let card = make_card(
            vec![make_shape(200.0, 50.0), make_shape(200.0, 50.0)],
            CssStyle {
                display: Some(Display::Flex),
                flex_direction: Some(FlexDirection::Column),
                gap: Some(Gap::Uniform(LengthPercentage::Px(10.0))),
                padding: Some(Edges::Uniform(LengthPercentage::Px(20.0))),
                width: Some(CSize::Length(CLP::Px(300.0))),
                height: Some(CSize::Length(CLP::Px(200.0))),
                ..Default::default()
            },
        );
        let scene = vec![ChildComponent {
            component: card,
            position: Some(crate::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        }];
        let built = build_scene(&scene, (1920.0, 1080.0));
        assert_eq!(built.root.children.len(), 1);
        let card_box = &built.root.children[0];
        assert_eq!(card_box.css.display, Some(Display::Flex));
        assert_eq!(card_box.css.flex_direction, Some(FlexDirection::Column));
        assert_eq!(card_box.children.len(), 2);

        let layout = run_layout(&built.root, (1920.0, 1080.0), &ConversionContext::default());
        let card_layout = layout.get(card_box.id).expect("card laid out");
        assert_eq!(card_layout.x, 0.0);
        assert_eq!(card_layout.y, 0.0);
        assert_eq!(card_layout.width, 300.0);
        assert_eq!(card_layout.height, 200.0);

        let c1 = layout
            .get(card_box.children[0].id)
            .expect("shape 1 laid out");
        let c2 = layout
            .get(card_box.children[1].id)
            .expect("shape 2 laid out");
        // Padding 20 from top, then first shape 50 high, gap 10 → 80.
        assert_eq!(c1.x, 20.0);
        assert_eq!(c1.y, 20.0);
        assert_eq!(c2.x, 20.0);
        assert_eq!(c2.y, 80.0);
    }

    #[test]
    fn absolute_child_uses_top_left() {
        let scene = vec![ChildComponent {
            component: Component::Shape(crate::shape::Shape {
                shape: rustmotion_core::schema::ShapeType::Rect,
                text: None,
                timing: Default::default(),
                style: CssStyle {
                    width: Some(CSize::Length(CLP::Px(100.0))),
                    height: Some(CSize::Length(CLP::Px(80.0))),
                    ..Default::default()
                },
                timeline: Vec::new(),
                stagger: None,
                fill: None,
                stroke: None,
            }),
            position: Some(crate::PositionMode::Absolute { x: 40.0, y: 30.0 }),
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        }];
        let built = build_scene(&scene, (400.0, 400.0));
        let layout = run_layout(&built.root, (400.0, 400.0), &ConversionContext::default());
        let shape_id = built.root.children[0].id;
        let l = layout.get(shape_id).expect("shape laid out");
        assert_eq!(l.x, 40.0);
        assert_eq!(l.y, 30.0);
        assert_eq!(l.width, 100.0);
        assert_eq!(l.height, 80.0);
    }

    #[test]
    fn horizontal_divider_stretches_to_parent_width() {
        let divider = ChildComponent {
            component: Component::Divider(crate::divider::Divider {
                direction: DividerDirection::Horizontal,
                thickness: 4.0,
                line_style: Default::default(),
                length: None,
                timing: Default::default(),
                style: CssStyle::default(),
                timeline: Vec::new(),
                stagger: None,
            }),
            position: None,
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        };
        let scene = vec![divider];
        let built = build_scene(&scene, (800.0, 600.0));
        let layout = run_layout(&built.root, (800.0, 600.0), &ConversionContext::default());
        let id = built.root.children[0].id;
        let l = layout.get(id).expect("divider laid out");
        assert_eq!(l.height, 4.0);
        assert_eq!(l.width, 800.0);
    }

    #[test]
    fn text_child_in_flex_card_gets_cosmic_intrinsic_size() {
        // A flex column card with no fixed size — its children's intrinsic
        // sizes should determine the card's width/height. The text child
        // must be measured via cosmic-text, not collapse to 0×0.
        use crate::card::Card;
        use crate::text::Text;

        use rustmotion_core::css::units::Length;

        let text = ChildComponent {
            component: Component::Text(Text {
                content: "Hello World".into(),
                max_width: None,
                timing: Default::default(),
                style: CssStyle {
                    font_size: Some(Length::Px(40.0)),
                    ..Default::default()
                },
                timeline: Vec::new(),
                stagger: None,
                text_shadow: None,
                stroke: None,
                text_background: None,
                caret: None,
                states: Vec::new(),
                swap: None,
            }),
            position: None,
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        };

        let card = ChildComponent {
            component: Component::Card(Card {
                children: vec![text],
                timing: Default::default(),
                style: CssStyle {
                    display: Some(Display::Flex),
                    flex_direction: Some(FlexDirection::Column),
                    padding: Some(Edges::Uniform(LengthPercentage::Px(20.0))),
                    ..Default::default()
                },
                timeline: Vec::new(),
                stagger: None,
                time_scale: None,
                time_offset: None,
            }),
            position: Some(crate::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        };

        let scene = vec![card];
        let built = build_scene(&scene, (1920.0, 1080.0));
        let layout = run_layout(&built.root, (1920.0, 1080.0), &ConversionContext::default());

        let card_id = built.root.children[0].id;
        let text_id = built.root.children[0].children[0].id;
        let text_layout = layout.get(text_id).expect("text laid out");

        assert!(
            text_layout.width > 0.0,
            "text width should be > 0, got {}",
            text_layout.width
        );
        assert!(
            text_layout.height >= 40.0,
            "text height should be at least one line tall, got {}",
            text_layout.height
        );

        // Card height should hug the text + 2×padding(20) = ~text_h + 40.
        let card_layout = layout.get(card_id).expect("card laid out");
        assert!(
            card_layout.height >= text_layout.height + 40.0 - 1.0,
            "card height ({}) should fit text + padding ({}+40)",
            card_layout.height,
            text_layout.height,
        );
    }

    #[test]
    fn arrow_intrinsic_size_uses_endpoint_bbox_plus_arrowhead() {
        let arrow = ChildComponent {
            component: Component::Arrow(crate::arrow::Arrow {
                x1: 10.0,
                y1: 20.0,
                x2: 110.0,
                y2: 80.0,
                cp: None,
                cp1: None,
                cp2: None,
                curve: None,
                width: 4.0,
                color: "#fff".into(),
                arrow_end: true,
                arrow_start: false,
                arrow_size: 12.0,
                dashed: None,
                timing: Default::default(),
                style: CssStyle::default(),
                timeline: Vec::new(),
                stagger: None,
            }),
            position: Some(crate::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        };
        let scene = vec![arrow];
        let built = build_scene(&scene, (800.0, 600.0));
        let layout = run_layout(&built.root, (800.0, 600.0), &ConversionContext::default());
        let l = layout
            .get(built.root.children[0].id)
            .expect("arrow laid out");
        // bbox 100×60 + (16 padding + 12 arrow_size) = 128×88.
        assert_eq!(l.width, 128.0);
        assert_eq!(l.height, 88.0);
    }

    #[test]
    fn connector_intrinsic_size_uses_endpoint_bbox_plus_arrowhead() {
        let conn = ChildComponent {
            component: Component::Connector(crate::connector::Connector {
                from: crate::connector::ConnectorPoint { x: 50.0, y: 0.0 },
                to: crate::connector::ConnectorPoint { x: 150.0, y: 50.0 },
                routing: Default::default(),
                curvature: 0.4,
                width: 2.0,
                color: "#fff".into(),
                arrow_end: true,
                arrow_start: false,
                arrow_size: 10.0,
                dashed: None,
                timing: Default::default(),
                style: CssStyle::default(),
                timeline: Vec::new(),
                stagger: None,
            }),
            position: Some(crate::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        };
        let scene = vec![conn];
        let built = build_scene(&scene, (800.0, 600.0));
        let layout = run_layout(&built.root, (800.0, 600.0), &ConversionContext::default());
        let l = layout
            .get(built.root.children[0].id)
            .expect("connector laid out");
        // bbox 100×50 + (16 + 10) = 126×76.
        assert_eq!(l.width, 126.0);
        assert_eq!(l.height, 76.0);
    }

    #[test]
    fn counter_intrinsic_size_reserves_space_for_max_value() {
        // 1234 → 1234 → format with 0 decimals → measure largest absolute value.
        // Expectation: width > 0 (cosmic-text didn't fail), height ≈ font_size × line_height.
        use crate::counter::Counter;

        use rustmotion_core::css::units::Length;
        let counter = ChildComponent {
            component: Component::Counter(Counter {
                duration: None,
                from: 0.0,
                to: 1234.0,
                decimals: 0,
                separator: None,
                prefix: None,
                suffix: None,
                easing: Default::default(),
                timing: Default::default(),
                style: CssStyle {
                    font_size: Some(Length::Px(64.0)),
                    ..Default::default()
                },
                timeline: Vec::new(),
                stagger: None,
                text_shadow: None,
                stroke: None,
            }),
            position: Some(crate::PositionMode::Absolute { x: 10.0, y: 20.0 }),
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        };
        let scene = vec![counter];
        let built = build_scene(&scene, (800.0, 600.0));
        let layout = run_layout(&built.root, (800.0, 600.0), &ConversionContext::default());
        let id = built.root.children[0].id;
        let l = layout.get(id).expect("counter laid out");
        assert!(
            l.width > 0.0,
            "counter width should be > 0, got {}",
            l.width
        );
        // line_height defaults to font_size × 1.3 = 83.2. Allow some slack.
        assert!(
            l.height >= 60.0,
            "counter height should be ≥ ~one line ({}), got {}",
            64.0,
            l.height
        );
    }

    #[test]
    fn badge_intrinsic_size_includes_padding_and_text() {
        // Default size = Md → font_size 14, h_pad 12, v_pad 6, icon 18.
        // Without an icon, height ≈ 6×2 + 14×1.3 ≈ 30.2.
        use crate::badge::{Badge, BadgeSize, BadgeVariant};

        let badge = ChildComponent {
            component: Component::Badge(Badge {
                text: "New".into(),
                icon: None,
                variant: BadgeVariant::Solid,
                badge_size: BadgeSize::Md,
                dot: false,
                dot_color: None,
                pulse: false,
                count: None,
                timing: Default::default(),
                style: CssStyle::default(),
                timeline: Vec::new(),
                stagger: None,
            }),
            position: Some(crate::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        };
        let scene = vec![badge];
        let built = build_scene(&scene, (400.0, 200.0));
        let layout = run_layout(&built.root, (400.0, 200.0), &ConversionContext::default());
        let id = built.root.children[0].id;
        let l = layout.get(id).expect("badge laid out");
        // h_pad×2 = 24 alone, plus the text width.
        assert!(
            l.width > 24.0,
            "badge width should exceed padding alone, got {}",
            l.width
        );
        assert!(
            (l.height - 30.2).abs() < 2.0,
            "badge height should be ~30.2, got {}",
            l.height
        );
    }

    #[test]
    fn line_intrinsic_size_matches_endpoint_bounding_box() {
        let line = ChildComponent {
            component: Component::Line(crate::line::Line {
                x1: 10.0,
                y1: 20.0,
                x2: 110.0,
                y2: 80.0,
                width: 2.0,
                color: "#fff".into(),
                dashed: None,
                timing: Default::default(),
                style: CssStyle::default(),
                timeline: Vec::new(),
                stagger: None,
            }),
            position: Some(crate::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        };
        let scene = vec![line];
        let built = build_scene(&scene, (800.0, 600.0));
        let layout = run_layout(&built.root, (800.0, 600.0), &ConversionContext::default());
        let id = built.root.children[0].id;
        let l = layout.get(id).expect("line laid out");
        assert_eq!(l.width, 100.0);
        assert_eq!(l.height, 60.0);
    }

    // ─── M2: rich_text intrinsic wired into component_intrinsic ─────────────

    #[test]
    fn rich_text_child_gets_a_non_zero_intrinsic_size() {
        // Before the fix: rich_text had no `component_intrinsic` entry, so
        // an auto-sized rich_text laid out at 0×0 and was invisible.
        use crate::rich_text::{RichText, RichTextSpan};
        use rustmotion_core::css::units::Length;

        let rich_text = ChildComponent {
            component: Component::RichText(RichText {
                spans: vec![
                    RichTextSpan {
                        text: "Save ".into(),
                        color: None,
                        font_size: None,
                        font_weight: None,
                        font_family: None,
                        font_style: None,
                        letter_spacing: None,
                    },
                    RichTextSpan {
                        text: "40%".into(),
                        color: Some("#5C39EE".into()),
                        font_size: None,
                        font_weight: None,
                        font_family: None,
                        font_style: None,
                        letter_spacing: None,
                    },
                ],
                max_width: None,
                timing: Default::default(),
                style: CssStyle {
                    font_size: Some(Length::Px(40.0)),
                    ..Default::default()
                },
                timeline: Vec::new(),
                stagger: None,
            }),
            position: Some(crate::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        };
        let scene = vec![rich_text];
        let built = build_scene(&scene, (800.0, 600.0));
        let layout = run_layout(&built.root, (800.0, 600.0), &ConversionContext::default());
        let id = built.root.children[0].id;
        let l = layout.get(id).expect("rich_text laid out");
        assert!(
            l.width > 0.0,
            "rich_text width should be > 0, got {}",
            l.width
        );
        assert!(
            l.height > 0.0,
            "rich_text height should be > 0, got {}",
            l.height
        );
    }

    // ─── M3: the `glow` effect renders as a coloured drop-shadow ────────────

    #[test]
    fn glow_effect_adds_a_coloured_drop_shadow_filter() {
        use rustmotion_core::css::style::{Color, FilterFn};
        use rustmotion_core::css::units::Length as CLength;
        use rustmotion_core::schema::{AnimationEffect, GlowConfig};

        let mut shape = make_shape(100.0, 100.0);
        if let Component::Shape(ref mut s) = shape.component {
            s.style.animation = vec![AnimationEffect::Glow(GlowConfig {
                color: "#5C39EE".to_string(),
                radius: 12.0,
                intensity: 1.0,
            })];
        }
        let anim = BuildAnimationCtx {
            time: 0.0,
            scenario_time: 0.0,
            scene_duration: 1.0,
            fps: 30,
        };
        let scene = [shape];
        let built = build_scene_with_anim(&scene, (400.0, 400.0), anim);
        let filters = built.root.children[0]
            .css
            .filter
            .as_ref()
            .expect("glow must add a filter list");
        let shadow = filters
            .iter()
            .find(|f| matches!(f, FilterFn::DropShadow { .. }))
            .expect("a DropShadow filter must be present");
        let FilterFn::DropShadow { blur, color, .. } = shadow else {
            unreachable!()
        };
        match blur {
            Some(CLength::Px(px)) => {
                assert_eq!(*px, 12.0, "blur radius must match GlowConfig.radius")
            }
            other => panic!("expected Some(Length::Px(12.0)) blur, got {:?}", other),
        }
        // The defect this fixes: `color: None` resolves to black downstream
        // (see `paint_pass.rs`'s `unwrap_or(SColor::BLACK)`). The colour must
        // be `Some` and must match the configured glow colour, not black.
        match color {
            Some(Color::Rgba { r, g, b, a }) => {
                assert_eq!(*r, 0x5C);
                assert_eq!(*g, 0x39);
                assert_eq!(*b, 0xEE);
                assert!(*a > 0.0, "alpha must be > 0 for the glow to be visible");
            }
            other => panic!("expected Some(Color::Rgba{{..}}), got {:?}", other),
        }
    }

    #[test]
    fn glow_effect_scales_alpha_by_intensity() {
        use rustmotion_core::css::style::{Color, FilterFn};
        use rustmotion_core::schema::{AnimationEffect, GlowConfig};

        let mut shape = make_shape(100.0, 100.0);
        if let Component::Shape(ref mut s) = shape.component {
            s.style.animation = vec![AnimationEffect::Glow(GlowConfig {
                color: "#FFFFFFFF".to_string(), // opaque white
                radius: 10.0,
                intensity: 0.5,
            })];
        }
        let anim = BuildAnimationCtx {
            time: 0.0,
            scenario_time: 0.0,
            scene_duration: 1.0,
            fps: 30,
        };
        let scene = [shape];
        let built = build_scene_with_anim(&scene, (400.0, 400.0), anim);
        let filters = built.root.children[0].css.filter.as_ref().unwrap();
        let FilterFn::DropShadow { color, .. } = filters
            .iter()
            .find(|f| matches!(f, FilterFn::DropShadow { .. }))
            .unwrap()
        else {
            unreachable!()
        };
        let Some(Color::Rgba { a, .. }) = color else {
            panic!("expected Rgba color");
        };
        assert!(
            (*a - 0.5).abs() < 1e-4,
            "intensity 0.5 on an opaque colour must scale alpha to ~0.5, got {}",
            a
        );
    }

    #[test]
    fn no_glow_effect_leaves_filter_untouched() {
        let shape = make_shape(100.0, 100.0);
        let anim = BuildAnimationCtx {
            time: 0.0,
            scenario_time: 0.0,
            scene_duration: 1.0,
            fps: 30,
        };
        let scene = [shape];
        let built = build_scene_with_anim(&scene, (400.0, 400.0), anim);
        assert!(built.root.children[0].css.filter.is_none());
    }

    // ─── #126 / W3: the 23 components with no size source ───────────────────

    /// Build a `ChildComponent` from a JSON literal (matches the `type`-tagged
    /// `Component` enum's own `Deserialize` impl) — much less error-prone than
    /// a full struct literal for components with a dozen+ fields.
    fn child_from_json(json: serde_json::Value) -> ChildComponent {
        let component: Component =
            serde_json::from_value(json.clone()).unwrap_or_else(|e| panic!("{e}\n{json:#}"));
        ChildComponent {
            component,
            position: None,
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        }
    }

    /// Lay out a single unsized child inside a card and return its final
    /// `(width, height)`. The card itself has no explicit width or height
    /// either, so this also exercises acceptance criterion #3 ("a card with
    /// height: auto sizes to the component instead of collapsing to
    /// padding").
    fn layout_in_auto_card(child_json: serde_json::Value) -> (f32, f32) {
        let card = make_card(vec![child_from_json(child_json)], CssStyle::default());
        let scene = vec![ChildComponent {
            component: card,
            position: Some(crate::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        }];
        let built = build_scene(&scene, (1920.0, 1080.0));
        let layout = run_layout(&built.root, (1920.0, 1080.0), &ConversionContext::default());
        let child_id = built.root.children[0].children[0].id;
        let l = layout.get(child_id).expect("child laid out");
        (l.width, l.height)
    }

    /// Every one of the 23 components #126 lists gets a positive width *and*
    /// height with no `style` at all — before this file's fix they all laid
    /// out at 0×0 inside a card (proven by rendering: see the workstream's
    /// scratch `ink-measure` harness, which shows every one of these going
    /// from 0 painted pixels to a positive count under the identical fixture).
    /// A positive layout box is the necessary precondition for any of that
    /// painted ink — this test is the fast, render-free regression guard.
    #[test]
    fn all_23_unsized_components_get_a_positive_box_in_a_card() {
        let cases: &[(&str, serde_json::Value)] = &[
            ("callout", json!({"type":"callout","text":"Hello"})),
            (
                "chart",
                json!({"type":"chart","chart_type":"bar","data":[{"value":10}]}),
            ),
            ("comparison", json!({"type":"comparison"})),
            (
                "dot_map",
                json!({"type":"dot_map","points":[{"lat":10.0,"lng":10.0}]}),
            ),
            ("gauge", json!({"type":"gauge","value":50})),
            ("gif", json!({"type":"gif","src":"a.gif"})),
            ("heatmap", json!({"type":"heatmap","data":[[1.0,2.0]]})),
            ("icon", json!({"type":"icon","icon":"lucide:home"})),
            ("image", json!({"type":"image","src":"a.png"})),
            ("lottie", json!({"type":"lottie","data":"{}"})),
            ("marquee", json!({"type":"marquee","content":"hi"})),
            (
                "mockup",
                json!({"type":"mockup","device":"iphone","src":"a.png"}),
            ),
            ("pill_nav", json!({"type":"pill_nav","items":["A","B"]})),
            ("shape", json!({"type":"shape","shape":"rect"})),
            ("skeleton", json!({"type":"skeleton"})),
            (
                "skeleton_text",
                json!({"type":"skeleton","variant":"text","lines":3}),
            ),
            (
                "sparkline",
                json!({"type":"sparkline","data":[1.0,2.0,3.0]}),
            ),
            ("stat", json!({"type":"stat","value":"42"})),
            (
                "stepper",
                json!({"type":"stepper","steps":[{"label":"A"},{"label":"B"}]}),
            ),
            ("svg", json!({"type":"svg","data":"<svg></svg>"})),
            (
                "tag_cloud",
                json!({"type":"tag_cloud","tags":[{"text":"rust","weight":1.0}]}),
            ),
            ("tooltip", json!({"type":"tooltip","text":"hi"})),
            ("treemap", json!({"type":"treemap","data":[{"value":10.0}]})),
            ("video", json!({"type":"video","src":"a.mp4"})),
        ];
        for (name, json) in cases {
            let (w, h) = layout_in_auto_card(json.clone());
            assert!(w > 0.0, "{name}: width should be > 0, got {w}");
            assert!(h > 0.0, "{name}: height should be > 0, got {h}");
        }
    }

    #[test]
    fn heatmap_intrinsic_size_matches_cell_grid_formula() {
        // 2 rows x 3 cols, default cell_size=14, cell_gap=3.
        // width = (3-1)*(14+3) + 14 = 48, height = (2-1)*17 + 14 = 31.
        let (w, h) =
            layout_in_auto_card(json!({"type":"heatmap","data":[[1.0,2.0,3.0],[4.0,5.0,6.0]]}));
        assert_eq!(w, 48.0);
        assert_eq!(h, 31.0);
    }

    #[test]
    fn sparkline_gets_the_documented_120x40_default() {
        // .claude/skills/rustmotion/rules/data-viz-components.md: "Sparkline:
        // ... compact (120x40 default), inline use."
        let (w, h) = layout_in_auto_card(json!({"type":"sparkline","data":[1.0,2.0,3.0]}));
        assert_eq!(w, 120.0);
        assert_eq!(h, 40.0);
    }

    #[test]
    fn stat_gets_the_documented_280x180_default() {
        // .claude/skills/rustmotion/rules/stat-cards.md's own "GOOD" example.
        let (w, h) = layout_in_auto_card(json!({"type":"stat","value":"42"}));
        assert_eq!(w, 280.0);
        assert_eq!(h, 180.0);
    }

    #[test]
    fn gauge_default_size_is_square() {
        let (w, h) = layout_in_auto_card(json!({"type":"gauge","value":50}));
        assert_eq!(w, h);
        assert!(w > 0.0);
    }

    #[test]
    fn pill_nav_height_promotes_its_own_declared_field() {
        // `height` is already a field on PillNav (default 44.0) — the fix
        // just promotes it to CSS, like Progress/Switch/Slider above.
        let (_, h) = layout_in_auto_card(json!({"type":"pill_nav","items":["Overview"]}));
        assert_eq!(h, 44.0);
    }

    #[test]
    fn callout_width_grows_with_its_own_text_content() {
        // A longer text must produce a wider box (content-derived, not a
        // fixed constant) — proves the measured-text path is actually wired
        // up, not just a padding-only fallback.
        let (short_w, _) = layout_in_auto_card(json!({"type":"callout","text":"Hi"}));
        let (long_w, _) = layout_in_auto_card(
            json!({"type":"callout","text":"This is a much longer callout message"}),
        );
        assert!(
            long_w > short_w,
            "longer text should measure a wider box: short={short_w}, long={long_w}"
        );
    }

    #[test]
    fn three_stats_in_a_flex_row_all_get_a_positive_width() {
        // #126's second bug: 3 `stat`s in a flex-row card with no explicit
        // size rendered zero pixels because *width* (not height) collapsed
        // to 0 — align-items:stretch only helps the cross axis, which is
        // height in a row.
        let stats = vec![
            child_from_json(json!({"type":"stat","value":"45.2K","label":"Users"})),
            child_from_json(json!({"type":"stat","value":"12%","label":"Growth"})),
            child_from_json(json!({"type":"stat","value":"$8.1M","label":"Revenue"})),
        ];
        let card = make_card(
            stats,
            CssStyle {
                display: Some(Display::Flex),
                flex_direction: Some(FlexDirection::Row),
                gap: Some(Gap::Uniform(LengthPercentage::Px(16.0))),
                ..Default::default()
            },
        );
        let scene = vec![ChildComponent {
            component: card,
            position: Some(crate::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        }];
        let built = build_scene(&scene, (1920.0, 1080.0));
        let layout = run_layout(&built.root, (1920.0, 1080.0), &ConversionContext::default());
        for (i, child) in built.root.children[0].children.iter().enumerate() {
            let l = layout.get(child.id).expect("stat laid out");
            assert!(
                l.width > 0.0,
                "stat #{i}: width should be > 0, got {}",
                l.width
            );
            assert!(
                l.height > 0.0,
                "stat #{i}: height should be > 0, got {}",
                l.height
            );
        }
    }

    // ── Round 4 audit, lot LAYOUT, constat 2: the CSS cascade is wired ──────
    // `crates/rustmotion-core/src/css/cascade.rs::inherit_from` existed but
    // nothing called it — `color`/`font-*` set on a container never reached
    // children lacking their own value.

    #[test]
    fn card_color_cascades_to_text_child_with_no_color_of_its_own() {
        use rustmotion_core::css::style::Color;

        let card = make_card(
            vec![make_text("hello", CssStyle::default())],
            CssStyle {
                color: Some(Color::String("#ff0000".into())),
                ..Default::default()
            },
        );
        let scene = vec![ChildComponent {
            component: card,
            position: Some(crate::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        }];
        let built = build_scene(&scene, (800.0, 600.0));
        let text_box = &built.root.children[0].children[0];
        assert_eq!(
            text_box.css.color,
            Some(Color::String("#ff0000".into())),
            "text child declares no color of its own — it should inherit the card's"
        );
    }

    #[test]
    fn text_own_color_wins_over_inherited_card_color() {
        use rustmotion_core::css::style::Color;

        let card = make_card(
            vec![make_text(
                "hello",
                CssStyle {
                    color: Some(Color::String("#00ff00".into())),
                    ..Default::default()
                },
            )],
            CssStyle {
                color: Some(Color::String("#ff0000".into())),
                ..Default::default()
            },
        );
        let scene = vec![ChildComponent {
            component: card,
            position: Some(crate::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        }];
        let built = build_scene(&scene, (800.0, 600.0));
        let text_box = &built.root.children[0].children[0];
        assert_eq!(
            text_box.css.color,
            Some(Color::String("#00ff00".into())),
            "text child's own explicit color must win over the inherited card color"
        );
    }

    #[test]
    fn card_display_does_not_cascade_to_text_child() {
        // `display` is not an inheritable CSS property — only the documented
        // inheritable list (color, font-*, text-align, white-space, ...)
        // should propagate.
        let card = make_card(
            vec![make_text("hello", CssStyle::default())],
            CssStyle {
                display: Some(Display::Flex),
                ..Default::default()
            },
        );
        let scene = vec![ChildComponent {
            component: card,
            position: Some(crate::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        }];
        let built = build_scene(&scene, (800.0, 600.0));
        let text_box = &built.root.children[0].children[0];
        assert_eq!(text_box.css.display, None);
    }

    // ── Round 4 audit, lot LAYOUT, constat 4: `apply_intrinsic_overrides`'s
    // default size ignored an explicit `aspect-ratio`. ─────────────────────

    fn make_aspect_shape(width: f32, aspect_ratio: f32) -> ChildComponent {
        ChildComponent {
            component: Component::Shape(crate::shape::Shape {
                shape: rustmotion_core::schema::ShapeType::Rect,
                text: None,
                timing: Default::default(),
                style: CssStyle {
                    width: Some(CSize::Length(CLP::Px(width))),
                    aspect_ratio: Some(aspect_ratio),
                    ..Default::default()
                },
                timeline: Vec::new(),
                stagger: None,
                fill: None,
                stroke: None,
            }),
            position: Some(crate::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        }
    }

    #[test]
    fn explicit_width_with_aspect_ratio_derives_height_instead_of_the_hardcoded_default() {
        // `shape`'s hardcoded default is 80×80 (see `apply_intrinsic_overrides`).
        // `width: 400` + `aspect-ratio: 16/9` should derive height = 225, not
        // fall back to the unrelated 80px default.
        let scene = vec![make_aspect_shape(400.0, 16.0 / 9.0)];
        let built = build_scene(&scene, (1920.0, 1080.0));
        let layout = run_layout(&built.root, (1920.0, 1080.0), &ConversionContext::default());
        let l = layout
            .get(built.root.children[0].id)
            .expect("shape laid out");
        assert!(
            (l.width - 400.0).abs() < 1.0,
            "width should stay the author's explicit 400, got {}",
            l.width
        );
        assert!(
            (l.height - 225.0).abs() < 1.0,
            "height should derive from width/aspect-ratio (400/1.778=225), got {}",
            l.height
        );
    }

    #[test]
    fn neither_axis_set_with_aspect_ratio_derives_height_from_the_default_width() {
        // No width/height at all: the natural default width (80 for shape)
        // is kept, but height should come from the aspect-ratio, not the
        // unrelated 80px default.
        let scene = vec![make_aspect_shape_no_width(2.0)];
        let built = build_scene(&scene, (1920.0, 1080.0));
        let layout = run_layout(&built.root, (1920.0, 1080.0), &ConversionContext::default());
        let l = layout
            .get(built.root.children[0].id)
            .expect("shape laid out");
        assert!(
            (l.width - 80.0).abs() < 1.0,
            "width should keep the natural default (80), got {}",
            l.width
        );
        assert!(
            (l.height - 40.0).abs() < 1.0,
            "height should derive from the default width/aspect-ratio (80/2=40), got {}",
            l.height
        );
    }

    fn make_aspect_shape_no_width(aspect_ratio: f32) -> ChildComponent {
        ChildComponent {
            component: Component::Shape(crate::shape::Shape {
                shape: rustmotion_core::schema::ShapeType::Rect,
                text: None,
                timing: Default::default(),
                style: CssStyle {
                    aspect_ratio: Some(aspect_ratio),
                    ..Default::default()
                },
                timeline: Vec::new(),
                stagger: None,
                fill: None,
                stroke: None,
            }),
            position: Some(crate::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        }
    }
}
