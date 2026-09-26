use rustmotion::error::{Result, RustmotionError};
use std::path::{Path, PathBuf};

struct SkillFile {
    path: &'static str,
    content: &'static str,
}

const CLAUDE_MD: &str = include_str!("../../CLAUDE.md");

const SKILL_FILES: &[SkillFile] = include!(concat!(env!("OUT_DIR"), "/skill_files.rs"));

fn resolve_target(global: bool) -> Result<PathBuf> {
    if global {
        let home = dirs::home_dir().ok_or(RustmotionError::FileRead {
            path: "~".to_string(),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not determine home directory",
            ),
        })?;
        Ok(home)
    } else {
        Ok(PathBuf::from("."))
    }
}

fn write_if_changed(path: &Path, content: &str) -> Result<bool> {
    if let Ok(existing) = std::fs::read_to_string(path) {
        if existing == content {
            return Ok(false);
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)?;
    Ok(true)
}

pub fn install(global: bool) -> Result<()> {
    let root = resolve_target(global)?;
    let mut written = 0u32;
    let mut skipped = 0u32;

    for sf in SKILL_FILES {
        let target = root.join(sf.path);
        if write_if_changed(&target, sf.content)? {
            written += 1;
        } else {
            skipped += 1;
        }
    }

    if !global {
        let claude_path = root.join("CLAUDE.md");
        let existing = std::fs::read_to_string(&claude_path).ok();
        let merged = crate::cli::claude_md::merge(existing.as_deref(), CLAUDE_MD);
        if write_if_changed(&claude_path, &merged)? {
            written += 1;
        } else {
            skipped += 1;
        }
    }

    let location = if global {
        "~/.claude/skills/rustmotion/"
    } else {
        ".claude/skills/rustmotion/"
    };

    if written == 0 {
        println!(
            "Skills already up to date ({} files) in {}",
            skipped, location
        );
    } else {
        println!(
            "Installed {} file(s) to {} ({} unchanged)",
            written, location, skipped
        );
        if !global {
            println!("Claude Code will now use rustmotion skills in this project.");
        }
    }

    Ok(())
}

pub fn list() {
    println!("rustmotion skills ({} files)\n", SKILL_FILES.len());
    println!("  SKILL.md (main skill definition)\n");
    println!("Rules:");
    for sf in SKILL_FILES {
        if sf.path.contains("/rules/") {
            let name = sf.path.rsplit('/').next().unwrap_or(sf.path);
            let title = sf
                .content
                .lines()
                .next()
                .unwrap_or("")
                .trim_start_matches("# ")
                .trim_start_matches("Rule: ");
            println!("  {:<35} {}", name, title);
        }
    }
    println!("\nUsage:");
    println!("  rustmotion skills install          Install to current project (.claude/skills/)");
    println!("  rustmotion skills install --global  Install globally (~/.claude/skills/)");
    println!("  rustmotion skills show <name>       Show a rule (e.g. 'hex-colors')");
}

pub fn show(name: &str) -> Result<()> {
    let needle = name.trim_end_matches(".md");
    for sf in SKILL_FILES {
        let filename = sf
            .path
            .rsplit('/')
            .next()
            .unwrap_or("")
            .trim_end_matches(".md");
        if filename == needle {
            print!("{}", sf.content);
            return Ok(());
        }
    }

    if needle.eq_ignore_ascii_case("skill") {
        if let Some(sf) = SKILL_FILES.iter().find(|sf| sf.path.ends_with("/SKILL.md")) {
            print!("{}", sf.content);
            return Ok(());
        }
    }

    Err(RustmotionError::UnknownSkill {
        name: name.to_string(),
    })
}

pub fn uninstall(global: bool) -> Result<()> {
    let root = resolve_target(global)?;
    let skills_dir = root.join(".claude/skills/rustmotion");
    let location = if global {
        "~/.claude/skills/rustmotion/"
    } else {
        ".claude/skills/rustmotion/"
    };

    if !skills_dir.exists() {
        println!("Nothing to remove — {} does not exist.", location);
        return Ok(());
    }

    std::fs::remove_dir_all(&skills_dir)?;
    let mut removed = 1;

    if !global {
        let claude_path = root.join("CLAUDE.md");
        if let Ok(existing) = std::fs::read_to_string(&claude_path) {
            match crate::cli::claude_md::strip(&existing) {
                Some(remaining) => {
                    if remaining != existing {
                        std::fs::write(&claude_path, remaining)?;
                        removed += 1;
                    }
                }
                None => {
                    std::fs::remove_file(&claude_path)?;
                    removed += 1;
                }
            }
        }
    }

    let skills_parent = root.join(".claude/skills");
    if skills_parent.exists() && skills_parent.read_dir()?.next().is_none() {
        std::fs::remove_dir(&skills_parent).ok();
        let claude_dir = root.join(".claude");
        if claude_dir.exists() && claude_dir.read_dir()?.next().is_none() {
            std::fs::remove_dir(&claude_dir).ok();
        }
    }

    println!(
        "Removed rustmotion skills from {} ({} items)",
        location, removed
    );
    Ok(())
}
