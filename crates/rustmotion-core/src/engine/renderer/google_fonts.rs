//! Google Fonts download and disk-cache helpers.
//!
//! ## Cache layout
//!
//! ```text
//! ~/.cache/rustmotion/fonts/
//!     inter-400.ttf
//!     inter-700.ttf
//!     jetbrains-mono-400.ttf
//! ```
//!
//! File names are derived from the family name by lower-casing and replacing
//! spaces with hyphens: `"JetBrains Mono"` → `"jetbrains-mono"`.
//!
//! ## Network protocol
//!
//! 1. `GET https://fonts.googleapis.com/css2?family=<Family+Name>:wght@<w1>;<w2>`
//!    with `ureq`'s default (non-browser) User-Agent.  Google returns a CSS
//!    stylesheet containing `url(…)` entries pointing at `.ttf` files.
//! 2. We extract every `url(…)` from the CSS and download each TTF.
//!
//! No new crate dependencies are introduced; `ureq` (v3) is already in
//! `rustmotion-core`.

use std::path::{Path, PathBuf};

use crate::error::{Result, RustmotionError};

// ─── Cache directory ────────────────────────────────────────────────────────

/// Returns the platform-appropriate font cache directory.
///
/// - macOS/Linux: `$HOME/.cache/rustmotion/fonts`
/// - Windows:     `%LOCALAPPDATA%\rustmotion\fonts`
///
/// The directory is created if it does not exist.
pub fn font_cache_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    #[cfg(not(target_os = "windows"))]
    let base = std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join(".cache"))
        .unwrap_or_else(|| PathBuf::from(".cache"));

    base.join("rustmotion").join("fonts")
}

// ─── Public entry point ─────────────────────────────────────────────────────

/// Ensure TTFs for `family` + `weights` are present on disk (download if needed)
/// and return their paths.
///
/// `cache_dir` is accepted as a parameter so tests can inject a temporary
/// directory without touching `$HOME`.
pub fn resolve_google_font(
    family: &str,
    weights: &[u16],
    cache_dir: &Path,
) -> Result<Vec<PathBuf>> {
    std::fs::create_dir_all(cache_dir).map_err(RustmotionError::Io)?;

    let slug = family_slug(family);
    let mut paths = Vec::with_capacity(weights.len());
    let mut missing_weights: Vec<u16> = Vec::new();

    // Check which weights are already cached.
    for &weight in weights {
        let dest = cache_dir.join(format!("{slug}-{weight}.ttf"));
        if dest.exists() {
            paths.push(dest);
        } else {
            missing_weights.push(weight);
        }
    }

    if missing_weights.is_empty() {
        return Ok(paths);
    }

    // Fetch the CSS2 stylesheet from Google Fonts.
    let url = build_css2_url(family, &missing_weights);
    let css = fetch_css2(&url, family)?;

    // Parse the TTF URLs out of the CSS.
    let ttf_urls = parse_ttf_urls(&css);
    if ttf_urls.is_empty() {
        return Err(RustmotionError::GoogleFontsNoUrls {
            family: family.to_string(),
        });
    }

    // Download each TTF. We map each URL to a weight in order because
    // Google Fonts returns one face per weight in the order requested.
    for (i, &weight) in missing_weights.iter().enumerate() {
        let ttf_url = ttf_urls
            .get(i)
            .unwrap_or_else(|| &ttf_urls[ttf_urls.len() - 1]);
        let dest = cache_dir.join(format!("{slug}-{weight}.ttf"));
        fetch_ttf(ttf_url, &dest, family, weight)?;
        paths.push(dest);
    }

    Ok(paths)
}

// ─── URL construction ────────────────────────────────────────────────────────

/// Build the Google Fonts CSS2 API URL.
///
/// Spaces in the family name are replaced with `+`; weights are joined with
/// `;` as per the CSS2 API spec.
///
/// # Examples
///
/// ```
/// # use rustmotion_core::engine::renderer::google_fonts::build_css2_url;
/// let url = build_css2_url("JetBrains Mono", &[400, 700]);
/// assert_eq!(url, "https://fonts.googleapis.com/css2?family=JetBrains+Mono:wght@400;700");
/// ```
pub fn build_css2_url(family: &str, weights: &[u16]) -> String {
    let family_encoded = family.replace(' ', "+");
    let weights_str = weights
        .iter()
        .map(|w| w.to_string())
        .collect::<Vec<_>>()
        .join(";");
    format!("https://fonts.googleapis.com/css2?family={family_encoded}:wght@{weights_str}")
}

// ─── CSS parsing ─────────────────────────────────────────────────────────────

/// Extract all `url(…)` values that end with `.ttf` from a Google Fonts CSS
/// response.  No regex crate is used — we find `url(` markers and extract the
/// inner content.
///
/// # Examples
///
/// ```
/// # use rustmotion_core::engine::renderer::google_fonts::parse_ttf_urls;
/// let css = r#"src: url(https://fonts.gstatic.com/s/inter/v13/foo-400.ttf) format('truetype');"#;
/// let urls = parse_ttf_urls(css);
/// assert_eq!(urls, vec!["https://fonts.gstatic.com/s/inter/v13/foo-400.ttf"]);
/// ```
pub fn parse_ttf_urls(css: &str) -> Vec<String> {
    let mut urls = Vec::new();
    let mut search = css;
    while let Some(start) = search.find("url(") {
        search = &search[start + 4..];
        // Strip optional surrounding quotes.
        let inner = search.trim_start_matches(['\'', '"']);
        let end = inner.find([')', '\'', '"']).unwrap_or(inner.len());
        let url = &inner[..end];
        if url.ends_with(".ttf") || url.ends_with(".ttf)") {
            urls.push(url.trim_end_matches(')').to_string());
        }
        // Advance past the closing paren.
        if let Some(close) = search.find(')') {
            search = &search[close + 1..];
        } else {
            break;
        }
    }
    urls
}

// ─── File-name slug ──────────────────────────────────────────────────────────

/// Convert a font family name to a filesystem-safe slug.
///
/// `"JetBrains Mono"` → `"jetbrains-mono"`
///
/// # Examples
///
/// ```
/// # use rustmotion_core::engine::renderer::google_fonts::family_slug;
/// assert_eq!(family_slug("JetBrains Mono"), "jetbrains-mono");
/// assert_eq!(family_slug("Inter"), "inter");
/// ```
pub fn family_slug(family: &str) -> String {
    family.to_lowercase().replace(' ', "-")
}

// ─── Network helpers ─────────────────────────────────────────────────────────

fn fetch_css2(url: &str, family: &str) -> Result<String> {
    let response = ureq::get(url).call().map_err(|e| {
        let cache_hint = font_cache_dir()
            .join(format!("{}-<weight>.ttf", family_slug(family)))
            .display()
            .to_string();
        RustmotionError::GoogleFontsFetch {
            family: family.to_string(),
            url: url.to_string(),
            reason: e.to_string(),
            cache_hint,
        }
    })?;

    response
        .into_body()
        .read_to_string()
        .map_err(|e| RustmotionError::GoogleFontsFetch {
            family: family.to_string(),
            url: url.to_string(),
            reason: e.to_string(),
            cache_hint: font_cache_dir()
                .join(format!("{}-<weight>.ttf", family_slug(family)))
                .display()
                .to_string(),
        })
}

fn fetch_ttf(url: &str, dest: &Path, family: &str, weight: u16) -> Result<()> {
    let response = ureq::get(url)
        .call()
        .map_err(|e| RustmotionError::GoogleFontsTtfFetch {
            family: family.to_string(),
            weight,
            reason: e.to_string(),
        })?;

    let bytes =
        response
            .into_body()
            .read_to_vec()
            .map_err(|e| RustmotionError::GoogleFontsTtfFetch {
                family: family.to_string(),
                weight,
                reason: e.to_string(),
            })?;

    std::fs::write(dest, &bytes).map_err(RustmotionError::Io)?;
    Ok(())
}

// ─── Unit tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // --- URL construction ---

    #[test]
    fn url_single_weight() {
        let url = build_css2_url("Inter", &[400]);
        assert_eq!(
            url,
            "https://fonts.googleapis.com/css2?family=Inter:wght@400"
        );
    }

    #[test]
    fn url_multi_weight() {
        let url = build_css2_url("Inter", &[400, 700]);
        assert_eq!(
            url,
            "https://fonts.googleapis.com/css2?family=Inter:wght@400;700"
        );
    }

    #[test]
    fn url_spaces_become_plus() {
        let url = build_css2_url("JetBrains Mono", &[400]);
        assert_eq!(
            url,
            "https://fonts.googleapis.com/css2?family=JetBrains+Mono:wght@400"
        );
    }

    // --- CSS parsing ---

    #[test]
    fn parse_single_ttf_url() {
        let css =
            r#"src: url(https://fonts.gstatic.com/s/inter/v13/foo-400.ttf) format('truetype');"#;
        let urls = parse_ttf_urls(css);
        assert_eq!(
            urls,
            vec!["https://fonts.gstatic.com/s/inter/v13/foo-400.ttf"]
        );
    }

    #[test]
    fn parse_multiple_ttf_urls_from_embedded_css() {
        // Simulate a real (stripped) Google Fonts CSS2 response for Inter 400+700.
        let css = r#"
@font-face {
  font-family: 'Inter';
  font-style: normal;
  font-weight: 400;
  src: url(https://fonts.gstatic.com/s/inter/v13/UcC73FwrK3iLTeHuS_nVMrMxCp50SjIa1ZL7W0Q5nw.ttf) format('truetype');
}
@font-face {
  font-family: 'Inter';
  font-style: normal;
  font-weight: 700;
  src: url(https://fonts.gstatic.com/s/inter/v13/UcC73FwrK3iLTeHuS_nVMrMxCp50SjIa2ZL7W0Q5nw.ttf) format('truetype');
}
"#;
        let urls = parse_ttf_urls(css);
        assert_eq!(urls.len(), 2);
        assert!(urls[0].ends_with(".ttf"));
        assert!(urls[1].ends_with(".ttf"));
        assert_ne!(urls[0], urls[1]);
    }

    #[test]
    fn parse_css_with_no_ttf_returns_empty() {
        let css = r#"src: url(https://fonts.gstatic.com/foo.woff2) format('woff2');"#;
        let urls = parse_ttf_urls(css);
        assert!(urls.is_empty());
    }

    // --- File slug ---

    #[test]
    fn slug_lowercase_spaces_to_hyphens() {
        assert_eq!(family_slug("JetBrains Mono"), "jetbrains-mono");
        assert_eq!(family_slug("Inter"), "inter");
        assert_eq!(family_slug("Noto Sans SC"), "noto-sans-sc");
    }

    // --- Cache hit (zero network) ---

    /// Create a unique temp directory for the test (no tempfile crate needed).
    fn make_test_cache(test_name: &str) -> PathBuf {
        let base = std::env::temp_dir()
            .join("rustmotion-test-fonts")
            .join(test_name);
        std::fs::create_dir_all(&base).expect("create test cache dir");
        base
    }

    #[test]
    fn cache_hit_returns_path_without_network() {
        let cache_dir = make_test_cache("cache-hit-single");

        // Pre-position the TTF so no network call is needed.
        let pre_placed = cache_dir.join("inter-400.ttf");
        std::fs::write(&pre_placed, b"fake ttf data").unwrap();

        // resolve_google_font must return the pre-placed path without hitting
        // the network (if it did hit the network and failed, the test would
        // error — no mocking needed).
        let paths = resolve_google_font("Inter", &[400], &cache_dir).unwrap();
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], pre_placed);
    }

    #[test]
    fn cache_hit_multi_weight_all_present() {
        let cache_dir = make_test_cache("cache-hit-multi");

        std::fs::write(cache_dir.join("inter-400.ttf"), b"fake400").unwrap();
        std::fs::write(cache_dir.join("inter-700.ttf"), b"fake700").unwrap();

        let paths = resolve_google_font("Inter", &[400, 700], &cache_dir).unwrap();
        assert_eq!(paths.len(), 2);
    }

    // Live network test — excluded from normal CI runs.
    #[test]
    #[ignore = "requires network access"]
    fn live_fetch_inter_400() {
        let cache_dir = make_test_cache("live-inter-400");
        let paths = resolve_google_font("Inter", &[400], &cache_dir).unwrap();
        assert_eq!(paths.len(), 1);
        let size = std::fs::metadata(&paths[0]).unwrap().len();
        assert!(size > 1000, "expected a real TTF, got {size} bytes");
    }
}
