use anyhow::Result;
use skia_safe::{surfaces, Canvas, ColorType, Font, FontStyle, ImageInfo, Paint};

use super::animator::{apply_wiggles, resolve_animations, AnimatedProperties};
use super::renderer::{color4f_from_hex, font_mgr};
use crate::components::ChildComponent;
use crate::layout::{Constraints, LayoutNode};
use crate::schema::{Layer, Scene, VideoConfig};
use crate::traits::RenderContext;

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
    let props = if let Some(animatable) = component.as_animatable() {
        let config = animatable.animation_config();

        // Adjust animation time by start_at offset
        let anim_time = if let Some(timed) = component.as_timed() {
            let (start_at, _) = timed.timing();
            if let Some(start) = start_at {
                ctx.time - start
            } else {
                ctx.time
            }
        } else {
            ctx.time
        };

        let mut props = resolve_animations(
            &config.animations,
            config.preset.as_ref(),
            config.preset_config.as_ref(),
            anim_time,
            ctx.scene_duration,
        );

        // Apply wiggles additively (using original time, not adjusted time)
        if let Some(ref wiggles) = config.wiggle {
            apply_wiggles(&mut props, wiggles, ctx.time);
        }

        // Handle motion blur
        if let Some(blur_intensity) = config.motion_blur {
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

    // Apply scale and rotation around center
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

    // Margin offset
    let (mt, _mr, _mb, ml) = styled.margin();
    if mt.abs() > 0.001 || ml.abs() > 0.001 {
        canvas.translate((ml, mt));
    }

    // Padding inset
    let (pad_t, _pad_r, _pad_b, pad_l) = styled.padding();
    if pad_t.abs() > 0.001 || pad_l.abs() > 0.001 {
        canvas.translate((pad_l, pad_t));
    }

    // Apply opacity/blur via save_layer
    let needs_layer = props.opacity < 1.0 || props.blur > 0.01;
    if needs_layer {
        if props.blur > 0.01 {
            let filter = skia_safe::image_filters::blur(
                (props.blur, props.blur),
                skia_safe::TileMode::Clamp,
                None,
                None,
            );
            let mut layer_paint = Paint::default();
            layer_paint.set_alpha_f(props.opacity);
            if let Some(filter) = filter {
                layer_paint.set_image_filter(filter);
            }
            canvas.save_layer(&skia_safe::canvas::SaveLayerRec::default().paint(&layer_paint));
        } else {
            canvas.save_layer_alpha(None, (props.opacity * 255.0) as u32);
        }
    }

    // Render the component
    component.as_widget().render(canvas, layout, ctx)?;

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
        .ok_or_else(|| anyhow::anyhow!("Failed to create motion blur surface"))?;

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
            let anim_config = animatable.animation_config();
            let mut p = resolve_animations(
                &anim_config.animations,
                anim_config.preset.as_ref(),
                anim_config.preset_config.as_ref(),
                anim_time,
                ctx.scene_duration,
            );
            if let Some(ref wiggles) = anim_config.wiggle {
                apply_wiggles(&mut p, wiggles, sample_time);
            }
            p
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
    for (i, child) in children.iter().enumerate() {
        if i >= layout.children.len() {
            break;
        }
        let child_layout = &layout.children[i];

        canvas.save();
        canvas.translate((child_layout.x, child_layout.y));
        render_component(canvas, child, child_layout, ctx)?;
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
    let width = config.width as i32;
    let height = config.height as i32;
    let mut time = frame_index as f64 / config.fps as f64;

    // Apply freeze_at
    if let Some(freeze_at) = scene.freeze_at {
        if time > freeze_at {
            time = freeze_at;
        }
    }

    let info = ImageInfo::new(
        (width, height),
        ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );

    let mut surface = surfaces::raster(&info, None, None)
        .ok_or_else(|| anyhow::anyhow!("Failed to create Skia surface"))?;

    let canvas = surface.canvas();

    // Fill background
    let bg = scene.background.as_deref().unwrap_or(&config.background);
    canvas.clear(color4f_from_hex(bg));

    // Build render context
    let ctx = RenderContext {
        time,
        scene_duration: scene.duration,
        frame_index,
        fps: config.fps,
        video_width: config.width,
        video_height: config.height,
    };

    // Render component tree
    render_children(canvas, root_children, root_layout, &ctx)?;

    // Read pixels
    let row_bytes = width as usize * 4;
    let mut pixels = vec![0u8; row_bytes * height as usize];
    let dst_info = ImageInfo::new(
        (width, height),
        ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    surface
        .read_pixels(&dst_info, &mut pixels, row_bytes, (0, 0))
        .then_some(())
        .ok_or_else(|| anyhow::anyhow!("Failed to read pixels from Skia surface"))?;

    Ok(pixels)
}

/// Compute baseline offset for a Text component.
/// This matches the centering logic in Text::render(): (line_height + ascent - descent) / 2.
fn compute_text_baseline_offset(text: &crate::components::text::Text) -> f32 {
    let fm = font_mgr();
    let weight = match text.font_weight {
        crate::schema::FontWeight::Bold => skia_safe::font_style::Weight::BOLD,
        crate::schema::FontWeight::Normal => skia_safe::font_style::Weight::NORMAL,
    };
    let slant = match text.font_style {
        crate::schema::FontStyleType::Normal => skia_safe::font_style::Slant::Upright,
        crate::schema::FontStyleType::Italic => skia_safe::font_style::Slant::Italic,
        crate::schema::FontStyleType::Oblique => skia_safe::font_style::Slant::Oblique,
    };
    let font_style = FontStyle::new(weight, skia_safe::font_style::Width::NORMAL, slant);
    let typeface = fm
        .match_family_style(&text.font_family, font_style)
        .or_else(|| fm.match_family_style("Helvetica", font_style))
        .or_else(|| fm.match_family_style("Arial", font_style))
        .unwrap_or_else(|| fm.match_family_style("sans-serif", font_style).unwrap());
    let font = Font::from_typeface(typeface, text.font_size);
    let (_, metrics) = font.metrics();
    let ascent = -metrics.ascent;
    let descent = metrics.descent;
    let line_height = text.line_height.unwrap_or(text.font_size * 1.3);
    (line_height + ascent - descent) / 2.0
}

/// Compute baseline offset for a Counter component (used for v1 baseline compatibility).
fn compute_counter_baseline_offset(counter: &crate::components::counter::Counter) -> f32 {
    let fm = font_mgr();
    let font_style = match counter.font_weight {
        crate::schema::FontWeight::Bold => FontStyle::bold(),
        crate::schema::FontWeight::Normal => FontStyle::normal(),
    };
    let typeface = fm
        .match_family_style(&counter.font_family, font_style)
        .or_else(|| fm.match_family_style("Helvetica", font_style))
        .or_else(|| fm.match_family_style("Arial", font_style))
        .unwrap_or_else(|| fm.match_family_style("sans-serif", font_style).unwrap());
    let font = Font::from_typeface(typeface, counter.font_size);
    let (_, metrics) = font.metrics();
    let ascent = -metrics.ascent;
    let descent = metrics.descent;
    let line_height = counter.font_size * 1.3;
    (line_height + ascent - descent) / 2.0
}

/// Compute the layout tree for a set of root-level ChildComponents.
/// Each root child is laid out with absolute positioning based on its position field.
/// For v1 backward-compatibility, standalone text uses position.x as the alignment
/// anchor (center point for center-aligned, right edge for right-aligned).
pub fn compute_root_layout(
    children: &[ChildComponent],
    config: &VideoConfig,
) -> LayoutNode {
    let constraints = Constraints::tight(config.width as f32, config.height as f32);

    let mut child_nodes = Vec::with_capacity(children.len());
    for child in children {
        let (mut x, mut y) = child.absolute_position().unwrap_or((0.0, 0.0));
        let mut node = child.component.as_widget().layout(&constraints);

        // v1 compatibility: for standalone text/counter, position.y is the baseline
        // coordinate, but render() now adds the ascent offset to draw from the top of
        // the box. Subtract the ascent so standalone text baseline stays at position.y.
        match &child.component {
            crate::components::Component::Text(ref text) => {
                match text.align {
                    crate::schema::TextAlign::Center => x -= node.width / 2.0,
                    crate::schema::TextAlign::Right => x -= node.width,
                    crate::schema::TextAlign::Left => {}
                }
                let baseline_offset = compute_text_baseline_offset(text);
                y -= baseline_offset;
            }
            crate::components::Component::Counter(ref counter) => {
                let baseline_offset = compute_counter_baseline_offset(counter);
                y -= baseline_offset;
            }
            _ => {}
        }

        node.x = x;
        node.y = y;
        child_nodes.push(node);
    }

    LayoutNode::new(0.0, 0.0, config.width as f32, config.height as f32)
        .with_children(child_nodes)
}

/// Convert v1 layers to v2 ChildComponents using serde round-trip.
/// Both Layer and ChildComponent use compatible JSON formats with `#[serde(tag = "type")]`.
pub fn convert_layers_to_components(layers: &[Layer]) -> Result<Vec<ChildComponent>> {
    let json = serde_json::to_value(layers)?;
    let mut children: Vec<ChildComponent> = serde_json::from_value(json)?;
    // v1 layers always serialize `position: {x:0, y:0}` even when default.
    // In v2, this incorrectly makes container children absolute-positioned.
    // Strip default positions from children of containers recursively.
    for child in &mut children {
        strip_default_positions_in_containers(child);
    }
    Ok(children)
}

/// Recursively strip `position: Some(Absolute{0,0})` from container children.
/// This corrects the v1→v2 serde roundtrip: v1 default positions should become
/// v2 flow layout (position = None), not absolute at (0,0).
fn strip_default_positions_in_containers(child: &mut ChildComponent) {
    if let Some(children) = child.component.children_mut() {
        for c in children.iter_mut() {
            if matches!(&c.position, Some(crate::components::PositionMode::Absolute { x, y }) if x.abs() < 0.001 && y.abs() < 0.001)
            {
                c.position = None;
            }
            // Recurse into nested containers
            strip_default_positions_in_containers(c);
        }
    }
}

/// Convert a scene's layers and compute layout — ready for render_frame_v2.
pub fn prepare_scene(
    scene: &Scene,
    config: &VideoConfig,
) -> Result<(Vec<ChildComponent>, LayoutNode)> {
    let children = convert_layers_to_components(&scene.layers)?;
    let layout = compute_root_layout(&children, config);
    Ok((children, layout))
}

/// Render a single frame using v2 pipeline, falling back to v1 on failure.
/// This is the unified entry point for both single-frame and video encoding.
pub fn render_scene_frame(
    config: &VideoConfig,
    scene: &Scene,
    frame_in_scene: u32,
    scene_total_frames: u32,
) -> Result<Vec<u8>> {
    match prepare_scene(scene, config) {
        Ok((children, layout)) => {
            render_frame_v2(config, scene, frame_in_scene, scene_total_frames, &children, &layout)
        }
        Err(_) => {
            super::render_frame(config, scene, frame_in_scene, scene_total_frames)
        }
    }
}
