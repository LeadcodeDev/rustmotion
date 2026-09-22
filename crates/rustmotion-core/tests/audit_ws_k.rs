//! Regression tests for the workstream K (docs, schema, CI) audit findings:
//! documentation drift and inert schema fields.

/// Text immediately following the component count in the README's
/// Architecture section (see `README.md`'s "rustmotion ships N components,
/// each implementing the `Painter` trait" sentence).
const README_COUNT_MARKER: &str = " components, each implementing the `Painter` trait";

/// Text immediately following the component count in
/// `crates/rustmotion-components/Cargo.toml`'s `description` — the string
/// crates.io displays for the published crate.
const CARGO_TOML_COUNT_MARKER: &str = " components)\"";

/// Read the integer that appears immediately before `marker` in `haystack`,
/// skipping trailing whitespace. Panics with the marker text on failure so a
/// reworded sentence names exactly what moved instead of a bare parse error.
fn number_before(haystack: &str, marker: &str, haystack_name: &str) -> u32 {
    let idx = haystack.find(marker).unwrap_or_else(|| {
        panic!(
            "marker {marker:?} not found in {haystack_name} — did the component count sentence \
             move or get reworded? Update this test's marker to match."
        )
    });
    let digits: String = haystack[..idx]
        .chars()
        .rev()
        .skip_while(|c| c.is_whitespace())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let digits: String = digits.chars().rev().collect();
    digits.parse().unwrap_or_else(|_| {
        panic!("no number found immediately before marker {marker:?} in {haystack_name}")
    })
}

/// Count the variants of `pub enum Component` in
/// `crates/rustmotion-components/src/lib.rs`, by counting non-empty,
/// non-attribute lines between its opening `{` and closing `}`. Every
/// variant in that enum is declared on its own line (`Name(Type),`); the
/// only other lines in the block are `#[serde(...)]` attributes.
fn count_component_variants(lib_rs: &str) -> usize {
    let start_marker = "pub enum Component {";
    let start = lib_rs.find(start_marker).unwrap_or_else(|| {
        panic!("{start_marker:?} not found in rustmotion-components/src/lib.rs")
    }) + start_marker.len();
    let rest = &lib_rs[start..];
    let end = rest
        .find("\n}")
        .expect("no closing '}' found for `pub enum Component` block");
    let body = &rest[..end];
    body.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .count()
}

/// README.md and `rustmotion-components/Cargo.toml` (the text
/// crates.io shows for the published crate) both claimed "51 components"
/// while `Component` actually had 60 variants — and nothing kept the two in
/// sync. Locks the documented counts to the real one so a future component
/// addition/removal that forgets to update the docs fails CI instead of
/// drifting silently again.
#[test]
fn documented_component_count_matches_enum_variant_count() {
    let lib_rs = include_str!("../../rustmotion-components/src/lib.rs");
    let actual = count_component_variants(lib_rs);
    assert!(
        actual > 0,
        "found zero variants in `pub enum Component` — count parsing is broken"
    );

    let readme = include_str!("../../../README.md");
    let readme_count = number_before(readme, README_COUNT_MARKER, "README.md");
    assert_eq!(
        readme_count as usize, actual,
        "README.md claims {readme_count} components but `Component` has {actual} variants"
    );

    let cargo_toml = include_str!("../../rustmotion-components/Cargo.toml");
    let cargo_toml_count = number_before(
        cargo_toml,
        CARGO_TOML_COUNT_MARKER,
        "rustmotion-components/Cargo.toml",
    );
    assert_eq!(
        cargo_toml_count as usize, actual,
        "rustmotion-components/Cargo.toml's description claims {cargo_toml_count} components but \
         `Component` has {actual} variants"
    );
}
