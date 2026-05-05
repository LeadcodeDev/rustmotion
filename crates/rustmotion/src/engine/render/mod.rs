mod background;
mod canvas_guard;
mod scene;
mod transforms;

pub(crate) use canvas_guard::CanvasGuard;

// Re-export everything publicly so external code can use crate::engine::render::*
#[allow(unused_imports)]
pub use scene::{
    render_frame_v2, render_frame_v2_scaled,
    render_scene_frame, render_scene_frame_scaled,
    render_scene_frame_scaled_with_prev_bg,
    render_world_frame_scaled,
    render_scene_bg_scaled, render_scene_fg_scaled,
    compute_root_layout, compute_root_layout_all_flow,
    prepare_scene, deserialize_children, root_style,
};

use crate::error::Result;
use skia_safe::{surfaces, Canvas, ColorType, ImageInfo, Paint, M44, V3};

use rustmotion_core::engine::animator::{apply_orbits, apply_wiggles, extract_effects, resolve_animations, AnimatedProperties};
use rustmotion_core::engine::renderer::color4f_from_hex;
use crate::components::{ChildComponent, Overlay, OverlayAnchor};
use crate::error::RustmotionError;
use crate::layout::{Constraints, LayoutNode};
use crate::traits::{RenderContext, RenderPipeline};

use transforms::{draw_3d_shadow, draw_inner_shadow};

/// Engine implementation of RenderPipeline, bridging the components → engine gap.
pub struct EngineRenderPipeline;

impl RenderPipeline for EngineRenderPipeline {
    fn render_children(
        &self,
        canvas: &Canvas,
        children: &dyn std::any::Any,
        layout: &LayoutNode,
        ctx: &RenderContext,
        stagger: Option<f32>,
    ) -> Result<()> {
        let children = children
            .downcast_ref::<Vec<ChildComponent>>()
            .expect("render_children: children must be Vec<ChildComponent>");
        render_children_with_stagger(canvas, children, layout, ctx, stagger)
    }
}

/// Global pipeline instance.
pub static PIPELINE: EngineRenderPipeline = EngineRenderPipeline;

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

            // Propagate char animation
            if extracted_base.char_animation.is_some() {
                props.char_animation = extracted_base.char_animation;
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
            component.as_widget().render(canvas, layout, ctx, props, &PIPELINE)?;
            canvas.restore();
        }
    }

    // Render the component on top
    component.as_widget().render(canvas, layout, ctx, props, &PIPELINE)?;

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
                if extracted.char_animation.is_some() {
                    p.char_animation = extracted.char_animation;
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
    let count = children.len().min(layout.children.len());
    // Sort by z-index (stable sort preserves document order for equal z-index)
    let mut render_order: Vec<usize> = (0..count).collect();
    render_order.sort_by_key(|&i| children[i].z_index.unwrap_or(0));

    for &i in &render_order {
        let child = &children[i];
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
                if extracted.char_animation.is_some() {
                    props.char_animation = extracted.char_animation;
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

        widget.render(canvas, &overlay_layout, ctx, &props, &PIPELINE)?;

        if needs_layer { canvas.restore(); }
        canvas.restore();
    }

    Ok(())
}
