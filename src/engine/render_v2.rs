use anyhow::Result;
use skia_safe::{surfaces, Canvas, ColorType, ImageInfo, Paint, M44, V3};

use super::animator::{apply_orbits, apply_wiggles, extract_effects, resolve_animations, AnimatedProperties};
use super::renderer::color4f_from_hex;
use crate::components::{ChildComponent, Overlay, OverlayAnchor};
use crate::error::RustmotionError;
use crate::layout::{Constraints, LayoutNode};
use crate::schema::{AnimatedBackground, Camera, GradientType, InnerShadow, LayerStyle, Scene, SceneLayout, VideoConfig};
use crate::traits::{Container, RenderContext, Styled};

/// Render a single v2 ChildComponent with full animation/transform support.
///
/// This is the core of the v2 render pipeline. It:
/// 1. Checks timing (start_at/end_at)
/// 2. Resolves animations and wiggles
/// 3. Applies canvas transforms (translate, scale, rotate)
/// 4. Applies opacity/blur via Skia save_layer
/// 5. Calls widget.render()
pub fn render_component(
    canvas: &Canvas,
    child: &ChildComponent,
    layout: &LayoutNode,
    ctx: &RenderContext,
) -> Result<()> {
    let component = &child.component;

    // 1. Check timing
    if let Some(timed) = component.as_timed() {
        let (start_at, end_at) = timed.timing();
        if let Some(start) = start_at {
            if ctx.time < start {
                return Ok(());
            }
        }
        if let Some(end) = end_at {
            if ctx.time > end {
                return Ok(());
            }
        }
    }

    // 2. Resolve animations
    // Compute base animation time adjusted for start_at and stagger
    let anim_time = {
        let base_time = if let Some(timed) = component.as_timed() {
            let (start_at, _) = timed.timing();
            if let Some(start) = start_at {
                ctx.time - start
            } else {
                ctx.time
            }
        } else {
            ctx.time
        };
        (base_time - ctx.stagger_offset).max(0.0)
    };

    let props = {
        let styled = component.as_styled();
        let timeline = &styled.style_config().timeline;

        // Collect all animation effects: base animation + active timeline steps
        let base_effects = component.as_animatable()
            .map(|a| a.animation_effects())
            .unwrap_or(&[]);

        // Timeline: merge active steps' effects with base effects
        let timeline_effects: Vec<_> = if !timeline.is_empty() {
            timeline.iter()
                .filter(|step| anim_time >= step.at)
                .flat_map(|step| {
                    // Adjust each step's animation times relative to step.at
                    step.animation.iter().map(move |effect| {
                        (effect, step.at)
                    })
                })
                .collect()
        } else {
            Vec::new()
        };

        let has_effects = !base_effects.is_empty() || !timeline_effects.is_empty();

        if has_effects {
            // Extract base effects
            let extracted_base = extract_effects(base_effects);

            // Resolve base presets + keyframes
            let mut props = AnimatedProperties::default();
            for (preset, preset_config) in &extracted_base.presets {
                let p = resolve_animations(
                    &[],
                    Some(preset),
                    Some(preset_config),
                    anim_time,
                    ctx.scene_duration,
                );
                props.merge(&p);
            }
            if !extracted_base.keyframes.is_empty() {
                let kf_animations: Vec<_> = extracted_base.keyframes.into_iter().cloned().collect();
                let kf_props = resolve_animations(
                    &kf_animations,
                    None,
                    None,
                    anim_time,
                    ctx.scene_duration,
                );
                props.merge(&kf_props);
            }

            // Resolve timeline step effects (time relative to each step's `at`)
            for (effect, step_at) in &timeline_effects {
                let step_time = anim_time - step_at;
                let step_effects = std::slice::from_ref(*effect);
                let extracted_step = extract_effects(step_effects);

                for (preset, preset_config) in &extracted_step.presets {
                    let p = resolve_animations(
                        &[],
                        Some(preset),
                        Some(preset_config),
                        step_time,
                        ctx.scene_duration,
                    );
                    props.merge(&p);
                }
                if !extracted_step.keyframes.is_empty() {
                    let kf_animations: Vec<_> = extracted_step.keyframes.into_iter().cloned().collect();
                    let kf_props = resolve_animations(
                        &kf_animations,
                        None,
                        None,
                        step_time,
                        ctx.scene_duration,
                    );
                    props.merge(&kf_props);
                }
            }

            // Apply wiggles additively (base only)
            if !extracted_base.wiggles.is_empty() {
                let wiggles: Vec<_> = extracted_base.wiggles.into_iter().cloned().collect();
                apply_wiggles(&mut props, &wiggles, ctx.time);
            }

            // Apply orbit effects (base only)
            if !extracted_base.orbits.is_empty() {
                let orbits: Vec<_> = extracted_base.orbits.into_iter().cloned().collect();
                apply_orbits(&mut props, &orbits, ctx.time);
            }

            // Handle motion blur
            if let Some(blur_intensity) = extracted_base.motion_blur {
                if blur_intensity > 0.01 {
                    return render_component_with_motion_blur(
                        canvas,
                        child,
                        layout,
                        ctx,
                        blur_intensity,
                    );
                }
            }

            props
        } else {
            AnimatedProperties::default()
        }
    };

    // Skip if fully transparent
    if props.opacity <= 0.0 {
        return Ok(());
    }

    render_component_inner(canvas, child, layout, ctx, &props)
}

fn render_component_inner(
    canvas: &Canvas,
    child: &ChildComponent,
    layout: &LayoutNode,
    ctx: &RenderContext,
    props: &AnimatedProperties,
) -> Result<()> {
    let component = &child.component;
    let styled = component.as_styled();

    canvas.save();

    // Component center for scale/rotation (relative to layout origin)
    let cx = layout.width / 2.0;
    let cy = layout.height / 2.0;

    // Apply position offset from animation
    canvas.translate((props.translate_x, props.translate_y));

    // Motion path: move element along an SVG path based on motion_progress
    if let Some(ref motion_path_svg) = styled.style_config().motion_path {
        if props.motion_progress >= 0.0 {
            if let Some(path) = skia_safe::Path::from_svg(motion_path_svg) {
                let mut measure = skia_safe::PathMeasure::new(&path, false, None);
                let length = measure.length();
                let distance = length * props.motion_progress.clamp(0.0, 1.0);
                if let Some((pos, _tangent)) = measure.pos_tan(distance) {
                    canvas.translate((pos.x, pos.y));
                }
            }
        }
    }

    // Check if we need 3D perspective transforms
    let needs_3d = props.rotate_x.abs() > 0.01
        || props.rotate_y.abs() > 0.01
        || props.perspective >= 0.0;

    // 3D adaptive shadow: draw ground-plane shadow BEFORE applying 3D transform
    if needs_3d {
        if let Some(ref shadow) = styled.style_config().box_shadow {
            draw_3d_shadow(canvas, layout.width, layout.height, styled.style_config().border_radius_or(0.0), shadow, props);
        }
    }

    if needs_3d {
        // 3D transform via M44: translate to center, apply perspective + rotations, translate back
        let perspective_dist = if props.perspective >= 0.0 { props.perspective } else { 800.0 };

        // Build the 4x4 transform matrix
        let mut m = M44::translate(cx, cy, 0.0);

        // Apply perspective: shrink Z contribution to simulate depth
        if perspective_dist > 0.0 {
            // Perspective matrix: row 3 col 2 = -1/d
            // This makes objects further in Z appear smaller
            let persp = M44::col_major(&[
                1.0, 0.0, 0.0, 0.0,
                0.0, 1.0, 0.0, 0.0,
                0.0, 0.0, 1.0, 0.0,
                0.0, 0.0, -1.0 / perspective_dist, 1.0,
            ]);
            m.pre_concat(&persp);
        }

        // Apply rotations (in degrees → radians)
        if props.rotate_x.abs() > 0.01 {
            let rad = props.rotate_x * std::f32::consts::PI / 180.0;
            m.pre_concat(&M44::rotate(V3::new(1.0, 0.0, 0.0), rad));
        }
        if props.rotate_y.abs() > 0.01 {
            let rad = props.rotate_y * std::f32::consts::PI / 180.0;
            m.pre_concat(&M44::rotate(V3::new(0.0, 1.0, 0.0), rad));
        }

        // Apply 2D rotation (Z axis)
        if props.rotation.abs() > 0.01 {
            let rad = props.rotation * std::f32::consts::PI / 180.0;
            m.pre_concat(&M44::rotate(V3::new(0.0, 0.0, 1.0), rad));
        }

        // Apply scale
        if (props.scale_x - 1.0).abs() > 0.001 || (props.scale_y - 1.0).abs() > 0.001 {
            let scale_m = M44::col_major(&[
                props.scale_x, 0.0, 0.0, 0.0,
                0.0, props.scale_y, 0.0, 0.0,
                0.0, 0.0, 1.0, 0.0,
                0.0, 0.0, 0.0, 1.0,
            ]);
            m.pre_concat(&scale_m);
        }

        // Translate back from center
        m.pre_concat(&M44::translate(-cx, -cy, 0.0));

        canvas.concat_44(&m);
    } else {
        // Standard 2D transforms (faster path, no M44 overhead)
        if (props.scale_x - 1.0).abs() > 0.001
            || (props.scale_y - 1.0).abs() > 0.001
            || props.rotation.abs() > 0.01
        {
            canvas.translate((cx, cy));
            if props.rotation.abs() > 0.01 {
                canvas.rotate(props.rotation, None);
            }
            if (props.scale_x - 1.0).abs() > 0.001 || (props.scale_y - 1.0).abs() > 0.001 {
                canvas.scale((props.scale_x, props.scale_y));
            }
            canvas.translate((-cx, -cy));
        }
    }

    // Margin offset
    let (mt, _mr, _mb, ml) = styled.margin();
    if mt.abs() > 0.001 || ml.abs() > 0.001 {
        canvas.translate((ml, mt));
    }

    // Padding inset (only for leaf widgets; containers handle padding internally)
    if !component.is_container() {
        let (pad_t, _pad_r, _pad_b, pad_l) = styled.padding();
        if pad_t.abs() > 0.001 || pad_l.abs() > 0.001 {
            canvas.translate((pad_l, pad_t));
        }
    }

    // Backdrop blur (glassmorphism): apply blur to content behind this element
    let backdrop_blur = styled.backdrop_blur();
    if backdrop_blur > 0.01 {
        if let Some(blur_filter) = skia_safe::image_filters::blur(
            (backdrop_blur, backdrop_blur),
            skia_safe::TileMode::Clamp,
            None,
            None,
        ) {
            // Clip to element bounds, then save_layer with backdrop filter
            let bounds = skia_safe::Rect::from_xywh(0.0, 0.0, layout.width, layout.height);
            canvas.save();
            canvas.clip_rect(bounds, skia_safe::ClipOp::Intersect, true);
            canvas.save_layer(
                &skia_safe::canvas::SaveLayerRec::default().backdrop(&blur_filter),
            );
            canvas.restore(); // restore save_layer (backdrop is now applied)
            canvas.restore(); // restore clip
        }
    }

    // Build image filter chain (blur only)
    let mut image_filter: Option<skia_safe::ImageFilter> = None;

    if props.blur > 0.01 {
        image_filter = skia_safe::image_filters::blur(
            (props.blur, props.blur),
            skia_safe::TileMode::Clamp,
            None,
            None,
        );
    }

    // Apply opacity/blur via save_layer
    let needs_layer = props.opacity < 1.0 || image_filter.is_some();
    if needs_layer {
        let mut layer_paint = Paint::default();
        layer_paint.set_alpha_f(props.opacity);
        if let Some(filter) = image_filter {
            layer_paint.set_image_filter(filter);
        }
        canvas.save_layer(&skia_safe::canvas::SaveLayerRec::default().paint(&layer_paint));
    }

    // Glow pre-pass: render only the blurred halo, content renders on top
    // Extract glow from animation effects
    let glow_config = if let Some(animatable) = component.as_animatable() {
        extract_effects(animatable.animation_effects()).glow.cloned()
    } else {
        None
    };
    if let Some(ref glow) = glow_config {
        let radius = if props.glow_radius >= 0.0 { props.glow_radius } else { glow.radius };
        let intensity = if props.glow_intensity >= 0.0 { props.glow_intensity } else { glow.intensity };
        let mut glow_color = color4f_from_hex(&glow.color);
        glow_color.a = (glow_color.a * intensity).min(1.0);
        let glow_filter = skia_safe::image_filters::drop_shadow_only(
            (0.0, 0.0),
            (radius, radius),
            glow_color,
            None,
            None,
            None,
        );
        if let Some(glow_f) = glow_filter {
            let mut glow_paint = Paint::default();
            glow_paint.set_image_filter(glow_f);
            canvas.save_layer(&skia_safe::canvas::SaveLayerRec::default().paint(&glow_paint));
            component.as_widget().render(canvas, layout, ctx, props)?;
            canvas.restore();
        }
    }

    // Render the component on top
    component.as_widget().render(canvas, layout, ctx, props)?;

    // Inner shadow (inset shadow effect)
    if let Some(ref inner_shadow) = styled.style_config().inner_shadow {
        draw_inner_shadow(canvas, layout.width, layout.height, styled.style_config().border_radius.unwrap_or(0.0), inner_shadow);
    }

    if needs_layer {
        canvas.restore();
    }
    canvas.restore();

    Ok(())
}

fn render_component_with_motion_blur(
    canvas: &Canvas,
    child: &ChildComponent,
    layout: &LayoutNode,
    ctx: &RenderContext,
    intensity: f32,
) -> Result<()> {
    let num_samples = if intensity < 0.3 { 3 } else { 5 };
    let frame_duration = 1.0 / ctx.fps as f64;
    let spread = frame_duration * intensity as f64;

    let width = ctx.video_width as i32;
    let height = ctx.video_height as i32;
    let info = ImageInfo::new(
        (width, height),
        ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    let mut temp_surface = surfaces::raster(&info, None, None)
        .ok_or(RustmotionError::MotionBlurSurface)?;

    temp_surface
        .canvas()
        .clear(skia_safe::Color4f::new(0.0, 0.0, 0.0, 0.0));

    let component = &child.component;

    for i in 0..num_samples {
        let t = if num_samples > 1 {
            (i as f64 / (num_samples - 1) as f64 - 0.5) * spread
        } else {
            0.0
        };
        let sample_time = (ctx.time + t).max(0.0);

        let anim_time = if let Some(timed) = component.as_timed() {
            let (start_at, _) = timed.timing();
            if let Some(start) = start_at {
                sample_time - start
            } else {
                sample_time
            }
        } else {
            sample_time
        };

        let mut props = if let Some(animatable) = component.as_animatable() {
            let effects = animatable.animation_effects();
            if !effects.is_empty() {
                let extracted = extract_effects(effects);
                let mut p = AnimatedProperties::default();
                for (preset, preset_config) in &extracted.presets {
                    let pp = resolve_animations(&[], Some(preset), Some(preset_config), anim_time, ctx.scene_duration);
                    p.merge(&pp);
                }
                if !extracted.keyframes.is_empty() {
                    let kf: Vec<_> = extracted.keyframes.into_iter().cloned().collect();
                    let kp = resolve_animations(&kf, None, None, anim_time, ctx.scene_duration);
                    p.merge(&kp);
                }
                if !extracted.wiggles.is_empty() {
                    let wiggles: Vec<_> = extracted.wiggles.into_iter().cloned().collect();
                    apply_wiggles(&mut p, &wiggles, sample_time);
                }
                if !extracted.orbits.is_empty() {
                    let orbits: Vec<_> = extracted.orbits.into_iter().cloned().collect();
                    apply_orbits(&mut p, &orbits, sample_time);
                }
                p
            } else {
                AnimatedProperties::default()
            }
        } else {
            AnimatedProperties::default()
        };

        props.opacity /= num_samples as f32;
        render_component_inner(temp_surface.canvas(), child, layout, ctx, &props)?;
    }

    let image = temp_surface.image_snapshot();
    canvas.draw_image(&image, (0.0, 0.0), None);

    Ok(())
}

/// Render a list of ChildComponents at their layout positions.
/// This is used by containers to render their children with animation support.
pub fn render_children(
    canvas: &Canvas,
    children: &[ChildComponent],
    layout: &LayoutNode,
    ctx: &RenderContext,
) -> Result<()> {
    render_children_with_stagger(canvas, children, layout, ctx, None)
}

pub fn render_children_with_stagger(
    canvas: &Canvas,
    children: &[ChildComponent],
    layout: &LayoutNode,
    ctx: &RenderContext,
    stagger: Option<f32>,
) -> Result<()> {
    for (i, child) in children.iter().enumerate() {
        if i >= layout.children.len() {
            break;
        }
        let child_layout = &layout.children[i];

        let child_ctx = if let Some(stagger_val) = stagger {
            let mut c = ctx.clone();
            c.stagger_offset = ctx.stagger_offset + i as f64 * stagger_val as f64;
            c
        } else {
            ctx.clone()
        };

        canvas.save();
        canvas.translate((child_layout.x, child_layout.y));
        render_component(canvas, child, child_layout, &child_ctx)?;

        // Render overlays positioned relative to this component's bounds
        if !child.overlays.is_empty() {
            render_overlays(canvas, &child.overlays, child_layout, &child_ctx)?;
        }

        canvas.restore();
    }
    Ok(())
}

/// Render overlay components positioned relative to a parent's bounds.
fn render_overlays(
    canvas: &Canvas,
    overlays: &[Overlay],
    parent_layout: &LayoutNode,
    ctx: &RenderContext,
) -> Result<()> {
    let pw = parent_layout.width;
    let ph = parent_layout.height;

    for overlay in overlays {
        // Measure the overlay component
        let widget = overlay.component.as_widget();
        let constraints = Constraints { min_width: 0.0, max_width: pw, min_height: 0.0, max_height: ph };
        let (ow, oh) = widget.measure(&constraints);

        // Compute position based on anchor
        let (ax, ay) = match overlay.anchor {
            OverlayAnchor::TopRight => (pw - ow / 2.0, -oh / 2.0),
            OverlayAnchor::TopLeft => (-ow / 2.0, -oh / 2.0),
            OverlayAnchor::BottomRight => (pw - ow / 2.0, ph - oh / 2.0),
            OverlayAnchor::BottomLeft => (-ow / 2.0, ph - oh / 2.0),
            OverlayAnchor::Center => ((pw - ow) / 2.0, (ph - oh) / 2.0),
        };

        let x = ax + overlay.offset_x;
        let y = ay + overlay.offset_y;

        let overlay_layout = LayoutNode::new(0.0, 0.0, ow, oh);

        // Resolve animations for the overlay component
        let props = if let Some(animatable) = overlay.component.as_animatable() {
            let effects = animatable.animation_effects();
            if !effects.is_empty() {
                let extracted = extract_effects(effects);
                let mut props = AnimatedProperties::default();
                for (preset, preset_config) in &extracted.presets {
                    let p = resolve_animations(&[], Some(preset), Some(preset_config), ctx.time, ctx.scene_duration);
                    props.merge(&p);
                }
                if !extracted.keyframes.is_empty() {
                    let kf_animations: Vec<_> = extracted.keyframes.into_iter().cloned().collect();
                    let kf_props = resolve_animations(&kf_animations, None, None, ctx.time, ctx.scene_duration);
                    props.merge(&kf_props);
                }
                props
            } else {
                AnimatedProperties::default()
            }
        } else {
            AnimatedProperties::default()
        };

        if props.opacity <= 0.0 { continue; }

        canvas.save();
        canvas.translate((x, y));

        // Apply opacity
        let needs_layer = props.opacity < 1.0;
        if needs_layer {
            let mut layer_paint = Paint::default();
            layer_paint.set_alpha_f(props.opacity);
            canvas.save_layer(&skia_safe::canvas::SaveLayerRec::default().paint(&layer_paint));
        }

        widget.render(canvas, &overlay_layout, ctx, &props)?;

        if needs_layer { canvas.restore(); }
        canvas.restore();
    }

    Ok(())
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
    root_layout: &LayoutNode,
) -> Result<Vec<u8>> {
    render_frame_v2_scaled(config, scene, frame_index, _total_frames, root_children, root_layout, 1.0)
}

/// Render a frame with an optional scale factor for higher-resolution output.
/// The layout is computed at video dimensions; the surface and rendering are
/// scaled up so text and vector graphics remain sharp.
pub fn render_frame_v2_scaled(
    config: &VideoConfig,
    scene: &Scene,
    frame_index: u32,
    _total_frames: u32,
    root_children: &[ChildComponent],
    root_layout: &LayoutNode,
    scale_factor: f32,
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
    let bg = scene.background.as_deref().unwrap_or(&config.background);
    canvas.clear(color4f_from_hex(bg));

    // Animated background gradient(s)
    for anim_bg in &scene.animated_background {
        draw_animated_background(canvas, anim_bg, time as f32, config.width as f32, config.height as f32);
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
    let has_camera = scene.camera.is_some();
    if let Some(ref camera) = scene.camera {
        apply_camera_transform(canvas, camera, time as f32, config.width as f32, config.height as f32);
    }

    // Render component tree
    render_children(canvas, root_children, root_layout, &ctx)?;

    // Restore camera transform
    if has_camera {
        canvas.restore();
    }

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

/// Adapter that wraps root-level children as a flex container.
/// This lets us reuse `layout_flex` for scene-level layout.
struct RootContainer<'a> {
    children: &'a [ChildComponent],
    style: LayerStyle,
}

impl<'a> Container for RootContainer<'a> {
    fn children(&self) -> &[ChildComponent] {
        self.children
    }
}

impl<'a> Styled for RootContainer<'a> {
    fn style_config(&self) -> &LayerStyle {
        &self.style
    }
}

impl<'a> RootContainer<'a> {
    fn new(children: &'a [ChildComponent], scene_layout: Option<&SceneLayout>) -> Self {
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
        Self { children, style }
    }
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
    let root = RootContainer::new(children, scene_layout);
    crate::layout::flex::layout_flex(&root, &constraints)
}

/// Compute layout for a scene's children — ready for render_frame_v2.
pub fn prepare_scene<'a>(
    scene: &'a Scene,
    config: &VideoConfig,
) -> (&'a [ChildComponent], LayoutNode) {
    let layout = compute_root_layout(&scene.children, config, scene.layout.as_ref());
    (&scene.children, layout)
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
    render_frame_v2(config, scene, frame_in_scene, scene_total_frames, children, &layout)
}

pub fn render_scene_frame_scaled(
    config: &VideoConfig,
    scene: &Scene,
    frame_in_scene: u32,
    scene_total_frames: u32,
    scale_factor: f32,
) -> Result<Vec<u8>> {
    let (children, layout) = prepare_scene(scene, config);
    render_frame_v2_scaled(config, scene, frame_in_scene, scene_total_frames, children, &layout, scale_factor)
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
    let bg_color = view.background.as_deref().unwrap_or(&config.background);
    canvas.clear(color4f_from_hex(bg_color));

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
        let active_bgs = if active_scene.animated_background.is_empty() {
            &view.animated_background
        } else {
            &active_scene.animated_background
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

            let bgs_a = if scene_a.animated_background.is_empty() {
                &view.animated_background
            } else {
                &scene_a.animated_background
            };
            let bgs_b = if scene_b.animated_background.is_empty() {
                &view.animated_background
            } else {
                &scene_b.animated_background
            };

            // Calculate crossfade progress based on camera pan position
            let pan_half = timeline.camera_pan_duration / 2.0;
            let (_, _scene_b_start) = timeline.scene_windows[scene_b_idx];
            let pan_start = timeline.scene_windows[scene_b_idx].0 - pan_half;
            let pan_end = timeline.scene_windows[scene_b_idx].0 + pan_half;
            let crossfade = if pan_end > pan_start {
                ((time - pan_start) / (pan_end - pan_start)).clamp(0.0, 1.0) as f32
            } else {
                1.0
            };

            // Draw scene A backgrounds with fading alpha
            if crossfade < 1.0 {
                for bg in bgs_a {
                    draw_animated_background(canvas, bg, time as f32, vw, vh);
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
                        draw_animated_background(bg_canvas, bg, time as f32, vw, vh);
                    }
                    let snapshot = bg_surface.image_snapshot();
                    let mut paint = skia_safe::Paint::default();
                    paint.set_alpha_f(crossfade);
                    canvas.save();
                    if scale_factor != 1.0 { canvas.reset_matrix(); }
                    canvas.draw_image(&snapshot, (0.0, 0.0), Some(&paint));
                    canvas.restore();
                    if scale_factor != 1.0 { canvas.scale((scale_factor, scale_factor)); }
                }
            }
        } else {
            // Single active scene — just draw its backgrounds
            for bg in active_bgs {
                draw_animated_background(canvas, bg, time as f32, vw, vh);
            }
        }
    } else {
        // No active scene — draw view-level backgrounds
        for bg in &view.animated_background {
            draw_animated_background(canvas, bg, time as f32, vw, vh);
        }
    }

    // 3. Calculate camera position and apply world transform
    let (cam_x, cam_y) = timeline.camera_at(time, &view.camera_easing);
    let viewport_cx = vw / 2.0;
    let viewport_cy = vh / 2.0;

    canvas.save();
    canvas.translate((viewport_cx - cam_x, viewport_cy - cam_y));

    // 4. Render each visible scene at its world position
    for vis in &visible {
        let scene = &view.scenes[vis.scene_idx];
        let (wx, wy) = scene.world_position.as_ref()
            .map(|p| (p.x, p.y))
            .unwrap_or((0.0, 0.0));

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

        let (children, layout) = prepare_scene(scene, config);

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

        render_children(canvas, children, &layout, &ctx)?;

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

    let bg = scene.background.as_deref().unwrap_or(&config.background);
    canvas.clear(color4f_from_hex(bg));
    for anim_bg in &scene.animated_background {
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
    render_children(canvas, children, &layout, &ctx)?;
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

/// Draw an animated background (gradient, concentric circles, or grid dots).
fn draw_animated_background(
    canvas: &Canvas,
    bg: &AnimatedBackground,
    time: f32,
    width: f32,
    height: f32,
) {
    match bg.preset.as_deref() {
        Some("concentric_circles") => draw_bg_concentric_circles(canvas, bg, time, width, height),
        Some("grid_dots") => draw_bg_grid_dots(canvas, bg, time, width, height),
        Some("halo") => draw_bg_halo(canvas, bg, time, width, height),
        _ => draw_bg_gradient_shift(canvas, bg, time, width, height),
    }
}

fn draw_bg_gradient_shift(
    canvas: &Canvas,
    bg: &AnimatedBackground,
    time: f32,
    width: f32,
    height: f32,
) {
    use skia_safe::{gradient_shader::GradientShaderColors, Point};

    if bg.colors.len() < 2 {
        return;
    }

    let colors: Vec<skia_safe::Color4f> = bg.colors.iter().map(|c| color4f_from_hex(c)).collect();
    let angle = (bg.speed * time) % 360.0;
    let rad = angle.to_radians();

    let shader = match bg.gradient_type {
        GradientType::Linear => {
            let cx = width / 2.0;
            let cy = height / 2.0;
            let half_diag = (width.powi(2) + height.powi(2)).sqrt() / 2.0;
            let start = Point::new(cx - rad.cos() * half_diag, cy - rad.sin() * half_diag);
            let end = Point::new(cx + rad.cos() * half_diag, cy + rad.sin() * half_diag);
            skia_safe::shader::Shader::linear_gradient(
                (start, end),
                GradientShaderColors::ColorsInSpace(&colors, Some(skia_safe::ColorSpace::new_srgb())),
                None,
                skia_safe::TileMode::Clamp,
                None,
                None,
            )
        }
        GradientType::Radial => {
            let center = Point::new(width / 2.0, height / 2.0);
            let radius = width.max(height) / 2.0;
            skia_safe::shader::Shader::radial_gradient(
                center,
                radius,
                GradientShaderColors::ColorsInSpace(&colors, Some(skia_safe::ColorSpace::new_srgb())),
                None,
                skia_safe::TileMode::Clamp,
                None,
                None,
            )
        }
    };

    if let Some(shader) = shader {
        let mut paint = Paint::default();
        paint.set_shader(shader);
        canvas.draw_rect(skia_safe::Rect::from_wh(width, height), &paint);
    }
}

/// Soft colored glow zones (halo preset).
fn draw_bg_halo(
    canvas: &Canvas,
    bg: &AnimatedBackground,
    time: f32,
    width: f32,
    height: f32,
) {
    for zone in &bg.zones {
        let cx = zone.x * width;
        let cy = zone.y * height;
        let base_radius = zone.radius * width.max(height);
        // Subtle breathing animation
        let breath = 1.0 + 0.05 * (time * bg.speed).sin();
        let radius = base_radius * breath;

        let color = color4f_from_hex(&zone.color);
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        paint.set_color4f(color, None);
        paint.set_mask_filter(skia_safe::MaskFilter::blur(
            skia_safe::BlurStyle::Normal,
            radius / 2.0,
            false,
        ));
        canvas.draw_circle((cx, cy), radius, &paint);
    }
}

/// Expanding concentric circles from center.
fn draw_bg_concentric_circles(
    canvas: &Canvas,
    bg: &AnimatedBackground,
    time: f32,
    width: f32,
    height: f32,
) {
    use skia_safe::PaintStyle;

    let color_str = bg.colors.first().map(|s| s.as_str()).unwrap_or("#FFFFFF20");
    let mut paint = super::renderer::paint_from_hex(color_str);
    paint.set_style(PaintStyle::Stroke);
    paint.set_stroke_width(bg.element_size);
    paint.set_anti_alias(true);

    let cx = width / 2.0;
    let cy = height / 2.0;
    let max_radius = (width.powi(2) + height.powi(2)).sqrt() / 2.0;
    let spacing = if let Some(count) = bg.count {
        if count > 0 {
            max_radius / count as f32
        } else {
            bg.spacing.max(20.0)
        }
    } else {
        bg.spacing.max(20.0)
    };
    let offset = (time * bg.speed) % spacing;

    let mut r = offset;
    while r < max_radius {
        // Fade out as circles expand
        let alpha = 1.0 - (r / max_radius).clamp(0.0, 1.0);
        paint.set_alpha_f(alpha * 0.3);
        canvas.draw_circle((cx, cy), r, &paint);
        r += spacing;
    }
}

/// Animated dot grid pattern.
fn draw_bg_grid_dots(
    canvas: &Canvas,
    bg: &AnimatedBackground,
    time: f32,
    width: f32,
    height: f32,
) {
    let color_str = bg.colors.first().map(|s| s.as_str()).unwrap_or("#FFFFFF15");
    let mut paint = super::renderer::paint_from_hex(color_str);
    paint.set_anti_alias(true);

    let spacing = bg.spacing.max(20.0);
    let dot_radius = bg.element_size / 2.0;
    let offset_y = (time * bg.speed * 0.5) % spacing;

    let mut y = -spacing + offset_y;
    while y < height + spacing {
        let mut x = 0.0_f32;
        while x < width + spacing {
            // Pulse: subtle size variation based on position + time
            let phase = (x * 0.01 + y * 0.01 + time * 2.0).sin() * 0.3 + 0.7;
            let r = dot_radius * phase;
            paint.set_alpha_f(phase * 0.4);
            canvas.draw_circle((x, y), r, &paint);
            x += spacing;
        }
        y += spacing;
    }
}

/// Draw an inner (inset) shadow inside an element.
///
/// Technique: clip to the element bounds, then draw a large inverted rect
/// with blur that bleeds inward from the edges.
fn draw_inner_shadow(
    canvas: &Canvas,
    width: f32,
    height: f32,
    corner_radius: f32,
    shadow: &InnerShadow,
) {
    use skia_safe::{PaintStyle, Path, Rect, RRect, ClipOp};

    let bounds = Rect::from_xywh(0.0, 0.0, width, height);
    let rrect = RRect::new_rect_xy(bounds, corner_radius, corner_radius);

    canvas.save();
    // Clip to element bounds
    canvas.clip_rrect(rrect, ClipOp::Intersect, true);

    // Draw a large rect around the element with a hole in the center,
    // offset by the shadow offset, with blur applied.
    let expand = shadow.blur * 3.0 + 50.0;
    let outer = Rect::from_xywh(
        -expand + shadow.offset_x,
        -expand + shadow.offset_y,
        width + expand * 2.0,
        height + expand * 2.0,
    );
    let inner = Rect::from_xywh(shadow.offset_x, shadow.offset_y, width, height);
    let inner_rrect = RRect::new_rect_xy(inner, corner_radius, corner_radius);

    let mut path = Path::new();
    path.add_rect(outer, None);
    path.add_rrect(inner_rrect, None);
    path.set_fill_type(skia_safe::PathFillType::EvenOdd);

    let mut paint = super::renderer::paint_from_hex(&shadow.color);
    paint.set_style(PaintStyle::Fill);
    if shadow.blur > 0.0 {
        paint.set_mask_filter(skia_safe::MaskFilter::blur(
            skia_safe::BlurStyle::Normal,
            shadow.blur / 2.0,
            false,
        ));
    }

    canvas.draw_path(&path, &paint);
    canvas.restore();
}

/// Draw a 3D-adaptive ground-plane shadow.
///
/// When a component has 3D rotation (rotate_x/rotate_y), the shadow is drawn
/// on the "ground plane" (before 3D transforms) with offset/blur adapted to
/// the tilt angle. This creates a realistic shadow that grows and shifts
/// as the element tilts away from the viewer.
fn draw_3d_shadow(
    canvas: &Canvas,
    width: f32,
    height: f32,
    corner_radius: f32,
    shadow: &crate::schema::CardShadow,
    props: &AnimatedProperties,
) {
    use skia_safe::{Rect, RRect};

    let rx = props.rotate_x * std::f32::consts::PI / 180.0;
    let ry = props.rotate_y * std::f32::consts::PI / 180.0;

    // Shadow offset shifts based on rotation angle (light from top-left)
    let depth_factor = (height * 0.4).min(80.0);
    let extra_offset_x = shadow.offset_x - ry.sin() * depth_factor;
    let extra_offset_y = shadow.offset_y + rx.sin() * depth_factor;

    // Shadow blur increases with tilt angle
    let tilt_magnitude = (rx.abs() + ry.abs()).min(1.0);
    let extra_blur = shadow.blur + tilt_magnitude * 20.0;

    // Shadow scales slightly based on perspective foreshortening
    let scale_x = 1.0 + ry.sin().abs() * 0.15;
    let scale_y = 1.0 + rx.sin().abs() * 0.15;
    let sw = width * scale_x;
    let sh = height * scale_y;
    let sx = extra_offset_x - (sw - width) / 2.0;
    let sy = extra_offset_y - (sh - height) / 2.0;

    let shadow_rect = Rect::from_xywh(sx, sy, sw, sh);
    let shadow_rrect = RRect::new_rect_xy(shadow_rect, corner_radius, corner_radius);

    let mut shadow_paint = super::renderer::paint_from_hex(&shadow.color);
    if extra_blur > 0.0 {
        shadow_paint.set_mask_filter(skia_safe::MaskFilter::blur(
            skia_safe::BlurStyle::Normal,
            extra_blur / 2.0,
            false,
        ));
    }
    canvas.draw_rrect(shadow_rrect, &shadow_paint);
}

/// Interpolate a camera property at a given time using its keyframes.
fn interpolate_camera_property(camera: &Camera, property: &str, time: f32) -> f32 {
    use super::animator::ease;

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
fn apply_camera_transform(canvas: &Canvas, camera: &Camera, time: f32, width: f32, height: f32) {
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
