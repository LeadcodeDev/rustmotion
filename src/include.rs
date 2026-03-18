use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::error::RustmotionError;
use crate::schema::{
    EasingType, IncludeDirective, ResolvedScenario, ResolvedView, Scene, SceneEntry, Scenario,
    ViewType,
};

const MAX_INCLUDE_DEPTH: u8 = 8;

/// Where the parent scenario was loaded from — determines how relative paths are resolved.
pub enum IncludeSource {
    /// Loaded from a file; relative paths resolve against this file's directory.
    File(PathBuf),
    /// Loaded from --json or stdin; relative paths are rejected.
    Inline,
}

/// Expand all include directives in a scenario, producing resolved views.
pub fn resolve_includes(scenario: Scenario, source: &IncludeSource) -> Result<ResolvedScenario> {
    let mut audio = scenario.audio;
    let mut included_paths = Vec::new();
    let has_scenes = !scenario.scenes.is_empty();
    let has_composition = scenario.composition.is_some();

    if has_scenes && has_composition {
        return Err(RustmotionError::CompositionAndScenesConflict.into());
    }

    let views = if let Some(composition) = scenario.composition {
        // New format: composition with views
        let mut views = Vec::with_capacity(composition.len());
        for view in composition {
            let scenes = resolve_entries(view.scenes, source, 0, &mut audio, &mut included_paths)?;
            views.push(ResolvedView {
                view_type: view.view_type,
                scenes,
                transition: view.transition,
                background: view.background,
                animated_background: view.animated_background,
                camera_easing: view.camera_easing,
                camera_pan_duration: view.camera_pan_duration,
            });
        }
        views
    } else {
        // Backward compat: wrap top-level scenes in a single slide view
        let scenes = resolve_entries(scenario.scenes, source, 0, &mut audio, &mut included_paths)?;
        vec![ResolvedView {
            view_type: ViewType::Slide,
            scenes,
            transition: None,
            background: None,
            animated_background: Vec::new(),
            camera_easing: EasingType::EaseInOut,
            camera_pan_duration: 0.8,
        }]
    };

    Ok(ResolvedScenario {
        video: scenario.video,
        audio,
        fonts: scenario.fonts,
        views,
        included_paths,
    })
}

fn resolve_entries(
    entries: Vec<SceneEntry>,
    source: &IncludeSource,
    depth: u8,
    audio: &mut Vec<crate::schema::AudioTrack>,
    included_paths: &mut Vec<PathBuf>,
) -> Result<Vec<Scene>> {
    let mut result = Vec::new();

    for entry in entries {
        match entry {
            SceneEntry::Scene(scene) => {
                result.push(scene);
            }
            SceneEntry::Include(directive) => {
                if depth >= MAX_INCLUDE_DEPTH {
                    return Err(RustmotionError::IncludeDepthExceeded {
                        limit: MAX_INCLUDE_DEPTH,
                        path: directive.include.clone(),
                    }.into());
                }
                let scenes = fetch_and_resolve(&directive, source, depth + 1, audio, included_paths)?;
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
    included_paths: &mut Vec<PathBuf>,
) -> Result<Vec<Scene>> {
    let is_remote = directive.include.starts_with("http://")
        || directive.include.starts_with("https://");

    let (json_str, child_source) = if is_remote {
        let body = fetch_remote(&directive.include)?;
        let child_source = IncludeSource::File(PathBuf::from(&directive.include));
        (body, child_source)
    } else {
        let path = resolve_local_path(&directive.include, parent_source)?;
        let body = std::fs::read_to_string(&path).map_err(|_| {
            RustmotionError::IncludeFileNotFound { path: path.display().to_string() }
        })?;
        // Track this included file for watch mode
        included_paths.push(path.clone());
        let child_source = IncludeSource::File(path);
        (body, child_source)
    };

    // Parse as raw Value first, apply variable substitution, then deserialize
    let mut json_value: serde_json::Value =
        serde_json::from_str(&json_str).map_err(RustmotionError::from)?;

    crate::variables::apply_variables(
        &mut json_value,
        directive.config.as_ref(),
        &directive.include,
    )?;

    let child_scenario: Scenario =
        serde_json::from_value(json_value).map_err(RustmotionError::from)?;

    // Merge audio tracks from the included file
    audio.extend(child_scenario.audio);

    // Recursively resolve any nested includes
    let mut scenes = resolve_entries(child_scenario.scenes, &child_source, depth, audio, included_paths)?;

    // Apply scene index filter if specified
    if let Some(ref indices) = directive.scenes {
        let total = scenes.len();
        for &idx in indices {
            if idx >= total {
                return Err(RustmotionError::IncludeSceneOutOfBounds {
                    index: idx,
                    path: directive.include.clone(),
                    total,
                }.into());
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
            return Err(RustmotionError::IncludeInlinePath {
                path: relative.to_string(),
            }.into());
        }
    }
}

fn fetch_remote(url: &str) -> Result<String> {
    let response = ureq::get(url)
        .call()
        .map_err(|e| RustmotionError::IncludeRemoteFetch { url: url.to_string(), reason: e.to_string() })?;
    let body = response
        .into_body()
        .read_to_string()
        .map_err(|e| RustmotionError::IncludeRemoteFetch { url: url.to_string(), reason: e.to_string() })?;
    Ok(body)
}
