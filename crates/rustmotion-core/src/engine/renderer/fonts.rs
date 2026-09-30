use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use skia_safe::{Font, FontHinting, FontMgr, FontStyle, Typeface};

use crate::error::{Result, RustmotionError};
use crate::schema::FontEntry;

use super::google_fonts::{font_cache_dir, resolve_google_font};

thread_local! {
    static THREAD_FONT_MGR: FontMgr = FontMgr::default();
    static CUSTOM_TYPEFACES: RefCell<GenerationCache<(String, i32, bool), Typeface>> =
        RefCell::new(GenerationCache::new());
    static FALLBACK_CACHE: RefCell<GenerationCache<(String, i32, bool, u32), Option<Typeface>>> =
        RefCell::new(GenerationCache::new());
}

static FONT_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn invalidate_font_caches() {
    FONT_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Release);
}

struct GenerationCache<K, V> {
    generation: u64,
    map: HashMap<K, V>,
}

impl<K: std::hash::Hash + Eq, V> GenerationCache<K, V> {
    fn new() -> Self {
        Self {
            generation: FONT_GENERATION.load(std::sync::atomic::Ordering::Acquire),
            map: HashMap::new(),
        }
    }

    fn current(&mut self) -> &mut HashMap<K, V> {
        let generation = FONT_GENERATION.load(std::sync::atomic::Ordering::Acquire);
        if self.generation != generation {
            self.map.clear();
            self.generation = generation;
        }
        &mut self.map
    }
}

pub fn font_mgr() -> FontMgr {
    THREAD_FONT_MGR.with(|mgr| mgr.clone())
}

#[derive(Clone)]
struct CustomFontVariant {
    data: Vec<u8>,
    weight: i32,
    italic: bool,
    generation: u64,
}

fn custom_font_registry() -> &'static Mutex<HashMap<String, Vec<CustomFontVariant>>> {
    static REG: OnceLock<Mutex<HashMap<String, Vec<CustomFontVariant>>>> = OnceLock::new();
    REG.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn register_custom_font_variant(family: &str, data: Vec<u8>, weight: i32, italic: bool) {
    let mut reg = custom_font_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let generation = FONT_GENERATION.load(std::sync::atomic::Ordering::Acquire);
    let variants = reg.entry(family.to_string()).or_default();
    let replacement = CustomFontVariant {
        data,
        weight,
        italic,
        generation,
    };
    match variants
        .iter_mut()
        .find(|v| v.weight == weight && v.italic == italic)
    {
        Some(existing) if existing.generation == generation => {}
        Some(existing) => *existing = replacement,
        None => variants.push(replacement),
    }
}

#[cfg(test)]
fn custom_font_bytes(family: &str, weight: i32, italic: bool) -> Option<Vec<u8>> {
    let reg = custom_font_registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let variants = reg.get(family)?;
    closest_variant(variants, weight, italic).map(|v| v.data.clone())
}

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

fn custom_typeface(family: &str, style: FontStyle) -> Option<Typeface> {
    let weight = *style.weight();
    let italic = style.slant() != skia_safe::font_style::Slant::Upright;
    let cache_key = (family.to_string(), weight, italic);
    CUSTOM_TYPEFACES.with(|cache| {
        if let Some(tf) = cache.borrow_mut().current().get(&cache_key) {
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
        cache.borrow_mut().current().insert(cache_key, tf.clone());
        Some(tf)
    })
}

#[cfg(test)]
fn what_this_thread_sees_in_its_fallback_cache() -> usize {
    FALLBACK_CACHE.with(|cache| cache.borrow_mut().current().len())
}

pub fn resolve_custom_typeface(family: &str, style: FontStyle) -> Option<Typeface> {
    custom_typeface(family, style)
}

pub fn resolve_font_entry(entry: &FontEntry) -> Result<Vec<std::path::PathBuf>> {
    match (&entry.source, &entry.path) {
        (Some(_), Some(_)) => Err(RustmotionError::FontSourceAndPathConflict {
            family: entry.family.clone(),
        }),
        (Some(source), None) if source == "google" => {
            let weights = entry
                .weights
                .as_deref()
                .filter(|w| !w.is_empty())
                .unwrap_or(&[400]);
            let cache_dir = font_cache_dir();
            resolve_google_font(&entry.family, weights, &cache_dir)
        }
        (Some(other), None) => Err(RustmotionError::Generic(format!(
            "FontEntry for '{}': unknown source value '{}' (only \"google\" is supported)",
            entry.family, other
        ))),
        (None, Some(path)) => Ok(vec![std::path::PathBuf::from(path)]),
        (None, None) => Err(RustmotionError::FontMissingPath {
            family: entry.family.clone(),
        }),
    }
}

pub fn load_custom_fonts(fonts: &[FontEntry]) {
    if fonts.is_empty() {
        return;
    }
    invalidate_font_caches();
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

pub fn typeface_with_fallback(family: &str, style: FontStyle) -> Result<Typeface> {
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

pub fn subpixel_font(typeface: impl Into<Typeface>, size: impl Into<Option<f32>>) -> Font {
    let mut font = Font::from_typeface(typeface, size);
    font.set_subpixel(true);
    font.set_baseline_snap(false);
    font.set_hinting(FontHinting::None);
    font
}

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

pub fn fallback_typeface_for_char(
    primary_family: &str,
    style: FontStyle,
    c: char,
) -> Option<Typeface> {
    let weight = *style.weight();
    let italic = style.slant() != skia_safe::font_style::Slant::Upright;
    let key = (primary_family.to_string(), weight, italic, c as u32);
    FALLBACK_CACHE.with(|cache| {
        if let Some(hit) = cache.borrow_mut().current().get(&key) {
            return hit.clone();
        }
        let resolved =
            font_mgr().match_family_style_character(primary_family, style, &[], c as i32);
        cache.borrow_mut().current().insert(key, resolved.clone());
        resolved
    })
}

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

    #[test]
    fn registered_custom_font_resolves_over_system_fallback() {
        let path = format!(
            "{}/.cache/rustmotion/fonts/anton-400.ttf",
            std::env::var("HOME").unwrap_or_default()
        );
        let Ok(bytes) = std::fs::read(&path) else {
            return;
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
            return;
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
        let cache_dir = std::env::temp_dir()
            .join("rustmotion-test-fonts")
            .join("fonts-rs-google-cache");
        std::fs::create_dir_all(&cache_dir).unwrap();
        std::fs::write(cache_dir.join("inter-400.ttf"), b"fake ttf").unwrap();

        let entry = google_entry("Inter", None);
        let paths = crate::engine::renderer::google_fonts::resolve_google_font(
            &entry.family,
            &[400],
            &cache_dir,
        )
        .unwrap();
        assert_eq!(paths.len(), 1);
    }
}

#[cfg(test)]
mod cache_invalidation_reaches_every_thread_tests {
    use super::*;
    use rayon::prelude::*;
    use std::sync::atomic::Ordering;

    static GENERATION_IS_SHARED: Mutex<()> = Mutex::new(());

    fn hold_the_generation_still() -> std::sync::MutexGuard<'static, ()> {
        GENERATION_IS_SHARED
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    fn a_face(marker: u8) -> Vec<u8> {
        let mut data = vec![
            0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x10, 0x00, 0x03, 0x00, 0x04,
        ];
        data.push(marker);
        data
    }

    fn fill_this_threads_fallback_cache() -> usize {
        for c in ['a', 'e', 'i', 'o', 'u', 'y', 'z'] {
            fallback_typeface_for_char("rm-test-no-such-family", FontStyle::normal(), c);
        }
        what_this_thread_sees_in_its_fallback_cache()
    }

    #[test]
    fn a_rayon_worker_drops_its_own_copy_when_the_generation_moves() {
        let _serialised = hold_the_generation_still();
        let before: Vec<usize> = (0..256)
            .into_par_iter()
            .map(|_| fill_this_threads_fallback_cache())
            .collect();
        assert!(
            before.iter().any(|&len| len > 0),
            "test setup: the workers must actually have filled a thread-local cache, or the \
             assertion below proves nothing"
        );

        invalidate_font_caches();

        let after: Vec<usize> = (0..256)
            .into_par_iter()
            .map(|_| what_this_thread_sees_in_its_fallback_cache())
            .collect();
        assert!(
            after.iter().all(|&len| len == 0),
            "the clearing is lazy, so this is what each worker sees on its next lookup — and \
             that is the guarantee: clearing on the calling thread alone leaves the threads \
             that actually render frames serving the stale face forever: {after:?}"
        );
    }

    #[test]
    fn a_generation_cache_holds_its_entries_until_the_generation_moves() {
        let _serialised = hold_the_generation_still();
        let mut cache: GenerationCache<u32, u32> = GenerationCache::new();
        cache.current().insert(1, 10);
        cache.current().insert(2, 20);
        assert_eq!(
            cache.current().len(),
            2,
            "nothing moved, nothing is dropped"
        );
        invalidate_font_caches();
        assert_eq!(
            cache.current().len(),
            0,
            "the next access after a bump is what clears it — a thread that is busy rendering \
             does not have to be interrupted for its cache to go stale"
        );
    }

    #[test]
    fn a_second_file_in_the_same_load_does_not_clobber_the_first() {
        let _serialised = hold_the_generation_still();
        let family = "rm-test-same-load";
        invalidate_font_caches();
        register_custom_font_variant(family, a_face(1), 400, false);
        register_custom_font_variant(family, a_face(2), 400, false);
        assert_eq!(
            custom_font_bytes(family, 400, false),
            Some(a_face(1)),
            "one FontEntry can resolve to several files; two of them landing on the same weight \
             and slant must not fight, the first still wins"
        );
    }

    #[test]
    fn the_next_load_supersedes_the_variant_the_previous_one_registered() {
        let _serialised = hold_the_generation_still();
        let family = "rm-test-reload";
        invalidate_font_caches();
        register_custom_font_variant(family, a_face(1), 400, false);

        invalidate_font_caches();
        register_custom_font_variant(family, a_face(2), 400, false);
        assert_eq!(
            custom_font_bytes(family, 400, false),
            Some(a_face(2)),
            "editing the file under --watch is a new load, and its bytes must win — being \
             skipped is what kept serving the previous face for the rest of the process"
        );
    }

    #[test]
    fn loading_an_empty_font_list_does_not_throw_away_every_threads_cache() {
        let _serialised = hold_the_generation_still();
        let generation = FONT_GENERATION.load(Ordering::Acquire);
        load_custom_fonts(&[]);
        assert_eq!(
            FONT_GENERATION.load(Ordering::Acquire),
            generation,
            "a scenario with no custom fonts reloads constantly under --watch; bumping there \
             would clear every worker's font cache for nothing"
        );
    }
}
