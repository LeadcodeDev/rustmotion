use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use skia_safe::{FontMgr, FontStyle, Typeface};

use crate::error::{Result, RustmotionError};
use crate::schema::FontEntry;

use super::google_fonts::{font_cache_dir, resolve_google_font};

// Thread-local FontMgr instance, created once per thread and reused
thread_local! {
    static THREAD_FONT_MGR: FontMgr = FontMgr::default();
    // Per-thread cache of Typefaces built from the global custom-font bytes,
    // so each render thread builds each custom face at most once.
    static CUSTOM_TYPEFACES: RefCell<HashMap<String, Typeface>> = RefCell::new(HashMap::new());
}

pub fn font_mgr() -> FontMgr {
    THREAD_FONT_MGR.with(|mgr| mgr.clone())
}

/// Global registry of custom/Google-font bytes, keyed by family name. Filled
/// once by [`load_custom_fonts`] on the main thread; read by every render
/// thread through [`custom_typeface`]. A family maps to the first file
/// registered for it (one weight per custom family via this path — sufficient
/// for accent display faces; multi-weight custom families are future work).
fn custom_font_registry() -> &'static Mutex<HashMap<String, Vec<u8>>> {
    static REG: OnceLock<Mutex<HashMap<String, Vec<u8>>>> = OnceLock::new();
    REG.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Store a custom font's bytes under `family` (first registration wins).
pub fn register_custom_font_bytes(family: &str, data: Vec<u8>) {
    let mut reg = custom_font_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    reg.entry(family.to_string()).or_insert(data);
}

/// The raw bytes registered for `family`, if any (test/introspection helper).
pub fn custom_font_bytes(family: &str) -> Option<Vec<u8>> {
    custom_font_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(family)
        .cloned()
}

/// Resolve a registered custom font to a Typeface, building it from the global
/// bytes on first use per thread and caching it thereafter. `None` when no
/// custom font is registered under `family`.
fn custom_typeface(family: &str) -> Option<Typeface> {
    CUSTOM_TYPEFACES.with(|cache| {
        if let Some(tf) = cache.borrow().get(family) {
            return Some(tf.clone());
        }
        let data = custom_font_bytes(family)?;
        let sk_data = skia_safe::Data::new_copy(&data);
        let tf = font_mgr().new_from_data(&sk_data, None)?;
        cache.borrow_mut().insert(family.to_string(), tf.clone());
        Some(tf)
    })
}

/// Validate a `FontEntry` and resolve it to a list of TTF file paths.
///
/// - Local entry (`path` set, `source` absent): returns `[path]` as-is.
/// - Google Fonts entry (`source = "google"`, `path` absent): downloads
///   (or reads from cache) and returns one path per requested weight.
/// - Conflict (`source` and `path` both set): returns an error.
/// - Neither (`path` absent and `source` absent): returns an error.
pub fn resolve_font_entry(entry: &FontEntry) -> Result<Vec<std::path::PathBuf>> {
    match (&entry.source, &entry.path) {
        // Conflict: both path and source set.
        (Some(_), Some(_)) => Err(RustmotionError::FontSourceAndPathConflict {
            family: entry.family.clone(),
        }),
        // Google Fonts.
        (Some(source), None) if source == "google" => {
            let weights = entry
                .weights
                .as_deref()
                .filter(|w| !w.is_empty())
                .unwrap_or(&[400]);
            let cache_dir = font_cache_dir();
            resolve_google_font(&entry.family, weights, &cache_dir)
        }
        // Unknown source value — treat as a user error.
        (Some(other), None) => Err(RustmotionError::Generic(format!(
            "FontEntry for '{}': unknown source value '{}' (only \"google\" is supported)",
            entry.family, other
        ))),
        // Local file.
        (None, Some(path)) => Ok(vec![std::path::PathBuf::from(path)]),
        // Neither path nor source.
        (None, None) => Err(RustmotionError::FontMissingPath {
            family: entry.family.clone(),
        }),
    }
}

/// Load custom fonts from FontEntry definitions. Emits a single warning per
/// missing or unreadable file so the user notices broken paths up-front.
pub fn load_custom_fonts(fonts: &[FontEntry]) {
    let font_mgr = font_mgr();
    for entry in fonts {
        match resolve_font_entry(entry) {
            Err(e) => {
                eprintln!("Warning: {e}");
            }
            Ok(paths) => {
                for path in paths {
                    register_font_file(&font_mgr, &entry.family, &path);
                }
            }
        }
    }
}

/// Register a single TTF/OTF file into the given FontMgr.
fn register_font_file(font_mgr: &FontMgr, family: &str, path: &std::path::Path) {
    if !path.exists() {
        eprintln!(
            "Warning: custom font '{}' not found at '{}' — falling back to system fonts",
            family,
            path.display()
        );
        return;
    }
    match std::fs::read(path) {
        Ok(data) => {
            let sk_data = skia_safe::Data::new_copy(&data);
            if font_mgr.new_from_data(&sk_data, None).is_none() {
                eprintln!(
                    "Warning: failed to register custom font '{}' from '{}'",
                    family,
                    path.display()
                );
                return;
            }
            // Skia's default FontMgr can build a Typeface from `new_from_data`
            // but never exposes it to `match_family_style` (name lookup only
            // sees installed system fonts). So keep the raw bytes in a global
            // registry; `typeface_with_fallback` builds and caches a Typeface
            // from them per thread, ahead of the system match.
            register_custom_font_bytes(family, data);
        }
        Err(e) => {
            eprintln!(
                "Warning: failed to read custom font '{}' from '{}': {}",
                family,
                path.display(),
                e
            );
        }
    }
}

/// Resolve a typeface for `family` falling back through Helvetica → Arial →
/// the OS default. Returns `RustmotionError::FontNotFound` only if the host
/// system has no usable font at all (essentially unreachable on every
/// supported platform). Use this instead of `.expect("FontNotFound")` so we
/// never panic from a `paint` callback.
pub fn typeface_with_fallback(family: &str, style: FontStyle) -> Result<Typeface> {
    // Custom/Google fonts declared in the scenario win over system fonts:
    // they are not visible to `match_family_style`, so resolve them from the
    // registry first.
    if let Some(t) = custom_typeface(family) {
        return Ok(t);
    }
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

// ─── Unit tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn local_entry(path: &str) -> FontEntry {
        FontEntry {
            path: Some(path.to_string()),
            family: "TestFamily".to_string(),
            source: None,
            weights: None,
        }
    }

    fn google_entry(family: &str, weights: Option<Vec<u16>>) -> FontEntry {
        FontEntry {
            path: None,
            family: family.to_string(),
            source: Some("google".to_string()),
            weights,
        }
    }

    fn neither_entry() -> FontEntry {
        FontEntry {
            path: None,
            family: "Broken".to_string(),
            source: None,
            weights: None,
        }
    }

    fn conflict_entry() -> FontEntry {
        FontEntry {
            path: Some("fonts/Inter.ttf".to_string()),
            family: "Inter".to_string(),
            source: Some("google".to_string()),
            weights: None,
        }
    }

    #[test]
    fn local_entry_resolves_to_path() {
        let entry = local_entry("fonts/Inter.ttf");
        let paths = resolve_font_entry(&entry).unwrap();
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].to_str().unwrap(), "fonts/Inter.ttf");
    }

    #[test]
    fn custom_font_registry_stores_first_and_serves_bytes() {
        register_custom_font_bytes("RmProbeRegistryFamily", vec![1, 2, 3]);
        // First registration wins (a later weight must not clobber it).
        register_custom_font_bytes("RmProbeRegistryFamily", vec![9, 9]);
        assert_eq!(
            custom_font_bytes("RmProbeRegistryFamily"),
            Some(vec![1, 2, 3])
        );
        assert!(custom_font_bytes("RmProbeUnregistered").is_none());
    }

    /// The bug this fix targets: a registered custom family must resolve to the
    /// custom typeface, not the Helvetica/Arial fallback. Uses the cached Anton
    /// TTF when present (Google-font path); skips on a cold cache so CI without
    /// network still passes — the render QA is the visual counterpart.
    #[test]
    fn registered_custom_font_resolves_over_system_fallback() {
        let path = format!(
            "{}/.cache/rustmotion/fonts/anton-400.ttf",
            std::env::var("HOME").unwrap_or_default()
        );
        let Ok(bytes) = std::fs::read(&path) else {
            return; // cold font cache → skip (render QA covers it)
        };
        register_custom_font_bytes("Anton", bytes);
        let tf = typeface_with_fallback("Anton", FontStyle::normal()).unwrap();
        assert_eq!(
            tf.family_name(),
            "Anton",
            "must resolve the custom face, not a system fallback"
        );
    }

    #[test]
    fn neither_path_nor_source_is_error() {
        let entry = neither_entry();
        let err = resolve_font_entry(&entry).unwrap_err();
        assert!(
            matches!(err, RustmotionError::FontMissingPath { .. }),
            "expected FontMissingPath, got: {err}"
        );
    }

    #[test]
    fn path_and_source_conflict_is_error() {
        let entry = conflict_entry();
        let err = resolve_font_entry(&entry).unwrap_err();
        assert!(
            matches!(err, RustmotionError::FontSourceAndPathConflict { .. }),
            "expected FontSourceAndPathConflict, got: {err}"
        );
    }

    #[test]
    fn google_entry_with_cached_file_resolves() {
        // Build a pre-warmed cache dir and inject it via resolve_google_font directly.
        let cache_dir = std::env::temp_dir()
            .join("rustmotion-test-fonts")
            .join("fonts-rs-google-cache");
        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::write(cache_dir.join("inter-400.ttf"), b"fake ttf").unwrap();

        let entry = google_entry("Inter", None);
        // We call resolve_google_font directly with the injected dir to avoid
        // any real network in unit tests.
        let paths = crate::engine::renderer::google_fonts::resolve_google_font(
            &entry.family,
            &[400],
            &cache_dir,
        )
        .unwrap();
        assert_eq!(paths.len(), 1);
    }
}
