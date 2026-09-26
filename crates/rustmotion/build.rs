use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn collect_md_files(dir: &Path, out: &mut Vec<PathBuf>, directories: &mut Vec<PathBuf>) {
    directories.push(dir.to_path_buf());

    let mut entries: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("failed to read directory {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_md_files(&path, out, directories);
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            out.push(path);
        }
    }
}

fn main() {
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is set by cargo"));

    let skills_root = manifest_dir.join("skills");

    println!("cargo:rerun-if-changed=build.rs");

    let skill_md = skills_root.join("SKILL.md");
    assert!(
        skill_md.is_file(),
        "expected {} to exist — is the workspace layout intact?",
        skill_md.display()
    );

    let rules_dir = skills_root.join("rules");
    let mut rule_files = Vec::new();
    let mut walked_directories = vec![skills_root.clone()];
    collect_md_files(&rules_dir, &mut rule_files, &mut walked_directories);

    let mut all_files = vec![skill_md];
    all_files.extend(rule_files);

    for watched in walked_directories.iter().chain(all_files.iter()) {
        println!("cargo:rerun-if-changed={}", watched.display());
    }

    let mut generated = String::from("&[\n");
    for path in &all_files {
        let rel_path = Path::new(".claude/skills/rustmotion")
            .join(path.strip_prefix(&skills_root).unwrap_or(path))
            .to_str()
            .map(str::to_owned)
            .unwrap_or_else(|| panic!("non-UTF-8 skill file path: {}", path.display()))
            .replace('\\', "/");
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
