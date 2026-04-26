use skia_safe::{FontMgr, FontStyle, Typeface};

use crate::error::{Result, RustmotionError};
use crate::schema::FontEntry;

// Thread-local FontMgr instance, created once per thread and reused
thread_local! {
    static THREAD_FONT_MGR: FontMgr = FontMgr::default();
}

pub fn font_mgr() -> FontMgr {
    THREAD_FONT_MGR.with(|mgr| mgr.clone())
}

/// Load custom fonts from FontEntry definitions. Emits a single warning per
/// missing or unreadable file so the user notices broken paths up-front.
pub fn load_custom_fonts(fonts: &[FontEntry]) {
    let font_mgr = font_mgr();
    for entry in fonts {
        let path = std::path::Path::new(&entry.path);
        if !path.exists() {
            eprintln!(
                "Warning: custom font '{}' not found at '{}' — falling back to system fonts",
                entry.family, entry.path
            );
            continue;
        }
        match std::fs::read(path) {
            Ok(data) => {
                let sk_data = skia_safe::Data::new_copy(&data);
                if font_mgr.new_from_data(&sk_data, None).is_none() {
                    eprintln!(
                        "Warning: failed to register custom font '{}' from '{}'",
                        entry.family, entry.path
                    );
                }
            }
            Err(e) => {
                eprintln!(
                    "Warning: failed to read custom font '{}' from '{}': {}",
                    entry.family, entry.path, e
                );
            }
        }
    }
}

/// Resolve a typeface for `family` falling back through Helvetica → Arial →
/// the OS default. Returns `RustmotionError::FontNotFound` only if the host
/// system has no usable font at all (essentially unreachable on every
/// supported platform). Use this instead of `.expect("FontNotFound")` so we
/// never panic from a `paint` callback.
pub fn typeface_with_fallback(family: &str, style: FontStyle) -> Result<Typeface> {
    let fm = font_mgr();
    if let Some(t) = fm.match_family_style(family, style) {
        return Ok(t);
    }
    if let Some(t) = fm.match_family_style("Helvetica", style) {
        return Ok(t);
    }
    if let Some(t) = fm.match_family_style("Arial", style) {
        return Ok(t);
    }
    if let Some(t) = fm.legacy_make_typeface(None, style) {
        return Ok(t);
    }
    Err(RustmotionError::FontNotFound)
}

/// Resolve the system emoji typeface. Cached per thread.
pub fn emoji_typeface() -> Option<Typeface> {
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
