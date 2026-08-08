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
    // keyed by (family, weight, italic) — the exact variant `custom_typeface`
    // picked — so each render thread builds each custom face at most once.
    static CUSTOM_TYPEFACES: RefCell<HashMap<(String, i32, bool), Typeface>> =
        RefCell::new(HashMap::new());
}

pub fn font_mgr() -> FontMgr {
    THREAD_FONT_MGR.with(|mgr| mgr.clone())
}

/// One registered custom-font file: its raw bytes plus the `(weight,
/// italic)` style Skia parsed out of the file itself when it was
/// registered — the ground truth for what that file actually renders as,
/// independent of which nominal weight the caller happened to request it
/// under.
#[derive(Clone)]
struct CustomFontVariant {
    data: Vec<u8>,
    weight: i32,
    italic: bool,
}

/// Global registry of custom/Google-font bytes, keyed by family name. Filled
/// once by [`load_custom_fonts`] on the main thread; read by every render
/// thread through [`custom_typeface`]. Each family holds every distinct
/// `(weight, italic)` variant registered for it — e.g. a Google Fonts
/// declaration with `weights: [400, 700]` registers two variants — so
/// [`custom_typeface`] can pick whichever is the closest match to what a
/// paint call asks for, instead of always returning the first file that
/// happened to register (the previous behaviour: every variant after the
/// first was invisible, and every weight/style request resolved to
/// whichever one file won the race).
fn custom_font_registry() -> &'static Mutex<HashMap<String, Vec<CustomFontVariant>>> {
    static REG: OnceLock<Mutex<HashMap<String, Vec<CustomFontVariant>>>> = OnceLock::new();
    REG.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register a custom font's bytes under `family`, tagged with the `(weight,
/// italic)` style Skia reports for the parsed file. A no-op if that exact
/// `(family, weight, italic)` combination is already registered.
pub fn register_custom_font_variant(family: &str, data: Vec<u8>, weight: i32, italic: bool) {
    let mut reg = custom_font_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let variants = reg.entry(family.to_string()).or_default();
    if !variants
        .iter()
        .any(|v| v.weight == weight && v.italic == italic)
    {
        variants.push(CustomFontVariant {
            data,
            weight,
            italic,
        });
    }
}

/// The raw bytes registered for `family`'s closest `(weight, italic)` match,
/// if any variant is registered under that family (test/introspection
/// helper).
#[cfg(test)]
fn custom_font_bytes(family: &str, weight: i32, italic: bool) -> Option<Vec<u8>> {
    let reg = custom_font_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let variants = reg.get(family)?;
    closest_variant(variants, weight, italic).map(|v| v.data.clone())
}

/// Pick the registered variant closest to `(weight, italic)`: exact
/// italic-ness match preferred, then the smallest weight distance — the
/// same nearest-match spirit as CSS font matching (`font-weight`/
/// `font-style` never fail to resolve to *something*, they resolve to the
/// closest available face).
fn closest_variant(
    variants: &[CustomFontVariant],
    weight: i32,
    italic: bool,
) -> Option<&CustomFontVariant> {
    variants.iter().min_by_key(|v| {
        let italic_penalty = if v.italic == italic { 0 } else { 1_000_000 };
        italic_penalty + (v.weight - weight).abs()
    })
}

/// Resolve a registered custom font to a Typeface for the requested `style`,
/// building it from the global bytes on first use per thread and caching it
/// thereafter. `None` when no custom font is registered under `family`.
fn custom_typeface(family: &str, style: FontStyle) -> Option<Typeface> {
    let weight = *style.weight();
    let italic = style.slant() != skia_safe::font_style::Slant::Upright;
    let cache_key = (family.to_string(), weight, italic);
    CUSTOM_TYPEFACES.with(|cache| {
        if let Some(tf) = cache.borrow().get(&cache_key) {
            return Some(tf.clone());
        }
        let data = {
            let reg = custom_font_registry()
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let variants = reg.get(family)?;
            closest_variant(variants, weight, italic)?.data.clone()
        };
        let sk_data = skia_safe::Data::new_copy(&data);
        let tf = font_mgr().new_from_data(&sk_data, None)?;
        cache.borrow_mut().insert(cache_key, tf.clone());
        Some(tf)
    })
}

/// Look up only the custom/Google-font registry for `family` at the
/// requested `style`, without falling through to any system font. Exposed
/// for callers (e.g. `codeblock`/`terminal`'s monospace font resolver) that
/// need to check "did the scenario declare a custom font for this family"
/// *before* trying their own family-specific system fallback chain — unlike
/// [`typeface_with_fallback`], which interleaves a single system-family
/// lookup between the custom check and its own generic Helvetica/Arial
/// catch-all, an order that doesn't suit every caller (see issue: codeblock/
/// terminal's hardcoded monospace fallback list was never reached because
/// `typeface_with_fallback`'s own system lookup already matched a decoy
/// system family, e.g. "JetBrains Mono").
pub fn resolve_custom_typeface(family: &str, style: FontStyle) -> Option<Typeface> {
    custom_typeface(family, style)
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
            let Some(tf) = font_mgr.new_from_data(&sk_data, None) else {
                eprintln!(
                    "Warning: failed to register custom font '{}' from '{}'",
                    family,
                    path.display()
                );
                return;
            };
            // Skia's default FontMgr can build a Typeface from `new_from_data`
            // but never exposes it to `match_family_style` (name lookup only
            // sees installed system fonts). So keep the raw bytes in a global
            // registry; `typeface_with_fallback` builds and caches a Typeface
            // from them per thread, ahead of the system match. Tag the
            // variant with the (weight, italic) Skia parsed out of the file
            // itself — the ground truth for what it actually renders as —
            // so a family with several registered weights (e.g. Google
            // Fonts `weights: [400, 700]`) exposes every one of them instead
            // of only whichever file happened to register first.
            let parsed_style = tf.font_style();
            let weight = *parsed_style.weight();
            let italic = parsed_style.slant() != skia_safe::font_style::Slant::Upright;
            register_custom_font_variant(family, data, weight, italic);
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
    // registry first — matched against the requested `style` so a family
    // registered with several weights picks the right one instead of
    // whichever file happened to register first (#6).
    if let Some(t) = custom_typeface(family, style) {
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

/// Resolve a system fallback typeface that actually contains a glyph for
/// `c`, for when `primary_family`'s own face doesn't cover it (audit #3:
/// CJK/Arabic/Devanagari/other scripts rendered as `.notdef` tofu when only
/// a Latin `font-family` was requested, because neither measurement nor
/// painting ever looked past the single requested typeface). This is
/// Skia's font-fallback-by-character API — the same mechanism a browser
/// uses to substitute, say, a CJK font for Chinese text embedded in an
/// otherwise-Latin paragraph, instead of leaving `.notdef` tofu. Memoized
/// per thread (keyed on the inputs that actually affect the OS's fallback
/// decision) since callers may probe this once per uncovered code point
/// during run segmentation. Returns `None` if no installed font covers `c`
/// either — the caller falls back to the originally requested (tofu-
/// producing) font, exactly the pre-fix behaviour, not worse.
pub fn fallback_typeface_for_char(
    primary_family: &str,
    style: FontStyle,
    c: char,
) -> Option<Typeface> {
    thread_local! {
        static FALLBACK_CACHE: RefCell<HashMap<(String, i32, bool, u32), Option<Typeface>>> =
            RefCell::new(HashMap::new());
    }
    let weight = *style.weight();
    let italic = style.slant() != skia_safe::font_style::Slant::Upright;
    let key = (primary_family.to_string(), weight, italic, c as u32);
    FALLBACK_CACHE.with(|cache| {
        if let Some(hit) = cache.borrow().get(&key) {
            return hit.clone();
        }
        let resolved =
            font_mgr().match_family_style_character(primary_family, style, &[], c as i32);
        cache.borrow_mut().insert(key, resolved.clone());
        resolved
    })
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
    fn custom_font_registry_stores_distinct_weights_and_serves_bytes() {
        register_custom_font_variant("RmProbeRegistryFamily", vec![1, 2, 3], 400, false);
        // A *different* (weight, italic) is a genuinely new variant — not a
        // clobber of the first (the old `family`-only-keyed `or_insert`
        // registry made every registration after the first invisible; #6).
        register_custom_font_variant("RmProbeRegistryFamily", vec![9, 9], 700, false);
        assert_eq!(
            custom_font_bytes("RmProbeRegistryFamily", 400, false),
            Some(vec![1, 2, 3])
        );
        assert_eq!(
            custom_font_bytes("RmProbeRegistryFamily", 700, false),
            Some(vec![9, 9])
        );
        assert!(custom_font_bytes("RmProbeUnregistered", 400, false).is_none());
    }

    #[test]
    fn registering_the_same_weight_twice_keeps_the_first() {
        register_custom_font_variant("RmProbeDupeFamily", vec![1, 2, 3], 400, false);
        register_custom_font_variant("RmProbeDupeFamily", vec![9, 9], 400, false);
        assert_eq!(
            custom_font_bytes("RmProbeDupeFamily", 400, false),
            Some(vec![1, 2, 3]),
            "re-registering the same (weight, italic) must not clobber the first file"
        );
    }

    #[test]
    fn custom_typeface_lookup_picks_the_closest_registered_weight() {
        // Pure selection-logic reproduction of #6's fix mechanism,
        // independent of any font actually installed on the host: three
        // variants registered under one family; a lookup for an
        // intermediate weight must pick the *closest* one, not always the
        // first registered — the defect the audit measured (bold and
        // normal always resolving to the same file).
        register_custom_font_variant("RmProbeClosestFamily", vec![1], 400, false);
        register_custom_font_variant("RmProbeClosestFamily", vec![2], 700, false);
        register_custom_font_variant("RmProbeClosestFamily", vec![3], 900, false);

        let reg = custom_font_registry()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let variants = reg.get("RmProbeClosestFamily").expect("registered above");
        assert_eq!(
            closest_variant(variants, 650, false).unwrap().weight,
            700,
            "650 should resolve to the nearest registered weight, 700"
        );
        assert_eq!(
            closest_variant(variants, 100, false).unwrap().weight,
            400,
            "100 should resolve to the nearest registered weight, 400"
        );
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
        let fm = font_mgr();
        let parsed = fm
            .new_from_data(&skia_safe::Data::new_copy(&bytes), None)
            .expect("cached TTF must parse");
        let style = parsed.font_style();
        register_custom_font_variant(
            "Anton",
            bytes,
            *style.weight(),
            style.slant() != skia_safe::font_style::Slant::Upright,
        );
        let tf = typeface_with_fallback("Anton", FontStyle::normal()).unwrap();
        assert_eq!(
            tf.family_name(),
            "Anton",
            "must resolve the custom face, not a system fallback"
        );
    }

    /// End-to-end reproduction of #6: a family registered with two distinct
    /// weights (mirrors `fonts: [{"family":"Inter","source":"google",
    /// "weights":[400,700]}]`) must resolve *different* typefaces for
    /// `font-weight: normal` vs `font-weight: bold`. Before the fix,
    /// `custom_typeface` ignored `style` entirely and `register_custom_
    /// font_bytes` kept only the first-registered file, so the audit's two
    /// rendered PNGs (bold vs normal) came out byte-for-byte identical.
    /// Skips on a cold font cache (no network access in CI) — the render QA
    /// in `examples/` is the visual counterpart.
    #[test]
    fn family_with_two_registered_weights_resolves_distinct_typefaces() {
        let cache_dir = format!(
            "{}/.cache/rustmotion/fonts",
            std::env::var("HOME").unwrap_or_default()
        );
        let (Ok(normal_bytes), Ok(bold_bytes)) = (
            std::fs::read(format!("{cache_dir}/inter-400.ttf")),
            std::fs::read(format!("{cache_dir}/inter-700.ttf")),
        ) else {
            return; // cold font cache → skip (render QA covers it)
        };

        let fm = font_mgr();
        let normal_parsed = fm
            .new_from_data(&skia_safe::Data::new_copy(&normal_bytes), None)
            .expect("cached TTF must parse");
        let bold_parsed = fm
            .new_from_data(&skia_safe::Data::new_copy(&bold_bytes), None)
            .expect("cached TTF must parse");
        let normal_weight = *normal_parsed.font_style().weight();
        let bold_weight = *bold_parsed.font_style().weight();

        register_custom_font_variant("RmProbeInterFamily", normal_bytes, normal_weight, false);
        register_custom_font_variant("RmProbeInterFamily", bold_bytes, bold_weight, false);

        let resolved_normal =
            typeface_with_fallback("RmProbeInterFamily", FontStyle::normal()).unwrap();
        let resolved_bold =
            typeface_with_fallback("RmProbeInterFamily", FontStyle::bold()).unwrap();

        assert_ne!(
            *resolved_normal.font_style().weight(),
            *resolved_bold.font_style().weight(),
            "requesting normal vs bold on the same custom family must resolve different weights \
             (both used to resolve to whichever file registered first)"
        );
        assert_eq!(*resolved_bold.font_style().weight(), bold_weight);
        assert_eq!(*resolved_normal.font_style().weight(), normal_weight);
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
