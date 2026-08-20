//! Guards `rustmotion skills install` against silently dropping rule files.
//!
//! `SKILL_FILES` (embedded by `build.rs`, see `src/cli/skills.rs`) is meant to
//! mirror every `.md` file under `.claude/skills/rustmotion/` exactly.
//! Before `build.rs` existed, that table was a hand-maintained literal list
//! that fell 17 files behind disk without ever failing a build or a test
//! (issue #165) — including `geometry-safety.md`, `world-view.md` and
//! `audio-reactive.md`, the three rules CLAUDE.md cites by name.
//!
//! This test runs the actual compiled `rustmotion` binary's
//! `skills install` against a scratch directory and diffs the resulting
//! file set + content against the source tree, so any future drift between
//! disk and the embedded table fails loudly and names the missing files —
//! rather than requiring someone to remember to update a list by hand.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// `crates/rustmotion` -> `crates` -> `<workspace root>`.
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("rustmotion is expected at <workspace>/crates/rustmotion")
        .to_path_buf()
}

/// Recursively collect every file under `dir`, as paths relative to `dir`.
fn collect_files_relative(dir: &Path) -> BTreeSet<PathBuf> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeSet<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
        {
            let entry =
                entry.unwrap_or_else(|e| panic!("read_dir entry in {}: {e}", dir.display()));
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                out.insert(
                    path.strip_prefix(base)
                        .unwrap_or_else(|_| {
                            panic!("{} not under {}", path.display(), base.display())
                        })
                        .to_path_buf(),
                );
            }
        }
    }
    let mut out = BTreeSet::new();
    walk(dir, dir, &mut out);
    out
}

/// Minimal RAII scratch directory — avoids pulling in a `tempfile`
/// dev-dependency for two tests.
struct ScratchDir(PathBuf);

impl ScratchDir {
    fn new(label: &str) -> Self {
        let unique = format!(
            "rustmotion-skills-test-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock before UNIX epoch")
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).expect("create scratch dir");
        Self(path)
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run_skills_install(cwd: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rustmotion"))
        .args(["skills", "install"])
        .current_dir(cwd)
        .output()
        .expect("failed to spawn `rustmotion skills install`")
}

/// The red-phase test for issue #165: what `skills install` writes must be
/// byte-for-byte identical (file set + content) to
/// `.claude/skills/rustmotion/` on disk. Before `build.rs`, this failed by
/// naming exactly the 17 rule files missing from the hand-written list.
#[test]
fn skills_install_matches_source_tree() {
    let root = workspace_root();
    let source_skills_dir = root.join(".claude/skills/rustmotion");
    let source_files = collect_files_relative(&source_skills_dir);
    assert!(
        !source_files.is_empty(),
        "expected {} to contain rule files",
        source_skills_dir.display()
    );

    let scratch = ScratchDir::new("skills-install");
    let output = run_skills_install(&scratch.0);
    assert!(
        output.status.success(),
        "`rustmotion skills install` failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let installed_skills_dir = scratch.0.join(".claude/skills/rustmotion");
    assert!(
        installed_skills_dir.is_dir(),
        "`rustmotion skills install` did not create {}",
        installed_skills_dir.display()
    );
    let installed_files = collect_files_relative(&installed_skills_dir);

    let missing: Vec<_> = source_files.difference(&installed_files).collect();
    let extra: Vec<_> = installed_files.difference(&source_files).collect();

    assert!(
        missing.is_empty() && extra.is_empty(),
        "`rustmotion skills install` diverges from .claude/skills/rustmotion/:\n\
         missing ({} files on disk but never installed): {:#?}\n\
         extra ({} files installed but absent from disk): {:#?}",
        missing.len(),
        missing,
        extra.len(),
        extra
    );

    // Matching file sets isn't enough on its own: also confirm content is
    // byte-for-byte what's on disk, so a build.rs path-mapping bug (e.g. two
    // files swapped with each other's content) fails here too.
    for rel in &source_files {
        let source_content = fs::read(source_skills_dir.join(rel))
            .unwrap_or_else(|e| panic!("read source {}: {e}", rel.display()));
        let installed_content = fs::read(installed_skills_dir.join(rel))
            .unwrap_or_else(|e| panic!("read installed {}: {e}", rel.display()));
        assert_eq!(
            source_content,
            installed_content,
            "installed content for {} does not match .claude/skills/rustmotion/{}",
            rel.display(),
            rel.display()
        );
    }
}

/// Sanity check that `skills install` actually writes a non-trivial number
/// of files into a fresh, empty target — guards against the comparison test
/// above passing vacuously (e.g. both sides empty because the binary
/// silently failed to run).
#[test]
fn skills_install_writes_files_into_empty_target() {
    let scratch = ScratchDir::new("skills-install-count");
    let output = run_skills_install(&scratch.0);
    assert!(
        output.status.success(),
        "`rustmotion skills install` failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let installed_skills_dir = scratch.0.join(".claude/skills/rustmotion");
    let installed_files = collect_files_relative(&installed_skills_dir);
    assert!(
        installed_files.len() >= 47,
        "expected at least 47 files installed (1 SKILL.md + 46+ rules), got {}: {:#?}",
        installed_files.len(),
        installed_files
    );
}
