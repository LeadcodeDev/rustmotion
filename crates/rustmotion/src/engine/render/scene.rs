use crate::error::Result;
use skia_safe::{surfaces, Canvas, ClipOp, ColorType, ImageInfo, Paint, Rect};

use super::background::draw_animated_background;
use super::background::draw_world_bg_with_parallax;
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
    let scaled_w = (config.width as f32 * scale_factor) as i32;
    let scaled_h = (config.height as f32 * scale_factor) as i32;
    let scene_time = SceneTime::for_frame(scene, frame_index, config.fps);
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

            if progress < 1.0 {
                let bg_info = ImageInfo::new(
                    (scaled_w, scaled_h),
                    ColorType::RGBA8888,
                    skia_safe::AlphaType::Premul,
                    None,
                );
                if let Some(mut prev_surface) = surfaces::raster(&bg_info, None, None) {
                    let prev_canvas = prev_surface.canvas();
                    if scale_factor != 1.0 {
                        prev_canvas.scale((scale_factor, scale_factor));
                    }
                    prev_canvas.clear(skia_safe::Color4f::new(0.0, 0.0, 0.0, 0.0));
                    for anim_bg in &prev.animated {
                        draw_animated_background(prev_canvas, anim_bg, continuous_time, w, h);
                    }
                    let snapshot = prev_surface.image_snapshot();
                    let mut paint = Paint::default();
                    paint.set_alpha_f(1.0 - progress);
                    canvas.save();
                    if scale_factor != 1.0 {
                        canvas.reset_matrix();
                    }
                    canvas.draw_image(&snapshot, (0.0, 0.0), Some(&paint));
                    canvas.restore();
                }
            }
            if progress > 0.0 {
                let bg_info = ImageInfo::new(
                    (scaled_w, scaled_h),
                    ColorType::RGBA8888,
                    skia_safe::AlphaType::Premul,
                    None,
                );
                if let Some(mut cur_surface) = surfaces::raster(&bg_info, None, None) {
                    let cur_canvas = cur_surface.canvas();
                    if scale_factor != 1.0 {
                        cur_canvas.scale((scale_factor, scale_factor));
                    }
                    cur_canvas.clear(skia_safe::Color4f::new(0.0, 0.0, 0.0, 0.0));
                    for anim_bg in &cur_bg.animated {
                        draw_animated_background(cur_canvas, anim_bg, continuous_time, w, h);
                    }
                    let snapshot = cur_surface.image_snapshot();
                    let mut paint = Paint::default();
                    paint.set_alpha_f(progress);
                    canvas.save();
                    if scale_factor != 1.0 {
                        canvas.reset_matrix();
                    }
                    canvas.draw_image(&snapshot, (0.0, 0.0), Some(&paint));
                    canvas.restore();
                }
            }
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

    let layout = run_layout(&built.root, viewport, &conversion);
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

pub fn prepare_scene(scene: &Scene, _config: &VideoConfig) -> Vec<ChildComponent> {
    deserialize_children(scene)
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
    let layout = run_layout(
        &built.root,
        (vw, vh),
        &ConversionContext::for_viewport(vw, vh),
    );
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
    let shake = scene_shake_offset(scene, time);
    let x = interpolate_camera_property(camera, "x", time) + shake.x as f32;
    let y = interpolate_camera_property(camera, "y", time) + shake.y as f32;
    let zoom = interpolate_camera_property(camera, "zoom", time);
    let rotation = interpolate_camera_property(camera, "rotation", time) + shake.rotation as f32;
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
