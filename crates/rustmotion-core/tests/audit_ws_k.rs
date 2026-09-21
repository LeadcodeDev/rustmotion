//! Regression tests for the workstream K (docs, schema, CI) audit findings:
//! RM-02, RM-47, RM-52.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

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

/// RM-02 / RM-47: README.md and `rustmotion-components/Cargo.toml` (the text
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

/// Public schema fields that parse successfully but are not read anywhere
/// outside `crates/rustmotion-core/src/schema/` — kept out of
/// `every_public_schema_field_is_read_somewhere_or_allowlisted`'s failure so
/// a *known, tracked* gap doesn't block CI, while a *new* one still does.
/// Each entry names the finding that tracks closing it and the reason it
/// isn't closed by workstream K itself. Removing an entry once the field is
/// wired (or deleted) is the expected way this list shrinks.
const KNOWN_INERT_FIELDS: &[(&str, &str)] = &[
    (
        "codec",
        "RM-50: VideoConfig.codec is not threaded into the encode path. Honouring it means \
         editing crates/rustmotion/src/cli/mod.rs (the Render/Batch/Still command handlers) and \
         crates/rustmotion/src/encode/, both outside workstream K's owned files — see the \
         workstream K report's handover for the exact wiring point.",
    ),
    (
        "intensity",
        "RM-51: MotionBlurConfig.intensity is read into AnimatedProperties.motion_blur \
         (crates/rustmotion-core/src/engine/animator.rs:259) but nothing reads that field \
         afterwards. Wiring it into the ghost-opacity math, or deleting it, both touch \
         crates/rustmotion-components/src/box_builder.rs and crates/rustmotion-core/src/engine/\
         animator.rs — outside workstream K's owned files.",
    ),
    (
        "version",
        "Not one of workstream K's 9 named findings — surfaced by this guard test itself. \
         Scenario.version defaults to \"1.0\" and deserializes, but crates/rustmotion/src/\
         loader.rs never reads it back (no version-gating or migration logic exists yet). \
         Wiring or removing it touches loader.rs, outside workstream K's owned files; flagged \
         in the workstream K report for triage.",
    ),
    (
        "target",
        "Not one of workstream K's 9 named findings — surfaced by this guard test itself, with \
         a caveat this test can't resolve on its own: Annotation.target is written by \
         rustmotion-studio (crates/rustmotion-studio/src/editor/annotations.rs:94) as raw JSON \
         (a `\"target\": {...}` object literal, not a `.target` field access — this grep-based \
         check only matches Rust member access), so it may be consumed by the `apply-annotations` \
         Claude Code skill reading the scenario file's raw JSON rather than by any Rust code path. \
         Allowlisted rather than asserted dead; flagged in the workstream K report for a human to \
         confirm one way or the other.",
    ),
];

/// `crates/rustmotion-core` -> `crates` -> `<workspace root>`.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("rustmotion-core is expected at <workspace>/crates/rustmotion-core")
        .to_path_buf()
}

/// Recursively collect every `.rs` file under `dir`, skipping `target/`.
fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
    for entry in entries {
        let entry = entry.unwrap_or_else(|e| panic!("read_dir entry in {}: {e}", dir.display()));
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().and_then(|n| n.to_str()) == Some("target") {
                continue;
            }
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// Extract the names of `pub <name>: <Type>,`-style struct fields from Rust
/// source text. Deliberately crude (no parser): looks for lines whose
/// trimmed text starts with `"pub "` and contains a `:` before any
/// non-identifier character, which matches plain field declarations
/// (`pub width: u32,`) while excluding `pub fn`/`pub struct`/`pub enum`
/// (no top-level `:`, or one buried behind non-identifier characters like
/// `(`/`<`/`&`/spaces that fail the identifier check below).
fn extract_pub_field_names(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("pub ") else {
            continue;
        };
        let Some(colon_idx) = rest.find(':') else {
            continue;
        };
        let candidate = rest[..colon_idx].trim();
        let is_plain_identifier = !candidate.is_empty()
            && candidate
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_');
        if is_plain_identifier {
            names.push(candidate.to_string());
        }
    }
    names
}

/// RM-52: nothing stopped a schema field from parsing successfully and then
/// being read by no code path — RM-50 (`VideoConfig.codec`) and RM-51
/// (`MotionBlurConfig.intensity`) are exactly that defect, and the format's
/// credibility as an LLM generation target rests on a field either doing
/// something or failing to parse. This test greps the workspace for a
/// member-access on every public schema field name and fails if one isn't
/// found anywhere outside its own definition — unless it's in
/// `KNOWN_INERT_FIELDS`, so a newly introduced inert field still fails CI.
#[test]
fn every_public_schema_field_is_read_somewhere_or_allowlisted() {
    let root = workspace_root();
    let schema_dir = root.join("crates/rustmotion-core/src/schema");

    let mut schema_files = Vec::new();
    collect_rs_files(&schema_dir, &mut schema_files);
    assert!(
        !schema_files.is_empty(),
        "expected {} to contain schema files",
        schema_dir.display()
    );

    let mut field_names: BTreeSet<String> = BTreeSet::new();
    for path in &schema_files {
        let content =
            fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        field_names.extend(extract_pub_field_names(&content));
    }
    assert!(
        field_names.len() > 50,
        "expected well over 50 public schema fields, found {} — field extraction is probably broken",
        field_names.len()
    );

    let self_path = root
        .join(file!())
        .canonicalize()
        .unwrap_or_else(|e| panic!("canonicalize {}: {e}", root.join(file!()).display()));

    let mut other_files = Vec::new();
    collect_rs_files(&root.join("crates"), &mut other_files);
    let other_sources: Vec<String> = other_files
        .iter()
        .filter(|path| !schema_files.contains(path))
        .filter(|path| path.canonicalize().map(|p| p != self_path).unwrap_or(true))
        .map(|path| {
            fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
        })
        .collect();

    let mut unread_fields = Vec::new();
    for name in &field_names {
        if KNOWN_INERT_FIELDS.iter().any(|(f, _)| f == name) {
            continue;
        }
        let access_pattern = format!(".{name}");
        let is_read = other_sources
            .iter()
            .any(|content| content.contains(&access_pattern));
        if !is_read {
            unread_fields.push(name.clone());
        }
    }

    assert!(
        unread_fields.is_empty(),
        "public schema field(s) never read outside crates/rustmotion-core/src/schema/: \
         {unread_fields:?}\nEither wire them into the engine/CLI, or add them to \
         KNOWN_INERT_FIELDS above with a reason and the finding that tracks closing it."
    );
}
