use std::sync::Arc;

use rustmotion_core::css::style::{AlignSelf, CssStyle, Position, Size as CSize};
use rustmotion_core::css::{apply_animated_props, LengthPercentage as CLP};
use rustmotion_core::engine::animator::{resolve_props_for_effects, AnimatedProperties};
use rustmotion_core::engine::box_tree::{BoxKind, BoxNode, NodeId};
use rustmotion_core::engine::deps::NodeRef;
use rustmotion_core::expr::Scope;
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

#[derive(Debug, Clone, Copy)]
pub struct BuildAnimationCtx {
    pub time: f64,
    pub scenario_time: f64,
    pub scene_duration: f64,
    pub fps: u32,
}

const ARROW_BBOX_PADDING: f32 = 16.0;

pub struct BuiltScene<'a> {
    pub root: BoxNode,
    pub components: Vec<Option<&'a ChildComponent>>,
    pub stagger_delays: Vec<f64>,
    pub time_params: Vec<(f64, f64)>,
    pub ghost_principal: Vec<(NodeId, NodeId)>,
}

pub fn build_scene<'a>(children: &'a [ChildComponent], viewport: (f32, f32)) -> BuiltScene<'a> {
    build_scene_with_root(children, viewport, default_root_css(viewport))
}

pub fn build_scene_with_root<'a>(
    children: &'a [ChildComponent],
    viewport: (f32, f32),
    root_css: CssStyle,
) -> BuiltScene<'a> {
    build_scene_from_refs(children.iter(), viewport, root_css, None)
}

pub fn build_scene_at_time<'a>(
    children: &'a [ChildComponent],
    viewport: (f32, f32),
    root_css: CssStyle,
    anim: BuildAnimationCtx,
) -> BuiltScene<'a> {
    build_scene_from_refs(children.iter(), viewport, root_css, Some(anim))
}

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

pub fn build_scene_from_refs<'a, I>(
    children: I,
    viewport: (f32, f32),
    root_css: CssStyle,
    anim: Option<BuildAnimationCtx>,
) -> BuiltScene<'a>
where
    I: IntoIterator<Item = &'a ChildComponent>,
{
    build_scene_from_refs_with_scope(children, viewport, root_css, anim, None)
}

pub fn build_scene_from_refs_with_scope<'a, I>(
    children: I,
    viewport: (f32, f32),
    root_css: CssStyle,
    anim: Option<BuildAnimationCtx>,
    outer_scope: Option<&dyn Scope>,
) -> BuiltScene<'a>
where
    I: IntoIterator<Item = &'a ChildComponent>,
{
    build_scene_from_refs_with_scope_quiet(children, viewport, root_css, anim, outer_scope, true)
}

pub fn build_scene_from_refs_with_scope_quiet<'a, I>(
    children: I,
    viewport: (f32, f32),
    mut root_css: CssStyle,
    anim: Option<BuildAnimationCtx>,
    outer_scope: Option<&dyn Scope>,
    warn_unresolved: bool,
) -> BuiltScene<'a>
where
    I: IntoIterator<Item = &'a ChildComponent>,
{
    let mut components: Vec<Option<&'a ChildComponent>> = vec![None];
    let mut stagger_delays: Vec<f64> = vec![0.0];
    let mut time_params: Vec<(f64, f64)> = vec![(1.0, 0.0)];
    let mut ghost_principal: Vec<(NodeId, NodeId)> = Vec::new();
    let mut next_id: NodeId = 1;

    let mut child_boxes = Vec::new();
    for (i, c) in children.into_iter().enumerate() {
        child_boxes.extend(build_child(
            c,
            &mut components,
            &mut stagger_delays,
            &mut time_params,
            &mut ghost_principal,
            &mut next_id,
            anim,
            format!("/children/{i}"),
            0.0,
            (1.0, 0.0),
            &root_css,
            viewport,
            outer_scope,
            warn_unresolved,
        ));
    }

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
        ghost_principal,
    }
}

pub fn collect_node_refs<'a, I>(children: I) -> Vec<(String, Vec<NodeRef>)>
where
    I: IntoIterator<Item = &'a ChildComponent>,
{
    let mut out = Vec::new();
    for child in children {
        collect_node_refs_into(child, &mut out);
    }
    out
}

fn collect_node_refs_into(child: &ChildComponent, out: &mut Vec<(String, Vec<NodeRef>)>) {
    if let Some(id) = &child.id {
        out.push((
            id.clone(),
            component_style(&child.component).expr.node_refs.clone(),
        ));
    }
    for c in container_children_of(&child.component) {
        collect_node_refs_into(c, out);
    }
}

fn container_children_of(component: &Component) -> &[ChildComponent] {
    match component {
        Component::Container(c) => &c.children,
        _ => &[],
    }
}

pub fn scene_uses_node_refs<'a, I>(children: I) -> bool
where
    I: IntoIterator<Item = &'a ChildComponent>,
{
    children.into_iter().any(scene_uses_node_refs_in)
}

fn scene_uses_node_refs_in(child: &ChildComponent) -> bool {
    if !component_style(&child.component).expr.node_refs.is_empty() {
        return true;
    }
    container_children_of(&child.component)
        .iter()
        .any(scene_uses_node_refs_in)
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

fn ghost_css_for(
    child: &ChildComponent,
    parent_css: &CssStyle,
    extra_delay: f64,
    scene_duration: f64,
    ghost_time: f64,
    ghost_opacity_scale: f32,
) -> CssStyle {
    let mut css = component_css(&child.component);
    css.position = Some(Position::Absolute);
    if let Some((x, y)) = child.absolute_position() {
        css.left = Some(CLP::Px(x));
        css.top = Some(CLP::Px(y));
    }
    if let Some(z) = child.z_index {
        css.z_index = Some(z);
    }
    rustmotion_core::css::cascade::inherit_from(parent_css, &mut css);
    if let Some(animatable) = child.component.as_animatable() {
        let steps = animatable.timeline_steps();
        if steps.iter().any(|s| s.style.is_some()) {
            let skip_opacity = css.transition.is_some();
            apply_style_states(&mut css, steps, ghost_time - extra_delay, skip_opacity);
            let overrides = resolve_transition_css_overrides(
                child.component.as_styled().style_config(),
                steps,
                ghost_time - extra_delay,
            );
            if let Some(br) = overrides.border_radius {
                css.border_radius = Some(br);
            }
            if let Some(bg) = overrides.background {
                css.background = Some(bg);
            }
        }
    }
    if let Some(ghost_effects) = effective_effects(&child.component, extra_delay, ghost_time) {
        let props = resolve_props_for_effects(&ghost_effects, ghost_time, scene_duration);
        apply_animated_props(&mut css, &props);
        apply_glow_effect(&mut css, &ghost_effects);
        carry_paint_pass_effects(&mut css, &ghost_effects);
        apply_directional_blur_props(&mut css, &props);
    }
    apply_pointer_path_transform(&mut css, &child.component, ghost_time);
    let base_opacity = css.opacity.unwrap_or(1.0);
    css.opacity = Some((base_opacity * ghost_opacity_scale).clamp(0.0, 1.0));
    css
}

#[allow(clippy::too_many_arguments)]
fn build_one_ghost<'a>(
    child: &'a ChildComponent,
    components: &mut Vec<Option<&'a ChildComponent>>,
    stagger_delays: &mut Vec<f64>,
    time_params: &mut Vec<(f64, f64)>,
    ghost_principal: &mut Vec<(NodeId, NodeId)>,
    next_id: &mut NodeId,
    anim: Option<BuildAnimationCtx>,
    actx: BuildAnimationCtx,
    stagger_delay: f64,
    extra_delay: f64,
    time_remap: (f64, f64),
    parent_css: &CssStyle,
    path: &str,
    viewport: (f32, f32),
    outer_scope: Option<&dyn Scope>,
    warn_unresolved: bool,
    offset: f64,
    ghost_opacity_scale: f32,
) -> BoxNode {
    let ghost_time = actx.time - offset;
    let ghost_time_remap = (time_remap.0, time_remap.1 - offset);
    let ghost_css = ghost_css_for(
        child,
        parent_css,
        extra_delay,
        actx.scene_duration,
        ghost_time,
        ghost_opacity_scale,
    );
    let ghost_intrinsic = component_intrinsic(&child.component, &ghost_css);

    let ghost_id = *next_id;
    *next_id += 1;
    components.push(Some(child));
    stagger_delays.push(extra_delay);
    time_params.push(ghost_time_remap);

    let ghost_children = container_children(
        &child.component,
        components,
        stagger_delays,
        time_params,
        ghost_principal,
        next_id,
        anim,
        path,
        stagger_delay,
        ghost_time_remap,
        &ghost_css,
        viewport,
        outer_scope,
        warn_unresolved,
    );

    BoxNode {
        id: ghost_id,
        kind: BoxKind::Ghost(Arc::new(ghost_id)),
        css: ghost_css,
        children: ghost_children,
        intrinsic: ghost_intrinsic,
        source_path: None,
        window: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn build_ghosts<'a>(
    child: &'a ChildComponent,
    components: &mut Vec<Option<&'a ChildComponent>>,
    stagger_delays: &mut Vec<f64>,
    time_params: &mut Vec<(f64, f64)>,
    ghost_principal: &mut Vec<(NodeId, NodeId)>,
    next_id: &mut NodeId,
    anim: Option<BuildAnimationCtx>,
    actx: BuildAnimationCtx,
    stagger_delay: f64,
    extra_delay: f64,
    time_remap: (f64, f64),
    effects: &[AnimationEffect],
    parent_css: &CssStyle,
    path: &str,
    viewport: (f32, f32),
    outer_scope: Option<&dyn Scope>,
    warn_unresolved: bool,
) -> Vec<BoxNode> {
    let (mb, tr) = detect_ghost_effects(effects);

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
        if mc.mode == rustmotion_core::schema::MotionBlurMode::Smear {
            return Vec::new();
        }
        let samples = mc.samples.clamp(1, 16);
        if samples <= 1 {
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

    let mut ghosts = Vec::new();

    match strategy {
        Strategy::MotionBlur {
            samples,
            shutter_window,
        } => {
            let ghost_opacity_scale = 1.0 / (samples + 1) as f32;
            for i in 1..=samples {
                let offset = i as f64 * shutter_window / samples as f64;
                ghosts.push(build_one_ghost(
                    child,
                    components,
                    stagger_delays,
                    time_params,
                    ghost_principal,
                    next_id,
                    anim,
                    actx,
                    stagger_delay,
                    extra_delay,
                    time_remap,
                    parent_css,
                    path,
                    viewport,
                    outer_scope,
                    warn_unresolved,
                    offset,
                    ghost_opacity_scale,
                ));
            }
        }
        Strategy::Trail {
            copies,
            spacing,
            falloff,
        } => {
            let mut trail_nodes = Vec::with_capacity(copies as usize);
            for i in 1..=copies {
                let offset = i as f64 * spacing;
                let ghost_opacity_scale = falloff.powi(i as i32);
                trail_nodes.push(build_one_ghost(
                    child,
                    components,
                    stagger_delays,
                    time_params,
                    ghost_principal,
                    next_id,
                    anim,
                    actx,
                    stagger_delay,
                    extra_delay,
                    time_remap,
                    parent_css,
                    path,
                    viewport,
                    outer_scope,
                    warn_unresolved,
                    offset,
                    ghost_opacity_scale,
                ));
            }
            trail_nodes.reverse();
            ghosts = trail_nodes;
        }
    }

    ghosts
}

#[allow(clippy::too_many_arguments)]
fn build_child<'a>(
    child: &'a ChildComponent,
    components: &mut Vec<Option<&'a ChildComponent>>,
    stagger_delays: &mut Vec<f64>,
    time_params: &mut Vec<(f64, f64)>,
    ghost_principal: &mut Vec<(NodeId, NodeId)>,
    next_id: &mut NodeId,
    anim: Option<BuildAnimationCtx>,
    path: String,
    stagger_delay: f64,
    time_remap: (f64, f64),
    parent_css: &CssStyle,
    viewport: (f32, f32),
    outer_scope: Option<&dyn Scope>,
    warn_unresolved: bool,
) -> Vec<BoxNode> {
    let local_actx = anim.map(|a| {
        let (scale, shift) = time_remap;
        BuildAnimationCtx {
            time: a.time * scale + shift,
            scenario_time: a.scenario_time,
            scene_duration: a.scene_duration,
            fps: a.fps,
        }
    });

    let anim_delay = stagger_delay
        + child
            .component
            .as_timed()
            .and_then(|t| t.timing().0)
            .unwrap_or(0.0);

    let mut ghosts: Vec<BoxNode> = Vec::new();
    if let Some(actx) = local_actx {
        if let Some(effects) = effective_effects(&child.component, anim_delay, actx.time) {
            ghosts = build_ghosts(
                child,
                components,
                stagger_delays,
                time_params,
                ghost_principal,
                next_id,
                anim,
                actx,
                stagger_delay,
                anim_delay,
                time_remap,
                &effects,
                parent_css,
                &path,
                viewport,
                outer_scope,
                warn_unresolved,
            );
        }
    }

    let id = *next_id;
    *next_id += 1;
    components.push(Some(child));
    stagger_delays.push(anim_delay);
    time_params.push(time_remap);
    for g in &ghosts {
        ghost_principal.push((g.id, id));
    }

    let mut css = component_css(&child.component);

    if let Some((x, y)) = child.absolute_position() {
        css.position = Some(Position::Absolute);
        css.left = Some(CLP::Px(x));
        css.top = Some(CLP::Px(y));
    }
    if let Some(z) = child.z_index {
        css.z_index = Some(z);
    }

    rustmotion_core::css::cascade::inherit_from(parent_css, &mut css);

    if let Some(animatable) = child.component.as_animatable() {
        let steps = animatable.timeline_steps();
        if steps.iter().any(|s| s.style.is_some()) {
            let t = local_actx.map(|a| a.time).unwrap_or(0.0);
            let skip_opacity = css.transition.is_some();
            apply_style_states(&mut css, steps, t - anim_delay, skip_opacity);
            let overrides = resolve_transition_css_overrides(
                child.component.as_styled().style_config(),
                steps,
                t - anim_delay,
            );
            if let Some(br) = overrides.border_radius {
                css.border_radius = Some(br);
            }
            if let Some(bg) = overrides.background {
                css.background = Some(bg);
            }
        }
    }

    if let Some(actx) = local_actx {
        if let Some(effects) = effective_effects(&child.component, anim_delay, actx.time) {
            let props = resolve_props_for_effects(&effects, actx.time, actx.scene_duration);
            apply_animated_props(&mut css, &props);
            apply_glow_effect(&mut css, &effects);
            carry_paint_pass_effects(&mut css, &effects);
            apply_directional_blur_props(&mut css, &props);
            apply_motion_blur_smear(
                &mut css,
                &effects,
                &child.component,
                anim_delay,
                actx.time,
                actx.scene_duration,
                actx.fps,
            );
        }
        apply_pointer_path_transform(&mut css, &child.component, actx.time);
        resolve_computed_style(
            &mut css,
            &path,
            actx,
            viewport,
            outer_scope,
            warn_unresolved,
        );
    }

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
                0.0
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
        ghost_principal,
        next_id,
        anim,
        &path,
        stagger_delay,
        time_remap,
        &css,
        viewport,
        outer_scope,
        warn_unresolved,
    );
    let intrinsic = component_intrinsic(&child.component, &css);

    let principal = BoxNode {
        id,
        kind: BoxKind::Component(Arc::new(id)),
        css,
        children: children_boxes,
        intrinsic,
        source_path: Some(path),
        window,
    };

    let mut result = ghosts;
    result.push(principal);
    result
}

fn resolve_computed_style(
    css: &mut CssStyle,
    path: &str,
    actx: BuildAnimationCtx,
    viewport: (f32, f32),
    outer_scope: Option<&dyn Scope>,
    warn_unresolved: bool,
) {
    if css.expr.is_empty() {
        return;
    }
    let clock = rustmotion_core::css::FrameClock {
        t: actx.time,
        t_abs: actx.scenario_time,
        duration: actx.scene_duration,
        width: viewport.0 as f64,
        height: viewport.1 as f64,
        fps: actx.fps as f64,
    };
    let scope = rustmotion_core::css::ComposedScope {
        clock,
        outer: outer_scope,
    };
    match css.expr.resolve(&scope) {
        Ok(props) => apply_animated_props(css, &props),
        Err(e) => {
            if warn_unresolved {
                eprintln!(
                    "warning: {path}: {e} — this property keeps its pre-expression value this frame"
                );
            }
        }
    }
}

pub fn effective_effects(
    component: &Component,
    extra_delay: f64,
    t: f64,
) -> Option<std::borrow::Cow<'_, [rustmotion_core::schema::AnimationEffect]>> {
    let animatable = component.as_animatable()?;
    let effects = animatable.animation_effects();
    let steps = animatable.timeline_steps();
    let smooth_color = matches!(component, Component::Text(_) | Component::Counter(_));
    let synthesized =
        transition_keyframes(component.as_styled().style_config(), steps, smooth_color);
    if steps.is_empty() && synthesized.is_empty() && extra_delay == 0.0 {
        return (!effects.is_empty()).then_some(std::borrow::Cow::Borrowed(effects));
    }
    let mut merged = effects.to_vec();
    for step in steps.iter().filter(|s| s.at <= t - extra_delay) {
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
    let saved_expr = css.expr.clone();
    if let Ok(mut merged) = serde_json::from_value::<CssStyle>(serde_json::Value::Object(base)) {
        merged.expr = merged.expr.prefer(saved_expr);
        *css = merged;
    }
}

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

pub(crate) struct TransitionCssOverrides {
    pub border_radius: Option<rustmotion_core::css::style::BorderRadius>,
    pub background: Option<rustmotion_core::css::style::Background>,
}

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

fn apply_pointer_path_transform(css: &mut CssStyle, component: &Component, time: f64) {
    use rustmotion_core::css::style::TransformFn;
    use rustmotion_core::css::units::LengthPercentage as CssLP;

    let Component::Pointer(p) = component else {
        return;
    };
    if p.path.is_empty() {
        return;
    }
    let (dx, dy) = crate::cursor::waypoint_offset(&p.path, time, p.click_duration, p.path_easing);
    css.transform
        .get_or_insert_with(Vec::new)
        .push(TransformFn::Translate {
            x: CssLP::Px(dx),
            y: CssLP::Px(dy),
        });
}

fn apply_directional_blur_props(css: &mut CssStyle, props: &AnimatedProperties) {
    use rustmotion_core::css::style::FilterFn;
    use rustmotion_core::css::units::Length;

    if props.blur_x <= 0.0 && props.blur_y <= 0.0 {
        return;
    }
    css.filter
        .get_or_insert_with(Vec::new)
        .push(FilterFn::Blur {
            radius: None,
            radius_x: (props.blur_x > 0.0).then_some(Length::Px(props.blur_x)),
            radius_y: (props.blur_y > 0.0).then_some(Length::Px(props.blur_y)),
        });
}

#[allow(clippy::too_many_arguments)]
fn apply_motion_blur_smear(
    css: &mut CssStyle,
    effects: &[rustmotion_core::schema::AnimationEffect],
    component: &Component,
    extra_delay: f64,
    t: f64,
    scene_duration: f64,
    fps: u32,
) {
    use rustmotion_core::css::style::FilterFn;
    use rustmotion_core::css::units::Length;
    use rustmotion_core::schema::AnimationEffect;
    use rustmotion_core::schema::MotionBlurMode;

    let Some(cfg) = effects.iter().find_map(|e| match e {
        AnimationEffect::MotionBlur(c) if c.mode == MotionBlurMode::Smear => Some(c),
        _ => None,
    }) else {
        return;
    };

    let shutter_window = (cfg.shutter / fps.max(1) as f64).max(1e-6);
    let sample = |at: f64| -> (f32, f32) {
        match effective_effects(component, extra_delay, at) {
            Some(e) => {
                let p = resolve_props_for_effects(&e, at, scene_duration);
                (p.translate_x, p.translate_y)
            }
            None => (0.0, 0.0),
        }
    };
    let (x0, y0) = sample(t - shutter_window);
    let (x1, y1) = sample(t);
    let dx = x1 - x0;
    let dy = y1 - y0;
    let magnitude = dx.hypot(dy);
    if magnitude < 0.5 {
        return;
    }
    let angle = dy.atan2(dx).to_degrees();
    css.filter
        .get_or_insert_with(Vec::new)
        .push(FilterFn::DirectionalBlur {
            angle,
            radius: Length::Px(magnitude),
        });
}

fn component_intrinsic(
    component: &Component,
    cascaded_css: &CssStyle,
) -> Option<Arc<dyn rustmotion_core::engine::box_tree::IntrinsicMeasure>> {
    let cascaded_component = component.with_cascaded_style(cascaded_css);
    let component = cascaded_component.as_ref().unwrap_or(component);

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
        Table(t) => Some(Arc::new(crate::intrinsic::TableIntrinsic::from_table(t))),
        RichText(rt) => Some(Arc::new(
            crate::intrinsic::RichTextIntrinsic::from_rich_text(rt),
        )),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn container_children<'a>(
    component: &'a Component,
    components: &mut Vec<Option<&'a ChildComponent>>,
    stagger_delays: &mut Vec<f64>,
    time_params: &mut Vec<(f64, f64)>,
    ghost_principal: &mut Vec<(NodeId, NodeId)>,
    next_id: &mut NodeId,
    anim: Option<BuildAnimationCtx>,
    parent_path: &str,
    inherited_delay: f64,
    time_remap: (f64, f64),
    parent_css: &CssStyle,
    viewport: (f32, f32),
    outer_scope: Option<&dyn Scope>,
    warn_unresolved: bool,
) -> Vec<BoxNode> {
    let (children, stagger, child_scale, child_offset): (&[ChildComponent], Option<f32>, f64, f64) =
        match component {
            Component::Container(c) => (
                &c.children,
                c.stagger,
                c.time_scale.unwrap_or(1.0),
                c.time_offset.unwrap_or(0.0),
            ),
            _ => return Vec::new(),
        };

    let child_scale = child_scale.max(1e-6);

    let (scale_acc, shift_acc) = time_remap;
    let new_scale = child_scale * scale_acc;
    let new_shift = child_scale * (shift_acc - child_offset);
    let child_remap = (new_scale, new_shift);

    let step = stagger.unwrap_or(0.0) as f64;
    let mut result = Vec::new();
    for (j, c) in children.iter().enumerate() {
        result.extend(build_child(
            c,
            components,
            stagger_delays,
            time_params,
            ghost_principal,
            next_id,
            anim,
            format!("{parent_path}/children/{j}"),
            inherited_delay + j as f64 * step,
            child_remap,
            parent_css,
            viewport,
            outer_scope,
            warn_unresolved,
        ));
    }
    result
}

fn component_css(component: &Component) -> CssStyle {
    let mut css = component_style(component).clone();
    apply_default_display(component, &mut css);
    apply_intrinsic_overrides(component, &mut css);
    css
}

fn apply_default_display(component: &Component, css: &mut CssStyle) {
    use rustmotion_core::css::style::Display;
    if css.display.is_some() {
        return;
    }
    if matches!(component, Component::Container(_)) {
        css.display = Some(if css.grid_template_columns.is_some() {
            Display::Grid
        } else {
            Display::Flex
        });
    }
}

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

fn apply_intrinsic_overrides(component: &Component, css: &mut CssStyle) {
    use Component::*;
    match component {
        Text(t) => {
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
            if css.width.is_none() {
                css.width = Some(CSize::Length(CLP::Px(cur.width)));
            }
            if css.height.is_none() {
                css.height = Some(CSize::Length(CLP::Px(cur.height)));
            }
        }
        SuccessCheck(c) => {
            apply_default_size(css, c.size, c.size);
        }
        Pointer(p) => {
            if css.width.is_none() {
                css.width = Some(CSize::Length(CLP::Px(p.size * 0.6)));
            }
            if css.height.is_none() {
                css.height = Some(CSize::Length(CLP::Px(p.size)));
            }
        }
        Particle(_) => {
            if css.width.is_none() {
                css.width = Some(CSize::Length(CLP::String("100%".into())));
            }
            if css.height.is_none() {
                css.height = Some(CSize::Length(CLP::String("100%".into())));
            }
        }
        Emitter(_) => {
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
                let font_size = c
                    .style
                    .font_size_px_ctx(&crate::intrinsic::measure_time_font_size_ctx(0.0), 16.0);
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

        Callout(t) => {
            let font_size = t
                .style
                .font_size_px_ctx(&crate::intrinsic::measure_time_font_size_ctx(0.0), 16.0);
            let family = t.style.font_family_or("Inter");
            let text_w = measure_text_line_width(&t.text, font_size, family, false);
            let h_pad = 12.0;
            let v_pad = 16.0;
            let line_h = font_size * 1.4;
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
            let font_size = t.style.font_size_px_ctx(
                &crate::intrinsic::measure_time_font_size_ctx(0.0),
                t.font_size,
            );
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
            let font_size = p
                .style
                .font_size_px_ctx(&crate::intrinsic::measure_time_font_size_ctx(0.0), 14.0);
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
            let font_size = m.style.font_size_px_ctx(
                &crate::intrinsic::measure_time_font_size_ctx(0.0),
                m.font_size,
            );
            apply_default_size(css, 800.0, font_size * 2.0);
        }
        Stepper(s) => {
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
            let rows = h.data.len();
            let cols = h.data.iter().map(|r| r.len()).max().unwrap_or(0);
            let step = h.cell_size + h.cell_gap;
            let w = (cols.max(1) as f32 - 1.0).max(0.0) * step + h.cell_size;
            let hh = (rows.max(1) as f32 - 1.0).max(0.0) * step + h.cell_size;
            apply_default_size(css, w, hh);
        }
        Sparkline(_) => {
            apply_default_size(css, 120.0, 40.0);
        }
        Stat(_) => {
            apply_default_size(css, 280.0, 180.0);
        }
        Gauge(g) => {
            const TARGET_RADIUS: f32 = 88.0;
            let size = 2.0 * (TARGET_RADIUS + g.track_width / 2.0 + 4.0);
            apply_default_size(css, size, size);
        }
        DotMap(_) => {
            apply_default_size(css, 640.0, 320.0);
        }
        Comparison(_) => {
            apply_default_size(css, 520.0, 280.0);
        }
        Treemap(_) => {
            apply_default_size(css, 416.0, 368.0);
        }
        Chart(c) => {
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
        Skeleton(s) => match s.variant {
            SkeletonVariant::Rectangle => apply_default_size(css, 400.0, 200.0),
            SkeletonVariant::Circle => apply_default_size(css, 64.0, 64.0),
            SkeletonVariant::Text => {
                let n = s.lines.max(1) as f32;
                let h = n * s.line_height + (n - 1.0).max(0.0) * s.line_gap;
                apply_default_size(css, 240.0, h);
            }
        },
        Mockup(m) => {
            let (dw, dh) = match m.device {
                MockupDevice::Iphone | MockupDevice::Android => (320.0, 690.0),
                MockupDevice::Laptop => (640.0, 400.0),
                MockupDevice::Browser => (640.0, 360.0),
            };
            apply_default_size(css, dw, dh);
        }
        Icon(_) => {
            apply_default_size(css, 64.0, 64.0);
        }
        Svg(_) => {
            apply_default_size(css, 200.0, 200.0);
        }
        Shape(_) => {
            apply_default_size(css, 80.0, 80.0);
        }
        Image(_) => {
            apply_default_size(css, 400.0, 300.0);
        }
        Video(_) | Gif(_) => {
            apply_default_size(css, 400.0, 225.0);
        }
        Lottie(_) => {
            apply_default_size(css, 300.0, 300.0);
        }

        _ => {}
    }
}

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

fn fixed_px(size: Option<&CSize>) -> Option<f32> {
    match size? {
        CSize::Length(lp) => match lp.try_parse()? {
            rustmotion_core::css::units::ParsedLength::Px(v) => Some(v),
            _ => None,
        },
        _ => None,
    }
}

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
        Emitter(c) => &c.style,
        Gauge(c) => &c.style,
        GradientText(c) => &c.style,
        Heatmap(c) => &c.style,
        Kbd(c) => &c.style,
        Line(c) => &c.style,
        List(c) => &c.style,
        Lottie(c) => &c.style,
        Marquee(c) => &c.style,
        Mockup(c) => &c.style,
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
        Timeline(c) => &c.style,
        Tooltip(c) => &c.style,
        Treemap(c) => &c.style,
        Container(c) => &c.style,
        AudioSpectrum(c) => &c.style,
        Waveform(c) => &c.style,
    }
}

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
        Emitter(_) => "emitter",
        Gauge(_) => "gauge",
        GradientText(_) => "gradient_text",
        Heatmap(_) => "heatmap",
        Kbd(_) => "kbd",
        Line(_) => "line",
        List(_) => "list",
        Lottie(_) => "lottie",
        Marquee(_) => "marquee",
        Mockup(_) => "mockup",
        Particle(_) => "particle",
        PillNav(_) => "pill_nav",
        Progress(_) => "progress",
        QrCode(_) => "qr_code",
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
        Timeline(_) => "timeline",
        Tooltip(_) => "tooltip",
        Treemap(_) => "treemap",
        Container(_) => "div",
        AudioSpectrum(_) => "audio_spectrum",
        Waveform(_) => "waveform",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_timeline_step_leaves_the_value_alone_until_its_at() {
        let component: Component = serde_json::from_value(json!({
            "type": "shape",
            "shape": "circle",
            "fill": "#1EA2C2",
            "style": { "width": 120, "height": 120 },
            "timeline": [
                { "at": 3.0, "animation": [{ "name": "keyframes", "duration": 1.0, "keyframes": [
                    { "property": "translate_x", "easing": "linear", "keyframes": [
                        { "time": 0.0, "value": 0.0 }, { "time": 1.0, "value": 200.0 }] }] }] },
                { "at": 6.0, "animation": [{ "name": "keyframes", "duration": 1.0, "keyframes": [
                    { "property": "translate_x", "easing": "linear", "keyframes": [
                        { "time": 0.0, "value": 200.0 }, { "time": 1.0, "value": 0.0 }] }] }] }
            ]
        }))
        .expect("component deserializes");

        let tx = |t: f64| match effective_effects(&component, 0.0, t) {
            Some(effects) => resolve_props_for_effects(&effects, t, 9.0).translate_x as f64,
            None => AnimatedProperties::default().translate_x as f64,
        };

        assert!(
            tx(0.5).abs() < 1.0,
            "before either step, translate_x is 0, got {}",
            tx(0.5)
        );
        assert!(
            (tx(3.5) - 100.0).abs() < 2.0,
            "halfway through step one, got {}",
            tx(3.5)
        );
        assert!(
            (tx(5.0) - 200.0).abs() < 1.0,
            "step one has ended and holds, got {}",
            tx(5.0)
        );
        assert!(
            (tx(6.5) - 100.0).abs() < 2.0,
            "halfway through step two, got {}",
            tx(6.5)
        );
        assert!(
            tx(8.0).abs() < 1.0,
            "step two has ended and holds, got {}",
            tx(8.0)
        );
    }

    #[test]
    fn start_at_rebases_the_entrance_animation_clock() {
        let scene = vec![ChildComponent {
            id: None,
            component: serde_json::from_value(json!({
                "type": "shape",
                "shape": "rect",
                "fill": "#1EA2C2",
                "start_at": 2.0,
                "style": {
                    "width": 120, "height": 120,
                    "animation": [{ "name": "fade_in_down", "duration": 0.6 }]
                }
            }))
            .expect("component deserializes"),
            position: None,
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        }];

        let opacity_at = |t: f64| -> f32 {
            let built = build_scene_at_time(
                &scene,
                (400.0, 400.0),
                default_root_css((400.0, 400.0)),
                BuildAnimationCtx {
                    time: t,
                    scenario_time: t,
                    scene_duration: 6.0,
                    fps: 30,
                },
            );
            built.root.children[0].css.opacity.unwrap_or(1.0)
        };

        assert!(
            opacity_at(2.0) < 0.3,
            "at start_at (2.0) the fade_in_down entrance should just be beginning, got opacity {}",
            opacity_at(2.0)
        );
        assert!(
            opacity_at(2.6) > 0.9,
            "0.6s after start_at (the entrance's own duration) it should have finished, got opacity {}",
            opacity_at(2.6)
        );
    }

    #[test]
    fn start_at_rebases_an_exit_animation_declared_after_the_entrance() {
        let scene = vec![ChildComponent {
            id: None,
            component: serde_json::from_value(json!({
                "type": "shape",
                "shape": "rect",
                "fill": "#1EA2C2",
                "start_at": 2.0,
                "style": {
                    "width": 120, "height": 120,
                    "animation": [
                        { "name": "fade_in_down", "duration": 0.6 },
                        { "name": "fade_out_up", "delay": 0.85, "duration": 0.3 }
                    ]
                }
            }))
            .expect("component deserializes"),
            position: None,
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        }];

        let opacity_at = |t: f64| -> f32 {
            let built = build_scene_at_time(
                &scene,
                (400.0, 400.0),
                default_root_css((400.0, 400.0)),
                BuildAnimationCtx {
                    time: t,
                    scenario_time: t,
                    scene_duration: 6.0,
                    fps: 30,
                },
            );
            built.root.children[0].css.opacity.unwrap_or(1.0)
        };

        assert!(
            opacity_at(2.5) > 0.5,
            "the exit's own delay (0.85s) has not elapsed since start_at (2.0), the node should \
             still be visible, got opacity {}",
            opacity_at(2.5)
        );
    }

    use rustmotion_core::css::style::{
        CssStyle, Display, Edges, FlexDirection, Gap, Size as CSize,
    };
    use rustmotion_core::css::taffy_bridge::ConversionContext;
    use rustmotion_core::css::units::LengthPercentage;
    use rustmotion_core::css::units::LengthPercentage as CLP;
    use rustmotion_core::engine::layout_pass::run_layout;
    use serde_json::json;

    fn make_card(children: Vec<ChildComponent>, style: CssStyle) -> Component {
        Component::Container(crate::container::ContainerComponent {
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
            id: None,
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
                draw_start: None,
                path_morph: None,
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
            id: None,
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
                morph: None,
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
        assert_eq!(built.components.len(), 1);
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
            id: None,
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
            id: None,
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
        assert_eq!(c1.x, 20.0);
        assert_eq!(c1.y, 20.0);
        assert_eq!(c2.x, 20.0);
        assert_eq!(c2.y, 80.0);
    }

    #[test]
    fn absolute_child_uses_top_left() {
        let scene = vec![ChildComponent {
            id: None,
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
                draw_start: None,
                path_morph: None,
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
            id: None,
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
        use crate::container::ContainerComponent;
        use crate::text::Text;

        use rustmotion_core::css::units::Length;

        let text = ChildComponent {
            id: None,
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
                morph: None,
            }),
            position: None,
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        };

        let card = ChildComponent {
            id: None,
            component: Component::Container(ContainerComponent {
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
            id: None,
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
        assert_eq!(l.width, 128.0);
        assert_eq!(l.height, 88.0);
    }

    #[test]
    fn connector_intrinsic_size_uses_endpoint_bbox_plus_arrowhead() {
        let conn = ChildComponent {
            id: None,
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
        assert_eq!(l.width, 126.0);
        assert_eq!(l.height, 76.0);
    }

    #[test]
    fn counter_intrinsic_size_reserves_space_for_max_value() {
        use crate::counter::Counter;

        use rustmotion_core::css::units::Length;
        let counter = ChildComponent {
            id: None,
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
        assert!(
            l.height >= 60.0,
            "counter height should be ≥ ~one line ({}), got {}",
            64.0,
            l.height
        );
    }

    #[test]
    fn badge_intrinsic_size_includes_padding_and_text() {
        use crate::badge::{Badge, BadgeSize, BadgeVariant};

        let badge = ChildComponent {
            id: None,
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
            id: None,
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

    #[test]
    fn rich_text_child_gets_a_non_zero_intrinsic_size() {
        use crate::rich_text::{RichText, RichTextSpan};
        use rustmotion_core::css::units::Length;

        let rich_text = ChildComponent {
            id: None,
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
                        background: None,
                        padding: None,
                        border_radius: None,
                        rotation: None,
                    },
                    RichTextSpan {
                        text: "40%".into(),
                        color: Some("#5C39EE".into()),
                        font_size: None,
                        font_weight: None,
                        font_family: None,
                        font_style: None,
                        letter_spacing: None,
                        background: None,
                        padding: None,
                        border_radius: None,
                        rotation: None,
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
                color: "#FFFFFFFF".to_string(),
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

    fn child_from_json(json: serde_json::Value) -> ChildComponent {
        let component: Component =
            serde_json::from_value(json.clone()).unwrap_or_else(|e| panic!("{e}\n{json:#}"));
        ChildComponent {
            id: None,
            component,
            position: None,
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        }
    }

    fn layout_in_auto_card(child_json: serde_json::Value) -> (f32, f32) {
        let card = make_card(vec![child_from_json(child_json)], CssStyle::default());
        let scene = vec![ChildComponent {
            id: None,
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
        let (w, h) =
            layout_in_auto_card(json!({"type":"heatmap","data":[[1.0,2.0,3.0],[4.0,5.0,6.0]]}));
        assert_eq!(w, 48.0);
        assert_eq!(h, 31.0);
    }

    #[test]
    fn sparkline_gets_the_documented_120x40_default() {
        let (w, h) = layout_in_auto_card(json!({"type":"sparkline","data":[1.0,2.0,3.0]}));
        assert_eq!(w, 120.0);
        assert_eq!(h, 40.0);
    }

    #[test]
    fn stat_gets_the_documented_280x180_default() {
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
        let (_, h) = layout_in_auto_card(json!({"type":"pill_nav","items":["Overview"]}));
        assert_eq!(h, 44.0);
    }

    #[test]
    fn callout_width_grows_with_its_own_text_content() {
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
            id: None,
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
            id: None,
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
            id: None,
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
        let card = make_card(
            vec![make_text("hello", CssStyle::default())],
            CssStyle {
                display: Some(Display::Flex),
                ..Default::default()
            },
        );
        let scene = vec![ChildComponent {
            id: None,
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

    fn make_aspect_shape(width: f32, aspect_ratio: f32) -> ChildComponent {
        ChildComponent {
            id: None,
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
                draw_start: None,
                path_morph: None,
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
            id: None,
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
                draw_start: None,
                path_morph: None,
            }),
            position: Some(crate::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        }
    }

    use crate::legacy_dispatch::LegacyPaintDispatcher;
    use rustmotion_core::css::style::{AlignItems, JustifyContent};
    use rustmotion_core::engine::paint_pass::{paint_tree, PaintFrame};

    #[test]
    fn ghost_does_not_steal_a_flex_slot() {
        let root_css = CssStyle {
            display: Some(Display::Flex),
            flex_direction: Some(FlexDirection::Column),
            align_items: Some(AlignItems::Center),
            justify_content: Some(JustifyContent::Center),
            gap: Some(Gap::Uniform(CLP::Px(20.0))),
            width: Some(CSize::Length(CLP::Px(640.0))),
            height: Some(CSize::Length(CLP::Px(360.0))),
            ..Default::default()
        };
        let scene = vec![
            child_from_json(json!({
                "type": "shape", "shape": "rect", "fill": "#FF4FB0",
                "style": { "width": 200, "height": 60, "animation": [{ "name": "motion_blur" }] }
            })),
            child_from_json(json!({
                "type": "shape", "shape": "rect", "fill": "#4B5BFF",
                "style": { "width": 200, "height": 60 }
            })),
        ];
        let anim = BuildAnimationCtx {
            time: 10.0 / 30.0,
            scenario_time: 10.0 / 30.0,
            scene_duration: 1.0,
            fps: 30,
        };
        let built = build_scene_at_time(&scene, (640.0, 360.0), root_css, anim);
        let layout = run_layout(&built.root, (640.0, 360.0), &ConversionContext::default());

        let principals: Vec<_> = built
            .root
            .children
            .iter()
            .filter(|n| matches!(n.kind, BoxKind::Component(_)))
            .collect();
        assert_eq!(
            principals.len(),
            2,
            "exactly the two real elements; ghosts must not be counted among them"
        );

        let first = layout.get(principals[0].id).expect("first box laid out");
        let second = layout.get(principals[1].id).expect("second box laid out");

        assert!(
            (first.y - 110.0).abs() < 1.0,
            "the motion-blurred box itself must sit at the flex-resolved position (110), got {}",
            first.y
        );
        assert!(
            (second.y - 190.0).abs() < 1.0,
            "a motion-blurred sibling must not push the next flex item down (expected 190, \
             regression of #66), got {}",
            second.y
        );
    }

    #[test]
    fn ghost_of_a_container_carries_its_nested_child() {
        let container_component: Component = serde_json::from_value(json!({
            "type": "div",
            "style": {
                "width": 200, "height": 100,
                "background": "#FF00FF",
                "animation": [{ "name": "trail", "copies": 1, "spacing": 0.05, "falloff": 1.0 }]
            },
            "children": [{
                "type": "shape", "shape": "rect", "fill": "#00FF00",
                "position": "absolute", "x": 20, "y": 20,
                "style": { "width": 40, "height": 40 }
            }]
        }))
        .expect("container deserializes");
        let scene = vec![ChildComponent {
            id: None,
            component: container_component,
            position: Some(crate::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
            bleed: false,
        }];

        let frame_time = 0.2;
        let anim = BuildAnimationCtx {
            time: frame_time,
            scenario_time: frame_time,
            scene_duration: 1.0,
            fps: 30,
        };
        let mut built = build_scene_at_time(
            &scene,
            (200.0, 100.0),
            default_root_css((200.0, 100.0)),
            anim,
        );

        let ghost_index = built
            .root
            .children
            .iter()
            .position(|n| matches!(n.kind, BoxKind::Ghost(_)))
            .expect("a ghost was built for the trail effect");
        let ghost = built.root.children.remove(ghost_index);
        assert_eq!(
            ghost.children.len(),
            1,
            "the ghost of a container must carry the container's own children, not an empty subtree"
        );

        let mini_root = BoxNode::container(default_root_css((200.0, 100.0)), vec![ghost]);
        let layout = run_layout(&mini_root, (200.0, 100.0), &ConversionContext::default());

        let mut surface =
            skia_safe::surfaces::raster_n32_premul((200, 100)).expect("raster surface");
        let canvas = surface.canvas();
        let dispatcher = LegacyPaintDispatcher::for_scene(&built);
        let frame = PaintFrame {
            light: Default::default(),
            time: frame_time,
            scenario_time: frame_time,
            frame_index: 6,
            fps: 30,
            video_width: 200,
            video_height: 100,
            scene_duration: 1.0,
            camera: None,
        };
        paint_tree(canvas, &mini_root, &layout, &frame, &dispatcher);

        let snapshot = surface.image_snapshot();
        let info = skia_safe::ImageInfo::new(
            (1, 1),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut buf = [0u8; 4];
        assert!(snapshot.read_pixels(
            &info,
            &mut buf,
            4,
            skia_safe::IPoint::new(40, 40),
            skia_safe::image::CachingHint::Disallow,
        ));
        assert!(
            buf[1] > 200 && buf[0] < 80 && buf[2] < 80,
            "the ghost must paint the nested green shape, not just the container's own \
             background; got {:?}",
            buf
        );
    }

    #[test]
    fn ghost_of_a_measured_leaf_keeps_its_intrinsic_size() {
        let scene = vec![child_from_json(json!({
            "type": "text",
            "content": "TEXT",
            "position": "absolute", "x": 100, "y": 40,
            "style": {
                "font-size": 96, "color": "#FFFFFF",
                "animation": [
                    { "name": "keyframes", "keyframes": [{ "property": "translate_x", "easing": "linear", "keyframes": [
                        { "time": 0, "value": 1200 }, { "time": 1, "value": -1200 }] }] },
                    { "name": "motion_blur", "samples": 4, "shutter": 1.0 }
                ]
            }
        }))];
        let anim = BuildAnimationCtx {
            time: 0.5,
            scenario_time: 0.5,
            scene_duration: 1.0,
            fps: 30,
        };
        let built = build_scene_at_time(
            &scene,
            (1280.0, 360.0),
            default_root_css((1280.0, 360.0)),
            anim,
        );
        let layout = run_layout(&built.root, (1280.0, 360.0), &ConversionContext::default());

        let ghosts: Vec<_> = built
            .root
            .children
            .iter()
            .filter(|n| matches!(n.kind, BoxKind::Ghost(_)))
            .collect();
        assert!(
            !ghosts.is_empty(),
            "expected ghosts for a motion-blurred text node"
        );
        for g in &ghosts {
            assert!(
                g.intrinsic.is_some(),
                "a ghost of a measured component (text) needs its own intrinsic to size itself"
            );
            let l = layout.get(g.id).expect("ghost laid out");
            assert!(
                l.width > 0.0 && l.height > 0.0,
                "text ghost collapsed to a zero-size box, got {}x{}",
                l.width,
                l.height
            );
        }
    }

    #[test]
    fn ghost_of_a_path_driven_pointer_samples_a_different_waypoint_offset() {
        let scene = vec![child_from_json(json!({
            "type": "pointer",
            "size": 220, "tone": "dark",
            "path": [
                { "time": 0.0, "x": 1900, "y": 1000 },
                { "time": 0.6, "x": 900, "y": 500 }
            ],
            "path_easing": "linear",
            "style": { "animation": [{ "name": "motion_blur", "samples": 4, "shutter": 3.0 }] }
        }))];
        let frame_time = 0.5;
        let anim = BuildAnimationCtx {
            time: frame_time,
            scenario_time: frame_time,
            scene_duration: 1.0,
            fps: 30,
        };
        let built = build_scene_at_time(
            &scene,
            (1920.0, 1080.0),
            default_root_css((1920.0, 1080.0)),
            anim,
        );

        let (path, click_duration, path_easing) = match &scene[0].component {
            Component::Pointer(p) => (p.path.clone(), p.click_duration, p.path_easing),
            _ => panic!("expected a pointer component"),
        };

        let ghost_ids: Vec<NodeId> = built
            .root
            .children
            .iter()
            .filter(|n| matches!(n.kind, BoxKind::Ghost(_)))
            .map(|n| n.id)
            .collect();
        assert!(ghost_ids.len() >= 2, "expected several ghosts");

        let offsets: Vec<(f32, f32)> = ghost_ids
            .iter()
            .map(|&id| {
                let (scale, shift) = built.time_params[id as usize];
                let local_time = frame_time * scale + shift;
                crate::cursor::waypoint_offset(&path, local_time, click_duration, path_easing)
            })
            .collect();

        let distinct = offsets
            .windows(2)
            .any(|w| (w[0].0 - w[1].0).abs() > 1.0 || (w[0].1 - w[1].1).abs() > 1.0);
        assert!(
            distinct,
            "ghosts of a path-driven pointer must sample different points along the path, got {:?}",
            offsets
        );

        let live_offset =
            crate::cursor::waypoint_offset(&path, frame_time, click_duration, path_easing);
        assert!(
            offsets
                .iter()
                .all(|o| (o.0 - live_offset.0).abs() > 1.0 || (o.1 - live_offset.1).abs() > 1.0),
            "every ghost sampled the live pointer position instead of its own instant; \
             got {:?} vs live {:?}",
            offsets,
            live_offset
        );
    }

    #[test]
    fn a_pointer_moved_by_path_actually_paints_ink_at_a_ghosts_own_position() {
        let scene = vec![child_from_json(json!({
            "type": "pointer",
            "size": 220, "tone": "dark",
            "path": [
                { "time": 0.0, "x": 1900, "y": 1000 },
                { "time": 0.6, "x": 900, "y": 500 }
            ],
            "path_easing": "linear",
            "style": { "animation": [{ "name": "motion_blur", "samples": 12, "shutter": 3.0 }] }
        }))];
        let frame_time = 0.5;
        let anim = BuildAnimationCtx {
            time: frame_time,
            scenario_time: frame_time,
            scene_duration: 1.0,
            fps: 30,
        };
        let built = build_scene_at_time(
            &scene,
            (1920.0, 1080.0),
            default_root_css((1920.0, 1080.0)),
            anim,
        );
        let layout = run_layout(&built.root, (1920.0, 1080.0), &ConversionContext::default());
        let mut surface = paint_scene_at(&built, &layout, 1920, 1080, frame_time);

        let (path, click_duration, path_easing) = match &scene[0].component {
            Component::Pointer(p) => (p.path.clone(), p.click_duration, p.path_easing),
            _ => panic!("expected a pointer component"),
        };
        let (fx, fy) = (0.1946_f32, 0.4390_f32);
        let size = 220.0_f32;
        let probe_for = |offset: (f32, f32)| {
            (
                (offset.0 + fx * size).round() as i32,
                (offset.1 + fy * size).round() as i32,
            )
        };

        let live_offset =
            crate::cursor::waypoint_offset(&path, frame_time, click_duration, path_easing);
        let live_probe = probe_for(live_offset);
        let live_alpha = read_pixel(&mut surface, live_probe.0, live_probe.1)[3];
        assert!(
            live_alpha > 200,
            "sanity check: the live, un-ghosted pointer must paint its own glyph at the \
             current path position; probed {:?}, alpha={live_alpha}",
            live_probe
        );

        let ghost_id = built
            .root
            .children
            .iter()
            .filter(|n| matches!(n.kind, BoxKind::Ghost(_)))
            .map(|n| n.id)
            .min_by(|&a, &b| {
                let ta = built.time_params[a as usize];
                let tb = built.time_params[b as usize];
                (frame_time * ta.0 + ta.1)
                    .partial_cmp(&(frame_time * tb.0 + tb.1))
                    .unwrap()
            })
            .expect("expected at least one ghost");
        let (scale, shift) = built.time_params[ghost_id as usize];
        let farthest_ghost_time = frame_time * scale + shift;
        let ghost_offset =
            crate::cursor::waypoint_offset(&path, farthest_ghost_time, click_duration, path_easing);
        let ghost_probe = probe_for(ghost_offset);
        let ghost_alpha = read_pixel(&mut surface, ghost_probe.0, ghost_probe.1)[3];
        assert!(
            ghost_alpha > 20,
            "a pointer moved by `path` must be ghosted along its trajectory just like one \
             moved by translate_x/y keyframes — the furthest motion_blur ghost (its own local \
             time {farthest_ghost_time}, opacity 1/13 of the live pointer) must paint ink at \
             its own point along the path ({ghost_offset:?}, live is {live_offset:?}), not \
             stay pixel-identical (fully transparent there) to the same pointer without \
             motion_blur; probed {:?}, alpha={ghost_alpha}",
            ghost_probe
        );

        let empty_alpha = read_pixel(&mut surface, 100, 100)[3];
        assert_eq!(
            empty_alpha, 0,
            "sanity check: a point far from every sampled instant of the path must stay \
             untouched background"
        );
    }

    #[test]
    fn animated_blur_x_becomes_a_directional_blur_filter() {
        use rustmotion_core::css::style::FilterFn;
        use rustmotion_core::css::units::Length;

        let scene = vec![child_from_json(json!({
            "type": "shape", "shape": "rect", "fill": "#FFFFFF",
            "style": {
                "width": 100, "height": 100,
                "animation": [{ "name": "keyframes", "duration": 1.0, "keyframes": [
                    { "property": "blur_x", "easing": "linear", "keyframes": [
                        { "time": 0.0, "value": 0.0 }, { "time": 1.0, "value": 40.0 }
                    ] }
                ] }]
            }
        }))];
        let anim = BuildAnimationCtx {
            time: 0.5,
            scenario_time: 0.5,
            scene_duration: 1.0,
            fps: 30,
        };
        let built = build_scene_at_time(
            &scene,
            (400.0, 400.0),
            default_root_css((400.0, 400.0)),
            anim,
        );

        let filters = built.root.children[0]
            .css
            .filter
            .clone()
            .expect("a blur filter was applied");
        let blur = filters
            .iter()
            .find(|f| matches!(f, FilterFn::Blur { .. }))
            .expect("a Blur filter");
        match blur {
            FilterFn::Blur {
                radius,
                radius_x,
                radius_y,
            } => {
                assert!(radius.is_none(), "isotropic radius must stay unset");
                assert!(
                    matches!(radius_x, Some(Length::Px(v)) if (*v - 20.0).abs() < 1.0),
                    "expected ~20px halfway through a 0->40 linear blur_x ramp, got {:?}",
                    radius_x
                );
                assert!(
                    radius_y.is_none(),
                    "blur_y was never animated, must stay unset"
                );
            }
            other => panic!("expected Blur, got {:?}", other),
        }
    }

    #[test]
    fn motion_blur_smear_mode_produces_no_ghosts_and_a_directional_filter_instead() {
        use rustmotion_core::css::style::FilterFn;
        use rustmotion_core::css::units::Length;

        let scene = vec![child_from_json(json!({
            "type": "shape", "shape": "rect", "fill": "#FFFFFF",
            "position": "absolute", "x": 0, "y": 0,
            "style": {
                "width": 100, "height": 60,
                "animation": [
                    { "name": "keyframes", "duration": 1.0, "keyframes": [
                        { "property": "translate_x", "easing": "linear", "keyframes": [
                            { "time": 0.0, "value": 0.0 }, { "time": 1.0, "value": 900.0 }
                        ] }
                    ] },
                    { "name": "motion_blur", "mode": "smear", "shutter": 1.0 }
                ]
            }
        }))];
        let anim = BuildAnimationCtx {
            time: 0.5,
            scenario_time: 0.5,
            scene_duration: 1.0,
            fps: 30,
        };
        let built = build_scene_at_time(
            &scene,
            (1920.0, 1080.0),
            default_root_css((1920.0, 1080.0)),
            anim,
        );

        assert_eq!(
            built.root.children.len(),
            1,
            "smear mode must not create ghost nodes"
        );
        assert!(matches!(built.root.children[0].kind, BoxKind::Component(_)));

        let filters = built.root.children[0]
            .css
            .filter
            .clone()
            .expect("a smear filter was applied");
        let blur = filters
            .iter()
            .find(|f| matches!(f, FilterFn::DirectionalBlur { .. }))
            .expect("a DirectionalBlur filter, not an axis-aligned Blur");
        match blur {
            FilterFn::DirectionalBlur { angle, radius } => {
                assert!(
                    angle.abs() < 1.0,
                    "a pure horizontal move must orient the streak along 0 degrees, got {angle}"
                );
                assert!(
                    matches!(radius, Length::Px(v) if (*v - 30.0).abs() < 3.0),
                    "900px/s over a 1/30s shutter window is a ~30px streak, got {:?}",
                    radius
                );
            }
            other => panic!("expected DirectionalBlur, got {:?}", other),
        }
    }

    #[test]
    fn a_diagonal_smear_orients_the_directional_blur_along_the_actual_velocity() {
        use rustmotion_core::css::style::FilterFn;

        let scene = vec![child_from_json(json!({
            "type": "shape", "shape": "rect", "fill": "#FFFFFF",
            "position": "absolute", "x": 0, "y": 0,
            "style": {
                "width": 100, "height": 60,
                "animation": [
                    { "name": "keyframes", "duration": 1.0, "keyframes": [
                        { "property": "translate_x", "easing": "linear", "keyframes": [
                            { "time": 0.0, "value": 0.0 }, { "time": 1.0, "value": 900.0 }
                        ] },
                        { "property": "translate_y", "easing": "linear", "keyframes": [
                            { "time": 0.0, "value": 0.0 }, { "time": 1.0, "value": 900.0 }
                        ] }
                    ] },
                    { "name": "motion_blur", "mode": "smear", "shutter": 1.0 }
                ]
            }
        }))];
        let anim = BuildAnimationCtx {
            time: 0.5,
            scenario_time: 0.5,
            scene_duration: 1.0,
            fps: 30,
        };
        let built = build_scene_at_time(
            &scene,
            (1920.0, 1080.0),
            default_root_css((1920.0, 1080.0)),
            anim,
        );

        let filters = built.root.children[0]
            .css
            .filter
            .clone()
            .expect("a smear filter was applied");
        let blur = filters
            .iter()
            .find(|f| matches!(f, FilterFn::DirectionalBlur { .. }))
            .expect("a DirectionalBlur filter");
        match blur {
            FilterFn::DirectionalBlur { angle, .. } => {
                assert!(
                    (angle - 45.0).abs() < 1.0,
                    "equal x and y velocity must orient the streak at 45 degrees, not the \
                     0/90 axes a box blur would be limited to, got {angle}"
                );
            }
            other => panic!("expected DirectionalBlur, got {:?}", other),
        }
    }

    fn read_pixel(surface: &mut skia_safe::Surface, x: i32, y: i32) -> [u8; 4] {
        let snapshot = surface.image_snapshot();
        let info = skia_safe::ImageInfo::new(
            (1, 1),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut buf = [0u8; 4];
        assert!(
            snapshot.read_pixels(
                &info,
                &mut buf,
                4,
                skia_safe::IPoint::new(x, y),
                skia_safe::image::CachingHint::Disallow,
            ),
            "pixel read should succeed"
        );
        buf
    }

    fn ink_extent_x(
        surface: &mut skia_safe::Surface,
        width: i32,
        height: i32,
        y0: i32,
        y1: i32,
    ) -> Option<(i32, i32)> {
        let snapshot = surface.image_snapshot();
        let info = skia_safe::ImageInfo::new(
            (width, height),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut buf = vec![0u8; (width * height * 4) as usize];
        assert!(
            snapshot.read_pixels(
                &info,
                &mut buf,
                (width * 4) as usize,
                skia_safe::IPoint::new(0, 0),
                skia_safe::image::CachingHint::Disallow,
            ),
            "pixel read should succeed"
        );
        let mut min_x: Option<i32> = None;
        let mut max_x: Option<i32> = None;
        for y in y0.max(0)..y1.min(height) {
            for x in 0..width {
                let alpha = buf[((y * width + x) * 4 + 3) as usize];
                if alpha > 0 {
                    min_x = Some(min_x.map_or(x, |m| m.min(x)));
                    max_x = Some(max_x.map_or(x, |m| m.max(x)));
                }
            }
        }
        min_x.zip(max_x)
    }

    fn paint_scene_at(
        built: &BuiltScene,
        layout: &rustmotion_core::engine::layout_pass::LayoutResult,
        width: i32,
        height: i32,
        time: f64,
    ) -> skia_safe::Surface {
        let mut surface =
            skia_safe::surfaces::raster_n32_premul((width, height)).expect("raster surface");
        let canvas = surface.canvas();
        let dispatcher = LegacyPaintDispatcher::for_scene(built);
        let frame = PaintFrame {
            light: Default::default(),
            time,
            scenario_time: time,
            frame_index: (time * 30.0).round() as u32,
            fps: 30,
            video_width: width as u32,
            video_height: height as u32,
            scene_duration: 1.0,
            camera: None,
        };
        paint_tree(canvas, &built.root, layout, &frame, &dispatcher);
        surface
    }

    #[test]
    fn a_diagonal_smear_paints_a_streak_only_along_the_velocity_not_across_it() {
        const VIEWPORT: f32 = 900.0;
        let mut principal = child_from_json(json!({
            "type": "shape", "shape": "rect", "fill": "#FFFFFF",
            "style": {
                "width": 80, "height": 80,
                "animation": [
                    { "name": "keyframes", "duration": 1.0, "keyframes": [
                        { "property": "translate_x", "easing": "linear", "keyframes": [
                            { "time": 0.0, "value": 0.0 }, { "time": 1.0, "value": 900.0 }
                        ] },
                        { "property": "translate_y", "easing": "linear", "keyframes": [
                            { "time": 0.0, "value": 0.0 }, { "time": 1.0, "value": 900.0 }
                        ] }
                    ] },
                    { "name": "motion_blur", "mode": "smear", "shutter": 1.0 }
                ]
            }
        }));
        principal.position = Some(crate::PositionMode::Absolute { x: 60.0, y: 60.0 });
        let scene = vec![principal];
        let anim = BuildAnimationCtx {
            time: 0.5,
            scenario_time: 0.5,
            scene_duration: 1.0,
            fps: 30,
        };
        let built = build_scene_at_time(
            &scene,
            (VIEWPORT, VIEWPORT),
            default_root_css((VIEWPORT, VIEWPORT)),
            anim,
        );
        let layout = run_layout(
            &built.root,
            (VIEWPORT, VIEWPORT),
            &ConversionContext::default(),
        );
        let mut surface = paint_scene_at(&built, &layout, VIEWPORT as i32, VIEWPORT as i32, 0.5);

        let center = (550.0_f32, 550.0_f32);
        let half_diagonal = 40.0 * std::f32::consts::SQRT_2;
        let reach = half_diagonal + 15.0;
        let along = (
            (center.0 + reach * std::f32::consts::FRAC_1_SQRT_2).round() as i32,
            (center.1 + reach * std::f32::consts::FRAC_1_SQRT_2).round() as i32,
        );
        let perpendicular = (
            (center.0 + reach * std::f32::consts::FRAC_1_SQRT_2).round() as i32,
            (center.1 - reach * std::f32::consts::FRAC_1_SQRT_2).round() as i32,
        );

        let along_alpha = read_pixel(&mut surface, along.0, along.1)[3];
        let perpendicular_alpha = read_pixel(&mut surface, perpendicular.0, perpendicular.1)[3];

        assert!(
            perpendicular_alpha < 10,
            "a point the same distance from the box's corner but 90 degrees off the 45-degree \
             motion must stay unlit — a directional blur has zero spread across its own axis; \
             got alpha {perpendicular_alpha}"
        );
        assert!(
            along_alpha > 25,
            "the same distance along the actual 45-degree motion must carry the blur's tail, \
             got alpha {along_alpha}"
        );
    }

    #[test]
    fn an_animation_touching_only_border_radius_reaches_the_painted_frame() {
        const SIZE: i32 = 200;
        let scene = vec![child_from_json(json!({
            "type": "div",
            "style": {
                "width": SIZE, "height": SIZE,
                "background": "#FF2D55",
                "animation": [{ "name": "keyframes", "duration": 1.0, "keyframes": [
                    { "property": "border_radius", "easing": "linear", "keyframes": [
                        { "time": 0.0, "value": 0.0 }, { "time": 1.0, "value": 100.0 }
                    ] }
                ] }]
            }
        }))];
        let anim = BuildAnimationCtx {
            time: 1.0,
            scenario_time: 1.0,
            scene_duration: 1.0,
            fps: 30,
        };
        let built = build_scene_at_time(
            &scene,
            (SIZE as f32, SIZE as f32),
            default_root_css((SIZE as f32, SIZE as f32)),
            anim,
        );
        let layout = run_layout(
            &built.root,
            (SIZE as f32, SIZE as f32),
            &ConversionContext::default(),
        );
        let mut surface = paint_scene_at(&built, &layout, SIZE, SIZE, 1.0);

        let corner = read_pixel(&mut surface, 2, 2);
        assert_eq!(
            corner[3], 0,
            "a 200x200 box animated to a 100px border-radius (t=1) must clip its very corner to \
             transparent; an animation touching only border_radius must still reach paint \
             through box_builder's css, got rgba {:?}",
            corner
        );
    }

    #[test]
    fn an_animation_touching_only_clip_path_progress_reaches_the_painted_frame() {
        const SIZE: i32 = 200;
        let scene = vec![child_from_json(json!({
            "type": "div",
            "style": {
                "width": SIZE, "height": SIZE,
                "background": "#00AAFF",
                "clip-path": {
                    "kind": "morph",
                    "from": { "kind": "circle", "radius": 10 },
                    "to": { "kind": "circle", "radius": 150 },
                    "progress": 0
                },
                "animation": [{ "name": "keyframes", "duration": 1.0, "keyframes": [
                    { "property": "clip_path_progress", "easing": "linear", "keyframes": [
                        { "time": 0.0, "value": 0.0 }, { "time": 1.0, "value": 1.0 }
                    ] }
                ] }]
            }
        }))];
        let anim = BuildAnimationCtx {
            time: 1.0,
            scenario_time: 1.0,
            scene_duration: 1.0,
            fps: 30,
        };
        let built = build_scene_at_time(
            &scene,
            (SIZE as f32, SIZE as f32),
            default_root_css((SIZE as f32, SIZE as f32)),
            anim,
        );
        let layout = run_layout(
            &built.root,
            (SIZE as f32, SIZE as f32),
            &ConversionContext::default(),
        );
        let mut surface = paint_scene_at(&built, &layout, SIZE, SIZE, 1.0);

        let corner = read_pixel(&mut surface, 5, 5);
        assert!(
            corner[3] > 200,
            "at t=1 the clip-path morph must have swept to its 150px-radius `to` circle, which \
             covers this corner; an animation touching only clip_path_progress must still reach \
             paint through box_builder's css, got rgba {:?}",
            corner
        );
    }

    #[test]
    fn an_animation_touching_only_gap_reaches_the_painted_frame() {
        const W: i32 = 400;
        const H: i32 = 80;
        let scene = vec![child_from_json(json!({
            "type": "div",
            "style": {
                "display": "flex", "flex-direction": "row",
                "width": W, "height": H,
                "animation": [{ "name": "keyframes", "duration": 1.0, "keyframes": [
                    { "property": "gap", "easing": "linear", "keyframes": [
                        { "time": 0.0, "value": 0.0 }, { "time": 1.0, "value": 100.0 }
                    ] }
                ] }]
            },
            "children": [
                { "type": "shape", "shape": "rect", "fill": "#00AA00",
                  "style": { "width": 80, "height": 80 } },
                { "type": "shape", "shape": "rect", "fill": "#0000AA",
                  "style": { "width": 80, "height": 80 } }
            ]
        }))];
        let anim = BuildAnimationCtx {
            time: 1.0,
            scenario_time: 1.0,
            scene_duration: 1.0,
            fps: 30,
        };
        let built = build_scene_at_time(
            &scene,
            (W as f32, H as f32),
            default_root_css((W as f32, H as f32)),
            anim,
        );
        let layout = run_layout(
            &built.root,
            (W as f32, H as f32),
            &ConversionContext::default(),
        );
        let mut surface = paint_scene_at(&built, &layout, W, H, 1.0);

        let between = read_pixel(&mut surface, 130, 40);
        assert_eq!(
            between[3], 0,
            "at t=1 a 100px gap must leave a transparent strip between the two 80px shapes \
             (x=80..180); an animation touching only gap must still reach layout through \
             box_builder's css, got rgba {:?}",
            between
        );
    }

    #[test]
    fn an_animation_touching_only_padding_reaches_the_painted_frame() {
        const SIZE: i32 = 200;
        let scene = vec![child_from_json(json!({
            "type": "div",
            "style": {
                "width": SIZE, "height": SIZE,
                "animation": [{ "name": "keyframes", "duration": 1.0, "keyframes": [
                    { "property": "padding", "easing": "linear", "keyframes": [
                        { "time": 0.0, "value": 0.0 }, { "time": 1.0, "value": 60.0 }
                    ] }
                ] }]
            },
            "children": [
                { "type": "shape", "shape": "rect", "fill": "#FFAA00",
                  "style": { "width": 40, "height": 40 } }
            ]
        }))];
        let anim = BuildAnimationCtx {
            time: 1.0,
            scenario_time: 1.0,
            scene_duration: 1.0,
            fps: 30,
        };
        let built = build_scene_at_time(
            &scene,
            (SIZE as f32, SIZE as f32),
            default_root_css((SIZE as f32, SIZE as f32)),
            anim,
        );
        let layout = run_layout(
            &built.root,
            (SIZE as f32, SIZE as f32),
            &ConversionContext::default(),
        );
        let mut surface = paint_scene_at(&built, &layout, SIZE, SIZE, 1.0);

        let near_origin = read_pixel(&mut surface, 10, 10);
        assert_eq!(
            near_origin[3], 0,
            "at t=1 a 60px padding must push the 40px child away from the container's top-left \
             corner; an animation touching only padding must still reach layout through \
             box_builder's css, got rgba {:?}",
            near_origin
        );
    }

    #[test]
    fn an_animated_font_size_reaches_layout_not_only_paint() {
        let make_text = |style_extra: serde_json::Value| {
            child_from_json(json!({
                "type": "text",
                "content": "The quick brown fox jumps over the lazy dog",
                "style": style_extra
            }))
        };

        let baseline_scene = vec![make_text(json!({
            "font-size": 20, "white-space": "nowrap", "align-self": "flex-start"
        }))];
        let baseline_built = build_scene(&baseline_scene, (4000.0, 400.0));
        let baseline_layout = run_layout(
            &baseline_built.root,
            (4000.0, 400.0),
            &ConversionContext::default(),
        );
        let baseline_width = baseline_layout
            .get(baseline_built.root.children[0].id)
            .expect("baseline text laid out")
            .width;

        let animated_scene = vec![make_text(json!({
            "font-size": 20,
            "white-space": "nowrap",
            "align-self": "flex-start",
            "animation": [{ "name": "keyframes", "duration": 1.0, "keyframes": [
                { "property": "font_size", "easing": "linear", "keyframes": [
                    { "time": 0.0, "value": 20.0 }, { "time": 1.0, "value": 120.0 }
                ] }
            ] }]
        }))];
        let anim = BuildAnimationCtx {
            time: 1.0,
            scenario_time: 1.0,
            scene_duration: 1.0,
            fps: 30,
        };
        let animated_built = build_scene_at_time(
            &animated_scene,
            (4000.0, 400.0),
            default_root_css((4000.0, 400.0)),
            anim,
        );
        let animated_layout = run_layout(
            &animated_built.root,
            (4000.0, 400.0),
            &ConversionContext::default(),
        );
        let animated_width = animated_layout
            .get(animated_built.root.children[0].id)
            .expect("animated text laid out")
            .width;

        assert!(
            animated_width > baseline_width * 2.0,
            "at t=1 the font_size animation has reached 120px (6x the declared static 20px); \
             the LAYOUT width must reflect that, not just the painted glyphs — issue #426/#430's \
             'paint right, layout stale' symptom. baseline={baseline_width}, \
             animated={animated_width}"
        );
    }

    #[test]
    fn an_animated_letter_spacing_on_text_reaches_layout_not_only_paint() {
        let make_text = |style_extra: serde_json::Value| {
            child_from_json(json!({
                "type": "text",
                "content": "WWWWWWWWWW",
                "style": style_extra
            }))
        };

        let baseline_scene = vec![make_text(json!({
            "letter-spacing": 0, "font-size": 40, "white-space": "nowrap",
            "align-self": "flex-start"
        }))];
        let baseline_built = build_scene(&baseline_scene, (4000.0, 400.0));
        let baseline_layout = run_layout(
            &baseline_built.root,
            (4000.0, 400.0),
            &ConversionContext::default(),
        );
        let baseline_width = baseline_layout
            .get(baseline_built.root.children[0].id)
            .expect("baseline text laid out")
            .width;

        let animated_scene = vec![make_text(json!({
            "letter-spacing": 0, "font-size": 40, "white-space": "nowrap",
            "align-self": "flex-start",
            "animation": [{ "name": "keyframes", "duration": 1.0, "keyframes": [
                { "property": "letter_spacing", "easing": "linear", "keyframes": [
                    { "time": 0.0, "value": 0.0 }, { "time": 1.0, "value": 40.0 }
                ] }
            ] }]
        }))];
        let anim = BuildAnimationCtx {
            time: 1.0,
            scenario_time: 1.0,
            scene_duration: 1.0,
            fps: 30,
        };
        let animated_built = build_scene_at_time(
            &animated_scene,
            (4000.0, 400.0),
            default_root_css((4000.0, 400.0)),
            anim,
        );
        let animated_layout = run_layout(
            &animated_built.root,
            (4000.0, 400.0),
            &ConversionContext::default(),
        );
        let animated_width = animated_layout
            .get(animated_built.root.children[0].id)
            .expect("animated text laid out")
            .width;

        assert!(
            animated_width > baseline_width + 200.0,
            "at t=1 a 40px letter-spacing animation must widen the measured LAYOUT box, or the \
             box the text was given overflows — issue #430's overflow symptom exactly. \
             baseline={baseline_width}, animated={animated_width}"
        );
    }

    #[test]
    fn an_animated_letter_spacing_reaches_rich_text_through_the_full_pipeline() {
        const W: i32 = 800;
        const H: i32 = 120;
        let scene = vec![child_from_json(json!({
            "type": "rich_text",
            "spans": [{ "text": "WWWWWWWWWW", "color": "#FFFFFF" }],
            "style": {
                "letter-spacing": 0, "font-size": 60,
                "animation": [{ "name": "keyframes", "duration": 1.0, "keyframes": [
                    { "property": "letter_spacing", "easing": "linear", "keyframes": [
                        { "time": 0.0, "value": 0.0 }, { "time": 1.0, "value": 40.0 }
                    ] }
                ] }]
            }
        }))];

        let render_extent_at = |time: f64| -> (i32, i32) {
            let anim = BuildAnimationCtx {
                time,
                scenario_time: time,
                scene_duration: 1.0,
                fps: 30,
            };
            let built = build_scene_at_time(
                &scene,
                (W as f32, H as f32),
                default_root_css((W as f32, H as f32)),
                anim,
            );
            let layout = run_layout(
                &built.root,
                (W as f32, H as f32),
                &ConversionContext::default(),
            );
            let mut surface = paint_scene_at(&built, &layout, W, H, time);
            ink_extent_x(&mut surface, W, H, 0, H).expect("rich_text must paint some ink")
        };

        let (start0, end0) = render_extent_at(0.0);
        let (start1, end1) = render_extent_at(1.0);
        let extent0 = end0 - start0;
        let extent1 = end1 - start1;

        assert!(
            extent1 > extent0 + 100,
            "an animated style.letter-spacing (no per-span override) must widen rich_text's \
             painted ink extent — both the box_builder wiring and rich_text's own static/\
             animated fallback must be in place (issue #430); extent at t=0 was {extent0}px \
             [{start0},{end0}], at t=1 {extent1}px [{start1},{end1}]"
        );
    }

    #[test]
    fn a_static_opacity_zero_is_overridden_by_an_active_fade_in_animation_end_to_end() {
        const SIZE: i32 = 100;
        let scene = vec![child_from_json(json!({
            "type": "div",
            "style": {
                "width": SIZE, "height": SIZE,
                "background": "#00FF00",
                "opacity": 0,
                "animation": [{ "name": "keyframes", "duration": 1.0, "keyframes": [
                    { "property": "opacity", "easing": "linear", "keyframes": [
                        { "time": 0.0, "value": 0.0 }, { "time": 1.0, "value": 0.8 }
                    ] }
                ] }]
            }
        }))];
        let anim = BuildAnimationCtx {
            time: 1.0,
            scenario_time: 1.0,
            scene_duration: 1.0,
            fps: 30,
        };
        let built = build_scene_at_time(
            &scene,
            (SIZE as f32, SIZE as f32),
            default_root_css((SIZE as f32, SIZE as f32)),
            anim,
        );
        let layout = run_layout(
            &built.root,
            (SIZE as f32, SIZE as f32),
            &ConversionContext::default(),
        );
        let mut surface = paint_scene_at(&built, &layout, SIZE, SIZE, 1.0);

        let center = read_pixel(&mut surface, SIZE / 2, SIZE / 2);
        assert!(
            center[3] > 100,
            "the fade-in animation resolves opacity to 0.8 at t=1; a declared `opacity: 0` \
             authored as the animation's own starting point must not multiply it back to zero \
             for the rest of the run (issue #430), got rgba {:?}",
            center
        );
    }
}
