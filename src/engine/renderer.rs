use anyhow::Result;
use skia_safe::{
    surfaces, Canvas, Color4f, ColorType, Font, FontMgr, FontStyle, ImageInfo, Paint, PaintStyle,
    Point, Rect, TextBlob,
};

use super::animator::{resolve_animations, AnimatedProperties};
use crate::schema::{
    Fill, FontWeight, GradientType, ImageFit, Layer, Scene, ShapeType, TextAlign, VideoConfig,
};

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

fn color4f_from_hex(hex: &str) -> Color4f {
    let (r, g, b, a) = parse_hex_color(hex);
    Color4f::new(
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        a as f32 / 255.0,
    )
}

fn paint_from_hex(hex: &str) -> Paint {
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
    let time = frame_index as f64 / config.fps as f64;

    let info = ImageInfo::new(
        (width, height),
        ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );

    let mut surface =
        surfaces::raster(&info, None, None).ok_or_else(|| anyhow::anyhow!("Failed to create Skia surface"))?;

    let canvas = surface.canvas();

    // Fill background
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

fn get_layer_animations(layer: &Layer) -> (&[crate::schema::Animation], Option<&crate::schema::AnimationPreset>, Option<&crate::schema::PresetConfig>) {
    match layer {
        Layer::Text(t) => (&t.animations, t.preset.as_ref(), t.preset_config.as_ref()),
        Layer::Shape(s) => (&s.animations, s.preset.as_ref(), s.preset_config.as_ref()),
        Layer::Image(i) => (&i.animations, i.preset.as_ref(), i.preset_config.as_ref()),
        Layer::Group(_) => (&[], None, None),
    }
}

fn render_layer(canvas: &Canvas, layer: &Layer, config: &VideoConfig, time: f64, scene_duration: f64) -> Result<()> {
    let (animations, preset, preset_config) = get_layer_animations(layer);
    let props = resolve_animations(animations, preset, preset_config, time, scene_duration);

    // Skip rendering if fully transparent
    if props.opacity <= 0.0 {
        return Ok(());
    }

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
        Layer::Text(text) => render_text(canvas, text, config, &props)?,
        Layer::Shape(shape) => render_shape(canvas, shape)?,
        Layer::Image(image) => render_image(canvas, image)?,
        Layer::Group(group) => render_group(canvas, group, config, time, scene_duration)?,
    }

    if needs_layer {
        canvas.restore(); // layer
    }
    canvas.restore(); // transform

    Ok(())
}

fn get_layer_center(layer: &Layer) -> (f32, f32) {
    match layer {
        Layer::Text(t) => (t.position.x, t.position.y),
        Layer::Shape(s) => (s.position.x + s.size.width / 2.0, s.position.y + s.size.height / 2.0),
        Layer::Image(i) => {
            let (w, h) = match &i.size {
                Some(s) => (s.width, s.height),
                None => (100.0, 100.0),
            };
            (i.position.x + w / 2.0, i.position.y + h / 2.0)
        }
        Layer::Group(g) => (g.position.x, g.position.y),
    }
}

fn render_text(
    canvas: &Canvas,
    text: &crate::schema::TextLayer,
    _config: &VideoConfig,
    props: &AnimatedProperties,
) -> Result<()> {
    let font_mgr = FontMgr::default();
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

    let mut paint = paint_from_hex(&text.color);
    paint.set_alpha_f(1.0); // opacity handled at layer level

    // Apply typewriter effect
    let content = if props.visible_chars >= 0 {
        let chars: Vec<char> = text.content.chars().collect();
        let visible = (props.visible_chars as usize).min(chars.len());
        chars[..visible].iter().collect()
    } else {
        text.content.clone()
    };

    let lines = wrap_text(&content, &font, text.max_width);
    let line_height = text.line_height.unwrap_or(text.font_size * 1.3);
    let letter_spacing = text.letter_spacing.unwrap_or(0.0);

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

fn render_shape(canvas: &Canvas, shape: &crate::schema::ShapeLayer) -> Result<()> {
    let x = shape.position.x;
    let y = shape.position.y;
    let w = shape.size.width;
    let h = shape.size.height;
    let rect = Rect::from_xywh(x, y, w, h);

    // Fill
    if let Some(fill) = &shape.fill {
        let mut paint = match fill {
            Fill::Solid(color) => paint_from_hex(color),
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

        match shape.shape {
            ShapeType::Rect => canvas.draw_rect(rect, &paint),
            ShapeType::RoundedRect => {
                let r = shape.corner_radius.unwrap_or(8.0);
                let rrect = skia_safe::RRect::new_rect_xy(rect, r, r);
                canvas.draw_rrect(rrect, &paint)
            }
            ShapeType::Circle => {
                let radius = w.min(h) / 2.0;
                canvas.draw_circle((x + w / 2.0, y + h / 2.0), radius, &paint)
            }
            ShapeType::Ellipse => canvas.draw_oval(rect, &paint),
        };
    }

    // Stroke
    if let Some(stroke) = &shape.stroke {
        let mut paint = paint_from_hex(&stroke.color);
        paint.set_style(PaintStyle::Stroke);
        paint.set_stroke_width(stroke.width);

        match shape.shape {
            ShapeType::Rect => canvas.draw_rect(rect, &paint),
            ShapeType::RoundedRect => {
                let r = shape.corner_radius.unwrap_or(8.0);
                let rrect = skia_safe::RRect::new_rect_xy(rect, r, r);
                canvas.draw_rrect(rrect, &paint)
            }
            ShapeType::Circle => {
                let radius = w.min(h) / 2.0;
                canvas.draw_circle((x + w / 2.0, y + h / 2.0), radius, &paint)
            }
            ShapeType::Ellipse => canvas.draw_oval(rect, &paint),
        };
    }

    Ok(())
}

fn render_image(canvas: &Canvas, image: &crate::schema::ImageLayer) -> Result<()> {
    let data = std::fs::read(&image.src)
        .map_err(|e| anyhow::anyhow!("Failed to load image '{}': {}", image.src, e))?;

    let skia_data = skia_safe::Data::new_copy(&data);
    let img = skia_safe::Image::from_encoded(skia_data)
        .ok_or_else(|| anyhow::anyhow!("Failed to decode image '{}'", image.src))?;

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

/// Convert RGBA pixels to YUV420 (I420) for H.264 encoding
pub fn rgba_to_yuv420(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let w = width as usize;
    let h = height as usize;
    let y_size = w * h;
    let uv_size = (w / 2) * (h / 2);
    let mut yuv = vec![0u8; y_size + 2 * uv_size];

    let (y_plane, uv_planes) = yuv.split_at_mut(y_size);
    let (u_plane, v_plane) = uv_planes.split_at_mut(uv_size);

    for row in 0..h {
        for col in 0..w {
            let idx = (row * w + col) * 4;
            let r = rgba[idx] as f32;
            let g = rgba[idx + 1] as f32;
            let b = rgba[idx + 2] as f32;

            let y = (0.299 * r + 0.587 * g + 0.114 * b).clamp(0.0, 255.0);
            y_plane[row * w + col] = y as u8;

            if row % 2 == 0 && col % 2 == 0 {
                let u = (-0.169 * r - 0.331 * g + 0.500 * b + 128.0).clamp(0.0, 255.0);
                let v = (0.500 * r - 0.419 * g - 0.081 * b + 128.0).clamp(0.0, 255.0);
                let uv_idx = (row / 2) * (w / 2) + (col / 2);
                u_plane[uv_idx] = u as u8;
                v_plane[uv_idx] = v as u8;
            }
        }
    }

    yuv
}
