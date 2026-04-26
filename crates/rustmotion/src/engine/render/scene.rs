use crate::error::Result;
use skia_safe::{surfaces, Canvas, ClipOp, ColorType, ImageInfo, Paint, Rect};

use super::background::draw_animated_background;
use super::background::draw_world_bg_with_parallax;
use super::{render_children, render_component, render_overlays};
use crate::components::ChildComponent;
use rustmotion_core::engine::animator::safe_div;
use rustmotion_core::engine::renderer::color4f_from_hex;
use crate::error::RustmotionError;
use crate::layout::{Constraints, LayoutNode};
use crate::schema::{Camera, LayerStyle, Scene, SceneLayout, VideoConfig};
use crate::traits::RenderContext;

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
    root_layout: &LayoutNode,
) -> Result<Vec<u8>> {
    render_frame_v2_scaled(config, scene, frame_index, _total_frames, root_children, root_layout, 1.0, None)

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
    root_layout: &LayoutNode,
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

    let mut surface = surfaces::raster(&info, None, None)
        .ok_or(RustmotionError::SurfaceCreation)?;

    let canvas = surface.canvas();

    // Apply scale factor before rendering
    if scale_factor != 1.0 {
        canvas.scale((scale_factor, scale_factor));
    }

    // Fill background
    let bg = scene.resolved_background.color.as_deref().unwrap_or(&config.background);
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
                    (scaled_w, scaled_h), ColorType::RGBA8888, skia_safe::AlphaType::Premul, None,
                );
                if let Some(mut prev_surface) = surfaces::raster(&bg_info, None, None) {
                    let prev_canvas = prev_surface.canvas();
                    if scale_factor != 1.0 { prev_canvas.scale((scale_factor, scale_factor)); }
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
                    if scale_factor != 1.0 { canvas.reset_matrix(); }
                    canvas.draw_image(&snapshot, (0.0, 0.0), Some(&paint));
                    canvas.restore();
                }
            }
            if progress > 0.0 {
                // Draw current background with growing alpha
                let bg_info = ImageInfo::new(
                    (scaled_w, scaled_h), ColorType::RGBA8888, skia_safe::AlphaType::Premul, None,
                );
                if let Some(mut cur_surface) = surfaces::raster(&bg_info, None, None) {
                    let cur_canvas = cur_surface.canvas();
                    if scale_factor != 1.0 { cur_canvas.scale((scale_factor, scale_factor)); }
                    cur_canvas.clear(skia_safe::Color4f::new(0.0, 0.0, 0.0, 0.0));
                    for anim_bg in &cur_bg.animated {
                        draw_animated_background(cur_canvas, anim_bg, continuous_time, w, h);
                    }
                    let snapshot = cur_surface.image_snapshot();
                    let mut paint = Paint::default();
                    paint.set_alpha_f(progress);
                    canvas.save();
                    if scale_factor != 1.0 { canvas.reset_matrix(); }
                    canvas.draw_image(&snapshot, (0.0, 0.0), Some(&paint));
                    canvas.restore();
                }
            }
        } else {
            // Transition ended — keep using continuous time for the rest of the scene
            for anim_bg in &cur_bg.animated {
                draw_animated_background(canvas, anim_bg, continuous_time, config.width as f32, config.height as f32);
            }
        }
    } else {
        for anim_bg in &cur_bg.animated {
            draw_animated_background(canvas, anim_bg, time as f32, config.width as f32, config.height as f32);
        }
    }

    // Build render context
    let ctx = RenderContext {
        time,
        scene_duration: scene.duration,
        frame_index,
        fps: config.fps,
        video_width: config.width,
        video_height: config.height,
        stagger_offset: 0.0,
    };

    // Apply virtual camera transform
    let camera_guard = if let Some(ref camera) = scene.camera {
        let g = super::CanvasGuard::new(canvas);
        apply_camera_transform(canvas, camera, time as f32, config.width as f32, config.height as f32);
        Some(g)
    } else {
        None
    };

    // Clip scene content to viewport dimensions so scaled elements don't overflow.
    // Both guards drop in reverse order on early-return, restoring the canvas.
    let clip_guard = super::CanvasGuard::new(canvas);
    canvas.clip_rect(
        Rect::from_wh(config.width as f32, config.height as f32),
        ClipOp::Intersect,
        true,
    );

    // Render component tree
    render_children(canvas, root_children, root_layout, &ctx)?;

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

/// Build a `LayerStyle` from an optional `SceneLayout` for root-level flex layout.
fn root_style(scene_layout: Option<&SceneLayout>) -> LayerStyle {
    let mut style = LayerStyle::default();
    if let Some(layout) = scene_layout {
        style.flex_direction = layout.direction.clone();
        style.gap = layout.gap;
        style.align_items = layout.align_items.clone();
        style.justify_content = layout.justify_content.clone();
        if let Some(p) = layout.padding {
            style.padding = Some(crate::schema::Spacing::Uniform(p));
        }
    }
    // Default direction is column (like a web page)
    if style.flex_direction.is_none() {
        style.flex_direction = Some(crate::schema::CardDirection::Column);
    }
    style
}

/// Compute the layout tree for a set of root-level ChildComponents.
/// Uses an implicit flex container at video dimensions (like a full-screen web page).
/// All root children participate in flex flow (position is stripped before layout).
/// Only children inside `Positioned` containers use absolute coordinates.
pub fn compute_root_layout(
    children: &[ChildComponent],
    config: &VideoConfig,
    scene_layout: Option<&SceneLayout>,
) -> LayoutNode {
    let constraints = Constraints::tight(config.width as f32, config.height as f32);
    let style = root_style(scene_layout);
    crate::layout::flex::layout_flex(children, &style, &constraints)
}

/// Like `compute_root_layout`, but forces all children into flex flow
/// (ignores absolute positions). Used for world scenes where content
/// should be centered regardless of position attributes in the JSON.
pub fn compute_root_layout_all_flow(
    children: &[ChildComponent],
    config: &VideoConfig,
    scene_layout: Option<&SceneLayout>,
) -> LayoutNode {
    let constraints = Constraints::tight(config.width as f32, config.height as f32);
    let style = root_style(scene_layout);
    crate::layout::flex::layout_flex_all_flow(children, &style, &constraints)
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

/// Compute layout for a scene's children — ready for render_frame_v2.
pub fn prepare_scene(
    scene: &Scene,
    config: &VideoConfig,
) -> (Vec<ChildComponent>, LayoutNode) {
    let children = deserialize_children(scene);
    let layout = compute_root_layout(&children, config, scene.layout.as_ref());
    (children, layout)
}

/// Render a single frame using the v2 pipeline.
/// This is the unified entry point for both single-frame and video encoding.
pub fn render_scene_frame(
    config: &VideoConfig,
    scene: &Scene,
    frame_in_scene: u32,
    scene_total_frames: u32,
) -> Result<Vec<u8>> {
    let (children, layout) = prepare_scene(scene, config);
    render_frame_v2(config, scene, frame_in_scene, scene_total_frames, &children, &layout)
}

pub fn render_scene_frame_scaled(
    config: &VideoConfig,
    scene: &Scene,
    frame_in_scene: u32,
    scene_total_frames: u32,
    scale_factor: f32,
) -> Result<Vec<u8>> {
    let (children, layout) = prepare_scene(scene, config);
    render_frame_v2_scaled(config, scene, frame_in_scene, scene_total_frames, &children, &layout, scale_factor, None)
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
    let (children, layout) = prepare_scene(scene, config);
    render_frame_v2_scaled(config, scene, frame_in_scene, scene_total_frames, &children, &layout, scale_factor, prev_bg)
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
        (scaled_w, scaled_h), ColorType::RGBA8888, skia_safe::AlphaType::Premul, None,
    );
    let mut surface = surfaces::raster(&info, None, None)
        .ok_or(RustmotionError::SurfaceCreation)?;
    let canvas = surface.canvas();

    if scale_factor != 1.0 {
        canvas.scale((scale_factor, scale_factor));
    }

    let vw = config.width as f32;
    let vh = config.height as f32;

    // 1. Draw base background (view-level or video-level)
    let bg_color = view.background.color.as_deref().unwrap_or(&config.background);
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
    let active_scene_idx = visible.iter()
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
        let non_persisted: Vec<_> = visible.iter()
            .filter(|v| !v.is_persisted)
            .collect();

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

            // Calculate crossfade progress based on camera pan position
            let pan_half = timeline.camera_pan_duration / 2.0;
            let (_, _scene_b_start) = timeline.scene_windows[scene_b_idx];
            let pan_start = timeline.scene_windows[scene_b_idx].0 - pan_half;
            let pan_end = timeline.scene_windows[scene_b_idx].0 + pan_half;
            let crossfade = safe_div(time - pan_start, pan_end - pan_start, 1.0).clamp(0.0, 1.0) as f32;

            // Draw scene A backgrounds with fading alpha
            if crossfade < 1.0 {
                for bg in bgs_a {
                    draw_world_bg_with_parallax(canvas, bg, time as f32, vw, vh, cam_x, cam_y);
                }
            }
            // Draw scene B backgrounds with growing alpha
            if crossfade > 0.0 && bgs_a as *const _ != bgs_b as *const _ {
                // If backgrounds are different, create a temporary surface for B and blend
                let bg_info = ImageInfo::new(
                    (scaled_w, scaled_h), ColorType::RGBA8888, skia_safe::AlphaType::Premul, None,
                );
                if let Some(mut bg_surface) = surfaces::raster(&bg_info, None, None) {
                    let bg_canvas = bg_surface.canvas();
                    if scale_factor != 1.0 { bg_canvas.scale((scale_factor, scale_factor)); }
                    bg_canvas.clear(skia_safe::Color4f::new(0.0, 0.0, 0.0, 0.0));
                    for bg in bgs_b {
                        draw_world_bg_with_parallax(bg_canvas, bg, time as f32, vw, vh, cam_x, cam_y);
                    }
                    let snapshot = bg_surface.image_snapshot();
                    let mut paint = skia_safe::Paint::default();
                    paint.set_alpha_f(crossfade);
                    // save()/restore() already bracket the matrix change; an
                    // extra canvas.scale() after restore would compound it.
                    canvas.save();
                    if scale_factor != 1.0 { canvas.reset_matrix(); }
                    canvas.draw_image(&snapshot, (0.0, 0.0), Some(&paint));
                    canvas.restore();
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
        let (wx, wy) = scene.world_position.as_ref()
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
        let anim_time = vis.local_time.max(0.0);
        let ctx = RenderContext {
            time: anim_time,
            scene_duration: scene.duration,
            frame_index: vis.local_frame,
            fps,
            video_width: config.width,
            video_height: config.height,
            stagger_offset: 0.0,
        };

        // Apply per-scene camera if present
        let has_camera = scene.camera.is_some();
        if let Some(ref camera) = scene.camera {
            apply_camera_transform(canvas, camera, anim_time as f32, vw, vh);
        }

        // World scenes: force content children into centered flex flow.
        // Decorative children (particles) are excluded from flex and rendered fullscreen.
        let world_default_layout = crate::schema::SceneLayout {
            direction: Some(crate::schema::CardDirection::Column),
            gap: Some(12.0),
            align_items: Some(crate::schema::CardAlign::Center),
            justify_content: Some(crate::schema::CardJustify::Center),
            padding: None,
        };
        let scene_layout = scene.layout.as_ref().unwrap_or(&world_default_layout);
        let scene_children = deserialize_children(scene);
        let layout = compute_root_layout(&scene_children, config, Some(scene_layout));

        // Render: decorative children get fullscreen layout, content children get flex layout
        for (i, child) in scene_children.iter().enumerate() {
            if i >= layout.children.len() { break; }
            if child.is_decorative() {
                // Render particles fullscreen
                let deco_layout = LayoutNode::new(0.0, 0.0, vw, vh);
                canvas.save();
                render_component(canvas, child, &deco_layout, &ctx)?;
                canvas.restore();
            } else {
                let child_layout = &layout.children[i];
                canvas.save();
                canvas.translate((child_layout.x, child_layout.y));
                render_component(canvas, child, child_layout, &ctx)?;
                if !child.overlays.is_empty() {
                    render_overlays(canvas, &child.overlays, child_layout, &ctx)?;
                }
                canvas.restore();
            }
        }

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
        (scaled_w, scaled_h), ColorType::RGBA8888, skia_safe::AlphaType::Premul, None,
    );
    surface.read_pixels(&dst_info, &mut pixels, row_bytes, (0, 0))
        .then_some(()).ok_or(RustmotionError::PixelRead)?;

    Ok(pixels)
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
        if time > freeze_at { time = freeze_at; }
    }
    let info = ImageInfo::new(
        (scaled_w, scaled_h), ColorType::RGBA8888, skia_safe::AlphaType::Premul, None,
    );
    let mut surface = surfaces::raster(&info, None, None)
        .ok_or(RustmotionError::SurfaceCreation)?;
    let canvas = surface.canvas();
    if scale_factor != 1.0 { canvas.scale((scale_factor, scale_factor)); }

    let bg = scene.resolved_background.color.as_deref().unwrap_or(&config.background);
    canvas.clear(color4f_from_hex(bg));
    for anim_bg in &scene.resolved_background.animated {
        draw_animated_background(canvas, anim_bg, time as f32, config.width as f32, config.height as f32);
    }

    let row_bytes = scaled_w as usize * 4;
    let mut pixels = vec![0u8; row_bytes * scaled_h as usize];
    let dst_info = ImageInfo::new(
        (scaled_w, scaled_h), ColorType::RGBA8888, skia_safe::AlphaType::Premul, None,
    );
    surface.read_pixels(&dst_info, &mut pixels, row_bytes, (0, 0))
        .then_some(()).ok_or(RustmotionError::PixelRead)?;
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
    let (children, layout) = prepare_scene(scene, config);
    let scaled_w = (config.width as f32 * scale_factor) as i32;
    let scaled_h = (config.height as f32 * scale_factor) as i32;
    let mut time = frame_in_scene as f64 / config.fps as f64;
    if let Some(freeze_at) = scene.freeze_at {
        if time > freeze_at { time = freeze_at; }
    }
    let info = ImageInfo::new(
        (scaled_w, scaled_h), ColorType::RGBA8888, skia_safe::AlphaType::Premul, None,
    );
    let mut surface = surfaces::raster(&info, None, None)
        .ok_or(RustmotionError::SurfaceCreation)?;
    let canvas = surface.canvas();
    if scale_factor != 1.0 { canvas.scale((scale_factor, scale_factor)); }

    // Transparent background
    canvas.clear(skia_safe::Color4f::new(0.0, 0.0, 0.0, 0.0));

    let ctx = RenderContext {
        time,
        scene_duration: scene.duration,
        frame_index: frame_in_scene,
        fps: config.fps,
        video_width: config.width,
        video_height: config.height,
        stagger_offset: 0.0,
    };

    let has_camera = scene.camera.is_some();
    if let Some(ref camera) = scene.camera {
        apply_camera_transform(canvas, camera, time as f32, config.width as f32, config.height as f32);
    }

    // Clip scene content to viewport dimensions so scaled elements don't overflow
    canvas.save();
    canvas.clip_rect(
        Rect::from_wh(config.width as f32, config.height as f32),
        ClipOp::Intersect,
        true,
    );
    render_children(canvas, &children, &layout, &ctx)?;
    canvas.restore();

    if has_camera { canvas.restore(); }

    let row_bytes = scaled_w as usize * 4;
    let mut pixels = vec![0u8; row_bytes * scaled_h as usize];
    let dst_info = ImageInfo::new(
        (scaled_w, scaled_h), ColorType::RGBA8888, skia_safe::AlphaType::Premul, None,
    );
    surface.read_pixels(&dst_info, &mut pixels, row_bytes, (0, 0))
        .then_some(()).ok_or(RustmotionError::PixelRead)?;
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

/// Apply camera transform to the canvas: translate, zoom, rotate around scene center.
pub(super) fn apply_camera_transform(canvas: &Canvas, camera: &Camera, time: f32, width: f32, height: f32) {
    let x = interpolate_camera_property(camera, "x", time);
    let y = interpolate_camera_property(camera, "y", time);
    let zoom = interpolate_camera_property(camera, "zoom", time);
    let rotation = interpolate_camera_property(camera, "rotation", time);

    let cx = width / 2.0;
    let cy = height / 2.0;

    canvas.save();

    // 1. Translate to center
    canvas.translate((cx, cy));
    // 2. Apply rotation
    if rotation.abs() > 0.001 {
        canvas.rotate(rotation, None);
    }
    // 3. Apply zoom
    if (zoom - 1.0).abs() > 0.001 {
        canvas.scale((zoom, zoom));
    }
    // 4. Translate back from center + apply camera pan offset
    canvas.translate((-cx - x, -cy - y));
}
