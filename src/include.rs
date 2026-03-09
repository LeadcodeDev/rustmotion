use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::schema::{IncludeDirective, ResolvedScenario, Scene, SceneEntry, Scenario};

const MAX_INCLUDE_DEPTH: u8 = 8;

/// Where the parent scenario was loaded from — determines how relative paths are resolved.
pub enum IncludeSource {
    /// Loaded from a file; relative paths resolve against this file's directory.
    File(PathBuf),
    /// Loaded from --json or stdin; relative paths are rejected.
    Inline,
}

/// Expand all include directives in a scenario, producing a flat list of resolved scenes.
pub fn resolve_includes(scenario: Scenario, source: &IncludeSource) -> Result<ResolvedScenario> {
    let mut audio = scenario.audio;
    let scenes = resolve_entries(scenario.scenes, source, 0, &mut audio)?;

    Ok(ResolvedScenario {
        version: scenario.version,
        video: scenario.video,
        audio,
        fonts: scenario.fonts,
        scenes,
    })
}

fn resolve_entries(
    entries: Vec<SceneEntry>,
    source: &IncludeSource,
    depth: u8,
    audio: &mut Vec<crate::schema::AudioTrack>,
) -> Result<Vec<Scene>> {
    let mut result = Vec::new();

    for entry in entries {
        match entry {
            SceneEntry::Scene(scene) => {
                result.push(scene);
            }
            SceneEntry::Include(directive) => {
                if depth >= MAX_INCLUDE_DEPTH {
                    bail!(
                        "include depth limit ({}) exceeded while resolving '{}'",
                        MAX_INCLUDE_DEPTH,
                        directive.include
                    );
                }
                let scenes = fetch_and_resolve(&directive, source, depth + 1, audio)?;
                result.extend(scenes);
            }
        }
    }

    Ok(result)
}

fn fetch_and_resolve(
    directive: &IncludeDirective,
    parent_source: &IncludeSource,
    depth: u8,
    audio: &mut Vec<crate::schema::AudioTrack>,
) -> Result<Vec<Scene>> {
    let is_remote = directive.include.starts_with("http://")
        || directive.include.starts_with("https://");

    let (json_str, child_source) = if is_remote {
        let body = fetch_remote(&directive.include)?;
        let child_source = IncludeSource::File(PathBuf::from(&directive.include));
        (body, child_source)
    } else {
        let path = resolve_local_path(&directive.include, parent_source)?;
        let body = std::fs::read_to_string(&path).with_context(|| {
            format!("include: file not found '{}'", path.display())
        })?;
        let child_source = IncludeSource::File(path);
        (body, child_source)
    };

    let child_scenario: Scenario = serde_json::from_str(&json_str)
        .with_context(|| format!("include: failed to parse '{}'", directive.include))?;

    // Merge audio tracks from the included file
    audio.extend(child_scenario.audio);

    // Recursively resolve any nested includes
    let mut scenes = resolve_entries(child_scenario.scenes, &child_source, depth, audio)?;

    // Apply scene index filter if specified
    if let Some(ref indices) = directive.scenes {
        let total = scenes.len();
        for &idx in indices {
            if idx >= total {
                bail!(
                    "include: scenes[{}] is out of bounds in '{}' (file has {} scenes)",
                    idx,
                    directive.include,
                    total
                );
            }
        }
        let mut slots: Vec<Option<Scene>> = scenes.into_iter().map(Some).collect();
        let mut filtered = Vec::with_capacity(indices.len());
        for &idx in indices {
            if let Some(scene) = slots[idx].take() {
                filtered.push(scene);
            }
        }
        scenes = filtered;
    }

    Ok(scenes)
}

fn resolve_local_path(relative: &str, source: &IncludeSource) -> Result<PathBuf> {
    match source {
        IncludeSource::File(parent_path) => {
            let parent_dir = parent_path
                .parent()
                .unwrap_or_else(|| Path::new("."));
            Ok(parent_dir.join(relative))
        }
        IncludeSource::Inline => {
            bail!(
                "include: cannot resolve relative path '{}' when scenario was given as --json (use a file path or URL instead)",
                relative
            );
        }
    }
}

fn fetch_remote(url: &str) -> Result<String> {
    let response = ureq::get(url)
        .call()
        .map_err(|e| anyhow::anyhow!("include: failed to fetch '{}': {}", url, e))?;
    let body = response
        .into_body()
        .read_to_string()
        .map_err(|e| anyhow::anyhow!("include: failed to read response for '{}': {}", url, e))?;
    Ok(body)
}
