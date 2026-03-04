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
    CaptionLayer, CaptionStyle, CardAlign, CardChild, CardDirection, CardDisplay, CardJustify,
    CardLayer, CounterLayer, Fill, FontWeight, GradientType, GridTrack, IconLayer, ImageFit, Layer,
    Position, Scene, ShapeText, ShapeType, TextAlign, TextLayer, VerticalAlign, VideoConfig,
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

pub(crate) fn asset_cache() -> &'static Arc<DashMap<String, skia_safe::Image>> {
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

pub(crate) fn gif_cache() -> &'static Arc<DashMap<String, Arc<(Vec<(Vec<u8>, u32, u32)>, Vec<f64>, f64)>>> {
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
    render_layer_in_container(canvas, layer, config, time, scene_duration, f32::INFINITY, f32::INFINITY)
}

/// Render a layer with container width constraints for text.
/// - `wrap_width`: constrains word wrapping for text children.
/// - `align_width`: overrides text alignment to be within this width (like a block element).
///   Use f32::INFINITY to keep legacy alignment behavior.
fn render_layer_in_container(canvas: &Canvas, layer: &Layer, config: &VideoConfig, time: f64, scene_duration: f64, wrap_width: f32, align_width: f32) -> Result<()> {
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

    render_layer_inner(canvas, layer, config, time, scene_duration, &props, wrap_width, align_width)
}

fn render_layer_inner(canvas: &Canvas, layer: &Layer, config: &VideoConfig, time: f64, scene_duration: f64, props: &AnimatedProperties, wrap_width: f32, align_width: f32) -> Result<()> {
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

    // Margin offset (standalone layers)
    let (mt, _mr, _mb, ml) = layer.props().margin();
    if mt.abs() > 0.001 || ml.abs() > 0.001 {
        canvas.translate((ml, mt));
    }

    // Padding inset (content offset)
    let (pad_t, _pad_r, _pad_b, pad_l) = layer.props().padding();
    if pad_t.abs() > 0.001 || pad_l.abs() > 0.001 {
        canvas.translate((pad_l, pad_t));
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

    // Convert f32::INFINITY to None for optional params
    let ww = if wrap_width.is_finite() { Some(wrap_width) } else { None };
    let aw = if align_width.is_finite() { Some(align_width) } else { None };

    match layer {
        Layer::Text(text) => render_text_constrained(canvas, text, config, props, ww, aw)?,
        Layer::Shape(shape) => render_shape(canvas, shape, props)?,
        Layer::Image(image) => render_image(canvas, image)?,
        Layer::Group(group) => render_group(canvas, group, config, time, scene_duration)?,
        Layer::Svg(svg) => render_svg(canvas, svg)?,
        Layer::Icon(icon) => render_icon(canvas, icon)?,
        Layer::Video(video) => render_video(canvas, video, time)?,
        Layer::Gif(gif) => render_gif(canvas, gif, time)?,
        Layer::Caption(caption) => render_caption(canvas, caption, config, time)?,
        Layer::Codeblock(cb) => codeblock::render_codeblock(canvas, cb, config, time, props)?,
        Layer::Counter(counter) => render_counter(canvas, counter, config, time, scene_duration, props)?,
        Layer::Card(card) | Layer::Flex(card) => render_card(canvas, card, config, time, scene_duration)?,
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

        render_layer_inner(temp_surface.canvas(), layer, config, sample_time, scene_duration, &props, f32::INFINITY, f32::INFINITY)?;
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
        Layer::Icon(i) => {
            let (w, h) = match &i.size {
                Some(sz) => (sz.width, sz.height),
                None => (24.0, 24.0),
            };
            (i.position.x + w / 2.0, i.position.y + h / 2.0)
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
        Layer::Counter(ct) => {
            // Same logic as TextLayer — measure formatted text to find center
            let font_mgr = font_mgr();
            use crate::schema::FontStyleType;
            let slant = match ct.font_style {
                FontStyleType::Normal => skia_safe::font_style::Slant::Upright,
                FontStyleType::Italic => skia_safe::font_style::Slant::Italic,
                FontStyleType::Oblique => skia_safe::font_style::Slant::Oblique,
            };
            let weight = match ct.font_weight {
                FontWeight::Bold => skia_safe::font_style::Weight::BOLD,
                FontWeight::Normal => skia_safe::font_style::Weight::NORMAL,
            };
            let font_style = FontStyle::new(weight, skia_safe::font_style::Width::NORMAL, slant);
            let typeface = font_mgr
                .match_family_style(&ct.font_family, font_style)
                .or_else(|| font_mgr.match_family_style("Helvetica", font_style))
                .or_else(|| font_mgr.match_family_style("Arial", font_style))
                .unwrap_or_else(|| font_mgr.match_family_style("sans-serif", font_style).unwrap());
            let font = Font::from_typeface(typeface, ct.font_size);
            let display = format_counter_value(ct.to, ct.decimals, &ct.separator, &ct.prefix, &ct.suffix);
            let (text_width, _) = font.measure_str(&display, None);
            let line_height = ct.font_size * 1.3;

            let cx = match ct.align {
                TextAlign::Left => ct.position.x + text_width / 2.0,
                TextAlign::Center => ct.position.x,
                TextAlign::Right => ct.position.x - text_width / 2.0,
            };
            let cy = ct.position.y - line_height / 2.0;
            (cx, cy)
        }
        Layer::Caption(c) => (c.position.x, c.position.y),
        Layer::Group(g) => (g.position.x, g.position.y),
        Layer::Card(c) | Layer::Flex(c) => {
            let (cw, ch) = measure_card_content(c);
            let (pt, pr, pb, pl) = c.padding.resolve();
            let w = c.size.as_ref().and_then(|s| s.width.fixed()).unwrap_or(cw + pl + pr);
            let h = c.size.as_ref().and_then(|s| s.height.fixed()).unwrap_or(ch + pt + pb);
            (c.position.x + w / 2.0, c.position.y + h / 2.0)
        }
    }
}

pub(crate) fn format_counter_value(
    value: f64,
    decimals: u8,
    separator: &Option<String>,
    prefix: &Option<String>,
    suffix: &Option<String>,
) -> String {
    // Format with decimals
    let formatted_number = format!("{:.prec$}", value, prec = decimals as usize);

    // Apply thousands separator if specified
    let formatted_number = if let Some(sep) = separator {
        let parts: Vec<&str> = formatted_number.split('.').collect();
        let integer_part = parts[0];

        // Handle negative sign
        let (sign, digits) = if integer_part.starts_with('-') {
            ("-", &integer_part[1..])
        } else {
            ("", integer_part)
        };

        let mut result = String::new();
        for (i, ch) in digits.chars().rev().enumerate() {
            if i > 0 && i % 3 == 0 {
                result.insert(0, sep.chars().next().unwrap_or(' '));
            }
            result.insert(0, ch);
        }

        if !sign.is_empty() {
            result.insert_str(0, sign);
        }

        if parts.len() > 1 {
            result.push('.');
            result.push_str(parts[1]);
        }

        result
    } else {
        formatted_number
    };

    // Build final string with prefix/suffix
    let mut result = String::new();
    if let Some(p) = prefix {
        result.push_str(p);
    }
    result.push_str(&formatted_number);
    if let Some(s) = suffix {
        result.push_str(s);
    }
    result
}

fn render_counter(
    canvas: &Canvas,
    counter: &CounterLayer,
    config: &VideoConfig,
    time: f64,
    scene_duration: f64,
    props: &AnimatedProperties,
) -> Result<()> {
    use super::animator::ease;

    // Calculate counter progress based on layer timing
    let start = counter.start_at.unwrap_or(0.0);
    let end = counter.end_at.unwrap_or(scene_duration);
    let duration = end - start;

    let t = if duration > 0.0 {
        ((time - start) / duration).clamp(0.0, 1.0)
    } else {
        1.0
    };

    let progress = ease(t, &counter.easing);
    let value = counter.from + (counter.to - counter.from) * progress;

    let content = format_counter_value(
        value,
        counter.decimals,
        &counter.separator,
        &counter.prefix,
        &counter.suffix,
    );

    // Build a temporary TextLayer and delegate to render_text
    let text_layer = TextLayer {
        content,
        position: counter.position.clone(),
        font_size: counter.font_size,
        color: counter.color.clone(),
        font_family: counter.font_family.clone(),
        font_weight: counter.font_weight.clone(),
        font_style: counter.font_style.clone(),
        align: counter.align.clone(),
        max_width: None,
        opacity: counter.opacity,
        line_height: None,
        letter_spacing: counter.letter_spacing,
        shadow: counter.shadow.clone(),
        stroke: counter.stroke.clone(),
        background: None,
        animations: Vec::new(),
        preset: None,
        preset_config: None,
        start_at: None,
        end_at: None,
        wiggle: None,
        motion_blur: None,
        padding: None,
        margin: None,
    };

    render_text(canvas, &text_layer, config, props)
}

fn render_text(
    canvas: &Canvas,
    text: &crate::schema::TextLayer,
    _config: &VideoConfig,
    props: &AnimatedProperties,
) -> Result<()> {
    render_text_constrained(canvas, text, _config, props, None, None)
}

/// Render text with optional container constraints.
/// - `wrap_width`: if Some, constrains word wrapping (combined with text.max_width).
/// - `align_width`: if Some, text alignment (left/center/right) is relative to this
///   width (like CSS text-align in a block). When None, uses legacy alignment.
fn render_text_constrained(
    canvas: &Canvas,
    text: &crate::schema::TextLayer,
    _config: &VideoConfig,
    props: &AnimatedProperties,
    wrap_width: Option<f32>,
    align_width: Option<f32>,
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

    // Effective max_width: use the most restrictive of text.max_width and wrap_width
    let effective_max_width = match (text.max_width, wrap_width) {
        (Some(tw), Some(ww)) => Some(tw.min(ww)),
        (Some(tw), None) => Some(tw),
        (None, Some(ww)) => Some(ww),
        (None, None) => None,
    };
    let lines = wrap_text(&content, &font, effective_max_width);
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

            // For alignment within a container, use advance width (font.measure_str)
            // to match the measurement used by measure_layer / flex layout.
            // blob.bounds().width() is the tight bounding box which differs slightly.
            let x = if let Some(aw) = align_width {
                let (advance_width, _) = font.measure_str(line, None);
                let advance_width = advance_width + letter_spacing * (line.chars().count() as f32 - 1.0).max(0.0);
                match text.align {
                    TextAlign::Left => text.position.x,
                    TextAlign::Center => text.position.x + (aw - advance_width) / 2.0,
                    TextAlign::Right => text.position.x + aw - advance_width,
                }
            } else {
                // Standalone text: align relative to position using bbox width
                match text.align {
                    TextAlign::Left => text.position.x,
                    TextAlign::Center => text.position.x - line_width / 2.0,
                    TextAlign::Right => text.position.x - line_width,
                }
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

pub(crate) fn wrap_text(text: &str, font: &Font, max_width: Option<f32>) -> Vec<String> {
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

pub(crate) fn make_text_blob_with_spacing(text: &str, font: &Font, spacing: f32) -> Option<TextBlob> {
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

pub(crate) fn draw_shape_path(canvas: &Canvas, shape_type: &ShapeType, x: f32, y: f32, w: f32, h: f32, corner_radius: Option<f32>, paint: &Paint) {
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

// --- Card layout helpers ---

fn measure_layer(layer: &Layer) -> (f32, f32) {
    match layer {
        Layer::Text(t) => {
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
            let lines = wrap_text(&t.content, &font, t.max_width);
            let line_height = t.line_height.unwrap_or(t.font_size * 1.3);
            let max_w = lines.iter().map(|l| {
                let (w, _) = font.measure_str(l, None);
                w
            }).fold(0.0f32, f32::max);
            let h = lines.len() as f32 * line_height;
            (max_w, h)
        }
        Layer::Shape(s) => (s.size.width, s.size.height),
        Layer::Image(i) => match &i.size {
            Some(s) => (s.width, s.height),
            None => (100.0, 100.0),
        },
        Layer::Svg(s) => match &s.size {
            Some(sz) => (sz.width, sz.height),
            None => (100.0, 100.0),
        },
        Layer::Icon(i) => match &i.size {
            Some(sz) => (sz.width, sz.height),
            None => (24.0, 24.0),
        },
        Layer::Video(v) => (v.size.width, v.size.height),
        Layer::Gif(g) => match &g.size {
            Some(sz) => (sz.width, sz.height),
            None => (100.0, 100.0),
        },
        Layer::Codeblock(cb) => match &cb.size {
            Some(s) => (s.width, s.height),
            None => (400.0, 300.0),
        },
        Layer::Counter(ct) => {
            let font_mgr = font_mgr();
            use crate::schema::FontStyleType;
            let slant = match ct.font_style {
                FontStyleType::Normal => skia_safe::font_style::Slant::Upright,
                FontStyleType::Italic => skia_safe::font_style::Slant::Italic,
                FontStyleType::Oblique => skia_safe::font_style::Slant::Oblique,
            };
            let weight = match ct.font_weight {
                FontWeight::Bold => skia_safe::font_style::Weight::BOLD,
                FontWeight::Normal => skia_safe::font_style::Weight::NORMAL,
            };
            let font_style = FontStyle::new(weight, skia_safe::font_style::Width::NORMAL, slant);
            let typeface = font_mgr
                .match_family_style(&ct.font_family, font_style)
                .or_else(|| font_mgr.match_family_style("Helvetica", font_style))
                .or_else(|| font_mgr.match_family_style("Arial", font_style))
                .unwrap_or_else(|| font_mgr.match_family_style("sans-serif", font_style).unwrap());
            let font = Font::from_typeface(typeface, ct.font_size);
            let display = format_counter_value(ct.to, ct.decimals, &ct.separator, &ct.prefix, &ct.suffix);
            let (text_width, _) = font.measure_str(&display, None);
            let line_height = ct.font_size * 1.3;
            (text_width, line_height)
        }
        Layer::Group(g) => {
            // Bounding box of children
            let mut max_x: f32 = 0.0;
            let mut max_y: f32 = 0.0;
            for child in &g.layers {
                let (cw, ch) = measure_layer(child);
                let pos = get_layer_position(child);
                max_x = max_x.max(pos.x + cw);
                max_y = max_y.max(pos.y + ch);
            }
            (max_x, max_y)
        }
        Layer::Card(c) | Layer::Flex(c) => {
            let (cw, ch) = measure_card_content(c);
            let (pt, pr, pb, pl) = c.padding.resolve();
            let w = c.size.as_ref().and_then(|s| s.width.fixed()).unwrap_or(cw + pl + pr);
            let h = c.size.as_ref().and_then(|s| s.height.fixed()).unwrap_or(ch + pt + pb);
            (w, h)
        }
        Layer::Caption(c) => {
            let w = c.max_width.unwrap_or(400.0);
            let h = c.font_size * 1.3;
            (w, h)
        }
    }
}

/// Measure layer including padding (for card layout sizing)
fn measure_layer_with_spacing(layer: &Layer) -> (f32, f32) {
    let (w, h) = measure_layer(layer);
    let (pt, pr, pb, pl) = layer.props().padding();
    (w + pl + pr, h + pt + pb)
}

/// Layout result for a single child in a card
struct LayoutResult {
    x: f32,
    y: f32,
    width: Option<f32>,
    height: Option<f32>,
    /// The child's natural (measured) width, always set.
    /// Used to pass as align_width so text alignment works correctly
    /// even when the flex layout already positioned the child.
    natural_width: f32,
}

fn get_layer_position(layer: &Layer) -> &Position {
    match layer {
        Layer::Text(t) => &t.position,
        Layer::Shape(s) => &s.position,
        Layer::Image(img) => &img.position,
        Layer::Svg(s) => &s.position,
        Layer::Icon(i) => &i.position,
        Layer::Video(v) => &v.position,
        Layer::Gif(g) => &g.position,
        Layer::Codeblock(cb) => &cb.position,
        Layer::Counter(ct) => &ct.position,
        Layer::Caption(c) => &c.position,
        Layer::Group(g) => &g.position,
        Layer::Card(c) | Layer::Flex(c) => &c.position,
    }
}

fn measure_card_content(card: &CardLayer) -> (f32, f32) {
    let sizes: Vec<(f32, f32)> = card.layers.iter().map(|c| {
        let (w, h) = measure_layer_with_spacing(&c.layer);
        let (mt, mr, mb, ml) = c.layer.props().margin();
        (w + ml + mr, h + mt + mb)
    }).collect();
    if sizes.is_empty() {
        return (0.0, 0.0);
    }
    let is_row = matches!(card.direction, CardDirection::Row | CardDirection::RowReverse);
    if is_row {
        let total_w: f32 = sizes.iter().map(|(w, _)| *w).sum::<f32>()
            + card.gap * (sizes.len() as f32 - 1.0).max(0.0);
        let max_h = sizes.iter().map(|(_, h)| *h).fold(0.0f32, f32::max);
        (total_w, max_h)
    } else {
        let max_w = sizes.iter().map(|(w, _)| *w).fold(0.0f32, f32::max);
        let total_h: f32 = sizes.iter().map(|(_, h)| *h).sum::<f32>()
            + card.gap * (sizes.len() as f32 - 1.0).max(0.0);
        (max_w, total_h)
    }
}

/// Compute flex layout for card children with grow/shrink/basis/stretch/reverse/align_self
fn compute_flex_layout(card: &CardLayer) -> Vec<LayoutResult> {
    let child_sizes: Vec<(f32, f32)> = card.layers.iter().map(|c| {
        let (w, h) = measure_layer_with_spacing(&c.layer);
        let (mt, mr, mb, ml) = c.layer.props().margin();
        (w + ml + mr, h + mt + mb)
    }).collect();
    if child_sizes.is_empty() {
        return vec![];
    }

    let (pt, pr, pb, pl) = card.padding.resolve();
    let container_w = card.size.as_ref().and_then(|s| s.width.fixed()).unwrap_or_else(|| {
        let (cw, _) = measure_card_content(card);
        cw + pl + pr
    }) - pl - pr;
    let container_h = card.size.as_ref().and_then(|s| s.height.fixed()).unwrap_or_else(|| {
        let (_, ch) = measure_card_content(card);
        ch + pt + pb
    }) - pt - pb;

    let is_row = matches!(card.direction, CardDirection::Row | CardDirection::RowReverse);
    let is_reverse = matches!(card.direction, CardDirection::RowReverse | CardDirection::ColumnReverse);

    let all_indices: Vec<usize> = (0..child_sizes.len()).collect();
    if !card.wrap {
        let results = layout_single_line_flex(
            &card.layers, &all_indices, &child_sizes, is_row, &card.justify, &card.align,
            card.gap, container_w, container_h,
        );
        if is_reverse {
            return reverse_layout(results, is_row, container_w, container_h);
        }
        return results;
    }

    // Wrap mode: partition children into lines
    let main_limit = if is_row { container_w } else { container_h };
    let mut lines: Vec<Vec<usize>> = vec![vec![]];
    let mut current_main = 0.0f32;

    for (i, &(cw, ch)) in child_sizes.iter().enumerate() {
        let child_main = if is_row { cw } else { ch };
        let needed = if lines.last().unwrap().is_empty() {
            child_main
        } else {
            current_main + card.gap + child_main
        };

        if needed > main_limit && !lines.last().unwrap().is_empty() {
            lines.push(vec![i]);
            current_main = child_main;
        } else {
            lines.last_mut().unwrap().push(i);
            current_main = needed;
        }
    }

    // Layout each line and stack them on the cross axis
    let mut results: Vec<LayoutResult> = (0..child_sizes.len())
        .map(|i| LayoutResult { x: 0.0, y: 0.0, width: None, height: None, natural_width: child_sizes[i].0 })
        .collect();
    let mut cross_offset = 0.0f32;

    for line in &lines {
        let line_sizes: Vec<(f32, f32)> = line.iter().map(|&i| child_sizes[i]).collect();
        let line_cross = line_sizes.iter().map(|&(w, h)| if is_row { h } else { w }).fold(0.0f32, f32::max);

        let line_results = layout_single_line_flex(
            &card.layers, line,
            &line_sizes, is_row, &card.justify, &card.align,
            card.gap, container_w, container_h,
        );

        for (j, &idx) in line.iter().enumerate() {
            let mut r = LayoutResult {
                x: line_results[j].x,
                y: line_results[j].y,
                width: line_results[j].width,
                height: line_results[j].height,
                natural_width: line_results[j].natural_width,
            };
            let child_align = card.layers[idx].align_self.as_ref().unwrap_or(&card.align);
            if is_row {
                let (cross_pos, stretch_size) = align_item_flex(line_sizes[j].1, line_cross, child_align);
                r.y = cross_offset + cross_pos;
                if stretch_size.is_some() {
                    r.height = stretch_size;
                }
            } else {
                let (cross_pos, stretch_size) = align_item_flex(line_sizes[j].0, line_cross, child_align);
                r.x = cross_offset + cross_pos;
                if stretch_size.is_some() {
                    r.width = stretch_size;
                }
            }
            results[idx] = r;
        }

        cross_offset += line_cross + card.gap;
    }

    if is_reverse {
        reverse_layout(results, is_row, container_w, container_h)
    } else {
        results
    }
}

fn reverse_layout(mut results: Vec<LayoutResult>, is_row: bool, container_w: f32, container_h: f32) -> Vec<LayoutResult> {
    // Mirror positions: pos = container_main - pos - child_size
    // We need child sizes; use width/height from LayoutResult or measure
    for r in results.iter_mut() {
        if is_row {
            // We don't know the exact child main size from LayoutResult alone,
            // but we stored the flex-adjusted size in width if stretched
            // For simplicity, mirror around the center
            r.x = container_w - r.x;
            // This puts the right edge at the mirrored position
            // We need to shift left by the child's main size, but we don't have it here.
            // Instead, reverse the order and recompute from the end
        } else {
            r.y = container_h - r.y;
        }
    }
    // Better approach: reverse the order of results and swap their positions
    // Actually, the simplest correct approach is to just reverse the iteration order
    // in the main function. But since we've already computed positions,
    // let's negate and re-offset.
    // The cleanest way: just reverse the vec
    results.reverse();
    results
}

/// Compute grid layout for card children
fn compute_grid_layout(card: &CardLayer) -> Vec<LayoutResult> {
    let n = card.layers.len();
    if n == 0 {
        return vec![];
    }

    let (pt, pr, pb, pl) = card.padding.resolve();
    let container_w = card.size.as_ref().and_then(|s| s.width.fixed()).unwrap_or(600.0) - pl - pr;
    let container_h = card.size.as_ref().and_then(|s| s.height.fixed()).unwrap_or(400.0) - pt - pb;

    let col_tracks = card.grid_template_columns.as_deref().unwrap_or(&[GridTrack::Fr(1.0)]);
    let row_tracks = card.grid_template_rows.as_deref().unwrap_or(&[GridTrack::Fr(1.0)]);

    let num_cols = col_tracks.len().max(1);
    let num_rows = row_tracks.len().max(1);

    // Measure children for Auto tracks (including padding + margin)
    let child_sizes: Vec<(f32, f32)> = card.layers.iter().map(|c| {
        let (w, h) = measure_layer_with_spacing(&c.layer);
        let (mt, mr, mb, ml) = c.layer.props().margin();
        (w + ml + mr, h + mt + mb)
    }).collect();

    // Place children in grid cells
    let mut placements: Vec<(usize, usize, usize, usize)> = Vec::with_capacity(n); // (col, row, col_span, row_span)
    let mut grid_occupied: Vec<Vec<bool>> = vec![vec![false; num_cols]; num_rows * 2]; // extra rows for overflow
    let mut auto_cursor = (0usize, 0usize); // (row, col)

    for child in &card.layers {
        let col_start = child.grid_column.as_ref().and_then(|g| g.start).map(|s| (s - 1).max(0) as usize);
        let row_start = child.grid_row.as_ref().and_then(|g| g.start).map(|s| (s - 1).max(0) as usize);
        let col_span = child.grid_column.as_ref().and_then(|g| g.span).unwrap_or(1).max(1) as usize;
        let row_span = child.grid_row.as_ref().and_then(|g| g.span).unwrap_or(1).max(1) as usize;

        if let (Some(c), Some(r)) = (col_start, row_start) {
            placements.push((c.min(num_cols - 1), r, col_span.min(num_cols - c.min(num_cols - 1)), row_span));
            // Mark occupied
            for dr in 0..row_span {
                for dc in 0..col_span {
                    let rr = r + dr;
                    let cc = c + dc;
                    if rr < grid_occupied.len() && cc < num_cols {
                        grid_occupied[rr][cc] = true;
                    }
                }
            }
        } else if let Some(c) = col_start {
            // Column specified, find next available row at that column
            let r = auto_cursor.0;
            placements.push((c.min(num_cols - 1), r, col_span.min(num_cols - c.min(num_cols - 1)), row_span));
            for dr in 0..row_span {
                for dc in 0..col_span {
                    let rr = r + dr;
                    let cc = c + dc;
                    if rr < grid_occupied.len() && cc < num_cols {
                        grid_occupied[rr][cc] = true;
                    }
                }
            }
        } else if let Some(r) = row_start {
            // Row specified, find next available col at that row
            let mut c = 0;
            while c < num_cols && r < grid_occupied.len() && grid_occupied[r][c] {
                c += 1;
            }
            let c = c.min(num_cols - 1);
            placements.push((c, r, col_span.min(num_cols - c), row_span));
            for dr in 0..row_span {
                for dc in 0..col_span.min(num_cols - c) {
                    let rr = r + dr;
                    let cc = c + dc;
                    if rr < grid_occupied.len() && cc < num_cols {
                        grid_occupied[rr][cc] = true;
                    }
                }
            }
        } else {
            // Auto placement: row-major
            let (mut ar, mut ac) = auto_cursor;
            // Find next free cell
            loop {
                if ar >= grid_occupied.len() {
                    // Extend grid
                    grid_occupied.push(vec![false; num_cols]);
                }
                if !grid_occupied[ar][ac] {
                    // Check if span fits
                    let mut fits = true;
                    for dc in 0..col_span {
                        if ac + dc >= num_cols {
                            fits = false;
                            break;
                        }
                    }
                    if fits {
                        break;
                    }
                }
                ac += 1;
                if ac >= num_cols {
                    ac = 0;
                    ar += 1;
                }
            }
            placements.push((ac, ar, col_span.min(num_cols - ac), row_span));
            for dr in 0..row_span {
                for dc in 0..col_span.min(num_cols - ac) {
                    let rr = ar + dr;
                    let cc = ac + dc;
                    if rr >= grid_occupied.len() {
                        grid_occupied.push(vec![false; num_cols]);
                    }
                    if cc < num_cols {
                        grid_occupied[rr][cc] = true;
                    }
                }
            }
            // Advance cursor
            ac += col_span;
            if ac >= num_cols {
                ac = 0;
                ar += 1;
            }
            auto_cursor = (ar, ac);
        }
    }

    // Determine actual number of rows used
    let actual_num_rows = placements.iter()
        .map(|&(_, r, _, rs)| r + rs)
        .max()
        .unwrap_or(num_rows)
        .max(num_rows);

    // Resolve track sizes
    let col_sizes = resolve_tracks(col_tracks, container_w, card.gap, num_cols, &child_sizes, &placements, true);
    // For rows, extend row_tracks if actual_num_rows > num_rows
    let mut extended_row_tracks: Vec<GridTrack> = row_tracks.to_vec();
    while extended_row_tracks.len() < actual_num_rows {
        extended_row_tracks.push(GridTrack::Auto);
    }
    let row_sizes = resolve_tracks(&extended_row_tracks, container_h, card.gap, actual_num_rows, &child_sizes, &placements, false);

    // Compute cell positions
    let mut col_offsets = vec![0.0f32; num_cols + 1];
    for i in 0..num_cols {
        col_offsets[i + 1] = col_offsets[i] + col_sizes[i] + card.gap;
    }
    let mut row_offsets = vec![0.0f32; actual_num_rows + 1];
    for i in 0..actual_num_rows {
        row_offsets[i + 1] = row_offsets[i] + row_sizes[i] + card.gap;
    }

    let mut results = Vec::with_capacity(n);
    for (i, &(col, row, col_span, row_span)) in placements.iter().enumerate() {
        let x = col_offsets[col];
        let y = row_offsets[row];
        let end_col = (col + col_span).min(num_cols);
        let end_row = (row + row_span).min(actual_num_rows);
        let w = col_offsets[end_col] - col_offsets[col] - card.gap;
        let h = row_offsets[end_row] - row_offsets[row] - card.gap;

        // Center child within cell based on align
        let (cw, ch) = child_sizes[i];
        let child_align = card.layers[i].align_self.as_ref().unwrap_or(&card.align);
        let (cx, _) = align_item_flex(cw, w.max(0.0), child_align);
        let (cy, _) = align_item_flex(ch, h.max(0.0), child_align);

        results.push(LayoutResult {
            x: x + cx,
            y: y + cy,
            width: Some(w.max(0.0)),
            height: Some(h.max(0.0)),
            natural_width: cw,
        });
    }

    results
}

/// Resolve grid track sizes from track definitions
fn resolve_tracks(
    tracks: &[GridTrack],
    container_size: f32,
    gap: f32,
    num_tracks: usize,
    child_sizes: &[(f32, f32)],
    placements: &[(usize, usize, usize, usize)],
    is_col: bool,
) -> Vec<f32> {
    let total_gaps = gap * (num_tracks as f32 - 1.0).max(0.0);
    let available = (container_size - total_gaps).max(0.0);

    let mut sizes = vec![0.0f32; num_tracks];
    let mut fr_total = 0.0f32;
    let mut fixed_total = 0.0f32;

    // First pass: resolve Px and Auto
    for (i, track) in tracks.iter().enumerate() {
        if i >= num_tracks {
            break;
        }
        match track {
            GridTrack::Px(v) => {
                sizes[i] = *v;
                fixed_total += *v;
            }
            GridTrack::Auto => {
                // Find max content size for children in this track
                let mut max_size = 0.0f32;
                for (ci, &(col, row, col_span, row_span)) in placements.iter().enumerate() {
                    let (track_start, span) = if is_col { (col, col_span) } else { (row, row_span) };
                    if track_start == i && span == 1 {
                        let s = if is_col { child_sizes[ci].0 } else { child_sizes[ci].1 };
                        max_size = max_size.max(s);
                    }
                }
                sizes[i] = max_size;
                fixed_total += max_size;
            }
            GridTrack::Fr(f) => {
                fr_total += f;
            }
        }
    }

    // Second pass: distribute remaining space to Fr tracks
    if fr_total > 0.0 {
        let fr_space = (available - fixed_total).max(0.0);
        for (i, track) in tracks.iter().enumerate() {
            if i >= num_tracks {
                break;
            }
            if let GridTrack::Fr(f) = track {
                sizes[i] = fr_space * f / fr_total;
            }
        }
    }

    sizes
}

fn layout_single_line_flex(
    all_children: &[CardChild],
    indices: &[usize],
    sizes: &[(f32, f32)],
    is_row: bool,
    justify: &CardJustify,
    align: &CardAlign,
    gap: f32,
    container_w: f32,
    container_h: f32,
) -> Vec<LayoutResult> {
    let n = sizes.len();
    let container_main = if is_row { container_w } else { container_h };
    let container_cross = if is_row { container_h } else { container_w };

    // Compute base main sizes (flex_basis or natural)
    let mut main_sizes: Vec<f32> = Vec::with_capacity(n);
    for i in 0..n {
        let child = &all_children[indices[i]];
        let natural = if is_row { sizes[i].0 } else { sizes[i].1 };
        let basis = child.flex_basis.unwrap_or(natural);
        main_sizes.push(basis);
    }
    let cross_sizes: Vec<f32> = sizes.iter().map(|&(w, h)| if is_row { h } else { w }).collect();

    let total_main: f32 = main_sizes.iter().sum::<f32>() + gap * (n as f32 - 1.0).max(0.0);
    let remaining = container_main - total_main;

    // flex_grow / flex_shrink
    if remaining > 0.0 {
        let total_grow: f32 = indices.iter().map(|&idx| all_children[idx].flex_grow.unwrap_or(0.0)).sum();
        if total_grow > 0.0 {
            for i in 0..n {
                let grow = all_children[indices[i]].flex_grow.unwrap_or(0.0);
                if grow > 0.0 {
                    main_sizes[i] += remaining * (grow / total_grow);
                }
            }
        }
    } else if remaining < 0.0 {
        let overflow = -remaining;
        let weighted_total: f32 = (0..n).map(|i| {
            let shrink = all_children[indices[i]].flex_shrink.unwrap_or(1.0);
            main_sizes[i] * shrink
        }).sum();
        if weighted_total > 0.0 {
            for i in 0..n {
                let shrink = all_children[indices[i]].flex_shrink.unwrap_or(1.0);
                let weight = main_sizes[i] * shrink;
                main_sizes[i] = (main_sizes[i] - overflow * weight / weighted_total).max(0.0);
            }
        }
    }

    // Justify: compute starting offset and effective gap
    let actual_total: f32 = main_sizes.iter().sum::<f32>() + gap * (n as f32 - 1.0).max(0.0);
    let new_remaining = (container_main - actual_total).max(0.0);

    let (mut main_pos, effective_gap) = match justify {
        CardJustify::Start => (0.0, gap),
        CardJustify::Center => (new_remaining / 2.0, gap),
        CardJustify::End => (new_remaining, gap),
        CardJustify::SpaceBetween => {
            if n <= 1 {
                (0.0, gap)
            } else {
                let total_no_gap: f32 = main_sizes.iter().sum();
                let space = (container_main - total_no_gap).max(0.0) / (n as f32 - 1.0);
                (0.0, space)
            }
        }
        CardJustify::SpaceAround => {
            if n == 0 {
                (0.0, gap)
            } else {
                let total_no_gap: f32 = main_sizes.iter().sum();
                let space = (container_main - total_no_gap).max(0.0) / n as f32;
                (space / 2.0, space)
            }
        }
        CardJustify::SpaceEvenly => {
            let total_no_gap: f32 = main_sizes.iter().sum();
            let space = (container_main - total_no_gap).max(0.0) / (n as f32 + 1.0);
            (space, space)
        }
    };

    let mut result = Vec::with_capacity(n);
    for i in 0..n {
        let child = &all_children[indices[i]];
        let child_align = child.align_self.as_ref().unwrap_or(align);
        let (cross_pos, stretch_size) = align_item_flex(cross_sizes[i], container_cross, child_align);

        let natural_w = sizes[i].0;
        let mut lr = if is_row {
            LayoutResult { x: main_pos, y: cross_pos, width: None, height: None, natural_width: natural_w }
        } else {
            LayoutResult { x: cross_pos, y: main_pos, width: None, height: None, natural_width: natural_w }
        };

        // Apply stretch on cross axis
        if let Some(s) = stretch_size {
            if is_row {
                lr.height = Some(s);
            } else {
                lr.width = Some(s);
            }
        }

        // If flex_grow changed the main size, store it
        let natural_main = if is_row { sizes[i].0 } else { sizes[i].1 };
        if (main_sizes[i] - natural_main).abs() > 0.01 {
            if is_row {
                lr.width = Some(main_sizes[i]);
            } else {
                lr.height = Some(main_sizes[i]);
            }
        }

        result.push(lr);
        main_pos += main_sizes[i] + effective_gap;
    }
    result
}

fn align_item_flex(item_size: f32, container_size: f32, align: &CardAlign) -> (f32, Option<f32>) {
    match align {
        CardAlign::Start => (0.0, None),
        CardAlign::Center => ((container_size - item_size) / 2.0, None),
        CardAlign::End => (container_size - item_size, None),
        CardAlign::Stretch => (0.0, Some(container_size)),
    }
}

fn render_card(
    canvas: &Canvas,
    card: &CardLayer,
    config: &VideoConfig,
    time: f64,
    scene_duration: f64,
) -> Result<()> {
    canvas.save();
    canvas.translate((card.position.x, card.position.y));

    let (pt, pr, pb, pl) = card.padding.resolve();
    let (content_w, content_h) = measure_card_content(card);
    let card_w = card.size.as_ref().and_then(|s| s.width.fixed()).unwrap_or(content_w + pl + pr);
    let card_h = card.size.as_ref().and_then(|s| s.height.fixed()).unwrap_or(content_h + pt + pb);
    let rect = Rect::from_xywh(0.0, 0.0, card_w, card_h);
    let rrect = skia_safe::RRect::new_rect_xy(rect, card.corner_radius, card.corner_radius);

    // 1. Shadow
    if let Some(ref shadow) = card.shadow {
        let shadow_rect = Rect::from_xywh(shadow.offset_x, shadow.offset_y, card_w, card_h);
        let shadow_rrect = skia_safe::RRect::new_rect_xy(shadow_rect, card.corner_radius, card.corner_radius);
        let mut shadow_paint = paint_from_hex(&shadow.color);
        if shadow.blur > 0.0 {
            shadow_paint.set_mask_filter(skia_safe::MaskFilter::blur(
                skia_safe::BlurStyle::Normal,
                shadow.blur / 2.0,
                false,
            ));
        }
        canvas.draw_rrect(shadow_rrect, &shadow_paint);
    }

    // 2. Background
    if let Some(ref bg) = card.background {
        let bg_paint = paint_from_hex(bg);
        canvas.draw_rrect(rrect, &bg_paint);
    }

    // 3. Clip to rounded rect for children
    canvas.save();
    canvas.clip_rrect(rrect, skia_safe::ClipOp::Intersect, true);

    // 4. Compute layout and render children
    let layout = match card.display {
        CardDisplay::Flex => compute_flex_layout(card),
        CardDisplay::Grid => compute_grid_layout(card),
    };
    canvas.translate((pl, pt));

    for (i, child_wrapper) in card.layers.iter().enumerate() {
        if i >= layout.len() {
            break;
        }
        let result = &layout[i];
        let child = &child_wrapper.layer;
        let child_pos = get_layer_position(child);

        // Translate-compensation: shift canvas so child's own position lands at layout position
        let (mt, _mr, _mb, ml) = child.props().margin();
        canvas.save();
        canvas.translate((result.x - child_pos.x + ml, result.y - child_pos.y + mt));

        // Container width for child rendering:
        // wrap_width: constrains word wrapping (card content width or forced width)
        // align_width: controls text alignment scope
        //   - forced width (stretch/grow): align within the cell
        //   - natural width: align within the child's own measured width
        //     (so center-align becomes a no-op, since flex already positioned the child)
        let (child_wrap_width, child_align_width) = if let Some(w) = result.width {
            let (child_pl, child_pr) = (child.props().padding().3, child.props().padding().1);
            let cw = (w - child_pl - child_pr).max(0.0);
            (cw, cw)
        } else {
            let wrap = (card_w - pl - pr).max(0.0);
            let align = result.natural_width;
            (wrap, align)
        };

        // Clip to cell bounds if grid/stretch provides forced size
        if let (Some(w), Some(h)) = (result.width, result.height) {
            let clip_rect = Rect::from_xywh(child_pos.x, child_pos.y, w, h);
            canvas.clip_rect(clip_rect, skia_safe::ClipOp::Intersect, false);
        }

        render_layer_in_container(canvas, child, config, time, scene_duration, child_wrap_width, child_align_width)?;
        canvas.restore();
    }

    canvas.restore(); // clip

    // 5. Border on top
    if let Some(ref border) = card.border {
        let mut border_paint = paint_from_hex(&border.color);
        border_paint.set_style(PaintStyle::Stroke);
        border_paint.set_stroke_width(border.width);
        canvas.draw_rrect(rrect, &border_paint);
    }

    canvas.restore(); // position translate

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

pub(crate) fn fetch_icon_svg(icon: &str, color: &str, width: u32, height: u32) -> Result<Vec<u8>> {
    let (prefix, name) = icon
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("Invalid icon format: '{}' (expected 'prefix:name')", icon))?;
    let hex_color = color.trim_start_matches('#');
    let url = format!(
        "https://api.iconify.design/{}/{}.svg?color=%23{}&width={}&height={}",
        prefix, name, hex_color, width, height
    );
    let response = ureq::get(&url)
        .call()
        .map_err(|e| anyhow::anyhow!("Failed to fetch icon '{}': {}", icon, e))?;
    let body = response
        .into_body()
        .read_to_vec()
        .map_err(|e| anyhow::anyhow!("Failed to read icon response: {}", e))?;
    Ok(body)
}

fn render_icon(canvas: &Canvas, icon: &IconLayer) -> Result<()> {
    let (target_w, target_h) = match &icon.size {
        Some(size) => (size.width as u32, size.height as u32),
        None => (24, 24),
    };

    let cache_key = format!(
        "icon:{}:{}:{}x{}",
        icon.icon, icon.color, target_w, target_h
    );

    let cache = asset_cache();
    let img = if let Some(cached) = cache.get(&cache_key) {
        cached.clone()
    } else {
        let svg_data = fetch_icon_svg(&icon.icon, &icon.color, target_w, target_h)?;

        let opt = usvg::Options::default();
        let tree = usvg::Tree::from_data(&svg_data, &opt)
            .map_err(|e| anyhow::anyhow!("Failed to parse icon SVG '{}': {}", icon.icon, e))?;

        let svg_size = tree.size();
        let render_w = target_w.max(1);
        let render_h = target_h.max(1);

        let mut pixmap = tiny_skia::Pixmap::new(render_w, render_h)
            .ok_or_else(|| anyhow::anyhow!("Failed to create pixmap for icon"))?;

        let scale_x = render_w as f32 / svg_size.width();
        let scale_y = render_h as f32 / svg_size.height();
        let transform = tiny_skia::Transform::from_scale(scale_x, scale_y);

        resvg::render(&tree, transform, &mut pixmap.as_mut());

        let img_data = skia_safe::Data::new_copy(pixmap.data());
        let img_info = ImageInfo::new(
            (render_w as i32, render_h as i32),
            ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let decoded =
            skia_safe::images::raster_from_data(&img_info, img_data, render_w as usize * 4)
                .ok_or_else(|| anyhow::anyhow!("Failed to create Skia image from icon"))?;
        cache.insert(cache_key, decoded.clone());
        decoded
    };

    let dst = Rect::from_xywh(
        icon.position.x,
        icon.position.y,
        target_w as f32,
        target_h as f32,
    );
    let paint = Paint::default();
    canvas.draw_image_rect(img, None, dst, &paint);

    Ok(())
}

/// Pre-fetch and cache all icon layers before rendering.
/// Call this before the render loop to avoid HTTP requests during parallel rendering.
pub fn prefetch_icons(scenes: &[crate::schema::Scene]) {
    use std::collections::HashSet;

    let mut seen = HashSet::new();

    fn collect_from_layer(
        layer: &Layer,
        seen: &mut HashSet<(String, String, u32, u32)>,
    ) {
        match layer {
            Layer::Icon(icon) => {
                let (w, h) = match &icon.size {
                    Some(size) => (size.width as u32, size.height as u32),
                    None => (24, 24),
                };
                seen.insert((icon.icon.clone(), icon.color.clone(), w, h));
            }
            Layer::Group(g) => {
                for child in &g.layers {
                    collect_from_layer(child, seen);
                }
            }
            Layer::Card(c) | Layer::Flex(c) => {
                for child in &c.layers {
                    collect_from_layer(&child.layer, seen);
                }
            }
            _ => {}
        }
    }

    for scene in scenes {
        for layer in &scene.layers {
            collect_from_layer(layer, &mut seen);
        }
    }

    let cache = asset_cache();
    for (icon, color, w, h) in &seen {
        let cache_key = format!("icon:{}:{}:{}x{}", icon, color, w, h);
        if cache.contains_key(&cache_key) {
            continue;
        }
        match fetch_icon_svg(icon, &color, *w, *h) {
            Ok(svg_data) => {
                let opt = usvg::Options::default();
                match usvg::Tree::from_data(&svg_data, &opt) {
                    Ok(tree) => {
                        let svg_size = tree.size();
                        let render_w = (*w).max(1);
                        let render_h = (*h).max(1);
                        if let Some(mut pixmap) = tiny_skia::Pixmap::new(render_w, render_h) {
                            let scale_x = render_w as f32 / svg_size.width();
                            let scale_y = render_h as f32 / svg_size.height();
                            let transform = tiny_skia::Transform::from_scale(scale_x, scale_y);
                            resvg::render(&tree, transform, &mut pixmap.as_mut());
                            let img_data = skia_safe::Data::new_copy(pixmap.data());
                            let img_info = ImageInfo::new(
                                (render_w as i32, render_h as i32),
                                ColorType::RGBA8888,
                                skia_safe::AlphaType::Premul,
                                None,
                            );
                            if let Some(decoded) = skia_safe::images::raster_from_data(
                                &img_info,
                                img_data,
                                render_w as usize * 4,
                            ) {
                                cache.insert(cache_key, decoded);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Warning: failed to parse icon '{}': {}", icon, e);
                    }
                }
            }
            Err(e) => {
                eprintln!("Warning: failed to fetch icon '{}': {}", icon, e);
            }
        }
    }
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

pub(crate) fn video_frame_cache() -> &'static Arc<DashMap<String, Arc<Vec<(f64, Vec<u8>, u32, u32)>>>> {
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

pub(crate) fn find_closest_frame(frames: &[(f64, Vec<u8>, u32, u32)], target_time: f64) -> Option<(&[u8], u32, u32)> {
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

pub(crate) fn extract_video_frame(src: &str, time: f64, width: u32, height: u32) -> Result<Vec<u8>> {
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
