//! Generates the `SKILL_FILES` table embedded into the `rustmotion` binary by
//! `src/skills.rs`.
//!
//! `rustmotion skills install` is the only channel through which an LLM agent
//! receives the generation rules under `.claude/skills/rustmotion/`. Before
//! this build script existed, the embedded table was a hand-maintained
//! literal list: any rule file added to disk was invisible to `install`
//! until someone remembered to add a matching entry. That silent gap let 17
//! of 47 rule files — including ones explicitly named by CLAUDE.md — never
//! reach an installed project (issue #165).
//!
//! Walking the directory at build time instead of listing files by hand
//! makes "every `.md` file under `.claude/skills/rustmotion/` is embedded"
//! true by construction, not by memory. `cargo:rerun-if-changed` on the
//! directory (Cargo scans it recursively) means adding, removing, or editing
//! a rule file triggers a rebuild of the generated table on the next build.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Recursively collect every `.md` file under `dir`, sorted by file name at
/// each directory level so the generated table has a stable, reproducible
/// order across machines and OSes.
fn collect_md_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("failed to read directory {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_md_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            out.push(path);
        }
    }
}

fn main() {
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo"));
    // crates/rustmotion-cli -> crates -> <workspace root>
    let workspace_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| {
            panic!(
                "expected {} to live at <workspace>/crates/rustmotion-cli",
                manifest_dir.display()
            )
        })
        .to_path_buf();

    let skills_root = workspace_root.join(".claude/skills/rustmotion");

    // Rerun whenever any file under the skills tree is added, removed, or
    // edited — Cargo scans directories given to rerun-if-changed recursively.
    println!("cargo:rerun-if-changed={}", skills_root.display());
    // Once any rerun-if-changed is emitted, Cargo stops rebuilding on build.rs
    // changes implicitly, so it must be listed explicitly too.
    println!("cargo:rerun-if-changed=build.rs");

    let skill_md = skills_root.join("SKILL.md");
    assert!(
        skill_md.is_file(),
        "expected {} to exist — is the workspace layout intact?",
        skill_md.display()
    );

    let rules_dir = skills_root.join("rules");
    let mut rule_files = Vec::new();
    collect_md_files(&rules_dir, &mut rule_files);

    // SKILL.md first — `skills::show("skill")` and `skills::list()` treat it
    // as the main skill definition — followed by every rule file,
    // alphabetically. This mirrors the order of the previous hand-written
    // list, but nothing downstream is allowed to depend on it: `show()`
    // locates SKILL.md by path, not by position.
    let mut all_files = vec![skill_md];
    all_files.extend(rule_files);

    let mut generated = String::from("&[\n");
    for path in &all_files {
        let rel_path = path
            .strip_prefix(&workspace_root)
            .unwrap_or(path)
            .to_str()
            .unwrap_or_else(|| panic!("non-UTF-8 skill file path: {}", path.display()))
            .replace('\\', "/"); // normalize separators if built on Windows
        let abs_path = path
            .to_str()
            .unwrap_or_else(|| panic!("non-UTF-8 skill file path: {}", path.display()));
        generated.push_str(&format!(
            "    SkillFile {{ path: {rel_path:?}, content: include_str!({abs_path:?}) }},\n"
        ));
    }
    generated.push_str("]\n");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set by cargo"));
    fs::write(out_dir.join("skill_files.rs"), generated).expect("failed to write skill_files.rs");
}
