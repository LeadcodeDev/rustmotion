//! cosmic-text → Skia bridge.
//!
//! Provides:
//! - a global [`FontSystem`] (lazy, system + bundled fonts)
//! - `measure_text(...)` for taffy's `measure_fn`
//! - `paint_text(...)` to draw a laid-out buffer onto a Skia canvas via
//!   per-glyph rasterization through `SwashCache`
//!
//! Shaping & line-breaking are delegated to cosmic-text. Paint is done by
//! rasterizing each glyph to an alpha mask, tinting it with the requested
//! color, and blitting it as a small `Image` into Skia.

use std::sync::{Mutex, OnceLock};

use cosmic_text::{
    Attrs, Buffer, Color as CColor, Family, FontSystem, Metrics, Shaping, SwashCache, SwashContent,
    Weight, Wrap,
};
use skia_safe::{images, Canvas, Color, ColorType, Data, ImageInfo, Paint, Point};

static FONT_SYSTEM: OnceLock<Mutex<FontSystem>> = OnceLock::new();
static SWASH_CACHE: OnceLock<Mutex<SwashCache>> = OnceLock::new();

/// Borrow the global FontSystem. Created on first use with system fonts.
pub fn font_system() -> &'static Mutex<FontSystem> {
    FONT_SYSTEM.get_or_init(|| Mutex::new(FontSystem::new()))
}

/// Borrow the global SwashCache (for glyph rasterization).
pub fn swash_cache() -> &'static Mutex<SwashCache> {
    SWASH_CACHE.get_or_init(|| Mutex::new(SwashCache::new()))
}

/// Result of measuring a text run.
#[derive(Debug, Clone, Copy, Default)]
pub struct TextMetrics {
    pub width: f32,
    pub height: f32,
}

/// Configuration for laying out a single text run.
#[derive(Debug, Clone)]
pub struct TextStyle<'a> {
    pub font_family: Option<&'a str>,
    pub font_size: f32,
    pub line_height: f32,
    pub weight: u16,
    pub italic: bool,
    pub max_width: Option<f32>,
    pub wrap: bool,
    pub letter_spacing: f32,
}

impl<'a> Default for TextStyle<'a> {
    fn default() -> Self {
        Self {
            font_family: None,
            font_size: 16.0,
            line_height: 0.0, // 0 → derived from font_size * 1.2
            weight: 400,
            italic: false,
            max_width: None,
            wrap: true,
            letter_spacing: 0.0,
        }
    }
}

fn metrics_for(style: &TextStyle) -> Metrics {
    let lh = if style.line_height > 0.0 {
        style.line_height
    } else {
        style.font_size * 1.2
    };
    Metrics::new(style.font_size, lh)
}

fn attrs_for<'a>(style: &'a TextStyle) -> Attrs<'a> {
    let mut a = Attrs::new();
    if let Some(fam) = style.font_family {
        a = a.family(Family::Name(fam));
    }
    a = a.weight(Weight(style.weight));
    if style.italic {
        a = a.style(cosmic_text::Style::Italic);
    }
    a = a.letter_spacing(style.letter_spacing);
    a
}

/// Measure a text string given font + line constraints.
pub fn measure_text(text: &str, style: &TextStyle) -> TextMetrics {
    if text.is_empty() {
        return TextMetrics {
            width: 0.0,
            height: metrics_for(style).line_height,
        };
    }
    // Poison-tolerant: a panic on another render thread (e.g. a Skia panic
    // caught by a preview worker's panic fence) must not poison text shaping
    // for every subsequent frame — the FontSystem stays usable, each shape
    // call builds its own Buffer.
    let mut fs = font_system().lock().unwrap_or_else(|e| e.into_inner());
    let metrics = metrics_for(style);
    let mut buf = Buffer::new(&mut fs, metrics);
    buf.set_size(style.max_width, None);
    buf.set_wrap(if style.wrap { Wrap::Word } else { Wrap::None });
    buf.set_text(text, &attrs_for(style), Shaping::Advanced, None);
    buf.shape_until_scroll(&mut fs, false);

    let mut max_w = 0.0_f32;
    let mut lines = 0;
    for run in buf.layout_runs() {
        max_w = max_w.max(run.line_w);
        lines += 1;
    }
    let lines = lines.max(1) as f32;
    TextMetrics {
        width: max_w,
        height: lines * metrics.line_height,
    }
}

/// Paint a text string onto a Skia canvas at `origin` (top-left).
pub fn paint_text(
    canvas: &Canvas,
    text: &str,
    origin: (f32, f32),
    style: &TextStyle,
    color: Color,
) {
    if text.is_empty() {
        return;
    }
    // Poison-tolerant for the same reason as in `measure_text`.
    let mut fs = font_system().lock().unwrap_or_else(|e| e.into_inner());
    let metrics = metrics_for(style);
    let mut buf = Buffer::new(&mut fs, metrics);
    buf.set_size(style.max_width, None);
    buf.set_wrap(if style.wrap { Wrap::Word } else { Wrap::None });
    buf.set_text(text, &attrs_for(style), Shaping::Advanced, None);
    buf.shape_until_scroll(&mut fs, false);

    let mut sc = swash_cache().lock().unwrap_or_else(|e| e.into_inner());
    let ccolor = CColor::rgba(color.r(), color.g(), color.b(), color.a());

    for run in buf.layout_runs() {
        let line_y = run.line_y;
        for glyph in run.glyphs.iter() {
            let physical = glyph.physical((origin.0, origin.1 + line_y), 1.0);

            let img = match sc.get_image(&mut fs, physical.cache_key) {
                Some(image) => image,
                None => continue,
            };
            if img.placement.width == 0 || img.placement.height == 0 {
                continue;
            }
            blit_glyph(
                canvas,
                &img.data,
                img.placement.width as i32,
                img.placement.height as i32,
                physical.x as f32 + img.placement.left as f32,
                physical.y as f32 - img.placement.top as f32,
                img.content == SwashContent::Color,
                glyph
                    .color_opt
                    .map(|c| Color::from_argb(c.a(), c.r(), c.g(), c.b()))
                    .unwrap_or_else(|| {
                        Color::from_argb(ccolor.a(), ccolor.r(), ccolor.g(), ccolor.b())
                    }),
            );
        }
    }
}

fn blit_glyph(
    canvas: &Canvas,
    data: &[u8],
    w: i32,
    h: i32,
    x: f32,
    y: f32,
    is_color: bool,
    tint: Color,
) {
    if w <= 0 || h <= 0 || data.is_empty() {
        return;
    }
    let mut rgba: Vec<u8> = Vec::with_capacity((w * h * 4) as usize);
    if is_color {
        // SwashContent::Color : data is already RGBA premultiplied per glyph.
        // Layout matches what Skia expects with ColorType::RGBA8888 & Premul.
        rgba.extend_from_slice(data);
    } else {
        // Mask: alpha-only. Tint with `tint` and premultiply.
        let tr = tint.r() as u32;
        let tg = tint.g() as u32;
        let tb = tint.b() as u32;
        let ta = tint.a() as u32;
        for &a in data.iter() {
            let alpha = ta * a as u32 / 255;
            rgba.push((tr * alpha / 255) as u8);
            rgba.push((tg * alpha / 255) as u8);
            rgba.push((tb * alpha / 255) as u8);
            rgba.push(alpha as u8);
        }
    }
    let info = ImageInfo::new(
        (w, h),
        ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    let row_bytes = (w * 4) as usize;
    let data_obj = Data::new_copy(&rgba);
    let Some(image) = images::raster_from_data(&info, data_obj, row_bytes) else {
        return;
    };
    let paint = Paint::default();
    canvas.draw_image(image, Point::new(x, y), Some(&paint));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measure_empty_returns_zero_width() {
        let m = measure_text("", &TextStyle::default());
        assert_eq!(m.width, 0.0);
        assert!(m.height > 0.0); // one empty line of line_height
    }

    #[test]
    fn measure_hello_world_has_positive_width() {
        let m = measure_text(
            "Hello, world!",
            &TextStyle {
                font_size: 24.0,
                ..Default::default()
            },
        );
        assert!(m.width > 0.0);
        assert!(m.height > 0.0);
    }

    #[test]
    fn measure_wraps_with_max_width() {
        let style = TextStyle {
            font_size: 16.0,
            max_width: Some(40.0),
            wrap: true,
            ..Default::default()
        };
        let m = measure_text("the quick brown fox", &style);
        assert!(m.height > 16.0 * 1.2, "should wrap to multiple lines");
    }

    #[test]
    fn measure_no_wrap_stays_one_line() {
        let style = TextStyle {
            font_size: 16.0,
            max_width: None,
            wrap: false,
            ..Default::default()
        };
        let m = measure_text("the quick brown fox", &style);
        assert!(m.height < 16.0 * 1.2 * 2.0, "should be one line");
    }

    /// A panic on another thread while it holds the font mutex (e.g. a Skia
    /// panic caught by a preview worker's panic fence) poisons the lock.
    /// Shaping must survive that — otherwise one panic turns every subsequent
    /// text render on every thread into a panic cascade.
    #[test]
    fn measure_survives_poisoned_font_mutex() {
        let _ = std::thread::spawn(|| {
            let _guard = font_system().lock().unwrap_or_else(|e| e.into_inner());
            panic!("deliberate poison");
        })
        .join();
        assert!(font_system().is_poisoned(), "setup must have poisoned");
        let m = measure_text("still shaping", &TextStyle::default());
        assert!(m.width > 0.0, "measure works despite the poisoned mutex");
    }
}
