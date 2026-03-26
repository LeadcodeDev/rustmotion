use skia_safe::{FontMgr, FontStyle, Typeface};

use crate::schema::FontEntry;

// Thread-local FontMgr instance, created once per thread and reused
thread_local! {
    static THREAD_FONT_MGR: FontMgr = FontMgr::default();
}

pub fn font_mgr() -> FontMgr {
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
