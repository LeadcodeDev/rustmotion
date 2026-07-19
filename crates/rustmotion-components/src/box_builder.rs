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

use crate::divider::DividerDirection;
use crate::timeline::TimelineDirection;
use crate::{ChildComponent, Component};

/// Frame-level context passed into the builder so animations can be resolved
/// per-node and merged into the resulting `CssStyle`. When `None`, the box
/// tree is built without any animation overrides (resting state).
#[derive(Debug, Clone, Copy)]
pub struct BuildAnimationCtx {
    pub time: f64,
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
        // Apply timeline style states at the ghost time.
        if let Some(animatable) = child.component.as_animatable() {
            let steps = animatable.timeline_steps();
            if steps.iter().any(|s| s.style.is_some()) {
                let skip_opacity = css.transition.is_some();
                apply_style_states(&mut css, steps, ghost_time - stagger_delay, skip_opacity);
            }
        }
        // Resolve animation props at the *ghost* time.
        let ghost_actx = BuildAnimationCtx {
            time: ghost_time,
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
) -> Vec<BoxNode> {
    // Compute the local animation context for this node — remapped by the
    // accumulated affine time transform from ancestor containers.
    // `t_local = scale * t_global + shift`
    let local_actx = anim.map(|a| {
        let (scale, shift) = time_remap;
        BuildAnimationCtx {
            time: a.time * scale + shift,
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
        }
    }

    // Resolve animations and apply transform/opacity/filter overrides on the
    // box's CSS. Only paint-time properties (transform, opacity, filter,
    // perspective) flow into CSS — internal animations like draw_progress or
    // char_animation remain on the `AnimatedProperties` legacy path.
    if let Some(actx) = local_actx {
        if let Some(effects) = effective_effects(&child.component, stagger_delay) {
            let props = resolve_props_for_effects(&effects, actx.time, actx.scene_duration);
            if props_has_paint_overrides(&props) {
                apply_animated_props(&mut css, &props);
            }
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
                        analysis.amplitude_smoothed(actx.time, ar.smoothing_frames)
                    }
                    AudioSource::Band { band } => {
                        analysis.band_smoothed(actx.time, *band, ar.smoothing_frames)
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
        Badge(b) => Some(Arc::new(crate::intrinsic::BadgeIntrinsic::from_badge(b))),
        Terminal(t) => Some(Arc::new(
            crate::intrinsic::TerminalIntrinsic::from_terminal(t),
        )),
        Table(t) => Some(Arc::new(crate::intrinsic::TableIntrinsic::from_table(t))),
        Codeblock(c) => Some(Arc::new(
            crate::intrinsic::CodeblockIntrinsic::from_codeblock(c),
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

/// Apply per-component CSS overrides for things that the legacy
/// `Widget::measure` derived from constraints (e.g. divider stretching to its
/// parent, line bounding box from its endpoints).
fn apply_intrinsic_overrides(component: &Component, css: &mut CssStyle) {
    use Component::*;
    match component {
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
        _ => {}
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
            }),
            position: None,
            x: None,
            y: None,
            z_index: None,
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
        };
        let scene = vec![line];
        let built = build_scene(&scene, (800.0, 600.0));
        let layout = run_layout(&built.root, (800.0, 600.0), &ConversionContext::default());
        let id = built.root.children[0].id;
        let l = layout.get(id).expect("line laid out");
        assert_eq!(l.width, 100.0);
        assert_eq!(l.height, 60.0);
    }
}
