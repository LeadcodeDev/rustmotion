use std::sync::{Arc, OnceLock};

use crate::error::Result;
use dashmap::DashMap;
use skia_safe::{
    Canvas, Color4f, ColorType, Font, FontMgr, FontStyle,
    ImageInfo, Paint, Point, Rect, TextBlob, Typeface,
};

use crate::components::{ChildComponent, Component};
use crate::error::RustmotionError;
use crate::schema::{
    FontEntry, ShapeType,
};

// Thread-local FontMgr instance, created once per thread and reused
thread_local! {
    static THREAD_FONT_MGR: FontMgr = FontMgr::default();
}

pub(crate) fn font_mgr() -> FontMgr {
    THREAD_FONT_MGR.with(|mgr| mgr.clone())
}

/// Load custom fonts from FontEntry definitions
pub fn load_custom_fonts(fonts: &[FontEntry]) {
    let font_mgr = font_mgr();
    for entry in fonts {
        let path = std::path::Path::new(&entry.path);
        if path.exists() {
            if let Ok(data) = std::fs::read(path) {
                let sk_data = skia_safe::Data::new_copy(&data);
                // Register with font manager - use FontMgr to create Typeface
                if let Some(_typeface) = font_mgr.new_from_data(&sk_data, None) {
                    // Font loaded successfully — it's now available via family name matching
                }
            }
        }
    }
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

// ─── Counter formatting ─────────────────────────────────────────────────────

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

// ─── Text utilities ─────────────────────────────────────────────────────────

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

// ─── Emoji support ──────────────────────────────────────────────────────────

/// Check if a character is an emoji or emoji-related codepoint.
fn is_emoji(c: char) -> bool {
    let cp = c as u32;
    matches!(cp,
        // Miscellaneous Symbols and Pictographs (includes skin tone modifiers 1F3FB-1F3FF)
        0x1F300..=0x1F5FF |
        // Emoticons
        0x1F600..=0x1F64F |
        // Transport and Map Symbols
        0x1F680..=0x1F6FF |
        // Supplemental Symbols and Pictographs
        0x1F900..=0x1F9FF |
        // Symbols and Pictographs Extended-A
        0x1FA00..=0x1FA6F |
        // Symbols and Pictographs Extended-B
        0x1FA70..=0x1FAFF |
        // Dingbats (includes ✂️..➰ and arrows/symbols)
        0x2702..=0x27B0 |
        // Miscellaneous Symbols (includes ☀️..⛿)
        0x2600..=0x26FF |
        // Variation Selectors (keep with preceding emoji)
        0xFE00..=0xFE0F |
        // Zero-Width Joiner
        0x200D |
        // Combining Enclosing Keycap
        0x20E3 |
        // Regional Indicator Symbols (flags)
        0x1F1E0..=0x1F1FF |
        // Tags block (flag subdivisions)
        0xE0020..=0xE007F |
        // Playing cards, mahjong
        0x1F004 | 0x1F0CF |
        // Misc technical (⌚ ⌛ ⏩..⏳ ⏸..⏺)
        0x231A..=0x231B |
        0x23E9..=0x23F3 |
        0x23F8..=0x23FA |
        // Arrows and geometric symbols used as emoji
        0x2934..=0x2935 |
        0x25AA..=0x25AB |
        0x25B6 | 0x25C0 |
        0x25FB..=0x25FE |
        // Arrows
        0x2B05..=0x2B07 |
        0x2B1B..=0x2B1C |
        0x2B50 | 0x2B55 |
        // CJK symbols
        0x3030 | 0x303D |
        0x3297 | 0x3299 |
        // Copyright, registered, trademark
        0x00A9 | 0x00AE | 0x2122
    )
}

/// Resolve the system emoji typeface. Cached per thread.
pub(crate) fn emoji_typeface() -> Option<Typeface> {
    thread_local! {
        static EMOJI_TF: Option<Typeface> = {
            let fm = FontMgr::default();
            let style = FontStyle::normal();
            fm.match_family_style("Apple Color Emoji", style)
                .or_else(|| fm.match_family_style("Noto Color Emoji", style))
                .or_else(|| fm.match_family_style("Segoe UI Emoji", style))
        };
    }
    EMOJI_TF.with(|tf| tf.clone())
}

/// A segment of text that uses either the primary font or the emoji font.
struct TextRun {
    start: usize, // byte offset
    end: usize,   // byte offset
    is_emoji: bool,
}

/// Segment text into runs of emoji vs non-emoji characters.
fn segment_text_runs(text: &str) -> Vec<TextRun> {
    let mut runs = Vec::new();
    let mut chars = text.char_indices().peekable();

    while let Some(&(start_byte, c)) = chars.peek() {
        let emoji = is_emoji(c);
        let mut end_byte = start_byte + c.len_utf8();
        chars.next();

        while let Some(&(_, next_c)) = chars.peek() {
            if is_emoji(next_c) != emoji {
                break;
            }
            end_byte += next_c.len_utf8();
            chars.next();
        }

        runs.push(TextRun {
            start: start_byte,
            end: end_byte,
            is_emoji: emoji,
        });
    }
    runs
}

/// Check if text contains any emoji characters.
pub(crate) fn has_emoji(text: &str) -> bool {
    text.chars().any(is_emoji)
}

/// Draw a text line with emoji font fallback.
/// If `emoji_font` is None, falls back to drawing everything with the primary font.
pub(crate) fn draw_text_with_fallback(
    canvas: &Canvas,
    text: &str,
    font: &Font,
    emoji_font: &Option<Font>,
    letter_spacing: f32,
    x: f32,
    y: f32,
    paint: &Paint,
) {
    // Fast path: no emoji font or no emoji in text
    if emoji_font.is_none() || !has_emoji(text) {
        if letter_spacing.abs() > 0.01 {
            if let Some(blob) = make_text_blob_with_spacing(text, font, letter_spacing) {
                canvas.draw_text_blob(&blob, (x, y), paint);
            }
        } else if let Some(blob) = TextBlob::new(text, font) {
            canvas.draw_text_blob(&blob, (x, y), paint);
        }
        return;
    }

    let emoji_font = emoji_font.as_ref().unwrap();
    let runs = segment_text_runs(text);
    let mut cursor_x = x;

    for run in &runs {
        let segment = &text[run.start..run.end];
        let f = if run.is_emoji { emoji_font } else { font };

        if letter_spacing.abs() > 0.01 {
            if let Some(blob) = make_text_blob_with_spacing(segment, f, letter_spacing) {
                canvas.draw_text_blob(&blob, (cursor_x, y), paint);
            }
        } else if let Some(blob) = TextBlob::new(segment, f) {
            canvas.draw_text_blob(&blob, (cursor_x, y), paint);
        }

        // Advance cursor by the measured width of this run
        let (w, _) = f.measure_str(segment, None);
        let extra = if letter_spacing.abs() > 0.01 {
            letter_spacing * (segment.chars().count() as f32 - 1.0).max(0.0)
        } else {
            0.0
        };
        cursor_x += w + extra;
    }
}

/// Measure the width of a text line with emoji font fallback.
pub(crate) fn measure_text_with_fallback(
    text: &str,
    font: &Font,
    emoji_font: &Option<Font>,
    letter_spacing: f32,
) -> f32 {
    // Fast path
    if emoji_font.is_none() || !has_emoji(text) {
        let (w, _) = font.measure_str(text, None);
        let extra = if letter_spacing.abs() > 0.01 {
            letter_spacing * (text.chars().count() as f32 - 1.0).max(0.0)
        } else {
            0.0
        };
        return w + extra;
    }

    let emoji_font = emoji_font.as_ref().unwrap();
    let runs = segment_text_runs(text);
    let mut total_w = 0.0f32;

    for run in &runs {
        let segment = &text[run.start..run.end];
        let f = if run.is_emoji { emoji_font } else { font };
        let (w, _) = f.measure_str(segment, None);
        let extra = if letter_spacing.abs() > 0.01 {
            letter_spacing * (segment.chars().count() as f32 - 1.0).max(0.0)
        } else {
            0.0
        };
        total_w += w + extra;
    }
    total_w
}

/// Wrap text respecting emoji font fallback for accurate measurement.
pub(crate) fn wrap_text_with_fallback(
    text: &str,
    font: &Font,
    emoji_font: &Option<Font>,
    max_width: Option<f32>,
) -> Vec<String> {
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

            let width = measure_text_with_fallback(&test, font, emoji_font, 0.0);
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

// ─── Shape drawing ──────────────────────────────────────────────────────────

/// Build a `Path` for the given shape type without drawing it.
/// Returns `None` for primitive shapes that don't use path (Rect, RoundedRect, Circle, Ellipse) —
/// those are drawn directly for performance.
pub(crate) fn build_shape_path(shape_type: &ShapeType, x: f32, y: f32, w: f32, h: f32, corner_radius: Option<f32>) -> Option<skia_safe::Path> {
    match shape_type {
        ShapeType::Rect => {
            let mut path = skia_safe::Path::new();
            path.add_rect(Rect::from_xywh(x, y, w, h), None);
            Some(path)
        }
        ShapeType::RoundedRect => {
            let r = corner_radius.unwrap_or(8.0);
            let rrect = skia_safe::RRect::new_rect_xy(Rect::from_xywh(x, y, w, h), r, r);
            let mut path = skia_safe::Path::new();
            path.add_rrect(rrect, None);
            Some(path)
        }
        ShapeType::Circle => {
            let radius = w.min(h) / 2.0;
            let mut path = skia_safe::Path::new();
            path.add_circle((x + w / 2.0, y + h / 2.0), radius, None);
            Some(path)
        }
        ShapeType::Ellipse => {
            let mut path = skia_safe::Path::new();
            path.add_oval(Rect::from_xywh(x, y, w, h), None);
            Some(path)
        }
        ShapeType::Triangle => {
            let mut path = skia_safe::Path::new();
            path.move_to((x + w / 2.0, y));
            path.line_to((x + w, y + h));
            path.line_to((x, y + h));
            path.close();
            Some(path)
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
                if i == 0 { path.move_to((px, py)); } else { path.line_to((px, py)); }
            }
            path.close();
            Some(path)
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
                if i == 0 { path.move_to((px, py)); } else { path.line_to((px, py)); }
            }
            path.close();
            Some(path)
        }
        ShapeType::Path { data } => {
            skia_safe::Path::from_svg(data)
        }
    }
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

// ─── Icon fetching ──────────────────────────────────────────────────────────

pub(crate) fn fetch_icon_svg(icon: &str, color: &str, width: u32, height: u32) -> Result<Vec<u8>> {
    let (prefix, name) = icon
        .split_once(':')
        .ok_or_else(|| RustmotionError::InvalidIconFormat { icon: icon.to_string() })?;
    let hex_color = color.trim_start_matches('#');
    let width = width.max(1);
    let height = height.max(1);
    let url = format!(
        "https://api.iconify.design/{}/{}.svg?color=%23{}&width={}&height={}",
        prefix, name, hex_color, width, height
    );
    let response = ureq::get(&url)
        .call()
        .map_err(|e| RustmotionError::IconFetch { icon: icon.to_string(), reason: e.to_string() })?;
    let body = response
        .into_body()
        .read_to_vec()
        .map_err(|e| RustmotionError::IconFetch { icon: icon.to_string(), reason: e.to_string() })?;
    Ok(body)
}

/// Pre-fetch and cache all icon components before rendering.
/// Call this before the render loop to avoid HTTP requests during parallel rendering.
pub fn prefetch_icons(scenes: &[crate::schema::Scene]) {
    use std::collections::HashSet;
    use crate::traits::Styled;

    let mut seen = HashSet::new();

    fn collect_from_component(
        child: &ChildComponent,
        seen: &mut HashSet<(String, String, u32, u32)>,
    ) {
        match &child.component {
            Component::Icon(icon) => {
                let (w, h) = match &icon.size {
                    Some(size) => (size.width as u32, size.height as u32),
                    None => (24, 24),
                };
                seen.insert((icon.icon.clone(), icon.style_config().color_or("#FFFFFF").to_string(), w, h));
            }
            Component::Card(c) => {
                for child in &c.children {
                    collect_from_component(child, seen);
                }
            }
            Component::Flex(c) => {
                for child in &c.children {
                    collect_from_component(child, seen);
                }
            }
            Component::Grid(c) => {
                for child in &c.children {
                    collect_from_component(child, seen);
                }
            }
            Component::Positioned(c) => {
                for child in &c.children {
                    collect_from_component(child, seen);
                }
            }
            Component::Container(c) => {
                for child in &c.children {
                    collect_from_component(child, seen);
                }
            }
            _ => {}
        }
    }

    for scene in scenes {
        for child in &scene.children {
            collect_from_component(child, &mut seen);
        }
    }

    let cache = asset_cache();
    for (icon, color, w, h) in &seen {
        let cache_key = format!("icon:{}:{}:{}x{}", icon, color, w, h);
        if cache.contains_key(&cache_key) {
            continue;
        }
        match fetch_icon_svg(icon, color, *w, *h) {
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

// ─── Video frame extraction ─────────────────────────────────────────────────

/// Cache for pre-extracted video frames: key = "src:width:height", value = sorted list of (time, RGBA data, width, height)
static VIDEO_FRAME_CACHE: OnceLock<Arc<DashMap<String, Arc<Vec<(f64, Vec<u8>, u32, u32)>>>>> = OnceLock::new();

pub(crate) fn video_frame_cache() -> &'static Arc<DashMap<String, Arc<Vec<(f64, Vec<u8>, u32, u32)>>>> {
    VIDEO_FRAME_CACHE.get_or_init(|| Arc::new(DashMap::new()))
}

/// Pre-extract all needed frames from video sources in a single ffmpeg pass.
/// Called before the render loop to populate the video frame cache.
pub fn preextract_video_frames(
    scenarios_scenes: &[crate::schema::Scene],
    fps: u32,
) {
    use crate::traits::Timed;

    fn collect_videos(child: &ChildComponent, scene_frames: u32, fps: u32) {
        if let Component::Video(video) = &child.component {
            let rate = video.playback_rate.unwrap_or(1.0);
            let trim_start = video.trim_start.unwrap_or(0.0);
            let width = video.size.width as u32;
            let height = video.size.height as u32;

            let cache_key = format!("{}:{}x{}", video.src, width, height);
            let cache = video_frame_cache();

            if cache.contains_key(&cache_key) {
                return;
            }

            let (start_at, end_at) = video.timing();
            let start_frame = start_at.map(|s| (s * fps as f64).round() as u32).unwrap_or(0);
            let end_frame = end_at.map(|e| (e * fps as f64).round() as u32).unwrap_or(scene_frames);

            let mut times = Vec::new();
            for f in start_frame..end_frame {
                let time = f as f64 / fps as f64;
                let source_time = trim_start + time * rate;
                times.push(source_time);
            }

            if times.is_empty() {
                return;
            }

            let min_time = times.first().copied().unwrap_or(0.0);
            let max_time = times.last().copied().unwrap_or(0.0);
            let duration = max_time - min_time + (1.0 / fps as f64);

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
                _ => {}
            }
        }

        // Recurse into containers
        if let Some(children) = match &child.component {
            Component::Card(c) => Some(&c.children),
            Component::Flex(c) => Some(&c.children),
            Component::Grid(c) => Some(&c.children),
            Component::Positioned(c) => Some(&c.children),
            Component::Container(c) => Some(&c.children),
            _ => None,
        } {
            for c in children {
                collect_videos(c, scene_frames, fps);
            }
        }
    }

    for scene in scenarios_scenes {
        let scene_frames = (scene.duration * fps as f64).round() as u32;
        for child in &scene.children {
            collect_videos(child, scene_frames, fps);
        }
    }
}

pub(crate) fn find_closest_frame(frames: &[(f64, Vec<u8>, u32, u32)], target_time: f64) -> Option<(&[u8], u32, u32)> {
    if frames.is_empty() {
        return None;
    }
    let idx = frames.partition_point(|(t, _, _, _)| *t < target_time);
    let best = if idx == 0 {
        0
    } else if idx >= frames.len() {
        frames.len() - 1
    } else {
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
        .map_err(|e| RustmotionError::FfmpegSpawn { reason: e.to_string() })?;

    if !output.status.success() {
        return Err(RustmotionError::FfmpegFrameExtract { src: src.to_string() }.into());
    }

    Ok(output.stdout)
}

// ─── YUV conversion ─────────────────────────────────────────────────────────

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
    let uv_combined: Vec<(u8, u8)> = (0..uv_h)
        .into_par_iter()
        .flat_map(|uv_row| {
            let row = uv_row * 2;
            (0..uv_w)
                .map(move |uv_col| {
                    let col = uv_col * 2;
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
