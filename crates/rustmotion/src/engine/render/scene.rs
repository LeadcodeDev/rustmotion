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
use rustmotion_core::css::units::{LengthContext, LengthPercentage};
use rustmotion_core::engine::animator::safe_div;
use rustmotion_core::engine::paint_pass::PlaneCamera;
use rustmotion_core::engine::renderer::color4f_from_hex;

/// Build the `ConversionContext` that resolves `vw`/`vh`/`%` units for a
/// layout pass, anchored to the *real* output viewport instead of
/// `ConversionContext::default()`'s hardcoded 1920×1080 (round 4 audit,
/// lot LAYOUT, constat 1). On a 1080×1920 vertical video — a resolution
/// this project documents as a common target — `width: "50vw"` used to
/// resolve as 50% of a phantom 1920px-wide viewport (960px) instead of 50%
/// of the real 1080px one (540px), a 78% error, and `vh` was off by the
/// same margin in the other axis. `font-size`/`root-font-size` stay at the
/// CSS initial `16px`: nothing upstream of this call resolves and threads a
/// root font-size through yet.
fn viewport_conversion_context(viewport_w: f32, viewport_h: f32) -> ConversionContext {
    ConversionContext {
        length: LengthContext {
            viewport_width: viewport_w,
            viewport_height: viewport_h,
            ..LengthContext::default()
        },
    }
}

/// Internal render-time context — bundles per-scene timing/dimension info that
/// the scene renderer threads down into its helpers. This is intentionally
/// private to the scene renderer; component painters receive `PaintCtx`.
#[derive(Debug, Clone)]
struct RenderContext {
    time: f64,
    scene_duration: f64,
    frame_index: u32,
    fps: u32,
    video_width: u32,
    video_height: u32,
    #[allow(dead_code)]
    stagger_offset: f64,
    /// Resolved scene camera for per-plane parallax (issue #90). `Some` only
    /// when the scene has a camera AND at least one top-level child declares
    /// `style.depth` — the paint pass then applies the camera per plane and
    /// the global `apply_camera_transform` must be skipped.
    camera: Option<PlaneCamera>,
}

/// True when any direct child of the scene declares an explicit `style.depth`
/// — the v1 parallax plane rule (planes = top-level children only).
fn scene_uses_depth(children: &[ChildComponent]) -> bool {
    children
        .iter()
        .any(|c| c.component.as_styled().style_config().depth.is_some())
}

/// Resolve the scene camera at `time` into the flat state consumed by the
/// per-plane paint path.
fn resolve_plane_camera(camera: &Camera, time: f32, vw: f32, vh: f32) -> PlaneCamera {
    let (origin_x, origin_y) = resolve_camera_origin(camera, time, vw, vh);
    PlaneCamera {
        pan_x: interpolate_camera_property(camera, "x", time),
        pan_y: interpolate_camera_property(camera, "y", time),
        zoom: interpolate_camera_property(camera, "zoom", time),
        rotation: interpolate_camera_property(camera, "rotation", time),
        origin_x,
        origin_y,
    }
}

/// Decide the camera mode for a slide-scene render: per-plane (`Some`) when
/// depth is in play, otherwise `None` (caller applies the global transform).
fn per_plane_camera(
    scene: &Scene,
    children: &[ChildComponent],
    time: f32,
    vw: f32,
    vh: f32,
) -> Option<PlaneCamera> {
    match &scene.camera {
        Some(cam) if scene_uses_depth(children) => Some(resolve_plane_camera(cam, time, vw, vh)),
        _ => None,
    }
}

/// Render a complete frame using the v2 component pipeline.
///
/// This function has the same signature as `render_frame` for easy integration
/// with the existing encoding pipeline.
pub fn render_frame_v2(
    config: &VideoConfig,
    scene: &Scene,
    frame_index: u32,
    _total_frames: u32,
    root_children: &[ChildComponent],
) -> Result<Vec<u8>> {
    render_frame_v2_scaled(
        config,
        scene,
        frame_index,
        _total_frames,
        root_children,
        1.0,
        None,
    )
}

/// Render a frame with an optional scale factor for higher-resolution output.
/// The layout is computed at video dimensions; the surface and rendering are
/// scaled up so text and vector graphics remain sharp.
///
/// `prev_bg` is the resolved background of the previous scene (for transition interpolation).
/// `prev_scene_duration` is the duration of the previous scene (for continuous animation time).
pub fn render_frame_v2_scaled(
    config: &VideoConfig,
    scene: &Scene,
    frame_index: u32,
    _total_frames: u32,
    root_children: &[ChildComponent],
    scale_factor: f32,
    prev_bg: Option<(&crate::schema::ResolvedBackground, f64)>,
) -> Result<Vec<u8>> {
    let scaled_w = (config.width as f32 * scale_factor) as i32;
    let scaled_h = (config.height as f32 * scale_factor) as i32;
    let mut time = frame_index as f64 / config.fps as f64;

    // Apply freeze_at
    if let Some(freeze_at) = scene.freeze_at {
        if time > freeze_at {
            time = freeze_at;
        }
    }

    let info = ImageInfo::new(
        (scaled_w, scaled_h),
        ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );

    let mut surface =
        surfaces::raster(&info, None, None).ok_or(RustmotionError::SurfaceCreation)?;

    let canvas = surface.canvas();

    // Apply scale factor before rendering
    if scale_factor != 1.0 {
        canvas.scale((scale_factor, scale_factor));
    }

    // Fill background
    let bg = scene
        .resolved_background
        .color
        .as_deref()
        .unwrap_or(&config.background);
    canvas.clear(color4f_from_hex(bg));

    // Animated background gradient(s) — with optional transition crossfade
    let cur_bg = &scene.resolved_background;
    if let (Some(ref transition), Some((prev, prev_duration))) = (&cur_bg.transition, prev_bg) {
        let t_elapsed = time;
        // Continuous time: add prev scene duration so animation doesn't jump at scene boundary
        let continuous_time = (prev_duration + time) as f32;
        if t_elapsed < transition.duration && !prev.animated.is_empty() {
            let progress = crate::engine::animator::ease(
                (t_elapsed / transition.duration).clamp(0.0, 1.0),
                &transition.easing,
            ) as f32;
            let w = config.width as f32;
            let h = config.height as f32;

            // Crossfade: draw prev bg (fading out) then current bg (fading in) on separate layers.
            // This avoids artifacts from interpolating speed/spacing directly.
            if progress < 1.0 {
                // Draw previous background with fading alpha
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
                    // Draw the pre-scaled snapshot in surface (post-scale) coords.
                    // save()/restore() bracket the matrix change so the scale
                    // factor returns to its prior value after restore — adding
                    // another canvas.scale() afterwards would compound it.
                    canvas.save();
                    if scale_factor != 1.0 {
                        canvas.reset_matrix();
                    }
                    canvas.draw_image(&snapshot, (0.0, 0.0), Some(&paint));
                    canvas.restore();
                }
            }
            if progress > 0.0 {
                // Draw current background with growing alpha
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
            // Transition ended — keep using continuous time for the rest of the scene
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

    // Camera mode: per-plane when depth is declared, global otherwise.
    let plane_cam = per_plane_camera(
        scene,
        root_children,
        time as f32,
        config.width as f32,
        config.height as f32,
    );

    // Build render context
    let ctx = RenderContext {
        time,
        scene_duration: scene.duration,
        frame_index,
        fps: config.fps,
        video_width: config.width,
        video_height: config.height,
        stagger_offset: 0.0,
        camera: plane_cam,
    };

    // Apply the global virtual camera transform (skipped in per-plane mode —
    // the paint pass applies it per top-level plane, scaled by depth).
    let camera_guard = match &scene.camera {
        Some(camera) if plane_cam.is_none() => {
            let g = super::CanvasGuard::new(canvas);
            apply_camera_transform(
                canvas,
                camera,
                time as f32,
                config.width as f32,
                config.height as f32,
            );
            Some(g)
        }
        _ => None,
    };

    // Clip scene content to viewport dimensions so scaled elements don't overflow.
    // Both guards drop in reverse order on early-return, restoring the canvas.
    let clip_guard = super::CanvasGuard::new(canvas);
    canvas.clip_rect(
        Rect::from_wh(config.width as f32, config.height as f32),
        ClipOp::Intersect,
        true,
    );

    // Render component tree through taffy + paint_tree + LegacyPaintDispatcher.
    render_with_new_pipeline(
        canvas,
        root_children,
        config.width as f32,
        config.height as f32,
        scene.layout.as_ref(),
        &ctx,
    );

    drop(clip_guard);
    drop(camera_guard);

    // Read pixels at scaled dimensions
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

/// The implicit layout a `world` scene gets when it declares no `layout` of
/// its own (round 4 audit, lot VALIDATION GÉOMÉTRIQUE, constat 4) — a
/// centred column, unlike a `slide` scene's implicit top-aligned column.
/// Single source of truth for both `render_world_frame_scaled` (which used
/// to inline this same struct literal) and `root_style`'s `ViewType::World`
/// branch below, so the geometry validator and the renderer can never see a
/// different default for the same (layout-absent) world scene.
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

/// Build a `CssStyle` from an optional `SceneLayout` for root-level flex
/// layout. `view_type` decides the fallback when `scene_layout` is absent:
/// a `slide` scene falls back to a plain top-aligned column (the historical
/// behaviour, byte-identical to before this parameter existed); a `world`
/// scene falls back to [`world_default_scene_layout`] — the same centred
/// layout `render_world_frame_scaled` synthesizes for a layout-absent world
/// scene. Before this, callers outside `scene.rs` (the geometry validator)
/// had no way to ask for the world fallback and always got the slide one,
/// validating a different root box than the one `render_world_frame_scaled`
/// actually lays out against.
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
    // Default direction is column (like a web page)
    if style.flex_direction.is_none() {
        style.flex_direction = Some(CssFlexDirection::Column);
    }
    style
}

/// Render `root_children` through the CSS-engine pipeline:
/// build a `BoxNode` tree, run taffy to lay it out, then paint via
/// `paint_tree` with the `LegacyPaintDispatcher` bridging to component
/// `Painter::paint_content` impls.
fn render_with_new_pipeline(
    canvas: &Canvas,
    root_children: &[ChildComponent],
    viewport_w: f32,
    viewport_h: f32,
    scene_layout: Option<&SceneLayout>,
    ctx: &RenderContext,
) {
    render_with_new_pipeline_iter(
        canvas,
        root_children.iter(),
        viewport_w,
        viewport_h,
        scene_layout,
        ctx,
    );
}

/// Iterator-based variant for callers (like world rendering) that want to
/// pass a filtered subset of children without cloning.
fn render_with_new_pipeline_iter<'a, I>(
    canvas: &Canvas,
    root_children: I,
    viewport_w: f32,
    viewport_h: f32,
    scene_layout: Option<&SceneLayout>,
    ctx: &RenderContext,
) where
    I: IntoIterator<Item = &'a ChildComponent>,
{
    use rustmotion_components::box_builder::{build_scene_from_refs, BuildAnimationCtx};
    use rustmotion_components::legacy_dispatch::LegacyPaintDispatcher;
    use rustmotion_core::engine::layout_pass::run_layout;
    use rustmotion_core::engine::paint_pass::{paint_tree, PaintFrame};

    // Mirror the legacy `root_style` so the new pipeline applies the same
    // scene-level flex configuration (direction, gap, padding, alignment).
    // `ViewType::Slide` here: this helper is shared by the ordinary
    // per-scene render path (always slide scenes — see `render_frame_v2_scaled`
    // / `render_scene_fg_scaled`) AND `render_world_frame_scaled` below, but
    // the latter always resolves its own `scene_layout` fallback (via
    // `world_default_scene_layout`) *before* calling in here, so
    // `scene_layout` is never `None` on that path — the `ViewType::World`
    // branch is unreachable from this call site either way.
    let root_css = root_style(scene_layout, ViewType::Slide);

    let anim = Some(BuildAnimationCtx {
        time: ctx.time,
        scene_duration: ctx.scene_duration,
        fps: ctx.fps,
    });
    let built = build_scene_from_refs(root_children, (viewport_w, viewport_h), root_css, anim);
    let layout = run_layout(
        &built.root,
        (viewport_w, viewport_h),
        &viewport_conversion_context(viewport_w, viewport_h),
    );
    let dispatcher = LegacyPaintDispatcher::for_scene(&built);
    let frame = PaintFrame {
        time: ctx.time,
        frame_index: ctx.frame_index,
        fps: ctx.fps,
        video_width: ctx.video_width,
        video_height: ctx.video_height,
        scene_duration: ctx.scene_duration,
        camera: ctx.camera,
    };
    paint_tree(canvas, &built.root, &layout, &frame, &dispatcher);
}

/// Paint a decorative leaf (e.g. Particle) over the full viewport without
/// going through taffy. Resolves animations and dispatches to
/// `Painter::paint_content` directly with a viewport-sized `BoxLayout`.
fn paint_decorative_fullscreen(
    canvas: &Canvas,
    child: &ChildComponent,
    viewport_w: f32,
    viewport_h: f32,
    ctx: &RenderContext,
) {
    use rustmotion_core::engine::animator::{resolve_props_for_effects, AnimatedProperties};
    use rustmotion_core::engine::layout_pass::BoxLayout;
    use rustmotion_core::traits::PaintCtx;

    if let Some(timed) = child.component.as_timed() {
        let (start_at, end_at) = timed.timing();
        if let Some(s) = start_at {
            if ctx.time < s {
                return;
            }
        }
        if let Some(e) = end_at {
            if ctx.time > e {
                return;
            }
        }
    }

    let props = match child.component.as_animatable() {
        Some(a) => {
            let effects = a.animation_effects();
            if effects.is_empty() {
                AnimatedProperties::default()
            } else {
                resolve_props_for_effects(effects, ctx.time, ctx.scene_duration)
            }
        }
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
        time: ctx.time,
        scene_duration: ctx.scene_duration,
        frame_index: ctx.frame_index,
        fps: ctx.fps,
        video_width: ctx.video_width,
        video_height: ctx.video_height,
        stagger_offset: 0.0,
    };
    canvas.save();
    painter.paint_content(canvas, &local, &props, &paint_ctx);
    canvas.restore();
}

/// Deserialize a scene's raw JSON children into typed ChildComponents.
///
/// Children that fail to deserialize are skipped, but a warning is emitted
/// to stderr so the user knows why a card or row went missing instead of
/// staring at a blank scene. (Render must remain best-effort: dropping a
/// single broken child should not nuke the whole frame.)
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

/// Deserialize a scene's children — ready for render_frame_v2.
pub fn prepare_scene(scene: &Scene, _config: &VideoConfig) -> Vec<ChildComponent> {
    deserialize_children(scene)
}

/// Render a single frame using the v2 pipeline.
/// This is the unified entry point for both single-frame and video encoding.
pub fn render_scene_frame(
    config: &VideoConfig,
    scene: &Scene,
    frame_in_scene: u32,
    scene_total_frames: u32,
) -> Result<Vec<u8>> {
    let children = prepare_scene(scene, config);
    render_frame_v2(config, scene, frame_in_scene, scene_total_frames, &children)
}

pub fn render_scene_frame_scaled(
    config: &VideoConfig,
    scene: &Scene,
    frame_in_scene: u32,
    scene_total_frames: u32,
    scale_factor: f32,
) -> Result<Vec<u8>> {
    let children = prepare_scene(scene, config);
    render_frame_v2_scaled(
        config,
        scene,
        frame_in_scene,
        scene_total_frames,
        &children,
        scale_factor,
        None,
    )
}

/// Like `render_scene_frame_scaled` but with the previous scene's resolved background for transition interpolation.
/// `prev_bg` is (resolved_background, prev_scene_duration).
pub fn render_scene_frame_scaled_with_prev_bg(
    config: &VideoConfig,
    scene: &Scene,
    frame_in_scene: u32,
    scene_total_frames: u32,
    scale_factor: f32,
    prev_bg: Option<(&crate::schema::ResolvedBackground, f64)>,
) -> Result<Vec<u8>> {
    let children = prepare_scene(scene, config);
    render_frame_v2_scaled(
        config,
        scene,
        frame_in_scene,
        scene_total_frames,
        &children,
        scale_factor,
        prev_bg,
    )
}

/// Compute the per-frame enriched hit-map for a scene at video resolution.
/// Mirrors the geometry setup of `render_frame_v2_scaled` (same time, scene
/// duration, camera transform, build + layout) but paints to a throwaway
/// surface purely to collect the on-screen bounding box of each component.
/// Used by the studio overlay; not part of the video encode path.
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

    let mut time = frame_in_scene as f64 / config.fps as f64;
    if let Some(freeze_at) = scene.freeze_at {
        if time > freeze_at {
            time = freeze_at;
        }
    }
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

    // Match the camera transform so hit rects line up with the rendered
    // frame. In per-plane mode the paint pass applies the (depth-scaled)
    // camera per top-level plane; the canvas matrix at each node then feeds
    // `local_to_device` so hit rects follow their plane automatically.
    let plane_cam = per_plane_camera(scene, &children, time as f32, vw, vh);
    let _camera_guard = match &scene.camera {
        Some(camera) if plane_cam.is_none() => {
            let g = super::CanvasGuard::new(canvas);
            apply_camera_transform(canvas, camera, time as f32, vw, vh);
            Some(g)
        }
        _ => None,
    };

    // `ViewType::Slide`: `render_scene_hits` is only ever called for
    // `FrameTask::Normal` (slide-view scenes) — world frames build hits
    // through a different path — see `encode/video/tasks.rs::render_frame_task_hits`.
    let root_css = root_style(scene.layout.as_ref(), ViewType::Slide);
    let anim = Some(BuildAnimationCtx {
        time,
        scene_duration: scene.duration,
        fps: config.fps,
    });
    let built = build_scene_from_refs(children.iter(), (vw, vh), root_css, anim);
    let layout = run_layout(&built.root, (vw, vh), &viewport_conversion_context(vw, vh));
    let dispatcher = LegacyPaintDispatcher::for_scene(&built);
    let frame = PaintFrame {
        time,
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

/// Render a single frame of a world view: shared background, camera translation, visible scenes.
pub fn render_world_frame_scaled(
    config: &VideoConfig,
    view: &crate::schema::ResolvedView,
    timeline: &crate::engine::world::WorldTimeline,
    frame_in_view: u32,
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

    // 1. Draw base background (view-level or video-level)
    let bg_color = view
        .background
        .color
        .as_deref()
        .unwrap_or(&config.background);
    canvas.clear(color4f_from_hex(bg_color));

    // Pre-compute camera position for background parallax
    let (cam_x, cam_y) = timeline.camera_at(time, &view.camera_easing);
    let viewport_cx = vw / 2.0;
    let viewport_cy = vh / 2.0;

    // 2. Draw animated backgrounds
    // Determine which scene's backgrounds to use (crossfade during pan)
    let visible = timeline.visible_scenes_at(time, &view.scenes, fps);

    // Find the "active" scene (the one currently being displayed, latest in sequence)
    // and optionally a "previous" scene during camera pan for crossfade
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

        // Check if we're during a camera pan (two non-persisted scenes visible)
        let non_persisted: Vec<_> = visible.iter().filter(|v| !v.is_persisted).collect();

        if non_persisted.len() >= 2 {
            // Crossfade between outgoing and incoming scene backgrounds
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

            // Calculate crossfade progress based on camera pan position.
            // This boundary's pan sits between `scene_a_idx` and
            // `scene_b_idx`, i.e. `boundary_pan_duration[scene_b_idx - 1]`
            // (see `WorldTimeline::boundary_pan_duration`) — using the
            // single view-level `camera_pan_duration` here instead would
            // desync this background crossfade window from the foreground
            // opacity/camera windows whenever the per-boundary clamp kicks
            // in (pan longer than a neighbouring scene's duration).
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
                // Same backgrounds on both sides (the documented recipe: a
                // shared `view.background` and no per-scene override) — a
                // crossfade of a layer with itself is that layer, so just
                // paint it once instead of doing the work twice.
                for bg in bgs_a {
                    draw_world_bg_with_parallax(canvas, bg, time as f32, vw, vh, cam_x, cam_y);
                }
            } else {
                // Distinct per-scene backgrounds: render each side into its
                // own transparent, full-frame layer and crossfade the RAW
                // pixel buffers in f32 — the same technique
                // `camera_pan_transition`'s `Static` mode uses for the
                // slide-view side of a scene-to-scene cut (see
                // `crates/rustmotion-core/src/engine/transition.rs`).
                //
                // The previous version painted `bgs_a` straight onto the
                // canvas at full alpha unconditionally, then composited
                // `bgs_b` over it through Skia's 8-bit Paint alpha: scene
                // A's background never faded at all, and scene B's alpha
                // byte discharged its whole accumulated per-frame
                // truncation as a single jump the instant `non_persisted`
                // dropped back below 2 — measured as a step at the pan's
                // end instead of a spread-out fade.
                let layer_a = render_world_bg_layer_pixels(
                    bgs_a,
                    time as f32,
                    vw,
                    vh,
                    cam_x,
                    cam_y,
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
            // Single active scene — just draw its backgrounds
            for bg in active_bgs {
                draw_world_bg_with_parallax(canvas, bg, time as f32, vw, vh, cam_x, cam_y);
            }
        }
    } else {
        // No active scene — draw view-level backgrounds
        for bg in &view.background.animated {
            draw_world_bg_with_parallax(canvas, bg, time as f32, vw, vh, cam_x, cam_y);
        }
    }

    // 3. Apply world camera transform
    canvas.save();
    canvas.translate((viewport_cx - cam_x, viewport_cy - cam_y));

    // 4. Render each visible scene at its world position
    for vis in &visible {
        let scene = &view.scenes[vis.scene_idx];
        // Use world-position if specified, otherwise fall back to horizontal grid
        let (wx, wy) = scene
            .world_position
            .as_ref()
            .map(|p| (p.x, p.y))
            .unwrap_or((vw / 2.0 + vis.scene_idx as f32 * vw, vh / 2.0));

        // Apply crossfade opacity via save_layer during camera pans
        let needs_opacity = vis.opacity < 1.0 - f32::EPSILON;
        if needs_opacity {
            let mut layer_paint = Paint::default();
            layer_paint.set_alpha_f(vis.opacity);
            canvas.save_layer_alpha_f(None, vis.opacity);
        }

        canvas.save();
        // Translate to scene's world position, offset so scene center = world position
        canvas.translate((wx - viewport_cx, wy - viewport_cy));

        // Use local_time for animations (clamped to 0 if pan hasn't finished)
        let mut anim_time = vis.local_time.max(0.0);
        // Apply freeze_at (parity with the other four render paths —
        // render_frame_v2_scaled, render_scene_hits, render_scene_bg_scaled,
        // render_scene_fg_scaled — all of which clamp `time` the same way).
        // Only the animation clock is clamped, not `frame_index`
        // (`vis.local_frame` below): the other paths keep advancing
        // `frame_index` past the freeze point too, and diverging here would
        // desync any effect keyed on frame index (e.g. grain) from a scene
        // that also appears in a slide view.
        if let Some(freeze_at) = scene.freeze_at {
            anim_time = anim_time.min(freeze_at);
        }
        // World views keep the global per-scene camera (depth planes are a
        // slide-view feature; the world pan is a separate transform).
        let ctx = RenderContext {
            time: anim_time,
            scene_duration: scene.duration,
            frame_index: vis.local_frame,
            fps,
            video_width: config.width,
            video_height: config.height,
            stagger_offset: 0.0,
            camera: None,
        };

        // Apply per-scene camera if present
        let has_camera = scene.camera.is_some();
        if let Some(ref camera) = scene.camera {
            apply_camera_transform(canvas, camera, anim_time as f32, vw, vh);
        }

        // World scenes: force content children into centered flex flow.
        // Decorative children (particles) are excluded from flex and rendered
        // fullscreen. `world_default_scene_layout` is the single source of
        // truth for this fallback — `root_style`'s `ViewType::World` branch
        // uses the exact same function, so the geometry validator sees the
        // same root layout this render path lays out against (round 4
        // audit, constat 4).
        let world_default_layout = world_default_scene_layout();
        let scene_layout = scene.layout.as_ref().unwrap_or(&world_default_layout);
        let scene_children = deserialize_children(scene);

        // Decorative children (particles) get a full-viewport BoxLayout and
        // paint directly via Painter::paint_content. Flex-flow children go
        // through the new pipeline so world scenes stay in sync with slide
        // views.
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
        );

        if has_camera {
            canvas.restore();
        }

        canvas.restore();

        // Restore opacity layer
        if needs_opacity {
            canvas.restore();
        }
    }

    // Restore world transform
    canvas.restore();

    // 5. Read pixels
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

/// Render a world-view scene's set of animated backgrounds into their own
/// transparent, full-frame surface and read back the raw RGBA8888 pixels.
/// Used to crossfade the outgoing/incoming scene's background layers in f32
/// (see the call site in `render_world_frame_scaled`) instead of Skia's
/// 8-bit Paint alpha. `None` only on surface-allocation failure.
#[allow(clippy::too_many_arguments)]
fn render_world_bg_layer_pixels(
    bgs: &[crate::schema::AnimatedBackground],
    time: f32,
    vw: f32,
    vh: f32,
    cam_x: f32,
    cam_y: f32,
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
        draw_world_bg_with_parallax(canvas, bg, time, vw, vh, cam_x, cam_y);
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

/// Blend two equally-sized RGBA8888 buffers in f32, byte-for-byte — the same
/// formula `blend_fade` in `crates/rustmotion-core/src/engine/transition.rs`
/// uses for slide-view crossfades. Kept as a local copy because that helper
/// is private to its own crate; see the call site's doc for why an f32
/// blend matters here instead of Skia's Paint alpha.
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

/// Render only the background (solid color + animated-background) of a scene.
pub fn render_scene_bg_scaled(
    config: &VideoConfig,
    scene: &Scene,
    frame_in_scene: u32,
    scale_factor: f32,
) -> Result<Vec<u8>> {
    let scaled_w = (config.width as f32 * scale_factor) as i32;
    let scaled_h = (config.height as f32 * scale_factor) as i32;
    let mut time = frame_in_scene as f64 / config.fps as f64;
    if let Some(freeze_at) = scene.freeze_at {
        if time > freeze_at {
            time = freeze_at;
        }
    }
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

/// Render only the children (foreground) of a scene on a transparent canvas.
pub fn render_scene_fg_scaled(
    config: &VideoConfig,
    scene: &Scene,
    frame_in_scene: u32,
    _scene_total_frames: u32,
    scale_factor: f32,
) -> Result<Vec<u8>> {
    let children = prepare_scene(scene, config);
    let scaled_w = (config.width as f32 * scale_factor) as i32;
    let scaled_h = (config.height as f32 * scale_factor) as i32;
    let mut time = frame_in_scene as f64 / config.fps as f64;
    if let Some(freeze_at) = scene.freeze_at {
        if time > freeze_at {
            time = freeze_at;
        }
    }
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

    // Transparent background
    canvas.clear(skia_safe::Color4f::new(0.0, 0.0, 0.0, 0.0));

    let plane_cam = per_plane_camera(
        scene,
        &children,
        time as f32,
        config.width as f32,
        config.height as f32,
    );
    let ctx = RenderContext {
        time,
        scene_duration: scene.duration,
        frame_index: frame_in_scene,
        fps: config.fps,
        video_width: config.width,
        video_height: config.height,
        stagger_offset: 0.0,
        camera: plane_cam,
    };

    let has_camera = scene.camera.is_some() && plane_cam.is_none();
    if let (Some(camera), None) = (&scene.camera, plane_cam) {
        apply_camera_transform(
            canvas,
            camera,
            time as f32,
            config.width as f32,
            config.height as f32,
        );
    }

    // Clip scene content to viewport dimensions so scaled elements don't overflow
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

/// Interpolate a camera property at a given time using its keyframes.
pub(super) fn interpolate_camera_property(camera: &Camera, property: &str, time: f32) -> f32 {
    use crate::engine::animator::ease;

    // Find keyframe track for this property
    let track = camera.keyframes.iter().find(|k| k.property == property);
    let track = match track {
        Some(t) if !t.values.is_empty() => t,
        _ => {
            // Return static value
            return match property {
                "x" => camera.x,
                "y" => camera.y,
                "zoom" => camera.zoom,
                "rotation" => camera.rotation,
                "origin.x" => camera.origin.as_ref().map(|o| o.x).unwrap_or(0.0),
                "origin.y" => camera.origin.as_ref().map(|o| o.y).unwrap_or(0.0),
                _ => 0.0,
            };
        }
    };

    let points = &track.values;
    let t = time as f64;

    // Before first keyframe
    if t <= points[0].time {
        return points[0].value;
    }

    // After last keyframe
    if t >= points[points.len() - 1].time {
        return points[points.len() - 1].value;
    }

    // Find segment
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

/// Resolve the camera focal point at `time`, in frame pixels (issue #89).
///
/// Priority per axis: keyframe track (`"origin.x"` / `"origin.y"`, dotted
/// like component compound properties) > static `camera.origin` > frame
/// centre (the historical hard-coded pivot — byte-identical when no origin
/// is declared).
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

/// Apply camera transform to the canvas: translate, zoom, rotate around the
/// camera origin (default: scene centre).
pub(super) fn apply_camera_transform(
    canvas: &Canvas,
    camera: &Camera,
    time: f32,
    width: f32,
    height: f32,
) {
    let x = interpolate_camera_property(camera, "x", time);
    let y = interpolate_camera_property(camera, "y", time);
    let zoom = interpolate_camera_property(camera, "zoom", time);
    let rotation = interpolate_camera_property(camera, "rotation", time);
    let (cx, cy) = resolve_camera_origin(camera, time, width, height);

    canvas.save();

    // 1. Translate to the focal point
    canvas.translate((cx, cy));
    // 2. Apply rotation
    if rotation.abs() > 0.001 {
        canvas.rotate(rotation, None);
    }
    // 3. Apply zoom
    if (zoom - 1.0).abs() > 0.001 {
        canvas.scale((zoom, zoom));
    }
    // 4. Translate back from the focal point + apply camera pan offset
    canvas.translate((-cx - x, -cy - y));
}
