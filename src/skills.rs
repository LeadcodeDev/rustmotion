use crate::error::{Result, RustmotionError};
use std::path::{Path, PathBuf};

/// An embedded skill or rule file.
struct SkillFile {
    /// Relative path from the target root (e.g. ".claude/skills/rustmotion/rules/hex-colors.md")
    path: &'static str,
    /// File content embedded at compile time.
    content: &'static str,
}

/// CLAUDE.md project instructions (written at project root).
const CLAUDE_MD: &str = include_str!("../CLAUDE.md");

/// All skill files embedded at compile time.
const SKILL_FILES: &[SkillFile] = &[
    SkillFile { path: ".claude/skills/rustmotion/SKILL.md", content: include_str!("../.claude/skills/rustmotion/SKILL.md") },
    // Rules
    SkillFile { path: ".claude/skills/rustmotion/rules/3d-perspective.md", content: include_str!("../.claude/skills/rustmotion/rules/3d-perspective.md") },
    SkillFile { path: ".claude/skills/rustmotion/rules/card-flex-layout.md", content: include_str!("../.claude/skills/rustmotion/rules/card-flex-layout.md") },
    SkillFile { path: ".claude/skills/rustmotion/rules/chart-types.md", content: include_str!("../.claude/skills/rustmotion/rules/chart-types.md") },
    SkillFile { path: ".claude/skills/rustmotion/rules/continuous-presets.md", content: include_str!("../.claude/skills/rustmotion/rules/continuous-presets.md") },
    SkillFile { path: ".claude/skills/rustmotion/rules/counter-standalone.md", content: include_str!("../.claude/skills/rustmotion/rules/counter-standalone.md") },
    SkillFile { path: ".claude/skills/rustmotion/rules/data-viz-components.md", content: include_str!("../.claude/skills/rustmotion/rules/data-viz-components.md") },
    SkillFile { path: ".claude/skills/rustmotion/rules/dot-map-coordinates.md", content: include_str!("../.claude/skills/rustmotion/rules/dot-map-coordinates.md") },
    SkillFile { path: ".claude/skills/rustmotion/rules/easing-guidelines.md", content: include_str!("../.claude/skills/rustmotion/rules/easing-guidelines.md") },
    SkillFile { path: ".claude/skills/rustmotion/rules/even-dimensions.md", content: include_str!("../.claude/skills/rustmotion/rules/even-dimensions.md") },
    SkillFile { path: ".claude/skills/rustmotion/rules/gradient-quality.md", content: include_str!("../.claude/skills/rustmotion/rules/gradient-quality.md") },
    SkillFile { path: ".claude/skills/rustmotion/rules/grid-card-height.md", content: include_str!("../.claude/skills/rustmotion/rules/grid-card-height.md") },
    SkillFile { path: ".claude/skills/rustmotion/rules/hex-colors.md", content: include_str!("../.claude/skills/rustmotion/rules/hex-colors.md") },
    SkillFile { path: ".claude/skills/rustmotion/rules/icon-format.md", content: include_str!("../.claude/skills/rustmotion/rules/icon-format.md") },
    SkillFile { path: ".claude/skills/rustmotion/rules/layer-order.md", content: include_str!("../.claude/skills/rustmotion/rules/layer-order.md") },
    SkillFile { path: ".claude/skills/rustmotion/rules/module-structure.md", content: include_str!("../.claude/skills/rustmotion/rules/module-structure.md") },
    SkillFile { path: ".claude/skills/rustmotion/rules/notification-stacking.md", content: include_str!("../.claude/skills/rustmotion/rules/notification-stacking.md") },
    SkillFile { path: ".claude/skills/rustmotion/rules/paint-context.md", content: include_str!("../.claude/skills/rustmotion/rules/paint-context.md") },
    SkillFile { path: ".claude/skills/rustmotion/rules/prefer-presets.md", content: include_str!("../.claude/skills/rustmotion/rules/prefer-presets.md") },
    SkillFile { path: ".claude/skills/rustmotion/rules/responsive-device-sizing.md", content: include_str!("../.claude/skills/rustmotion/rules/responsive-device-sizing.md") },
    SkillFile { path: ".claude/skills/rustmotion/rules/stagger-animations.md", content: include_str!("../.claude/skills/rustmotion/rules/stagger-animations.md") },
    SkillFile { path: ".claude/skills/rustmotion/rules/stat-cards.md", content: include_str!("../.claude/skills/rustmotion/rules/stat-cards.md") },
    SkillFile { path: ".claude/skills/rustmotion/rules/text-background.md", content: include_str!("../.claude/skills/rustmotion/rules/text-background.md") },
    SkillFile { path: ".claude/skills/rustmotion/rules/timeline-sequencing.md", content: include_str!("../.claude/skills/rustmotion/rules/timeline-sequencing.md") },
    SkillFile { path: ".claude/skills/rustmotion/rules/timing-constraints.md", content: include_str!("../.claude/skills/rustmotion/rules/timing-constraints.md") },
    SkillFile { path: ".claude/skills/rustmotion/rules/ui-controls.md", content: include_str!("../.claude/skills/rustmotion/rules/ui-controls.md") },
    SkillFile { path: ".claude/skills/rustmotion/rules/validate-json.md", content: include_str!("../.claude/skills/rustmotion/rules/validate-json.md") },
    SkillFile { path: ".claude/skills/rustmotion/rules/vertical-align.md", content: include_str!("../.claude/skills/rustmotion/rules/vertical-align.md") },
    SkillFile { path: ".claude/skills/rustmotion/rules/video-wizard.md", content: include_str!("../.claude/skills/rustmotion/rules/video-wizard.md") },
    SkillFile { path: ".claude/skills/rustmotion/rules/wiggle-additive.md", content: include_str!("../.claude/skills/rustmotion/rules/wiggle-additive.md") },
];

/// Resolve the target directory for skill installation.
fn resolve_target(global: bool) -> Result<PathBuf> {
    if global {
        let home = dirs::home_dir().ok_or(RustmotionError::FileRead {
            path: "~".to_string(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "Could not determine home directory"),
        })?;
        Ok(home)
    } else {
        Ok(PathBuf::from("."))
    }
}

/// Write a file, creating parent directories as needed.
/// Returns true if the file was written (new or updated), false if unchanged.
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

/// Install skills to the target directory.
/// - local (default): writes to `./.claude/skills/rustmotion/` + `./CLAUDE.md`
/// - global (`--global`): writes to `~/.claude/skills/rustmotion/`
pub fn install(global: bool) -> Result<()> {
    let root = resolve_target(global)?;
    let mut written = 0u32;
    let mut skipped = 0u32;

    // Write skill files
    for sf in SKILL_FILES {
        let target = root.join(sf.path);
        if write_if_changed(&target, sf.content)? {
            written += 1;
        } else {
            skipped += 1;
        }
    }

    // Write CLAUDE.md only in local mode
    if !global {
        let claude_path = root.join("CLAUDE.md");
        if write_if_changed(&claude_path, CLAUDE_MD)? {
            written += 1;
        } else {
            skipped += 1;
        }
    }

    let location = if global { "~/.claude/skills/rustmotion/" } else { ".claude/skills/rustmotion/" };

    if written == 0 {
        println!("Skills already up to date ({} files) in {}", skipped, location);
    } else {
        println!("Installed {} file(s) to {} ({} unchanged)", written, location, skipped);
        if !global {
            println!("Claude Code will now use rustmotion skills in this project.");
        }
    }

    Ok(())
}

/// List all available skills and rules.
pub fn list() {
    println!("rustmotion skills ({} files)\n", SKILL_FILES.len());
    println!("  SKILL.md (main skill definition)\n");
    println!("Rules:");
    for sf in SKILL_FILES {
        if sf.path.contains("/rules/") {
            let name = sf.path.rsplit('/').next().unwrap_or(sf.path);
            // Extract first line as title
            let title = sf.content.lines().next().unwrap_or("").trim_start_matches("# ").trim_start_matches("Rule: ");
            println!("  {:<35} {}", name, title);
        }
    }
    println!("\nUsage:");
    println!("  rustmotion skills install          Install to current project (.claude/skills/)");
    println!("  rustmotion skills install --global  Install globally (~/.claude/skills/)");
    println!("  rustmotion skills show <name>       Show a rule (e.g. 'hex-colors')");
}

/// Show the content of a specific rule.
pub fn show(name: &str) -> Result<()> {
    // Try matching by filename (with or without .md)
    let needle = name.trim_end_matches(".md");
    for sf in SKILL_FILES {
        let filename = sf.path.rsplit('/').next().unwrap_or("").trim_end_matches(".md");
        if filename == needle {
            print!("{}", sf.content);
            return Ok(());
        }
    }

    // Special case: SKILL.md
    if needle.eq_ignore_ascii_case("skill") {
        print!("{}", SKILL_FILES[0].content);
        return Ok(());
    }

    Err(RustmotionError::UnknownSkill { name: name.to_string() })
}

/// Uninstall skills from the target directory.
/// - local (default): removes `./.claude/skills/rustmotion/` + `./CLAUDE.md`
/// - global (`--global`): removes `~/.claude/skills/rustmotion/`
pub fn uninstall(global: bool) -> Result<()> {
    let root = resolve_target(global)?;
    let skills_dir = root.join(".claude/skills/rustmotion");
    let location = if global { "~/.claude/skills/rustmotion/" } else { ".claude/skills/rustmotion/" };

    if !skills_dir.exists() {
        println!("Nothing to remove — {} does not exist.", location);
        return Ok(());
    }

    std::fs::remove_dir_all(&skills_dir)?;
    let mut removed = 1;

    // Remove CLAUDE.md only in local mode
    if !global {
        let claude_path = root.join("CLAUDE.md");
        if claude_path.exists() {
            std::fs::remove_file(&claude_path)?;
            removed += 1;
        }
    }

    // Clean up empty parent directories
    let skills_parent = root.join(".claude/skills");
    if skills_parent.exists() && skills_parent.read_dir()?.next().is_none() {
        std::fs::remove_dir(&skills_parent).ok();
        let claude_dir = root.join(".claude");
        if claude_dir.exists() && claude_dir.read_dir()?.next().is_none() {
            std::fs::remove_dir(&claude_dir).ok();
        }
    }

    println!("Removed rustmotion skills from {} ({} items)", location, removed);
    Ok(())
}
