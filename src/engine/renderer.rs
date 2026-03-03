use std::sync::{Arc, OnceLock};

use anyhow::Result;
use dashmap::DashMap;
use skia_safe::{
    surfaces, Canvas, Color4f, ColorType, Font, FontMgr, FontStyle, ImageInfo, Paint, PaintStyle,
    Point, Rect, TextBlob,
};

use super::animator::{apply_wiggles, resolve_animations, AnimatedProperties};
use super::codeblock;
use crate::schema::{
    CaptionLayer, CaptionStyle, Fill, FontWeight, GradientType, ImageFit, Layer, Scene, ShapeText,
    ShapeType, TextAlign, VerticalAlign, VideoConfig,
};

// Thread-local FontMgr instance, created once per thread and reused
thread_local! {
    static THREAD_FONT_MGR: FontMgr = FontMgr::default();
}

pub(crate) fn font_mgr() -> FontMgr {
    THREAD_FONT_MGR.with(|mgr| mgr.clone())
}

/// Global asset cache for decoded images (keyed by file path)
static ASSET_CACHE: OnceLock<Arc<DashMap<String, skia_safe::Image>>> = OnceLock::new();

fn asset_cache() -> &'static Arc<DashMap<String, skia_safe::Image>> {
    ASSET_CACHE.get_or_init(|| Arc::new(DashMap::new()))
}

/// Clear the asset cache (call between renders if needed)
pub fn clear_asset_cache() {
    if let Some(cache) = ASSET_CACHE.get() {
        cache.clear();
    }
}

/// GIF frame data cache: stores decoded frames with pre-computed cumulative timestamps
/// (frames_rgba, cumulative_times, total_duration) keyed by file path
static GIF_CACHE: OnceLock<Arc<DashMap<String, Arc<(Vec<(Vec<u8>, u32, u32)>, Vec<f64>, f64)>>>> = OnceLock::new();

fn gif_cache() -> &'static Arc<DashMap<String, Arc<(Vec<(Vec<u8>, u32, u32)>, Vec<f64>, f64)>>> {
    GIF_CACHE.get_or_init(|| Arc::new(DashMap::new()))
}

/// Parse a hex color string (#RRGGBB or #RRGGBBAA) into RGBA components
pub fn parse_hex_color(hex: &str) -> (u8, u8, u8, u8) {
    let hex = hex.trim_start_matches('#');
    if hex.len() < 6 {
        return (0, 0, 0, 255);
    }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
    let a = if hex.len() >= 8 {
        u8::from_str_radix(&hex[6..8], 16).unwrap_or(255)
    } else {
        255
    };
    (r, g, b, a)
}

pub fn color4f_from_hex(hex: &str) -> Color4f {
    let (r, g, b, a) = parse_hex_color(hex);
    Color4f::new(
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        a as f32 / 255.0,
    )
}

pub fn paint_from_hex(hex: &str) -> Paint {
    let mut paint = Paint::new(color4f_from_hex(hex), None);
    paint.set_anti_alias(true);
    paint
}

/// Render a single frame as RGBA pixels using Skia
pub fn render_frame(
    config: &VideoConfig,
    scene: &Scene,
    frame_index: u32,
    _total_frames: u32,
) -> Result<Vec<u8>> {
    let width = config.width as i32;
    let height = config.height as i32;
    let mut time = frame_index as f64 / config.fps as f64;

    // Apply freeze_at: clamp time to freeze point
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

    let mut surface =
        surfaces::raster(&info, None, None).ok_or_else(|| anyhow::anyhow!("Failed to create Skia surface"))?;

    let canvas = surface.canvas();

    // Fill background (Skia handles premul conversion — must match exactly
    // so anti-aliased layer edges blend seamlessly with the background)
    let bg = scene.background.as_deref().unwrap_or(&config.background);
    canvas.clear(color4f_from_hex(bg));

    // Render layers in order
    for layer in &scene.layers {
        render_layer(canvas, layer, config, time, scene.duration)?;
    }

    // Read pixels back as RGBA
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

// Layer property access is now handled via the LayerProps trait (see schema/video.rs)
// accessed through layer.props()

fn render_layer(canvas: &Canvas, layer: &Layer, config: &VideoConfig, time: f64, scene_duration: f64) -> Result<()> {
    let lp = layer.props();

    // Check start_at / end_at timing
    let (start_at, end_at) = lp.timing();
    if let Some(start) = start_at {
        if time < start {
            return Ok(());
        }
    }
    if let Some(end) = end_at {
        if time > end {
            return Ok(());
        }
    }

    // Adjust time for animation: offset by start_at
    let anim_time = if let Some(start) = start_at {
        time - start
    } else {
        time
    };

    let (animations, preset, preset_config) = lp.animations();
    let mut props = resolve_animations(animations, preset, preset_config, anim_time, scene_duration);

    // Apply wiggles additively
    if let Some(wiggles) = lp.wiggle() {
        apply_wiggles(&mut props, wiggles, time);
    }

    // Skip rendering if fully transparent
    if props.opacity <= 0.0 {
        return Ok(());
    }

    // Motion blur: multi-sampling approach
    if let Some(blur_intensity) = lp.motion_blur() {
        if blur_intensity > 0.01 {
            return render_layer_with_motion_blur(canvas, layer, config, time, scene_duration, blur_intensity);
        }
    }

    render_layer_inner(canvas, layer, config, time, scene_duration, &props)
}

fn render_layer_inner(canvas: &Canvas, layer: &Layer, config: &VideoConfig, time: f64, scene_duration: f64, props: &AnimatedProperties) -> Result<()> {
    // Apply animated transforms
    canvas.save();

    // Get layer center for scale/rotation transforms
    let (cx, cy) = get_layer_center(layer);

    // Apply position offset from animation
    canvas.translate((props.translate_x, props.translate_y));

    // Apply scale and rotation around layer center
    if (props.scale_x - 1.0).abs() > 0.001 || (props.scale_y - 1.0).abs() > 0.001 || props.rotation.abs() > 0.01 {
        canvas.translate((cx, cy));
        if props.rotation.abs() > 0.01 {
            canvas.rotate(props.rotation, None);
        }
        if (props.scale_x - 1.0).abs() > 0.001 || (props.scale_y - 1.0).abs() > 0.001 {
            canvas.scale((props.scale_x, props.scale_y));
        }
        canvas.translate((-cx, -cy));
    }

    // Apply opacity via save_layer_alpha if needed
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

    match layer {
        Layer::Text(text) => render_text(canvas, text, config, props)?,
        Layer::Shape(shape) => render_shape(canvas, shape, props)?,
        Layer::Image(image) => render_image(canvas, image)?,
        Layer::Group(group) => render_group(canvas, group, config, time, scene_duration)?,
        Layer::Svg(svg) => render_svg(canvas, svg)?,
        Layer::Video(video) => render_video(canvas, video, time)?,
        Layer::Gif(gif) => render_gif(canvas, gif, time)?,
        Layer::Caption(caption) => render_caption(canvas, caption, config, time)?,
        Layer::Codeblock(cb) => codeblock::render_codeblock(canvas, cb, config, time, props)?,
    }

    if needs_layer {
        canvas.restore(); // layer
    }
    canvas.restore(); // transform

    Ok(())
}

fn render_layer_with_motion_blur(canvas: &Canvas, layer: &Layer, config: &VideoConfig, time: f64, scene_duration: f64, intensity: f32) -> Result<()> {
    let num_samples = if intensity < 0.3 { 3 } else { 5 };
    let fps = config.fps as f64;
    let frame_duration = 1.0 / fps;
    let spread = frame_duration * intensity as f64;

    let width = config.width as i32;
    let height = config.height as i32;
    let info = ImageInfo::new(
        (width, height),
        ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    let mut temp_surface = surfaces::raster(&info, None, None)
        .ok_or_else(|| anyhow::anyhow!("Failed to create motion blur surface"))?;

    temp_surface.canvas().clear(Color4f::new(0.0, 0.0, 0.0, 0.0));

    let lp = layer.props();

    for i in 0..num_samples {
        let offset = (i as f64 / (num_samples - 1) as f64 - 0.5) * spread;
        let sample_time = (time + offset).max(0.0);

        let (start_at, _) = lp.timing();
        let anim_time = if let Some(start) = start_at {
            sample_time - start
        } else {
            sample_time
        };

        let (animations, preset, preset_config) = lp.animations();
        let mut props = resolve_animations(animations, preset, preset_config, anim_time, scene_duration);
        if let Some(wiggles) = lp.wiggle() {
            apply_wiggles(&mut props, wiggles, sample_time);
        }
        props.opacity /= num_samples as f32;

        render_layer_inner(temp_surface.canvas(), layer, config, sample_time, scene_duration, &props)?;
    }

    // Draw the composited result onto the main canvas
    let image = temp_surface.image_snapshot();
    canvas.draw_image(&image, (0.0, 0.0), None);

    Ok(())
}

fn get_layer_center(layer: &Layer) -> (f32, f32) {
    match layer {
        Layer::Text(t) => {
            // Measure text to find its visual center
            let font_mgr = font_mgr();
            use crate::schema::FontStyleType;
            let slant = match t.font_style {
                FontStyleType::Normal => skia_safe::font_style::Slant::Upright,
                FontStyleType::Italic => skia_safe::font_style::Slant::Italic,
                FontStyleType::Oblique => skia_safe::font_style::Slant::Oblique,
            };
            let weight = match t.font_weight {
                FontWeight::Bold => skia_safe::font_style::Weight::BOLD,
                FontWeight::Normal => skia_safe::font_style::Weight::NORMAL,
            };
            let font_style = FontStyle::new(weight, skia_safe::font_style::Width::NORMAL, slant);
            let typeface = font_mgr
                .match_family_style(&t.font_family, font_style)
                .or_else(|| font_mgr.match_family_style("Helvetica", font_style))
                .or_else(|| font_mgr.match_family_style("Arial", font_style))
                .unwrap_or_else(|| font_mgr.match_family_style("sans-serif", font_style).unwrap());
            let font = Font::from_typeface(typeface, t.font_size);
            let (text_width, _) = font.measure_str(&t.content, None);
            let line_height = t.line_height.unwrap_or(t.font_size * 1.3);

            let cx = match t.align {
                TextAlign::Left => t.position.x + text_width / 2.0,
                TextAlign::Center => t.position.x,
                TextAlign::Right => t.position.x - text_width / 2.0,
            };
            let cy = t.position.y - line_height / 2.0;
            (cx, cy)
        }
        Layer::Shape(s) => (s.position.x + s.size.width / 2.0, s.position.y + s.size.height / 2.0),
        Layer::Image(i) => {
            let (w, h) = match &i.size {
                Some(s) => (s.width, s.height),
                None => (100.0, 100.0),
            };
            (i.position.x + w / 2.0, i.position.y + h / 2.0)
        }
        Layer::Svg(s) => {
            let (w, h) = match &s.size {
                Some(sz) => (sz.width, sz.height),
                None => (100.0, 100.0),
            };
            (s.position.x + w / 2.0, s.position.y + h / 2.0)
        }
        Layer::Video(v) => (v.position.x + v.size.width / 2.0, v.position.y + v.size.height / 2.0),
        Layer::Gif(g) => {
            let (w, h) = match &g.size {
                Some(sz) => (sz.width, sz.height),
                None => (100.0, 100.0),
            };
            (g.position.x + w / 2.0, g.position.y + h / 2.0)
        }
        Layer::Codeblock(cb) => {
            let (w, h) = match &cb.size {
                Some(s) => (s.width, s.height),
                None => (400.0, 300.0),
            };
            (cb.position.x + w / 2.0, cb.position.y + h / 2.0)
        }
        Layer::Caption(c) => (c.position.x, c.position.y),
        Layer::Group(g) => (g.position.x, g.position.y),
    }
}

fn render_text(
    canvas: &Canvas,
    text: &crate::schema::TextLayer,
    _config: &VideoConfig,
    props: &AnimatedProperties,
) -> Result<()> {
    use crate::schema::FontStyleType;

    let font_mgr = font_mgr();
    let slant = match text.font_style {
        FontStyleType::Normal => skia_safe::font_style::Slant::Upright,
        FontStyleType::Italic => skia_safe::font_style::Slant::Italic,
        FontStyleType::Oblique => skia_safe::font_style::Slant::Oblique,
    };
    let weight = match text.font_weight {
        FontWeight::Bold => skia_safe::font_style::Weight::BOLD,
        FontWeight::Normal => skia_safe::font_style::Weight::NORMAL,
    };
    let font_style = FontStyle::new(weight, skia_safe::font_style::Width::NORMAL, slant);

    let typeface = font_mgr
        .match_family_style(&text.font_family, font_style)
        .or_else(|| font_mgr.match_family_style("Helvetica", font_style))
        .or_else(|| font_mgr.match_family_style("Arial", font_style))
        .or_else(|| font_mgr.match_family_style("sans-serif", font_style))
        .or_else(|| {
            if font_mgr.count_families() > 0 {
                font_mgr.match_family_style(&font_mgr.family_name(0), font_style)
            } else {
                None
            }
        })
        .expect("No fonts available on this system");

    let font = Font::from_typeface(typeface, text.font_size);

    let color = props.color.as_deref().unwrap_or(&text.color);
    let mut paint = paint_from_hex(color);
    paint.set_alpha_f(1.0); // opacity handled at layer level

    // Apply typewriter effect (progress-based or absolute char count)
    let content = if props.visible_chars_progress >= 0.0 {
        let chars: Vec<char> = text.content.chars().collect();
        let visible = (props.visible_chars_progress * chars.len() as f32).round() as usize;
        let visible = visible.min(chars.len());
        chars[..visible].iter().collect()
    } else if props.visible_chars >= 0 {
        let chars: Vec<char> = text.content.chars().collect();
        let visible = (props.visible_chars as usize).min(chars.len());
        chars[..visible].iter().collect()
    } else {
        text.content.clone()
    };

    let lines = wrap_text(&content, &font, text.max_width);
    let line_height = text.line_height.unwrap_or(text.font_size * 1.3);
    let letter_spacing = text.letter_spacing.unwrap_or(0.0);

    // Prepare optional shadow and stroke paints
    let shadow_paint = text.shadow.as_ref().map(|shadow| {
        let mut p = paint_from_hex(&shadow.color);
        if shadow.blur > 0.01 {
            if let Some(filter) = skia_safe::image_filters::blur(
                (shadow.blur, shadow.blur),
                skia_safe::TileMode::Clamp,
                None,
                None,
            ) {
                p.set_image_filter(filter);
            }
        }
        (p, shadow.offset_x, shadow.offset_y)
    });

    let stroke_paint = text.stroke.as_ref().map(|stroke| {
        let mut p = paint_from_hex(&stroke.color);
        p.set_style(PaintStyle::Stroke);
        p.set_stroke_width(stroke.width);
        p
    });

    for (i, line) in lines.iter().enumerate() {
        if line.is_empty() {
            continue;
        }

        let blob = if letter_spacing.abs() > 0.01 {
            make_text_blob_with_spacing(line, &font, letter_spacing)
        } else {
            TextBlob::new(line, &font)
        };

        if let Some(blob) = blob {
            let blob_bounds = blob.bounds();
            let line_width = blob_bounds.width();

            let x = match text.align {
                TextAlign::Left => text.position.x,
                TextAlign::Center => text.position.x - line_width / 2.0,
                TextAlign::Right => text.position.x - line_width,
            };
            let y = text.position.y + i as f32 * line_height;

            // Draw background highlight behind text
            if let Some(ref bg) = text.background {
                let bg_paint = paint_from_hex(&bg.color);
                let bg_rect = Rect::from_xywh(
                    x - bg.padding + blob_bounds.left,
                    y - text.font_size + blob_bounds.top.min(0.0) - bg.padding / 2.0,
                    line_width + bg.padding * 2.0,
                    line_height + bg.padding,
                );
                if bg.corner_radius > 0.0 {
                    let rrect = skia_safe::RRect::new_rect_xy(bg_rect, bg.corner_radius, bg.corner_radius);
                    canvas.draw_rrect(rrect, &bg_paint);
                } else {
                    canvas.draw_rect(bg_rect, &bg_paint);
                }
            }

            // Draw shadow
            if let Some((ref sp, ox, oy)) = shadow_paint {
                canvas.draw_text_blob(&blob, (x + ox, y + oy), sp);
            }

            // Draw stroke (outline)
            if let Some(ref sp) = stroke_paint {
                canvas.draw_text_blob(&blob, (x, y), sp);
            }

            // Draw fill
            canvas.draw_text_blob(&blob, (x, y), &paint);
        }
    }

    Ok(())
}

fn wrap_text(text: &str, font: &Font, max_width: Option<f32>) -> Vec<String> {
    let explicit_lines: Vec<&str> = text.split('\n').collect();

    let max_w = match max_width {
        Some(w) => w,
        None => return explicit_lines.iter().map(|s| s.to_string()).collect(),
    };

    let mut result = Vec::new();
    for line in explicit_lines {
        let words: Vec<&str> = line.split_whitespace().collect();
        if words.is_empty() {
            result.push(String::new());
            continue;
        }

        let mut current_line = String::new();
        for word in words {
            let test = if current_line.is_empty() {
                word.to_string()
            } else {
                format!("{} {}", current_line, word)
            };

            let (width, _) = font.measure_str(&test, None);
            if width > max_w && !current_line.is_empty() {
                result.push(current_line);
                current_line = word.to_string();
            } else {
                current_line = test;
            }
        }
        if !current_line.is_empty() {
            result.push(current_line);
        }
    }
    result
}

fn make_text_blob_with_spacing(text: &str, font: &Font, spacing: f32) -> Option<TextBlob> {
    let glyphs = font.str_to_glyphs_vec(text);
    if glyphs.is_empty() {
        return None;
    }

    let mut widths = vec![0.0f32; glyphs.len()];
    font.get_widths(&glyphs, &mut widths);

    let mut positions = Vec::with_capacity(glyphs.len());
    let mut x = 0.0f32;
    for (i, _glyph) in glyphs.iter().enumerate() {
        positions.push(Point::new(x, 0.0));
        x += widths[i] + spacing;
    }

    TextBlob::from_pos_text(text, &positions, font)
}

fn render_shape(canvas: &Canvas, shape: &crate::schema::ShapeLayer, props: &AnimatedProperties) -> Result<()> {
    let x = shape.position.x;
    let y = shape.position.y;
    let w = shape.size.width;
    let h = shape.size.height;

    // Fill
    if let Some(fill) = &shape.fill {
        let mut paint = match fill {
            Fill::Solid(color) => {
                let c = props.color.as_deref().unwrap_or(color);
                paint_from_hex(c)
            }
            Fill::Gradient(gradient) => {
                let colors: Vec<Color4f> = gradient.colors.iter().map(|c| color4f_from_hex(c)).collect();
                let stops: Option<Vec<f32>> = gradient.stops.clone();

                let mut paint = Paint::default();
                paint.set_anti_alias(true);

                let shader = match gradient.gradient_type {
                    GradientType::Linear => {
                        let angle = gradient.angle.unwrap_or(0.0);
                        let rad = angle.to_radians();
                        let cx = x + w / 2.0;
                        let cy = y + h / 2.0;
                        let dx = (w / 2.0) * rad.cos();
                        let dy = (h / 2.0) * rad.sin();
                        let start = Point::new(cx - dx, cy - dy);
                        let end = Point::new(cx + dx, cy + dy);
                        skia_safe::shader::Shader::linear_gradient(
                            (start, end),
                            skia_safe::gradient_shader::GradientShaderColors::ColorsInSpace(
                                &colors,
                                Some(skia_safe::ColorSpace::new_srgb()),
                            ),
                            stops.as_deref(),
                            skia_safe::TileMode::Clamp,
                            None,
                            None,
                        )
                    }
                    GradientType::Radial => {
                        let center = Point::new(x + w / 2.0, y + h / 2.0);
                        let radius = w.max(h) / 2.0;
                        skia_safe::shader::Shader::radial_gradient(
                            center,
                            radius,
                            skia_safe::gradient_shader::GradientShaderColors::ColorsInSpace(
                                &colors,
                                Some(skia_safe::ColorSpace::new_srgb()),
                            ),
                            stops.as_deref(),
                            skia_safe::TileMode::Clamp,
                            None,
                            None,
                        )
                    }
                };
                if let Some(shader) = shader {
                    paint.set_shader(shader);
                }
                paint
            }
        };
        paint.set_style(PaintStyle::Fill);

        draw_shape_path(canvas, &shape.shape, x, y, w, h, shape.corner_radius, &paint);
    }

    // Stroke
    if let Some(stroke) = &shape.stroke {
        let mut paint = paint_from_hex(&stroke.color);
        paint.set_style(PaintStyle::Stroke);
        paint.set_stroke_width(stroke.width);
        draw_shape_path(canvas, &shape.shape, x, y, w, h, shape.corner_radius, &paint);
    }

    // Text
    if let Some(text) = &shape.text {
        render_shape_text(canvas, text, x, y, w, h)?;
    }

    Ok(())
}

fn render_shape_text(
    canvas: &Canvas,
    text: &ShapeText,
    shape_x: f32,
    shape_y: f32,
    shape_w: f32,
    shape_h: f32,
) -> Result<()> {
    let pad = text.padding.unwrap_or(0.0);
    let area_x = shape_x + pad;
    let area_y = shape_y + pad;
    let area_w = shape_w - 2.0 * pad;
    let area_h = shape_h - 2.0 * pad;

    let font_mgr = font_mgr();
    let font_style = match text.font_weight {
        FontWeight::Bold => FontStyle::bold(),
        FontWeight::Normal => FontStyle::normal(),
    };

    let typeface = font_mgr
        .match_family_style(&text.font_family, font_style)
        .or_else(|| font_mgr.match_family_style("Helvetica", font_style))
        .or_else(|| font_mgr.match_family_style("Arial", font_style))
        .or_else(|| font_mgr.match_family_style("sans-serif", font_style))
        .or_else(|| {
            if font_mgr.count_families() > 0 {
                font_mgr.match_family_style(&font_mgr.family_name(0), font_style)
            } else {
                None
            }
        })
        .expect("No fonts available on this system");

    let font = Font::from_typeface(typeface, text.font_size);
    let (_strike_width, metrics) = font.metrics();
    let ascent = -metrics.ascent;
    let line_height = text.line_height.unwrap_or(text.font_size * 1.3);
    let letter_spacing = text.letter_spacing.unwrap_or(0.0);

    let lines = wrap_text(&text.content, &font, Some(area_w));
    let descent = metrics.descent;
    // Total visual height: inter-line spacing for all but the last line, plus actual text height
    let total_h = if lines.len() > 1 {
        (lines.len() - 1) as f32 * line_height + ascent + descent
    } else {
        ascent + descent
    };

    let y_start = match text.vertical_align {
        VerticalAlign::Top => area_y + ascent,
        VerticalAlign::Middle => area_y + (area_h - total_h) / 2.0 + ascent,
        VerticalAlign::Bottom => area_y + area_h - total_h + ascent,
    };

    let mut paint = paint_from_hex(&text.color);
    paint.set_alpha_f(1.0);

    for (i, line) in lines.iter().enumerate() {
        if line.is_empty() {
            continue;
        }

        let blob = if letter_spacing.abs() > 0.01 {
            make_text_blob_with_spacing(line, &font, letter_spacing)
        } else {
            TextBlob::new(line, &font)
        };

        if let Some(blob) = blob {
            let blob_bounds = blob.bounds();
            let line_width = blob_bounds.width();

            let x = match text.align {
                TextAlign::Left => area_x - blob_bounds.left,
                TextAlign::Center => area_x + (area_w - line_width) / 2.0 - blob_bounds.left,
                TextAlign::Right => area_x + area_w - line_width - blob_bounds.left,
            };
            let y = y_start + i as f32 * line_height;

            canvas.draw_text_blob(&blob, (x, y), &paint);
        }
    }

    Ok(())
}

fn draw_shape_path(canvas: &Canvas, shape_type: &ShapeType, x: f32, y: f32, w: f32, h: f32, corner_radius: Option<f32>, paint: &Paint) {
    let rect = Rect::from_xywh(x, y, w, h);
    match shape_type {
        ShapeType::Rect => { canvas.draw_rect(rect, paint); }
        ShapeType::RoundedRect => {
            let r = corner_radius.unwrap_or(8.0);
            let rrect = skia_safe::RRect::new_rect_xy(rect, r, r);
            canvas.draw_rrect(rrect, paint);
        }
        ShapeType::Circle => {
            let radius = w.min(h) / 2.0;
            canvas.draw_circle((x + w / 2.0, y + h / 2.0), radius, paint);
        }
        ShapeType::Ellipse => { canvas.draw_oval(rect, paint); }
        ShapeType::Triangle => {
            let mut path = skia_safe::Path::new();
            path.move_to((x + w / 2.0, y));
            path.line_to((x + w, y + h));
            path.line_to((x, y + h));
            path.close();
            canvas.draw_path(&path, paint);
        }
        ShapeType::Star { points } => {
            let cx = x + w / 2.0;
            let cy = y + h / 2.0;
            let outer_r = w.min(h) / 2.0;
            let inner_r = outer_r * 0.4;
            let n = *points as usize;
            let mut path = skia_safe::Path::new();
            for i in 0..(n * 2) {
                let angle = (i as f32) * std::f32::consts::PI / n as f32 - std::f32::consts::FRAC_PI_2;
                let r = if i % 2 == 0 { outer_r } else { inner_r };
                let px = cx + r * angle.cos();
                let py = cy + r * angle.sin();
                if i == 0 {
                    path.move_to((px, py));
                } else {
                    path.line_to((px, py));
                }
            }
            path.close();
            canvas.draw_path(&path, paint);
        }
        ShapeType::Polygon { sides } => {
            let cx = x + w / 2.0;
            let cy = y + h / 2.0;
            let r = w.min(h) / 2.0;
            let n = *sides as usize;
            let mut path = skia_safe::Path::new();
            for i in 0..n {
                let angle = (i as f32) * 2.0 * std::f32::consts::PI / n as f32 - std::f32::consts::FRAC_PI_2;
                let px = cx + r * angle.cos();
                let py = cy + r * angle.sin();
                if i == 0 {
                    path.move_to((px, py));
                } else {
                    path.line_to((px, py));
                }
            }
            path.close();
            canvas.draw_path(&path, paint);
        }
        ShapeType::Path { data } => {
            if let Some(path) = skia_safe::Path::from_svg(data) {
                canvas.draw_path(&path, paint);
            }
        }
    }
}

fn render_image(canvas: &Canvas, image: &crate::schema::ImageLayer) -> Result<()> {
    let cache = asset_cache();
    let img = if let Some(cached) = cache.get(&image.src) {
        cached.clone()
    } else {
        let data = std::fs::read(&image.src)
            .map_err(|e| anyhow::anyhow!("Failed to load image '{}': {}", image.src, e))?;
        let skia_data = skia_safe::Data::new_copy(&data);
        let decoded = skia_safe::Image::from_encoded(skia_data)
            .ok_or_else(|| anyhow::anyhow!("Failed to decode image '{}'", image.src))?;
        cache.insert(image.src.clone(), decoded.clone());
        decoded
    };

    let img_w = img.width() as f32;
    let img_h = img.height() as f32;

    let (target_w, target_h) = match &image.size {
        Some(size) => (size.width, size.height),
        None => (img_w, img_h),
    };

    let (draw_w, draw_h, offset_x, offset_y) = match image.fit {
        ImageFit::Fill => (target_w, target_h, 0.0, 0.0),
        ImageFit::Contain => {
            let scale = (target_w / img_w).min(target_h / img_h);
            let w = img_w * scale;
            let h = img_h * scale;
            (w, h, (target_w - w) / 2.0, (target_h - h) / 2.0)
        }
        ImageFit::Cover => {
            let scale = (target_w / img_w).max(target_h / img_h);
            let w = img_w * scale;
            let h = img_h * scale;
            (w, h, (target_w - w) / 2.0, (target_h - h) / 2.0)
        }
    };

    let dst = Rect::from_xywh(
        image.position.x + offset_x,
        image.position.y + offset_y,
        draw_w,
        draw_h,
    );

    let paint = Paint::default();

    if matches!(image.fit, ImageFit::Cover) && image.size.is_some() {
        canvas.save();
        canvas.clip_rect(
            Rect::from_xywh(image.position.x, image.position.y, target_w, target_h),
            skia_safe::ClipOp::Intersect,
            true,
        );
        canvas.draw_image_rect(img, None, dst, &paint);
        canvas.restore();
    } else {
        canvas.draw_image_rect(img, None, dst, &paint);
    }

    Ok(())
}

fn render_group(
    canvas: &Canvas,
    group: &crate::schema::GroupLayer,
    config: &VideoConfig,
    time: f64,
    scene_duration: f64,
) -> Result<()> {
    canvas.save();
    canvas.translate((group.position.x, group.position.y));

    if group.opacity < 1.0 {
        canvas.save_layer_alpha(None, (group.opacity * 255.0) as u32);
    }

    for layer in &group.layers {
        render_layer(canvas, layer, config, time, scene_duration)?;
    }

    if group.opacity < 1.0 {
        canvas.restore();
    }
    canvas.restore();

    Ok(())
}

fn render_svg(canvas: &Canvas, svg: &crate::schema::SvgLayer) -> Result<()> {
    // Build a cache key: for file-based SVGs use path + size, for inline use hash of data + size
    let (target_w_opt, target_h_opt) = match &svg.size {
        Some(size) => (Some(size.width as u32), Some(size.height as u32)),
        None => (None, None),
    };
    let cache_key = if let Some(ref src) = svg.src {
        format!("svg:{}:{}x{}", src, target_w_opt.unwrap_or(0), target_h_opt.unwrap_or(0))
    } else if let Some(ref data) = svg.data {
        // Use a simple hash for inline SVG data
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        format!("svg-inline:{}:{}x{}", hasher.finish(), target_w_opt.unwrap_or(0), target_h_opt.unwrap_or(0))
    } else {
        return Err(anyhow::anyhow!("SVG layer must have either 'src' or 'data'"));
    };

    let cache = asset_cache();
    let img = if let Some(cached) = cache.get(&cache_key) {
        cached.clone()
    } else {
        let svg_data = if let Some(ref src) = svg.src {
            std::fs::read(src)
                .map_err(|e| anyhow::anyhow!("Failed to load SVG '{}': {}", src, e))?
        } else if let Some(ref data) = svg.data {
            data.as_bytes().to_vec()
        } else {
            unreachable!()
        };

        let opt = usvg::Options::default();
        let tree = usvg::Tree::from_data(&svg_data, &opt)
            .map_err(|e| anyhow::anyhow!("Failed to parse SVG: {}", e))?;

        let svg_size = tree.size();
        let target_w = target_w_opt.unwrap_or(svg_size.width() as u32);
        let target_h = target_h_opt.unwrap_or(svg_size.height() as u32);

        let mut pixmap = tiny_skia::Pixmap::new(target_w, target_h)
            .ok_or_else(|| anyhow::anyhow!("Failed to create pixmap for SVG"))?;

        let scale_x = target_w as f32 / svg_size.width();
        let scale_y = target_h as f32 / svg_size.height();
        let transform = tiny_skia::Transform::from_scale(scale_x, scale_y);

        resvg::render(&tree, transform, &mut pixmap.as_mut());

        let img_data = skia_safe::Data::new_copy(pixmap.data());
        let img_info = ImageInfo::new(
            (target_w as i32, target_h as i32),
            ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let decoded = skia_safe::images::raster_from_data(&img_info, img_data, target_w as usize * 4)
            .ok_or_else(|| anyhow::anyhow!("Failed to create Skia image from SVG"))?;
        cache.insert(cache_key, decoded.clone());
        decoded
    };

    let (target_w, target_h) = match &svg.size {
        Some(size) => (size.width, size.height),
        None => (img.width() as f32, img.height() as f32),
    };

    let dst = Rect::from_xywh(svg.position.x, svg.position.y, target_w, target_h);
    let paint = Paint::default();
    canvas.draw_image_rect(img, None, dst, &paint);

    Ok(())
}

/// Cache for pre-extracted video frames: key = "src:width:height", value = sorted list of (time, PNG data)
/// Video frame cache: stores decoded RGBA pixels + dimensions instead of raw PNG bytes
static VIDEO_FRAME_CACHE: OnceLock<Arc<DashMap<String, Arc<Vec<(f64, Vec<u8>, u32, u32)>>>>> = OnceLock::new();

fn video_frame_cache() -> &'static Arc<DashMap<String, Arc<Vec<(f64, Vec<u8>, u32, u32)>>>> {
    VIDEO_FRAME_CACHE.get_or_init(|| Arc::new(DashMap::new()))
}

/// Pre-extract all needed frames from a video source in a single ffmpeg pass.
/// Called before the render loop to populate the video frame cache.
pub fn preextract_video_frames(
    scenarios_scenes: &[crate::schema::Scene],
    fps: u32,
) {
    for scene in scenarios_scenes {
        let scene_frames = (scene.duration * fps as f64).round() as u32;
        for layer in &scene.layers {
            if let Layer::Video(video) = layer {
                let rate = video.playback_rate.unwrap_or(1.0);
                let trim_start = video.trim_start.unwrap_or(0.0);
                let width = video.size.width as u32;
                let height = video.size.height as u32;

                let cache_key = format!("{}:{}x{}", video.src, width, height);
                let cache = video_frame_cache();

                // Skip if already cached
                if cache.contains_key(&cache_key) {
                    continue;
                }

                // Collect all timestamps we need
                let start_frame = video.start_at.map(|s| (s * fps as f64).round() as u32).unwrap_or(0);
                let end_frame = video.end_at.map(|e| (e * fps as f64).round() as u32).unwrap_or(scene_frames);

                let mut times = Vec::new();
                for f in start_frame..end_frame {
                    let time = f as f64 / fps as f64;
                    let source_time = trim_start + time * rate;
                    times.push(source_time);
                }

                if times.is_empty() {
                    continue;
                }

                // Extract frames using a single ffmpeg process with fps filter
                let min_time = times.first().copied().unwrap_or(0.0);
                let max_time = times.last().copied().unwrap_or(0.0);
                let duration = max_time - min_time + (1.0 / fps as f64);

                // Extract as raw RGBA pixels directly (no PNG encode/decode overhead)
                let output = std::process::Command::new("ffmpeg")
                    .args([
                        "-ss", &format!("{:.3}", min_time),
                        "-t", &format!("{:.3}", duration),
                        "-i", &video.src,
                        "-vf", &format!("fps={},scale={}:{}", fps, width, height),
                        "-f", "rawvideo",
                        "-pix_fmt", "rgba",
                        "-y", "pipe:1",
                    ])
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::null())
                    .output();

                match output {
                    Ok(output) if output.status.success() => {
                        let frame_size = (width * height * 4) as usize;
                        let data = &output.stdout;
                        let num_frames = data.len() / frame_size;
                        let mut frames: Vec<(f64, Vec<u8>, u32, u32)> = Vec::with_capacity(num_frames);

                        for idx in 0..num_frames {
                            let start = idx * frame_size;
                            let frame_data = data[start..start + frame_size].to_vec();
                            let time = min_time + idx as f64 / fps as f64;
                            frames.push((time, frame_data, width, height));
                        }

                        cache.insert(cache_key, Arc::new(frames));
                    }
                    _ => {
                        // Fallback: cache will miss and we'll extract per-frame
                    }
                }
            }
        }
    }
}

fn render_video(canvas: &Canvas, video: &crate::schema::VideoLayer, time: f64) -> Result<()> {
    // Calculate source time based on trim and playback rate
    let rate = video.playback_rate.unwrap_or(1.0);
    let trim_start = video.trim_start.unwrap_or(0.0);
    let source_time = trim_start + time * rate;
    let width = video.size.width as u32;
    let height = video.size.height as u32;

    // Try to get frame from cache first (RGBA pixels)
    let cache_key = format!("{}:{}x{}", video.src, width, height);
    let cache = video_frame_cache();

    if let Some(cached_frames) = cache.get(&cache_key) {
        if let Some((rgba, fw, fh)) = find_closest_frame(&cached_frames, source_time) {
            let img_info = ImageInfo::new(
                (fw as i32, fh as i32),
                ColorType::RGBA8888,
                skia_safe::AlphaType::Premul,
                None,
            );
            let row_bytes = fw as usize * 4;
            let data = skia_safe::Data::new_copy(rgba);
            if let Some(img) = skia_safe::images::raster_from_data(&img_info, data, row_bytes) {
                let dst = Rect::from_xywh(video.position.x, video.position.y, video.size.width, video.size.height);
                let paint = Paint::default();
                canvas.draw_image_rect(img, None, dst, &paint);
            }
            return Ok(());
        }
    }

    // Fallback: extract single frame via ffmpeg (returns PNG)
    let frame_data = extract_video_frame(&video.src, source_time, width, height)?;
    let skia_data = skia_safe::Data::new_copy(&frame_data);
    if let Some(img) = skia_safe::Image::from_encoded(skia_data) {
        let dst = Rect::from_xywh(video.position.x, video.position.y, video.size.width, video.size.height);
        let paint = Paint::default();
        canvas.draw_image_rect(img, None, dst, &paint);
    }

    Ok(())
}

fn find_closest_frame(frames: &[(f64, Vec<u8>, u32, u32)], target_time: f64) -> Option<(&[u8], u32, u32)> {
    if frames.is_empty() {
        return None;
    }
    // Binary search for closest time
    let idx = frames.partition_point(|(t, _, _, _)| *t < target_time);
    let best = if idx == 0 {
        0
    } else if idx >= frames.len() {
        frames.len() - 1
    } else {
        // Compare prev and current
        if (frames[idx].0 - target_time).abs() < (frames[idx - 1].0 - target_time).abs() {
            idx
        } else {
            idx - 1
        }
    };
    let (_, ref rgba, w, h) = frames[best];
    Some((rgba, w, h))
}

fn extract_video_frame(src: &str, time: f64, width: u32, height: u32) -> Result<Vec<u8>> {
    let output = std::process::Command::new("ffmpeg")
        .args([
            "-ss", &format!("{:.3}", time),
            "-i", src,
            "-vframes", "1",
            "-vf", &format!("scale={}:{}", width, height),
            "-f", "image2pipe",
            "-vcodec", "png",
            "-y", "pipe:1",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run ffmpeg for video frame extraction: {}. Is ffmpeg installed?", e))?;

    if !output.status.success() {
        anyhow::bail!("ffmpeg failed to extract frame from '{}'", src);
    }

    Ok(output.stdout)
}

fn render_gif(canvas: &Canvas, gif_layer: &crate::schema::GifLayer, time: f64) -> Result<()> {
    let gcache = gif_cache();

    // Load or retrieve cached GIF frames
    let cached = if let Some(cached) = gcache.get(&gif_layer.src) {
        cached.clone()
    } else {
        let file = std::fs::File::open(&gif_layer.src)
            .map_err(|e| anyhow::anyhow!("Failed to open GIF '{}': {}", gif_layer.src, e))?;

        let mut decoder = gif::DecodeOptions::new();
        decoder.set_color_output(gif::ColorOutput::RGBA);
        let mut decoder = decoder.read_info(file)
            .map_err(|e| anyhow::anyhow!("Failed to decode GIF '{}': {}", gif_layer.src, e))?;

        let gif_width = decoder.width() as u32;
        let gif_height = decoder.height() as u32;

        let mut frames: Vec<(Vec<u8>, u32, u32)> = Vec::new();
        let mut cumulative_times: Vec<f64> = Vec::new();
        let mut accumulated = 0.0;

        while let Some(frame) = decoder.read_next_frame()
            .map_err(|e| anyhow::anyhow!("Failed to read GIF frame: {}", e))? {
            let delay = frame.delay as f64 / 100.0;
            let delay = if delay < 0.01 { 0.1 } else { delay };
            accumulated += delay;
            frames.push((frame.buffer.to_vec(), gif_width, gif_height));
            cumulative_times.push(accumulated);
        }

        let total_duration = accumulated;
        let cached = Arc::new((frames, cumulative_times, total_duration));
        gcache.insert(gif_layer.src.clone(), cached.clone());
        cached
    };

    let (ref frames, ref cumulative_times, total_duration) = *cached;

    if frames.is_empty() {
        return Ok(());
    }

    // Find the right frame for the current time using binary search
    let effective_time = if gif_layer.loop_gif {
        time % total_duration
    } else {
        time.min(total_duration)
    };

    let frame_idx = cumulative_times.partition_point(|&t| t <= effective_time).min(frames.len() - 1);

    let (ref frame_data, gif_width, gif_height) = frames[frame_idx];

    let (target_w, target_h) = match &gif_layer.size {
        Some(size) => (size.width, size.height),
        None => (gif_width as f32, gif_height as f32),
    };

    let img_info = ImageInfo::new(
        (gif_width as i32, gif_height as i32),
        ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    let row_bytes = gif_width as usize * 4;
    let data = skia_safe::Data::new_copy(frame_data);
    if let Some(img) = skia_safe::images::raster_from_data(&img_info, data, row_bytes) {
        let dst = Rect::from_xywh(gif_layer.position.x, gif_layer.position.y, target_w, target_h);
        let paint = Paint::default();
        canvas.draw_image_rect(img, None, dst, &paint);
    }

    Ok(())
}

fn render_caption(
    canvas: &Canvas,
    caption: &CaptionLayer,
    _config: &VideoConfig,
    time: f64,
) -> Result<()> {
    let font_mgr = font_mgr();
    let font_family = caption.font_family.as_deref().unwrap_or("Inter");
    let typeface = font_mgr
        .match_family_style(font_family, FontStyle::bold())
        .or_else(|| font_mgr.match_family_style("Helvetica", FontStyle::bold()))
        .or_else(|| font_mgr.match_family_style("Arial", FontStyle::bold()))
        .unwrap_or_else(|| font_mgr.match_family_style("sans-serif", FontStyle::bold()).unwrap());

    let font = Font::from_typeface(typeface, caption.font_size);

    match caption.style {
        CaptionStyle::WordByWord => {
            // Show only the active word
            for word in &caption.words {
                if time >= word.start && time < word.end {
                    let paint = paint_from_hex(&caption.active_color);
                    let (text_width, _) = font.measure_str(&word.text, None);

                    if let Some(ref bg_color) = caption.background {
                        let padding = caption.font_size * 0.3;
                        let bg_rect = Rect::from_xywh(
                            caption.position.x - text_width / 2.0 - padding,
                            caption.position.y - caption.font_size - padding / 2.0,
                            text_width + padding * 2.0,
                            caption.font_size * 1.4 + padding,
                        );
                        let bg_paint = paint_from_hex(bg_color);
                        let rrect = skia_safe::RRect::new_rect_xy(bg_rect, padding, padding);
                        canvas.draw_rrect(rrect, &bg_paint);
                    }

                    if let Some(blob) = TextBlob::new(&word.text, &font) {
                        let x = caption.position.x - text_width / 2.0;
                        canvas.draw_text_blob(&blob, (x, caption.position.y), &paint);
                    }
                    break;
                }
            }
        }
        CaptionStyle::Highlight | CaptionStyle::Karaoke => {
            // Show all words, highlight the active one
            let max_width = caption.max_width.unwrap_or(f32::MAX);
            let space_width = font.measure_str(" ", None).0;

            // Build lines with word wrapping
            let mut lines: Vec<Vec<(usize, f32)>> = vec![vec![]]; // (word_index, width)
            let mut current_x = 0.0f32;

            for (i, word) in caption.words.iter().enumerate() {
                let (word_width, _) = font.measure_str(&word.text, None);
                if current_x + word_width > max_width && !lines.last().unwrap().is_empty() {
                    lines.push(vec![]);
                    current_x = 0.0;
                }
                lines.last_mut().unwrap().push((i, word_width));
                current_x += word_width + space_width;
            }

            let line_height = caption.font_size * 1.4;

            // Draw background pill if configured
            if let Some(ref bg_color) = caption.background {
                let padding = caption.font_size * 0.3;
                let total_height = lines.len() as f32 * line_height;
                let max_line_width = lines.iter().map(|line| {
                    line.iter().map(|(_, w)| w).sum::<f32>() + (line.len().saturating_sub(1)) as f32 * space_width
                }).fold(0.0f32, f32::max);
                let bg_rect = Rect::from_xywh(
                    caption.position.x - max_line_width / 2.0 - padding,
                    caption.position.y - caption.font_size - padding / 2.0,
                    max_line_width + padding * 2.0,
                    total_height + padding,
                );
                let bg_paint = paint_from_hex(bg_color);
                let rrect = skia_safe::RRect::new_rect_xy(bg_rect, padding, padding);
                canvas.draw_rrect(rrect, &bg_paint);
            }

            for (line_idx, line) in lines.iter().enumerate() {
                let line_width: f32 = line.iter().map(|(_, w)| w).sum::<f32>()
                    + (line.len().saturating_sub(1)) as f32 * space_width;
                let mut x = caption.position.x - line_width / 2.0;
                let y = caption.position.y + line_idx as f32 * line_height;

                for (word_idx, word_width) in line {
                    let word = &caption.words[*word_idx];
                    let is_active = time >= word.start && time < word.end;
                    let color = if is_active { &caption.active_color } else { &caption.color };
                    let paint = paint_from_hex(color);

                    if let Some(blob) = TextBlob::new(&word.text, &font) {
                        canvas.draw_text_blob(&blob, (x, y), &paint);
                    }
                    x += word_width + space_width;
                }
            }
        }
    }

    Ok(())
}

/// Convert RGBA pixels to YUV420 (I420) for H.264 encoding.
/// Uses integer BT.601 arithmetic and rayon parallelization for performance.
pub fn rgba_to_yuv420(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    use rayon::prelude::*;

    let w = width as usize;
    let h = height as usize;
    let y_size = w * h;
    let uv_w = w / 2;
    let uv_h = h / 2;
    let uv_size = uv_w * uv_h;
    let mut yuv = vec![0u8; y_size + 2 * uv_size];

    let (y_plane, uv_planes) = yuv.split_at_mut(y_size);
    let (u_plane, v_plane) = uv_planes.split_at_mut(uv_size);

    // Compute Y plane in parallel (one row per task)
    y_plane
        .par_chunks_mut(w)
        .enumerate()
        .for_each(|(row, y_row)| {
            let row_offset = row * w * 4;
            for col in 0..w {
                let idx = row_offset + col * 4;
                let r = rgba[idx] as i32;
                let g = rgba[idx + 1] as i32;
                let b = rgba[idx + 2] as i32;
                y_row[col] = (((66 * r + 129 * g + 25 * b + 128) >> 8) + 16).clamp(0, 255) as u8;
            }
        });

    // Compute U and V planes in parallel (one row-pair per task)
    // Process pairs of rows for chroma subsampling
    let uv_combined: Vec<(u8, u8)> = (0..uv_h)
        .into_par_iter()
        .flat_map(|uv_row| {
            let row = uv_row * 2;
            (0..uv_w)
                .map(move |uv_col| {
                    let col = uv_col * 2;
                    // Average 2x2 block for chroma
                    let mut r_sum = 0i32;
                    let mut g_sum = 0i32;
                    let mut b_sum = 0i32;
                    for dr in 0..2 {
                        for dc in 0..2 {
                            let idx = ((row + dr) * w + (col + dc)) * 4;
                            r_sum += rgba[idx] as i32;
                            g_sum += rgba[idx + 1] as i32;
                            b_sum += rgba[idx + 2] as i32;
                        }
                    }
                    let r = r_sum >> 2;
                    let g = g_sum >> 2;
                    let b = b_sum >> 2;
                    let u = (((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128).clamp(0, 255) as u8;
                    let v = (((112 * r - 94 * g - 18 * b + 128) >> 8) + 128).clamp(0, 255) as u8;
                    (u, v)
                })
                .collect::<Vec<_>>()
        })
        .collect();

    for (i, (u, v)) in uv_combined.into_iter().enumerate() {
        u_plane[i] = u;
        v_plane[i] = v;
    }

    yuv
}
