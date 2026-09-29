use crate::error::Result;
use skia_safe::{surfaces, Canvas, ClipOp, ColorType, ImageInfo, Paint, Rect};
use std::sync::Arc;

use super::background::draw_animated_background;
use super::background::draw_world_bg_with_parallax;
use super::background::interpolate_animated_bg;
use crate::components::ChildComponent;
use crate::error::RustmotionError;
use crate::schema::{Camera, Scene, SceneLayout, VideoConfig, ViewType};
use rustmotion_core::css::style::{
    AlignItems as CssAlignItems, CssStyle, Edges, FlexDirection as CssFlexDirection, Gap,
    JustifyContent as CssJustifyContent,
};
use rustmotion_core::css::taffy_bridge::ConversionContext;
use rustmotion_core::css::units::LengthPercentage;
use rustmotion_core::engine::animator::safe_div;
use rustmotion_core::engine::deps::ResolvedFrame;
use rustmotion_core::engine::paint_pass::PlaneCamera;
use rustmotion_core::engine::renderer::color4f_from_hex;
use rustmotion_core::engine::shake::{shake_offset, ShakeOffset};
use rustmotion_core::expr::Scope;
use rustmotion_core::schema::time::TimeCtx;
use rustmotion_core::vars::{VarScope, VarSet, VarTable};

mod scene_time {
    use crate::schema::Scene;

    #[derive(Debug, Clone, Copy)]
    pub(super) struct SceneTime(f64);

    impl SceneTime {
        pub(super) fn for_frame(scene: &Scene, frame_index: u32, fps: u32) -> Self {
            Self::clamp(scene, frame_index as f64 / fps as f64)
        }

        pub(super) fn for_local_time(scene: &Scene, local_time: f64) -> Self {
            Self::clamp(scene, local_time)
        }

        fn clamp(scene: &Scene, raw: f64) -> Self {
            match scene.freeze_at {
                Some(freeze_at) if raw > freeze_at => SceneTime(freeze_at),
                _ => SceneTime(raw),
            }
        }

        pub(super) fn seconds(self) -> f64 {
            self.0
        }
    }
}
use scene_time::SceneTime;

#[derive(Debug, Clone)]
struct RenderContext {
    time: SceneTime,
    scenario_time: f64,
    scene_duration: f64,
    frame_index: u32,
    fps: u32,
    video_width: u32,
    video_height: u32,
    #[allow(dead_code)]
    stagger_offset: f64,
    camera: Option<PlaneCamera>,
}

fn scene_uses_depth(children: &[ChildComponent]) -> bool {
    children
        .iter()
        .any(|c| c.component.as_styled().style_config().depth.is_some())
}

fn scene_light(scene: &Scene) -> rustmotion_core::engine::paint_pass::LightDirection {
    use rustmotion_core::engine::paint_pass::LightDirection;
    match scene.light.as_ref() {
        Some(light) => LightDirection {
            x: light.x,
            y: light.y,
            intensity: light.intensity,
            color: light
                .color
                .as_deref()
                .map(|hex| {
                    let (r, g, b, a) = rustmotion_core::engine::renderer::parse_hex_color(hex);
                    skia_safe::Color::from_argb(a, r, g, b)
                })
                .unwrap_or(skia_safe::Color::from_argb(255, 255, 255, 255)),
        },
        None => LightDirection::default(),
    }
}

fn scene_shake_offset(scene: &Scene, time: f32) -> ShakeOffset {
    match &scene.shake {
        Some(shake) => {
            shake_offset(shake, &scene.resolved_time_ctx, time as f64).unwrap_or_default()
        }
        None => ShakeOffset::default(),
    }
}

static IDENTITY_CAMERA: Camera = Camera {
    x: 0.0,
    y: 0.0,
    zoom: 1.0,
    rotation: 0.0,
    origin: None,
    keyframes: Vec::new(),
    focus: 1.0,
    aperture: 0.0,
    rotate_x: 0.0,
    rotate_y: 0.0,
    perspective: 0.0,
    motion_blur: None,
};

fn effective_camera(scene: &Scene) -> Option<&Camera> {
    if let Some(camera) = scene.camera.as_ref() {
        Some(camera)
    } else if scene.shake.is_some() {
        Some(&IDENTITY_CAMERA)
    } else {
        None
    }
}

fn resolve_plane_camera(
    scene: &Scene,
    camera: &Camera,
    time: f32,
    vw: f32,
    vh: f32,
) -> PlaneCamera {
    let (origin_x, origin_y) = resolve_camera_origin(camera, time, vw, vh);
    let shake = scene_shake_offset(scene, time);
    PlaneCamera {
        pan_x: interpolate_camera_property(camera, "x", time) + shake.x as f32,
        pan_y: interpolate_camera_property(camera, "y", time) + shake.y as f32,
        zoom: interpolate_camera_property(camera, "zoom", time),
        rotation: interpolate_camera_property(camera, "rotation", time) + shake.rotation as f32,
        origin_x,
        origin_y,
        focus: interpolate_camera_property(camera, "focus", time),
        aperture: interpolate_camera_property(camera, "aperture", time),
        rotate_x: interpolate_camera_property(camera, "rotate_x", time),
        rotate_y: interpolate_camera_property(camera, "rotate_y", time),
        perspective: interpolate_camera_property(camera, "perspective", time),
    }
}

fn per_plane_camera(
    scene: &Scene,
    children: &[ChildComponent],
    time: f32,
    vw: f32,
    vh: f32,
) -> Option<PlaneCamera> {
    match effective_camera(scene) {
        Some(cam) if scene_uses_depth(children) => {
            Some(resolve_plane_camera(scene, cam, time, vw, vh))
        }
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_unpaired_layers_faded(
    canvas: &Canvas,
    layers: &[crate::schema::AnimatedBackground],
    continuous_time: f32,
    w: f32,
    h: f32,
    alpha: f32,
    scaled_w: i32,
    scaled_h: i32,
    scale_factor: f32,
) {
    if layers.is_empty() || alpha <= 0.0 {
        return;
    }
    let bg_info = ImageInfo::new(
        (scaled_w, scaled_h),
        ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    let Some(mut layer_surface) = surfaces::raster(&bg_info, None, None) else {
        return;
    };
    let layer_canvas = layer_surface.canvas();
    if scale_factor != 1.0 {
        layer_canvas.scale((scale_factor, scale_factor));
    }
    layer_canvas.clear(skia_safe::Color4f::new(0.0, 0.0, 0.0, 0.0));
    for anim_bg in layers {
        draw_animated_background(layer_canvas, anim_bg, continuous_time, w, h);
    }
    let snapshot = layer_surface.image_snapshot();
    let mut paint = Paint::default();
    paint.set_alpha_f(alpha);
    canvas.save();
    if scale_factor != 1.0 {
        canvas.reset_matrix();
    }
    canvas.draw_image(&snapshot, (0.0, 0.0), Some(&paint));
    canvas.restore();
}

pub fn render_frame_v2(
    config: &VideoConfig,
    scene: &Scene,
    frame_index: u32,
    scenario_time: f64,
    _total_frames: u32,
    root_children: &[ChildComponent],
) -> Result<Vec<u8>> {
    render_frame_v2_scaled(
        config,
        scene,
        frame_index,
        scenario_time,
        _total_frames,
        root_children,
        1.0,
        None,
    )
}

pub fn render_frame_v2_scaled(
    config: &VideoConfig,
    scene: &Scene,
    frame_index: u32,
    scenario_time: f64,
    _total_frames: u32,
    root_children: &[ChildComponent],
    scale_factor: f32,
    prev_bg: Option<(&crate::schema::ResolvedBackground, f64)>,
) -> Result<Vec<u8>> {
    if let Some(plan) = camera_motion_blur_plan(scene, frame_index, config.fps) {
        return render_frame_v2_scaled_camera_blurred(
            config,
            scene,
            frame_index,
            scenario_time,
            root_children,
            scale_factor,
            prev_bg,
            plan,
        );
    }
    let scene_time = SceneTime::for_frame(scene, frame_index, config.fps);
    render_frame_v2_scaled_core(
        config,
        scene,
        scene_time,
        frame_index,
        scenario_time,
        root_children,
        scale_factor,
        prev_bg,
    )
}

struct CameraMotionBlurPlan {
    samples: u32,
    shutter_seconds: f64,
}

fn camera_motion_blur_plan(
    scene: &Scene,
    frame_index: u32,
    fps: u32,
) -> Option<CameraMotionBlurPlan> {
    let camera = effective_camera(scene)?;
    let cfg = camera.motion_blur.as_ref()?;
    let nominal = frame_index as f64 / fps.max(1) as f64;
    let shutter_seconds = (cfg.shutter / fps.max(1) as f64).max(0.0);
    if shutter_seconds <= 0.0 {
        return None;
    }
    let pose_start = camera_pose_at(scene, camera, (nominal - shutter_seconds) as f32);
    let pose_end = camera_pose_at(scene, camera, nominal as f32);
    let moved = (pose_start.0 - pose_end.0).abs() > 0.01
        || (pose_start.1 - pose_end.1).abs() > 0.01
        || (pose_start.2 - pose_end.2).abs() > 0.0005
        || (pose_start.3 - pose_end.3).abs() > 0.01;
    if !moved {
        return None;
    }
    Some(CameraMotionBlurPlan {
        samples: cfg.samples.clamp(1, 16),
        shutter_seconds,
    })
}

fn camera_pose_at(scene: &Scene, camera: &Camera, time: f32) -> (f32, f32, f32, f32) {
    let shake = scene_shake_offset(scene, time);
    let x = interpolate_camera_property(camera, "x", time) + shake.x as f32;
    let y = interpolate_camera_property(camera, "y", time) + shake.y as f32;
    let zoom = interpolate_camera_property(camera, "zoom", time);
    let rotation = interpolate_camera_property(camera, "rotation", time) + shake.rotation as f32;
    (x, y, zoom, rotation)
}

#[allow(clippy::too_many_arguments)]
fn render_frame_v2_scaled_camera_blurred(
    config: &VideoConfig,
    scene: &Scene,
    frame_index: u32,
    scenario_time: f64,
    root_children: &[ChildComponent],
    scale_factor: f32,
    prev_bg: Option<(&crate::schema::ResolvedBackground, f64)>,
    plan: CameraMotionBlurPlan,
) -> Result<Vec<u8>> {
    let nominal = frame_index as f64 / config.fps.max(1) as f64;
    let samples = plan.samples.max(1);
    let mut accum: Option<Vec<u32>> = None;
    for i in 0..samples {
        let offset = (i as f64 + 0.5) * plan.shutter_seconds / samples as f64;
        let sample_time = nominal - offset;
        let scene_time = SceneTime::for_local_time(scene, sample_time);
        let pixels = render_frame_v2_scaled_core(
            config,
            scene,
            scene_time,
            frame_index,
            scenario_time,
            root_children,
            scale_factor,
            prev_bg,
        )?;
        match accum.as_mut() {
            Some(sum) => {
                for (s, p) in sum.iter_mut().zip(pixels.iter()) {
                    *s += *p as u32;
                }
            }
            None => accum = Some(pixels.iter().map(|&p| p as u32).collect()),
        }
    }
    let sum = accum.ok_or(RustmotionError::SurfaceCreation)?;
    Ok(sum
        .into_iter()
        .map(|s| ((s + samples / 2) / samples) as u8)
        .collect())
}

#[allow(clippy::too_many_arguments)]
fn render_frame_v2_scaled_core(
    config: &VideoConfig,
    scene: &Scene,
    scene_time: SceneTime,
    frame_index: u32,
    scenario_time: f64,
    root_children: &[ChildComponent],
    scale_factor: f32,
    prev_bg: Option<(&crate::schema::ResolvedBackground, f64)>,
) -> Result<Vec<u8>> {
    let scaled_w = (config.width as f32 * scale_factor) as i32;
    let scaled_h = (config.height as f32 * scale_factor) as i32;
    let time = scene_time.seconds();

    let info = ImageInfo::new(
        (scaled_w, scaled_h),
        ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );

    let mut surface =
        surfaces::raster(&info, None, None).ok_or(RustmotionError::SurfaceCreation)?;

    let canvas = surface.canvas();

    if scale_factor != 1.0 {
        canvas.scale((scale_factor, scale_factor));
    }

    let bg = scene
        .resolved_background
        .color
        .as_deref()
        .unwrap_or(&config.background);
    canvas.clear(color4f_from_hex(bg));

    let cur_bg = &scene.resolved_background;
    if let (Some(ref transition), Some((prev, prev_duration))) = (&cur_bg.transition, prev_bg) {
        let t_elapsed = time;
        let continuous_time = (prev_duration + time) as f32;
        if t_elapsed < transition.duration && !prev.animated.is_empty() {
            let progress = crate::engine::animator::ease(
                (t_elapsed / transition.duration).clamp(0.0, 1.0),
                &transition.easing,
            ) as f32;
            let w = config.width as f32;
            let h = config.height as f32;

            let paired = prev.animated.len().min(cur_bg.animated.len());
            for i in 0..paired {
                let blended =
                    interpolate_animated_bg(&prev.animated[i], &cur_bg.animated[i], progress);
                draw_animated_background(canvas, &blended, continuous_time, w, h);
            }

            draw_unpaired_layers_faded(
                canvas,
                &prev.animated[paired..],
                continuous_time,
                w,
                h,
                1.0 - progress,
                scaled_w,
                scaled_h,
                scale_factor,
            );
            draw_unpaired_layers_faded(
                canvas,
                &cur_bg.animated[paired..],
                continuous_time,
                w,
                h,
                progress,
                scaled_w,
                scaled_h,
                scale_factor,
            );
        } else {
            for anim_bg in &cur_bg.animated {
                draw_animated_background(
                    canvas,
                    anim_bg,
                    continuous_time,
                    config.width as f32,
                    config.height as f32,
                );
            }
        }
    } else {
        for anim_bg in &cur_bg.animated {
            draw_animated_background(
                canvas,
                anim_bg,
                time as f32,
                config.width as f32,
                config.height as f32,
            );
        }
    }

    let plane_cam = per_plane_camera(
        scene,
        root_children,
        time as f32,
        config.width as f32,
        config.height as f32,
    );

    let ctx = RenderContext {
        time: scene_time,
        scenario_time,
        scene_duration: scene.duration,
        frame_index,
        fps: config.fps,
        video_width: config.width,
        video_height: config.height,
        stagger_offset: 0.0,
        camera: plane_cam,
    };

    let camera_guard = match effective_camera(scene) {
        Some(camera) if plane_cam.is_none() => {
            let g = super::CanvasGuard::new(canvas);
            apply_camera_transform(
                canvas,
                scene,
                camera,
                time as f32,
                config.width as f32,
                config.height as f32,
            );
            Some(g)
        }
        _ => None,
    };

    let clip_guard = super::CanvasGuard::new(canvas);
    canvas.clip_rect(
        Rect::from_wh(config.width as f32, config.height as f32),
        ClipOp::Intersect,
        true,
    );

    render_with_new_pipeline(
        canvas,
        root_children,
        config.width as f32,
        config.height as f32,
        scene.layout.as_ref(),
        &ctx,
        scene,
    );

    drop(clip_guard);
    drop(camera_guard);

    let row_bytes = scaled_w as usize * 4;
    let mut pixels = vec![0u8; row_bytes * scaled_h as usize];
    let dst_info = ImageInfo::new(
        (scaled_w, scaled_h),
        ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    surface
        .read_pixels(&dst_info, &mut pixels, row_bytes, (0, 0))
        .then_some(())
        .ok_or(RustmotionError::PixelRead)?;

    Ok(pixels)
}

fn world_default_scene_layout() -> SceneLayout {
    use crate::schema::{CardAlign, CardDirection, CardJustify};
    SceneLayout {
        direction: Some(CardDirection::Column),
        gap: Some(12.0),
        align_items: Some(CardAlign::Center),
        justify_content: Some(CardJustify::Center),
        padding: None,
    }
}

pub fn root_style(scene_layout: Option<&SceneLayout>, view_type: ViewType) -> CssStyle {
    use crate::schema::{CardAlign, CardDirection, CardJustify};
    let owned_world_default;
    let scene_layout = match (scene_layout, view_type) {
        (Some(layout), _) => Some(layout),
        (None, ViewType::World) => {
            owned_world_default = world_default_scene_layout();
            Some(&owned_world_default)
        }
        (None, ViewType::Slide) => None,
    };

    let mut style = CssStyle::default();
    style.display = Some(rustmotion_core::css::style::Display::Flex);

    if let Some(layout) = scene_layout {
        if let Some(d) = &layout.direction {
            style.flex_direction = Some(match d {
                CardDirection::Row => CssFlexDirection::Row,
                CardDirection::Column => CssFlexDirection::Column,
                CardDirection::RowReverse => CssFlexDirection::RowReverse,
                CardDirection::ColumnReverse => CssFlexDirection::ColumnReverse,
            });
        }
        if let Some(g) = layout.gap {
            style.gap = Some(Gap::Uniform(LengthPercentage::Px(g)));
        }
        if let Some(a) = &layout.align_items {
            style.align_items = Some(match a {
                CardAlign::Start => CssAlignItems::FlexStart,
                CardAlign::End => CssAlignItems::FlexEnd,
                CardAlign::Center => CssAlignItems::Center,
                CardAlign::Stretch => CssAlignItems::Stretch,
            });
        }
        if let Some(j) = &layout.justify_content {
            style.justify_content = Some(match j {
                CardJustify::Start => CssJustifyContent::FlexStart,
                CardJustify::End => CssJustifyContent::FlexEnd,
                CardJustify::Center => CssJustifyContent::Center,
                CardJustify::SpaceBetween => CssJustifyContent::SpaceBetween,
                CardJustify::SpaceAround => CssJustifyContent::SpaceAround,
                CardJustify::SpaceEvenly => CssJustifyContent::SpaceEvenly,
            });
        }
        if let Some(p) = layout.padding {
            style.padding = Some(Edges::Uniform(LengthPercentage::Px(p)));
        }
    }
    if style.flex_direction.is_none() {
        style.flex_direction = Some(CssFlexDirection::Column);
    }
    style
}

struct EngineScope<'a> {
    vars: VarScope<'a>,
    frame: &'a ResolvedFrame,
}

impl Scope for EngineScope<'_> {
    fn var(&self, name: &str) -> Option<f64> {
        self.vars.resolve(name)
    }

    fn node_prop(&self, id: &str, prop: &str) -> Option<f64> {
        self.frame.node_prop(id, prop)
    }
}

fn compile_var_table_or_warn(vars: &VarSet, ctx: &TimeCtx, which: &str) -> VarTable {
    VarTable::compile(vars, ctx).unwrap_or_else(|e| {
        eprintln!("warning: {which} `vars` failed to compile ({e}) — treated as empty this frame");
        VarTable::compile(&VarSet::new(), ctx).expect("compiling an empty VarSet never fails")
    })
}

fn render_with_new_pipeline(
    canvas: &Canvas,
    root_children: &[ChildComponent],
    viewport_w: f32,
    viewport_h: f32,
    scene_layout: Option<&SceneLayout>,
    ctx: &RenderContext,
    scene: &Scene,
) {
    render_with_new_pipeline_iter(
        canvas,
        root_children.iter(),
        viewport_w,
        viewport_h,
        scene_layout,
        ctx,
        scene,
    );
}

fn render_with_new_pipeline_iter<'a, I>(
    canvas: &Canvas,
    root_children: I,
    viewport_w: f32,
    viewport_h: f32,
    scene_layout: Option<&SceneLayout>,
    ctx: &RenderContext,
    scene: &Scene,
) where
    I: IntoIterator<Item = &'a ChildComponent>,
{
    use rustmotion_components::box_builder::{
        build_scene_from_refs_with_scope, build_scene_from_refs_with_scope_quiet,
        collect_node_refs, scene_uses_node_refs, BuildAnimationCtx,
    };
    use rustmotion_components::legacy_dispatch::LegacyPaintDispatcher;
    use rustmotion_core::engine::layout_pass::run_layout;
    use rustmotion_core::engine::paint_pass::{paint_tree, PaintFrame};

    let root_css = root_style(scene_layout, ViewType::Slide);

    let anim = Some(BuildAnimationCtx {
        time: ctx.time.seconds(),
        scenario_time: ctx.scenario_time,
        scene_duration: ctx.scene_duration,
        fps: ctx.fps,
    });
    let viewport = (viewport_w, viewport_h);
    let conversion = ConversionContext::for_viewport(viewport_w, viewport_h);

    let children_vec: Vec<&'a ChildComponent> = root_children.into_iter().collect();

    let has_node_refs = scene_uses_node_refs(children_vec.iter().copied());
    let use_vars = !scene.resolved_scenario_vars.is_empty() || !scene.vars.is_empty();

    let built = if !use_vars && !has_node_refs {
        build_scene_from_refs_with_scope(
            children_vec.iter().copied(),
            viewport,
            root_css,
            anim,
            None,
        )
    } else {
        let scenario_table = compile_var_table_or_warn(
            &scene.resolved_scenario_vars,
            &scene.resolved_time_ctx,
            "scenario-level",
        );
        let scene_table =
            compile_var_table_or_warn(&scene.vars, &scene.resolved_time_ctx, "scene-level");

        if !has_node_refs {
            let empty_frame = ResolvedFrame::new();
            let engine_scope = EngineScope {
                vars: VarScope::new(&scenario_table, Some(&scene_table), ctx.scenario_time),
                frame: &empty_frame,
            };
            build_scene_from_refs_with_scope(
                children_vec.iter().copied(),
                viewport,
                root_css,
                anim,
                Some(&engine_scope as &dyn Scope),
            )
        } else {
            let empty_frame = ResolvedFrame::new();
            let base_engine_scope = EngineScope {
                vars: VarScope::new(&scenario_table, Some(&scene_table), ctx.scenario_time),
                frame: &empty_frame,
            };
            let base_built = build_scene_from_refs_with_scope_quiet(
                children_vec.iter().copied(),
                viewport,
                root_css.clone(),
                anim,
                Some(&base_engine_scope as &dyn Scope),
                false,
            );
            let base_layout = run_layout(&base_built.root, viewport, &conversion);
            let refs_by_id = collect_node_refs(children_vec.iter().copied());
            let resolved_frame =
                resolve_node_references(&base_built, &base_layout, viewport, &refs_by_id)
                    .unwrap_or_else(|e| {
                        eprintln!(
                            "warning: node(...) dependency graph: {e} — cross-node references unresolved this frame"
                        );
                        ResolvedFrame::new()
                    });

            let final_engine_scope = EngineScope {
                vars: VarScope::new(&scenario_table, Some(&scene_table), ctx.scenario_time),
                frame: &resolved_frame,
            };
            build_scene_from_refs_with_scope(
                children_vec.iter().copied(),
                viewport,
                root_css,
                anim,
                Some(&final_engine_scope as &dyn Scope),
            )
        }
    };

    let mut layout = run_layout(&built.root, viewport, &conversion);
    apply_ghost_layout_fixup(&built, &mut layout);
    let dispatcher = LegacyPaintDispatcher::for_scene(&built);
    let frame = PaintFrame {
        light: scene_light(scene),
        time: ctx.time.seconds(),
        scenario_time: ctx.scenario_time,
        frame_index: ctx.frame_index,
        fps: ctx.fps,
        video_width: ctx.video_width,
        video_height: ctx.video_height,
        scene_duration: ctx.scene_duration,
        camera: ctx.camera,
    };
    paint_tree(canvas, &built.root, &layout, &frame, &dispatcher);
}

fn paint_decorative_fullscreen(
    canvas: &Canvas,
    child: &ChildComponent,
    viewport_w: f32,
    viewport_h: f32,
    ctx: &RenderContext,
) {
    use rustmotion_components::box_builder::effective_effects;
    use rustmotion_core::engine::animator::{resolve_props_for_effects, AnimatedProperties};
    use rustmotion_core::engine::box_tree::PaintWindow;
    use rustmotion_core::engine::layout_pass::BoxLayout;
    use rustmotion_core::traits::PaintCtx;

    let time = ctx.time.seconds();
    let mut start_at = 0.0;
    if let Some(timed) = child.component.as_timed() {
        let (start, end) = timed.timing();
        if !(PaintWindow { start, end }).contains(time) {
            return;
        }
        start_at = start.unwrap_or(0.0);
    }

    let props = match effective_effects(&child.component, start_at, time) {
        Some(effects) => resolve_props_for_effects(&effects, time, ctx.scene_duration),
        None => AnimatedProperties::default(),
    };
    if props.opacity <= 0.0 {
        return;
    }

    let Some(painter) = child.component.as_painter() else {
        return;
    };

    let local = BoxLayout {
        x: 0.0,
        y: 0.0,
        width: viewport_w,
        height: viewport_h,
        ..Default::default()
    };
    let paint_ctx = PaintCtx {
        time,
        scenario_time: ctx.scenario_time,
        scene_duration: ctx.scene_duration,
        frame_index: ctx.frame_index,
        fps: ctx.fps,
        video_width: ctx.video_width,
        video_height: ctx.video_height,
        stagger_offset: start_at,
    };
    canvas.save();
    painter.paint_content(canvas, &local, &props, &paint_ctx);
    canvas.restore();
}

pub fn deserialize_children(scene: &Scene) -> Vec<ChildComponent> {
    scene.children.iter()
        .enumerate()
        .filter_map(|(i, v)| match serde_json::from_value::<ChildComponent>(v.clone()) {
            Ok(c) => Some(c),
            Err(e) => {
                let kind = v.get("type").and_then(|t| t.as_str()).unwrap_or("?");
                eprintln!("warning: scene child #{i} (type={kind}) failed to deserialize: {e} — child will not be rendered");
                None
            }
        })
        .collect()
}

pub fn prepare_scene(scene: &Scene, _config: &VideoConfig) -> Arc<Vec<ChildComponent>> {
    scene
        .prepared_children
        .get_or_init(|| Arc::new(deserialize_children(scene)))
        .clone()
        .downcast::<Vec<ChildComponent>>()
        .expect("prepare_scene is the only writer of Scene::prepared_children")
}

pub fn resolve_node_references(
    built: &rustmotion_components::box_builder::BuiltScene<'_>,
    layout: &rustmotion_core::engine::layout_pass::LayoutResult,
    viewport: (f32, f32),
    refs_by_id: &[(String, Vec<rustmotion_core::engine::deps::NodeRef>)],
) -> std::result::Result<
    rustmotion_core::engine::deps::ResolvedFrame,
    rustmotion_core::engine::deps::DepsError,
> {
    use rustmotion_core::engine::box_tree::NodeId;
    use rustmotion_core::engine::deps::{snapshot_node, DepGraph, ResolvedFrame};
    use std::collections::{HashMap, HashSet};

    let other_scene_ids: HashSet<String> = HashSet::new();
    let graph = DepGraph::build(refs_by_id, &other_scene_ids)?;

    let id_index: HashMap<&str, NodeId> = built
        .components
        .iter()
        .enumerate()
        .filter_map(|(node_id, c)| {
            let child = (*c)?;
            let id = child.id.as_deref()?;
            Some((id, node_id as NodeId))
        })
        .collect();

    let text_provider = ResolvingTextMetrics {
        components: &built.components,
    };
    let mut frame = ResolvedFrame::new();
    for id in graph.order() {
        let Some(&node_id) = id_index.get(id.as_str()) else {
            continue;
        };
        let Some(box_node) = built.root.find(node_id) else {
            continue;
        };
        if let Some(resolved) = snapshot_node(box_node, layout, viewport, &text_provider) {
            frame.insert(id.clone(), resolved);
        }
    }
    Ok(frame)
}

fn apply_ghost_layout_fixup(
    built: &rustmotion_components::box_builder::BuiltScene<'_>,
    layout: &mut rustmotion_core::engine::layout_pass::LayoutResult,
) {
    use rustmotion_core::engine::box_tree::NodeId;

    if built.ghost_principal.is_empty() {
        return;
    }
    let deltas: Vec<(NodeId, f32, f32)> = built
        .ghost_principal
        .iter()
        .filter_map(|&(ghost_id, principal_id)| {
            let ghost_box = layout.get(ghost_id)?;
            let principal_box = layout.get(principal_id)?;
            let dx = principal_box.x - ghost_box.x;
            let dy = principal_box.y - ghost_box.y;
            (dx != 0.0 || dy != 0.0).then_some((ghost_id, dx, dy))
        })
        .collect();

    for (ghost_id, dx, dy) in deltas {
        let Some(ghost_node) = built.root.find(ghost_id) else {
            continue;
        };
        shift_subtree_layout(ghost_node, dx, dy, layout);
    }
}

fn shift_subtree_layout(
    node: &rustmotion_core::engine::box_tree::BoxNode,
    dx: f32,
    dy: f32,
    layout: &mut rustmotion_core::engine::layout_pass::LayoutResult,
) {
    if let Some(box_layout) = layout.layouts.get_mut(&node.id) {
        box_layout.x += dx;
        box_layout.y += dy;
    }
    for child in &node.children {
        shift_subtree_layout(child, dx, dy, layout);
    }
}

struct ResolvingTextMetrics<'a> {
    components: &'a [Option<&'a rustmotion_components::ChildComponent>],
}

impl rustmotion_core::engine::deps::TextMetricsProvider for ResolvingTextMetrics<'_> {
    fn text_metrics(
        &self,
        payload: &(dyn std::any::Any + Send + Sync),
        content_box_width: f32,
    ) -> Option<rustmotion_core::engine::deps::TextMetrics> {
        use rustmotion_components::intrinsic::ComponentTextMetrics;
        use rustmotion_components::Component;
        use rustmotion_core::engine::box_tree::NodeId;

        let node_id = payload.downcast_ref::<NodeId>()?;
        let child = (*self.components.get(*node_id as usize)?)?;
        match &child.component {
            Component::Text(t) => ComponentTextMetrics.text_metrics(t, content_box_width),
            Component::GradientText(g) => ComponentTextMetrics.text_metrics(g, content_box_width),
            _ => None,
        }
    }
}

pub fn render_scene_frame(
    config: &VideoConfig,
    scene: &Scene,
    frame_in_scene: u32,
    scenario_time: f64,
    scene_total_frames: u32,
) -> Result<Vec<u8>> {
    let children = prepare_scene(scene, config);
    render_frame_v2(
        config,
        scene,
        frame_in_scene,
        scenario_time,
        scene_total_frames,
        &children,
    )
}

pub fn render_scene_frame_scaled(
    config: &VideoConfig,
    scene: &Scene,
    frame_in_scene: u32,
    scenario_time: f64,
    scene_total_frames: u32,
    scale_factor: f32,
) -> Result<Vec<u8>> {
    let children = prepare_scene(scene, config);
    render_frame_v2_scaled(
        config,
        scene,
        frame_in_scene,
        scenario_time,
        scene_total_frames,
        &children,
        scale_factor,
        None,
    )
}

pub fn render_scene_frame_scaled_with_prev_bg(
    config: &VideoConfig,
    scene: &Scene,
    frame_in_scene: u32,
    scenario_time: f64,
    scene_total_frames: u32,
    scale_factor: f32,
    prev_bg: Option<(&crate::schema::ResolvedBackground, f64)>,
) -> Result<Vec<u8>> {
    let children = prepare_scene(scene, config);
    render_frame_v2_scaled(
        config,
        scene,
        frame_in_scene,
        scenario_time,
        scene_total_frames,
        &children,
        scale_factor,
        prev_bg,
    )
}

pub fn render_scene_hits(
    config: &VideoConfig,
    scene: &Scene,
    frame_in_scene: u32,
) -> Vec<rustmotion_core::engine::paint_pass::EnrichedHit> {
    use rustmotion_components::box_builder::{
        build_scene_from_refs, component_kind, BuildAnimationCtx,
    };
    use rustmotion_components::legacy_dispatch::LegacyPaintDispatcher;
    use rustmotion_core::engine::layout_pass::run_layout;
    use rustmotion_core::engine::paint_pass::{paint_tree_with_hits, EnrichedHit, PaintFrame};

    let children = prepare_scene(scene, config);

    let time = SceneTime::for_frame(scene, frame_in_scene, config.fps).seconds();
    let vw = config.width as f32;
    let vh = config.height as f32;

    let info = ImageInfo::new(
        (config.width as i32, config.height as i32),
        ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    let Some(mut surface) = surfaces::raster(&info, None, None) else {
        return Vec::new();
    };
    let canvas = surface.canvas();

    let plane_cam = per_plane_camera(scene, &children, time as f32, vw, vh);
    let _camera_guard = match effective_camera(scene) {
        Some(camera) if plane_cam.is_none() => {
            let g = super::CanvasGuard::new(canvas);
            apply_camera_transform(canvas, scene, camera, time as f32, vw, vh);
            Some(g)
        }
        _ => None,
    };

    let root_css = root_style(scene.layout.as_ref(), ViewType::Slide);
    let anim = Some(BuildAnimationCtx {
        time,
        scenario_time: time,
        scene_duration: scene.duration,
        fps: config.fps,
    });
    let built = build_scene_from_refs(children.iter(), (vw, vh), root_css, anim);
    let mut layout = run_layout(
        &built.root,
        (vw, vh),
        &ConversionContext::for_viewport(vw, vh),
    );
    apply_ghost_layout_fixup(&built, &mut layout);
    let dispatcher = LegacyPaintDispatcher::for_scene(&built);
    let frame = PaintFrame {
        light: scene_light(scene),
        time,
        scenario_time: time,
        frame_index: frame_in_scene,
        fps: config.fps,
        video_width: config.width,
        video_height: config.height,
        scene_duration: scene.duration,
        camera: plane_cam,
    };
    let hits = paint_tree_with_hits(canvas, &built.root, &layout, &frame, &dispatcher);

    hits.into_iter()
        .filter_map(|h| {
            let child = built
                .components
                .get(h.node_id as usize)
                .copied()
                .flatten()?;
            let pointer = built
                .root
                .find(h.node_id)
                .and_then(|n| n.source_path.clone());
            Some(EnrichedHit {
                node_id: h.node_id,
                kind: component_kind(&child.component).to_string(),
                rect: h.rect,
                pointer,
            })
        })
        .collect()
}

pub fn render_world_frame_scaled(
    config: &VideoConfig,
    view: &crate::schema::ResolvedView,
    timeline: &crate::engine::world::WorldTimeline,
    frame_in_view: u32,
    scenario_time: f64,
    scale_factor: f32,
) -> Result<Vec<u8>> {
    let scaled_w = (config.width as f32 * scale_factor) as i32;
    let scaled_h = (config.height as f32 * scale_factor) as i32;
    let fps = config.fps;
    let time = frame_in_view as f64 / fps as f64;

    let info = ImageInfo::new(
        (scaled_w, scaled_h),
        ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    let mut surface =
        surfaces::raster(&info, None, None).ok_or(RustmotionError::SurfaceCreation)?;
    let canvas = surface.canvas();

    if scale_factor != 1.0 {
        canvas.scale((scale_factor, scale_factor));
    }

    let vw = config.width as f32;
    let vh = config.height as f32;

    let bg_color = view
        .background
        .color
        .as_deref()
        .unwrap_or(&config.background);
    canvas.clear(color4f_from_hex(bg_color));

    let (cam_x, cam_y) = timeline.camera_at(time, &view.camera_easing);
    let world = timeline.world_extent(vw, vh);
    let viewport_cx = vw / 2.0;
    let viewport_cy = vh / 2.0;

    let visible = timeline.visible_scenes_at(time, &view.scenes, fps);

    let active_scene_idx = visible
        .iter()
        .filter(|v| !v.is_persisted)
        .map(|v| v.scene_idx)
        .max();

    if let Some(active_idx) = active_scene_idx {
        let active_scene = &view.scenes[active_idx];
        let active_bgs = if active_scene.resolved_background.animated.is_empty() {
            &view.background.animated
        } else {
            &active_scene.resolved_background.animated
        };

        let non_persisted: Vec<_> = visible.iter().filter(|v| !v.is_persisted).collect();

        if non_persisted.len() >= 2 {
            let scene_a_idx = non_persisted[0].scene_idx;
            let scene_b_idx = non_persisted[1].scene_idx;
            let scene_a = &view.scenes[scene_a_idx];
            let scene_b = &view.scenes[scene_b_idx];

            let bgs_a = if scene_a.resolved_background.animated.is_empty() {
                &view.background.animated
            } else {
                &scene_a.resolved_background.animated
            };
            let bgs_b = if scene_b.resolved_background.animated.is_empty() {
                &view.background.animated
            } else {
                &scene_b.resolved_background.animated
            };

            let pan_half = timeline
                .boundary_pan_duration
                .get(scene_b_idx.saturating_sub(1))
                .copied()
                .unwrap_or(timeline.camera_pan_duration)
                / 2.0;
            let pan_start = timeline.scene_windows[scene_b_idx].0 - pan_half;
            let pan_end = timeline.scene_windows[scene_b_idx].0 + pan_half;
            let crossfade =
                safe_div(time - pan_start, pan_end - pan_start, 1.0).clamp(0.0, 1.0) as f32;

            if std::ptr::eq(bgs_a, bgs_b) {
                for bg in bgs_a {
                    draw_world_bg_with_parallax(
                        canvas,
                        bg,
                        time as f32,
                        vw,
                        vh,
                        cam_x,
                        cam_y,
                        world,
                    );
                }
            } else {
                let layer_a = render_world_bg_layer_pixels(
                    bgs_a,
                    time as f32,
                    vw,
                    vh,
                    cam_x,
                    cam_y,
                    world,
                    scaled_w,
                    scaled_h,
                    scale_factor,
                );
                let layer_b = render_world_bg_layer_pixels(
                    bgs_b,
                    time as f32,
                    vw,
                    vh,
                    cam_x,
                    cam_y,
                    world,
                    scaled_w,
                    scaled_h,
                    scale_factor,
                );
                if let (Some(la), Some(lb)) = (layer_a, layer_b) {
                    let blended = blend_world_bg_layers(&la, &lb, crossfade);
                    let bg_info = ImageInfo::new(
                        (scaled_w, scaled_h),
                        ColorType::RGBA8888,
                        skia_safe::AlphaType::Premul,
                        None,
                    );
                    let data = skia_safe::Data::new_copy(&blended);
                    if let Some(img) =
                        skia_safe::images::raster_from_data(&bg_info, data, scaled_w as usize * 4)
                    {
                        canvas.save();
                        if scale_factor != 1.0 {
                            canvas.reset_matrix();
                        }
                        canvas.draw_image(&img, (0.0, 0.0), None);
                        canvas.restore();
                    }
                }
            }
        } else {
            let uses_own_background = !active_scene.resolved_background.animated.is_empty();
            let bg_time = if uses_own_background {
                let local_time = visible
                    .iter()
                    .find(|v| v.scene_idx == active_idx && !v.is_persisted)
                    .map(|v| v.local_time.max(0.0))
                    .unwrap_or(time);
                SceneTime::for_local_time(active_scene, local_time).seconds() as f32
            } else {
                time as f32
            };
            for bg in active_bgs {
                draw_world_bg_with_parallax(canvas, bg, bg_time, vw, vh, cam_x, cam_y, world);
            }
        }
    } else {
        for bg in &view.background.animated {
            draw_world_bg_with_parallax(canvas, bg, time as f32, vw, vh, cam_x, cam_y, world);
        }
    }

    canvas.save();
    canvas.translate((viewport_cx - cam_x, viewport_cy - cam_y));

    for vis in &visible {
        let scene = &view.scenes[vis.scene_idx];
        let (wx, wy) = scene
            .world_position
            .as_ref()
            .map(|p| (p.x, p.y))
            .unwrap_or((vw / 2.0 + vis.scene_idx as f32 * vw, vh / 2.0));

        let needs_opacity = vis.opacity < 1.0 - f32::EPSILON;
        if needs_opacity {
            let mut layer_paint = Paint::default();
            layer_paint.set_alpha_f(vis.opacity);
            canvas.save_layer_alpha_f(None, vis.opacity);
        }

        canvas.save();
        canvas.translate((wx - viewport_cx, wy - viewport_cy));

        let scene_time = SceneTime::for_local_time(scene, vis.local_time.max(0.0));
        let anim_time = scene_time.seconds();
        let ctx = RenderContext {
            time: scene_time,
            scenario_time,
            scene_duration: scene.duration,
            frame_index: vis.local_frame,
            fps,
            video_width: config.width,
            video_height: config.height,
            stagger_offset: 0.0,
            camera: None,
        };

        let camera = effective_camera(scene);
        let has_camera = camera.is_some();
        if let Some(camera) = camera {
            apply_camera_transform(canvas, scene, camera, anim_time as f32, vw, vh);
        }

        let world_default_layout = world_default_scene_layout();
        let scene_layout = scene.layout.as_ref().unwrap_or(&world_default_layout);
        let scene_children = deserialize_children(scene);

        for child in scene_children.iter().filter(|c| c.is_decorative()) {
            paint_decorative_fullscreen(canvas, child, vw, vh, &ctx);
        }
        render_with_new_pipeline_iter(
            canvas,
            scene_children.iter().filter(|c| !c.is_decorative()),
            vw,
            vh,
            Some(scene_layout),
            &ctx,
            scene,
        );

        if has_camera {
            canvas.restore();
        }

        canvas.restore();

        if needs_opacity {
            canvas.restore();
        }
    }

    canvas.restore();

    let row_bytes = scaled_w as usize * 4;
    let mut pixels = vec![0u8; row_bytes * scaled_h as usize];
    let dst_info = ImageInfo::new(
        (scaled_w, scaled_h),
        ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    surface
        .read_pixels(&dst_info, &mut pixels, row_bytes, (0, 0))
        .then_some(())
        .ok_or(RustmotionError::PixelRead)?;

    Ok(pixels)
}

#[allow(clippy::too_many_arguments)]
fn render_world_bg_layer_pixels(
    bgs: &[crate::schema::AnimatedBackground],
    time: f32,
    vw: f32,
    vh: f32,
    cam_x: f32,
    cam_y: f32,
    world: (f32, f32, f32, f32),
    scaled_w: i32,
    scaled_h: i32,
    scale_factor: f32,
) -> Option<Vec<u8>> {
    let info = ImageInfo::new(
        (scaled_w, scaled_h),
        ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    let mut surface = surfaces::raster(&info, None, None)?;
    let canvas = surface.canvas();
    if scale_factor != 1.0 {
        canvas.scale((scale_factor, scale_factor));
    }
    canvas.clear(skia_safe::Color4f::new(0.0, 0.0, 0.0, 0.0));
    for bg in bgs {
        draw_world_bg_with_parallax(canvas, bg, time, vw, vh, cam_x, cam_y, world);
    }
    let row_bytes = scaled_w as usize * 4;
    let mut pixels = vec![0u8; row_bytes * scaled_h as usize];
    let dst_info = ImageInfo::new(
        (scaled_w, scaled_h),
        ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    surface
        .read_pixels(&dst_info, &mut pixels, row_bytes, (0, 0))
        .then_some(pixels)
}

fn blend_world_bg_layers(a: &[u8], b: &[u8], progress: f32) -> Vec<u8> {
    let inv = 1.0 - progress;
    a.iter()
        .zip(b.iter())
        .map(|(&av, &bv)| {
            let va = av as f32 * inv;
            let vb = bv as f32 * progress;
            (va + vb + 0.5) as u8
        })
        .collect()
}

pub fn render_scene_bg_scaled(
    config: &VideoConfig,
    scene: &Scene,
    frame_in_scene: u32,
    scale_factor: f32,
) -> Result<Vec<u8>> {
    let scaled_w = (config.width as f32 * scale_factor) as i32;
    let scaled_h = (config.height as f32 * scale_factor) as i32;
    let time = SceneTime::for_frame(scene, frame_in_scene, config.fps).seconds();
    let info = ImageInfo::new(
        (scaled_w, scaled_h),
        ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    let mut surface =
        surfaces::raster(&info, None, None).ok_or(RustmotionError::SurfaceCreation)?;
    let canvas = surface.canvas();
    if scale_factor != 1.0 {
        canvas.scale((scale_factor, scale_factor));
    }

    let bg = scene
        .resolved_background
        .color
        .as_deref()
        .unwrap_or(&config.background);
    canvas.clear(color4f_from_hex(bg));
    for anim_bg in &scene.resolved_background.animated {
        draw_animated_background(
            canvas,
            anim_bg,
            time as f32,
            config.width as f32,
            config.height as f32,
        );
    }

    let row_bytes = scaled_w as usize * 4;
    let mut pixels = vec![0u8; row_bytes * scaled_h as usize];
    let dst_info = ImageInfo::new(
        (scaled_w, scaled_h),
        ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    surface
        .read_pixels(&dst_info, &mut pixels, row_bytes, (0, 0))
        .then_some(())
        .ok_or(RustmotionError::PixelRead)?;
    Ok(pixels)
}

pub fn render_scene_fg_scaled(
    config: &VideoConfig,
    scene: &Scene,
    frame_in_scene: u32,
    scenario_time: f64,
    _scene_total_frames: u32,
    scale_factor: f32,
) -> Result<Vec<u8>> {
    let children = prepare_scene(scene, config);
    let scaled_w = (config.width as f32 * scale_factor) as i32;
    let scaled_h = (config.height as f32 * scale_factor) as i32;
    let scene_time = SceneTime::for_frame(scene, frame_in_scene, config.fps);
    let time = scene_time.seconds();
    let info = ImageInfo::new(
        (scaled_w, scaled_h),
        ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    let mut surface =
        surfaces::raster(&info, None, None).ok_or(RustmotionError::SurfaceCreation)?;
    let canvas = surface.canvas();
    if scale_factor != 1.0 {
        canvas.scale((scale_factor, scale_factor));
    }

    canvas.clear(skia_safe::Color4f::new(0.0, 0.0, 0.0, 0.0));

    let plane_cam = per_plane_camera(
        scene,
        &children,
        time as f32,
        config.width as f32,
        config.height as f32,
    );
    let ctx = RenderContext {
        time: scene_time,
        scenario_time,
        scene_duration: scene.duration,
        frame_index: frame_in_scene,
        fps: config.fps,
        video_width: config.width,
        video_height: config.height,
        stagger_offset: 0.0,
        camera: plane_cam,
    };

    let has_camera = effective_camera(scene).is_some() && plane_cam.is_none();
    if let (Some(camera), None) = (effective_camera(scene), plane_cam) {
        apply_camera_transform(
            canvas,
            scene,
            camera,
            time as f32,
            config.width as f32,
            config.height as f32,
        );
    }

    canvas.save();
    canvas.clip_rect(
        Rect::from_wh(config.width as f32, config.height as f32),
        ClipOp::Intersect,
        true,
    );
    render_with_new_pipeline(
        canvas,
        &children,
        config.width as f32,
        config.height as f32,
        scene.layout.as_ref(),
        &ctx,
        scene,
    );
    canvas.restore();

    if has_camera {
        canvas.restore();
    }

    let row_bytes = scaled_w as usize * 4;
    let mut pixels = vec![0u8; row_bytes * scaled_h as usize];
    let dst_info = ImageInfo::new(
        (scaled_w, scaled_h),
        ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    surface
        .read_pixels(&dst_info, &mut pixels, row_bytes, (0, 0))
        .then_some(())
        .ok_or(RustmotionError::PixelRead)?;
    Ok(pixels)
}

pub(super) fn interpolate_camera_property(camera: &Camera, property: &str, time: f32) -> f32 {
    use crate::engine::animator::ease;

    let track = camera.keyframes.iter().find(|k| k.property == property);
    let track = match track {
        Some(t) if !t.values.is_empty() => t,
        _ => {
            return match property {
                "x" => camera.x,
                "y" => camera.y,
                "zoom" => camera.zoom,
                "rotation" => camera.rotation,
                "focus" => camera.focus,
                "aperture" => camera.aperture,
                "rotate_x" => camera.rotate_x,
                "rotate_y" => camera.rotate_y,
                "perspective" => camera.perspective,
                "origin.x" => camera.origin.as_ref().map(|o| o.x).unwrap_or(0.0),
                "origin.y" => camera.origin.as_ref().map(|o| o.y).unwrap_or(0.0),
                _ => 0.0,
            };
        }
    };

    let points = &track.values;
    let t = time as f64;

    if t <= points[0].time {
        return points[0].value;
    }

    if t >= points[points.len() - 1].time {
        return points[points.len() - 1].value;
    }

    for i in 0..points.len() - 1 {
        let p0 = &points[i];
        let p1 = &points[i + 1];
        if t >= p0.time && t <= p1.time {
            let segment_t = if (p1.time - p0.time).abs() < 1e-9 {
                1.0
            } else {
                (t - p0.time) / (p1.time - p0.time)
            };
            let eased = ease(segment_t, &track.easing) as f32;
            return p0.value + (p1.value - p0.value) * eased;
        }
    }

    points[points.len() - 1].value
}

pub(super) fn resolve_camera_origin(
    camera: &Camera,
    time: f32,
    width: f32,
    height: f32,
) -> (f32, f32) {
    let has_track = |p: &str| {
        camera
            .keyframes
            .iter()
            .any(|k| k.property == p && !k.values.is_empty())
    };
    let ox = if camera.origin.is_some() || has_track("origin.x") {
        interpolate_camera_property(camera, "origin.x", time)
    } else {
        width / 2.0
    };
    let oy = if camera.origin.is_some() || has_track("origin.y") {
        interpolate_camera_property(camera, "origin.y", time)
    } else {
        height / 2.0
    };
    (ox, oy)
}

pub(super) fn apply_camera_transform(
    canvas: &Canvas,
    scene: &Scene,
    camera: &Camera,
    time: f32,
    width: f32,
    height: f32,
) {
    let (x, y, zoom, rotation) = camera_pose_at(scene, camera, time);
    let (cx, cy) = resolve_camera_origin(camera, time, width, height);

    canvas.save();

    canvas.translate((cx, cy));
    if rotation.abs() > 0.001 {
        canvas.rotate(rotation, None);
    }
    if (zoom - 1.0).abs() > 0.001 {
        canvas.scale((zoom, zoom));
    }
    canvas.translate((-cx - x, -cy - y));
}

#[cfg(test)]
mod prepared_children_tests {
    use super::*;

    fn scenario(children: &str) -> crate::schema::ResolvedScenario {
        let json = format!(
            r##"{{
              "version": "1.0",
              "video": {{ "width": 160, "height": 90, "fps": 30, "background": "#000000" }},
              "scenes": [
                {{ "duration": 1.0, "children": [{children}] }},
                {{ "duration": 1.0, "children": [{children}] }}
              ]
            }}"##
        );
        crate::loader::load_scenario_from_source(None, Some(&json)).expect("load")
    }

    const TEXT: &str = r##"{ "type": "text", "content": "hi", "style": { "font-size": 20 } }"##;

    #[test]
    fn a_scene_is_deserialized_once_and_every_later_frame_gets_that_same_vec() {
        let scenario = scenario(TEXT);
        let scene = &scenario.views[0].scenes[0];

        let first = prepare_scene(scene, &scenario.video);
        let second = prepare_scene(scene, &scenario.video);
        assert!(
            Arc::ptr_eq(&first, &second),
            "prepare_scene runs once per frame and its result depends only on the scene — a \
             second call must hand back the same allocation, not re-parse a 57-variant untagged \
             enum for every child"
        );
        assert_eq!(first.len(), 1);
    }

    #[test]
    fn two_scenes_do_not_share_one_anothers_children() {
        let scenario = scenario(TEXT);
        let first = prepare_scene(&scenario.views[0].scenes[0], &scenario.video);
        let second = prepare_scene(&scenario.views[0].scenes[1], &scenario.video);
        assert!(
            !Arc::ptr_eq(&first, &second),
            "the memo lives on the scene, so two scenes must each get their own"
        );
    }

    #[test]
    fn the_memo_carries_the_children_the_scene_actually_declares() {
        let scenario = scenario(TEXT);
        let prepared = prepare_scene(&scenario.views[0].scenes[0], &scenario.video);
        assert_eq!(
            prepared.len(),
            scenario.views[0].scenes[0].children.len(),
            "every readable child must be there — the memo is a cache, not a filter"
        );
    }

    #[test]
    fn a_child_that_cannot_be_read_is_reported_once_rather_than_once_per_frame() {
        let scenario = scenario(r##"{ "type": "no-such-component" }"##);
        let scene = &scenario.views[0].scenes[0];
        let first = prepare_scene(scene, &scenario.video);
        let second = prepare_scene(scene, &scenario.video);
        assert!(first.is_empty(), "the unreadable child is dropped");
        assert!(
            Arc::ptr_eq(&first, &second),
            "the stderr warning deserialize_children emits rode on that repetition — one \
             deserialization means one warning per scene, not one per frame"
        );
    }
}

#[cfg(test)]
mod halo_transition_wiring_tests {
    use crate::encode::video::{build_frame_tasks, render_frame_task, FrameTask};

    const W: usize = 400;
    const H: usize = 300;

    fn two_scene_halo_scenario(
        from_x: f64,
        to_x: f64,
        transition_duration: f64,
    ) -> crate::schema::ResolvedScenario {
        let json = format!(
            r##"{{
              "version": "1.0",
              "video": {{ "width": {W}, "height": {H}, "fps": 30, "background": "#000000" }},
              "scenes": [
                {{
                  "duration": 1.0,
                  "background": {{
                    "preset": "halo",
                    "halo": {{ "zones": [
                      {{ "color": "#FFFFFF", "x": {from_x}, "y": 0.5, "radius": 0.15,
                         "opacity": 1.0 }}
                    ] }}
                  }},
                  "children": []
                }},
                {{
                  "duration": 2.0,
                  "background": {{
                    "preset": "halo",
                    "halo": {{ "zones": [
                      {{ "color": "#FFFFFF", "x": {to_x}, "y": 0.5, "radius": 0.15,
                         "opacity": 1.0 }}
                    ] }},
                    "transition": {{ "duration": {transition_duration}, "easing": "linear" }}
                  }},
                  "children": []
                }}
              ]
            }}"##
        );
        crate::loader::load_scenario_from_source(None, Some(&json)).expect("load")
    }

    fn frame_at_scene_1_time(
        scenario: &crate::schema::ResolvedScenario,
        fps: u32,
        t: f64,
    ) -> Vec<u8> {
        let target_frame_in_scene = (t * fps as f64).round() as u32;
        let tasks = build_frame_tasks(scenario);
        let task = tasks
            .iter()
            .find(|task| {
                matches!(
                    task,
                    FrameTask::Normal {
                        scene_idx: 1,
                        frame_in_scene,
                        ..
                    } if *frame_in_scene == target_frame_in_scene
                )
            })
            .expect("a frame task for scene 1 at the requested time must exist");
        render_frame_task(&scenario.video, scenario, task).expect("render")
    }

    fn luma_at(buf: &[u8], w: usize, x: usize, y: usize) -> f32 {
        let i = (y * w + x) * 4;
        0.299 * buf[i] as f32 + 0.587 * buf[i + 1] as f32 + 0.114 * buf[i + 2] as f32
    }

    #[test]
    fn a_halo_transition_moves_the_zone_instead_of_cross_fading_two_static_copies() {
        let scenario = two_scene_halo_scenario(0.1, 0.9, 1.0);
        let mid = frame_at_scene_1_time(&scenario, 30, 0.5);

        let from_scene_position = (0.1 * W as f64) as usize;
        let luma_at_from_position = luma_at(&mid, W, from_scene_position, H / 2);

        assert!(
            luma_at_from_position < 40.0,
            "halfway through the transition, the outgoing scene's own halo position \
             (x={from_scene_position}) must show background again — a single interpolated \
             zone has already moved on towards the frame's midpoint by t=0.5; a leftover \
             50%-alpha copy still sitting at its original spot would mean the transition \
             cross-fades two static renders instead of interpolating one shape. measured \
             luma={luma_at_from_position}"
        );
    }
}

#[cfg(test)]
mod camera_motion_blur_tests {
    use super::camera_motion_blur_plan;
    use crate::encode::video::{build_frame_tasks, render_frame_task, FrameTask};

    const W: usize = 400;
    const H: usize = 200;

    fn panning_camera_scenario(motion_blur_json: &str) -> crate::schema::ResolvedScenario {
        let json = format!(
            r##"{{
              "version": "1.0",
              "video": {{ "width": {W}, "height": {H}, "fps": 30, "background": "#000000" }},
              "scenes": [{{
                "duration": 1.0,
                "camera": {{
                  "keyframes": [{{ "property": "x", "easing": "linear", "values": [
                    {{ "time": 0.0, "value": 0.0 }}, {{ "time": 0.45, "value": 0.0 }},
                    {{ "time": 0.55, "value": 60.0 }}, {{ "time": 1.0, "value": 60.0 }}
                  ] }}]{motion_blur_json}
                }},
                "children": [
                  {{ "type": "shape", "shape": "rect", "fill": "#FFFFFF",
                     "position": "absolute", "x": 200, "y": 0,
                     "style": {{ "width": 400, "height": 200 }} }}
                ]
              }}]
            }}"##
        );
        crate::loader::load_scenario_from_source(None, Some(&json)).expect("load")
    }

    fn frame_15(scenario: &crate::schema::ResolvedScenario) -> Vec<u8> {
        let tasks = build_frame_tasks(scenario);
        let task = tasks
            .iter()
            .find(|task| {
                matches!(
                    task,
                    FrameTask::Normal {
                        scene_idx: 0,
                        frame_in_scene: 15,
                        ..
                    }
                )
            })
            .expect("a frame task for scene 0 frame 15 must exist");
        render_frame_task(&scenario.video, scenario, task).expect("render")
    }

    fn rgb_at(frame: &[u8], x: usize, y: usize) -> (u8, u8, u8) {
        let i = (y * W + x) * 4;
        (frame[i], frame[i + 1], frame[i + 2])
    }

    #[test]
    fn a_fast_pan_with_motion_blur_softens_the_edge_a_crisp_render_would_keep_hard() {
        let scenario =
            panning_camera_scenario(r##", "motion_blur": { "samples": 8, "shutter": 1.0 }"##);
        let frame = frame_15(&scenario);

        let (r, g, b) = rgb_at(&frame, 180, 100);
        assert!(
            (60..=200).contains(&r) && (60..=200).contains(&g) && (60..=200).contains(&b),
            "the camera pans 60px in the 0.1s straddling this frame, so the rectangle's left \
             edge (nominally at screen x=170 at t=0.5) sweeps across x=180 for exactly half of \
             the 8 sub-frame samples this shutter window covers — averaging them must leave an \
             intermediate grey, not the hard black/white a single-sample render would give; \
             got rgb=({r},{g},{b})"
        );

        let (wr, wg, wb) = rgb_at(&frame, 350, 100);
        assert!(
            wr > 240 && wg > 240 && wb > 240,
            "well inside the rectangle for every sample in the window, the pixel must still \
             read as solid white; got rgb=({wr},{wg},{wb})"
        );

        let (br, bg, bb) = rgb_at(&frame, 30, 100);
        assert!(
            br < 15 && bg < 15 && bb < 15,
            "well outside the rectangle for every sample in the window, the pixel must still \
             read as solid background; got rgb=({br},{bg},{bb})"
        );
    }

    #[test]
    fn no_motion_blur_config_keeps_the_same_pan_perfectly_crisp() {
        let scenario = panning_camera_scenario("");
        let frame = frame_15(&scenario);

        let (r, g, b) = rgb_at(&frame, 180, 100);
        assert!(
            r > 240 && g > 240 && b > 240,
            "without camera.motion_blur, x=180 at t=0.5 is strictly past the single-sample \
             edge (screen x=170) and must read as solid white, not the blurred grey the other \
             test expects at the same coordinate; got rgb=({r},{g},{b})"
        );
    }

    #[test]
    fn a_camera_that_is_not_moving_is_not_planned_for_sub_frame_sampling() {
        let scenario = crate::loader::load_scenario_from_source(
            None,
            Some(&format!(
                r##"{{
                  "version": "1.0",
                  "video": {{ "width": {W}, "height": {H}, "fps": 30, "background": "#000000" }},
                  "scenes": [{{
                    "duration": 1.0,
                    "camera": {{ "zoom": 1.5,
                      "motion_blur": {{ "samples": 8, "shutter": 1.0 }} }},
                    "children": []
                  }}]
                }}"##
            )),
        )
        .expect("load");
        let scene = &scenario.views[0].scenes[0];

        assert!(
            camera_motion_blur_plan(scene, 15, 30).is_none(),
            "a camera held at a fixed pose must not be scheduled for sub-frame sampling — \
             there is nothing to average, only cost to pay"
        );
    }

    #[test]
    fn a_panning_camera_is_planned_for_sub_frame_sampling() {
        let scenario =
            panning_camera_scenario(r##", "motion_blur": { "samples": 8, "shutter": 1.0 }"##);
        let scene = &scenario.views[0].scenes[0];

        let plan = camera_motion_blur_plan(scene, 15, 30)
            .expect("a moving camera with motion_blur configured must be planned");
        assert_eq!(plan.samples, 8);
        assert!((plan.shutter_seconds - 1.0 / 30.0).abs() < 1e-9);
    }
}

#[cfg(test)]
mod ghost_in_flow_placement_tests {
    use crate::encode::video::{build_frame_tasks, render_frame_task, FrameTask};

    const W: usize = 400;
    const H: usize = 200;

    fn row_scenario_with_motion_blur_on_second_child() -> crate::schema::ResolvedScenario {
        let json = format!(
            r##"{{
              "version": "1.0",
              "video": {{ "width": {W}, "height": {H}, "fps": 30, "background": "#000000" }},
              "scenes": [{{
                "duration": 1.0,
                "children": [{{
                  "type": "div",
                  "style": {{ "display": "flex", "flex-direction": "row",
                              "width": {W}, "height": {H} }},
                  "children": [
                    {{ "type": "shape", "shape": "rect", "fill": "#FF0000",
                       "style": {{ "width": 100, "height": 100 }} }},
                    {{ "type": "shape", "shape": "rect", "fill": "#0000FF",
                       "style": {{ "width": 100, "height": 100,
                         "animation": [
                           {{ "name": "motion_blur", "samples": 8, "shutter": 1.0 }}
                         ] }} }}
                  ]
                }}]
              }}]
            }}"##
        );
        crate::loader::load_scenario_from_source(None, Some(&json)).expect("load")
    }

    fn first_frame(scenario: &crate::schema::ResolvedScenario) -> Vec<u8> {
        let tasks = build_frame_tasks(scenario);
        let task = tasks
            .iter()
            .find(|task| {
                matches!(
                    task,
                    FrameTask::Normal {
                        scene_idx: 0,
                        frame_in_scene: 0,
                        ..
                    }
                )
            })
            .expect("a frame task for scene 0 frame 0 must exist");
        render_frame_task(&scenario.video, scenario, task).expect("render")
    }

    #[test]
    fn a_motion_blurred_flex_sibling_does_not_ghost_onto_the_previous_in_flow_item() {
        let scenario = row_scenario_with_motion_blur_on_second_child();
        let frame = first_frame(&scenario);

        let i = (50 * W + 50) * 4;
        let (r, g, b) = (frame[i] as i32, frame[i + 1] as i32, frame[i + 2] as i32);
        assert!(
            b < 20,
            "the first flex item's own box (pure red, at x=50,y=50) must not carry a blue \
             tint from its motion-blurred sibling's ghosts — an in-flow ghost with no inset \
             lands wherever the container's default alignment puts an absolute child with no \
             top/left, which for a row is the same spot as the first item; got rgb=({r},{g},{b})"
        );
        assert!(
            r > 200,
            "the first flex item must still read as solid red where no ghost should ever \
             reach; got rgb=({r},{g},{b})"
        );
    }
}
