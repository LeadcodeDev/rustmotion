use std::net::{IpAddr, SocketAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};

use crate::error::Result;

use crate::error::RustmotionError;
use crate::schema::{
    AnimatedBackground, BackgroundEntry, BackgroundPreset, BackgroundValue, EasingType,
    IncludeDirective, ResolvedBackground, ResolvedScenario, ResolvedView, Scenario, Scene,
    SceneEntry, ViewType,
};

const MAX_INCLUDE_DEPTH: u8 = 8;

const MAX_EXPANDED_SCENES: usize = 5_000;

/// Response-size cap for a remote `include` fetch (RM-46). Scenario JSON is
/// not expected to be large; this is deliberately far below ureq's own 10 MB
/// default for `read_to_vec`/`read_to_string`.
const MAX_REMOTE_INCLUDE_BYTES: u64 = 4 * 1024 * 1024;

/// The CLI flag whose semantics this module implements the mechanism for:
/// remote `include` is denied unless a caller explicitly opts in by passing
/// `RemoteIncludePolicy::Allow` to [`resolve_includes_with_policy`]. Naming
/// it here keeps the error message and the flag rustmotion's CLI is expected
/// to expose in sync.
const ALLOW_REMOTE_INCLUDE_FLAG: &str = "--allow-remote-include";

/// Whether a [`resolve_includes_with_policy`] pass may perform outbound
/// network requests for `include: "https://..."` directives (RM-46: remote
/// fetching is deliberate design, but it previously had no allowlist and no
/// opt-out — any scenario, including one merely being `validate`d, could
/// make this process issue arbitrary GETs). Defaults to [`Self::Deny`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RemoteIncludePolicy {
    #[default]
    Deny,
    Allow,
}

/// Where the parent scenario was loaded from — determines how relative paths are resolved.
pub enum IncludeSource {
    /// Loaded from a file; relative paths resolve against this file's directory.
    File(PathBuf),
    /// Loaded from --json or stdin; relative paths are rejected.
    Inline,
}

/// Expand all include directives in a scenario, producing resolved views.
///
/// Remote (`http(s)://`) includes are denied by default — see
/// [`resolve_includes_with_policy`] to opt in. This is exactly
/// `resolve_includes_with_policy(scenario, source, RemoteIncludePolicy::Deny)`,
/// kept as its own entry point so every existing caller stays secure by
/// default without having to be rewritten to pass a policy (RM-46).
pub fn resolve_includes(scenario: Scenario, source: &IncludeSource) -> Result<ResolvedScenario> {
    resolve_includes_with_policy(scenario, source, RemoteIncludePolicy::Deny)
}

/// Same as [`resolve_includes`], with explicit control over whether remote
/// `include` directives may reach the network.
pub fn resolve_includes_with_policy(
    scenario: Scenario,
    source: &IncludeSource,
    remote_policy: RemoteIncludePolicy,
) -> Result<ResolvedScenario> {
    let mut audio = scenario.audio;
    let mut included_paths = Vec::new();
    let root_dir = local_include_root(source);
    let mut expanded_scenes: usize = 0;
    let has_scenes = !scenario.scenes.is_empty();
    let has_composition = scenario.composition.is_some();

    if has_scenes && has_composition {
        return Err(RustmotionError::CompositionAndScenesConflict);
    }

    let templates = &scenario.backgrounds;

    let views = if let Some(composition) = scenario.composition {
        // New format: composition with views
        let mut views = Vec::with_capacity(composition.len());
        for view in composition {
            let mut scenes = resolve_entries(
                view.scenes,
                source,
                0,
                &mut audio,
                &mut included_paths,
                remote_policy,
                root_dir.as_deref(),
                &mut expanded_scenes,
            )?;
            for scene in &mut scenes {
                resolve_scene_background(scene, templates)?;
            }
            let view_bg = resolve_background_value(
                view.background.as_ref(),
                &view.animated_background,
                templates,
            )?;
            views.push(ResolvedView {
                view_type: view.view_type,
                scenes,
                transition: view.transition,
                background: view_bg,
                camera_easing: view.camera_easing,
                camera_pan_duration: view.camera_pan_duration,
            });
        }
        views
    } else {
        // Backward compat: wrap top-level scenes in a single slide view
        let mut scenes = resolve_entries(
            scenario.scenes,
            source,
            0,
            &mut audio,
            &mut included_paths,
            remote_policy,
            root_dir.as_deref(),
            &mut expanded_scenes,
        )?;
        for scene in &mut scenes {
            resolve_scene_background(scene, templates)?;
        }
        vec![ResolvedView {
            view_type: ViewType::Slide,
            scenes,
            transition: None,
            background: ResolvedBackground::default(),
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
    remote_policy: RemoteIncludePolicy,
    root_dir: Option<&Path>,
    expanded_scenes: &mut usize,
) -> Result<Vec<Scene>> {
    let mut result = Vec::new();

    for entry in entries {
        match entry {
            SceneEntry::Scene(scene) => {
                *expanded_scenes += 1;
                if *expanded_scenes > MAX_EXPANDED_SCENES {
                    return Err(RustmotionError::Generic(format!(
                        "scenario expands to more than {MAX_EXPANDED_SCENES} scenes via \
                         'include' — refusing to keep expanding"
                    )));
                }
                result.push(scene);
            }
            SceneEntry::Include(directive) => {
                if depth >= MAX_INCLUDE_DEPTH {
                    return Err(RustmotionError::IncludeDepthExceeded {
                        limit: MAX_INCLUDE_DEPTH,
                        path: directive.include.clone(),
                    });
                }
                let scenes = fetch_and_resolve(
                    &directive,
                    source,
                    depth + 1,
                    audio,
                    included_paths,
                    remote_policy,
                    root_dir,
                    expanded_scenes,
                )?;
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
    remote_policy: RemoteIncludePolicy,
    root_dir: Option<&Path>,
    expanded_scenes: &mut usize,
) -> Result<Vec<Scene>> {
    *expanded_scenes += 1;
    if *expanded_scenes > MAX_EXPANDED_SCENES {
        return Err(RustmotionError::Generic(format!(
            "scenario expands to more than {MAX_EXPANDED_SCENES} scenes via 'include' — \
             refusing to keep expanding"
        )));
    }

    let is_remote =
        directive.include.starts_with("http://") || directive.include.starts_with("https://");

    let (json_str, child_source) = if is_remote {
        if remote_policy != RemoteIncludePolicy::Allow {
            return Err(RustmotionError::Generic(format!(
                "include '{}' is a remote URL, but remote includes are disabled by default \
                 (RM-46) — pass {ALLOW_REMOTE_INCLUDE_FLAG} to opt in",
                directive.include
            )));
        }
        let verified = verify_remote_url(&directive.include)?;
        eprintln!("include: fetching remote scenario {}", verified.as_str());
        let body = fetch_remote(&verified)?;
        let child_source = IncludeSource::File(PathBuf::from(&directive.include));
        (body, child_source)
    } else {
        let path = resolve_local_path(&directive.include, parent_source, root_dir)?;
        let body =
            std::fs::read_to_string(&path).map_err(|_| RustmotionError::IncludeFileNotFound {
                path: path.display().to_string(),
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
    // `components` (and any `for-each`/`use` inside this file's own scenes)
    // is scoped to this document: expanded here, per included file, using
    // ONLY this file's own `components` block — never the parent's, and
    // never visible to the parent's own `use` sites. See
    // `rustmotion_core::expand`'s module doc for why that scoping was
    // chosen over a cross-file component registry.
    crate::expand::expand_directives(&mut json_value, &directive.include)?;

    // An included file's assets are relative to *that* file, not to the parent
    // that pulled it in — otherwise moving an include would silently break
    // every path inside it.
    if let IncludeSource::File(ref p) = child_source {
        if let Some(dir) = p.parent() {
            crate::assets::rebase_relative_paths(&mut json_value, dir);
        }
    }

    let child_scenario: Scenario =
        serde_json::from_value(json_value).map_err(RustmotionError::from)?;

    // Merge audio tracks from the included file
    audio.extend(child_scenario.audio);

    // Recursively resolve any nested includes
    let mut scenes = resolve_entries(
        child_scenario.scenes,
        &child_source,
        depth,
        audio,
        included_paths,
        remote_policy,
        root_dir,
        expanded_scenes,
    )?;

    // Apply scene index filter if specified
    if let Some(ref indices) = directive.scenes {
        let total = scenes.len();
        for &idx in indices {
            if idx >= total {
                return Err(RustmotionError::IncludeSceneOutOfBounds {
                    index: idx,
                    path: directive.include.clone(),
                    total,
                });
            }
        }
        let mut filtered = Vec::with_capacity(indices.len());
        for &idx in indices {
            filtered.push(clone_scene_via_json(&scenes[idx])?);
        }
        scenes = filtered;
    }

    Ok(scenes)
}

fn clone_scene_via_json(scene: &Scene) -> Result<Scene> {
    let value = serde_json::to_value(scene).map_err(RustmotionError::from)?;
    serde_json::from_value(value).map_err(RustmotionError::from)
}

fn local_include_root(source: &IncludeSource) -> Option<PathBuf> {
    match source {
        IncludeSource::File(path) => {
            let dir = path.parent().unwrap_or_else(|| Path::new("."));
            let dir = if dir.as_os_str().is_empty() {
                Path::new(".")
            } else {
                dir
            };
            Some(std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf()))
        }
        IncludeSource::Inline => None,
    }
}

fn resolve_local_path(
    relative: &str,
    source: &IncludeSource,
    root_dir: Option<&Path>,
) -> Result<PathBuf> {
    match source {
        IncludeSource::File(parent_path) => {
            let parent_dir = parent_path.parent().unwrap_or_else(|| Path::new("."));
            let candidate = parent_dir.join(relative);
            if let Some(root) = root_dir {
                let effective =
                    std::fs::canonicalize(&candidate).unwrap_or_else(|_| candidate.clone());
                if !effective.starts_with(root) {
                    return Err(RustmotionError::Generic(format!(
                        "include '{relative}' resolves to '{}', which is outside the scenario's \
                         own directory '{}' — refusing",
                        effective.display(),
                        root.display()
                    )));
                }
            }
            Ok(candidate)
        }
        IncludeSource::Inline => Err(RustmotionError::IncludeInlinePath {
            path: relative.to_string(),
        }),
    }
}

/// A remote `include` URL that has already passed the SSRF policy check in
/// [`verify_remote_url`] — every address its host resolves to was confirmed
/// public. This is the only way to reach [`fetch_remote`]: an unchecked
/// `&str` cannot be passed to it, by construction.
struct VerifiedRemoteUrl(String);

impl VerifiedRemoteUrl {
    fn as_str(&self) -> &str {
        &self.0
    }
}

/// Resolves `url`'s host and rejects it if any resolved address is not
/// publicly routable: loopback, link-local (169.254.169.254, the cloud
/// metadata endpoint, included), or an RFC1918/ULA private range (RM-46).
///
/// This check runs once, ahead of the request `fetch_remote` makes moments
/// later; a DNS answer that changes between this resolution and that
/// connection ("DNS rebinding") is not defended against — the audit's
/// remediation asks for a resolve-time check, and closing the rebinding gap
/// fully would mean replacing `ureq`'s connector rather than configuring it.
fn verify_remote_url(url: &str) -> Result<VerifiedRemoteUrl> {
    let (host, port) = split_host_port(url)?;
    let addrs: Vec<SocketAddr> = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|e| {
            RustmotionError::Generic(format!("include '{url}' could not be resolved: {e}"))
        })?
        .collect();
    if addrs.is_empty() {
        return Err(RustmotionError::Generic(format!(
            "include '{url}' resolved to no addresses"
        )));
    }
    for addr in &addrs {
        let ip = addr.ip().to_canonical();
        if is_non_public(ip) {
            return Err(RustmotionError::Generic(format!(
                "include '{url}' resolves to {ip}, which is not a public address \
                 (loopback/link-local/private range) — refusing to prevent SSRF"
            )));
        }
    }
    Ok(VerifiedRemoteUrl(url.to_string()))
}

fn is_non_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_multicast()
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
                || v6.is_multicast()
        }
    }
}

/// Splits `scheme://[user:pass@]host[:port][/...]` into `(host, port)`,
/// defaulting the port from the scheme. Deliberately minimal — this crate
/// takes no `url` dependency for this, and only needs to know where to point
/// the resolver; `ureq` itself is still the one that rejects a malformed URL
/// when the actual request is made.
fn split_host_port(url: &str) -> Result<(String, u16)> {
    let (rest, default_port) = if let Some(rest) = url.strip_prefix("https://") {
        (rest, 443)
    } else if let Some(rest) = url.strip_prefix("http://") {
        (rest, 80)
    } else {
        return Err(RustmotionError::Generic(format!(
            "include '{url}' is not an http(s) URL"
        )));
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let authority = authority.rsplit('@').next().unwrap_or(authority);

    if let Some(rest) = authority.strip_prefix('[') {
        let end = rest.find(']').ok_or_else(|| {
            RustmotionError::Generic(format!("include '{url}' has an unterminated IPv6 host"))
        })?;
        let host = rest[..end].to_string();
        let port = rest[end + 1..]
            .strip_prefix(':')
            .and_then(|p| p.parse().ok())
            .unwrap_or(default_port);
        return Ok((host, port));
    }

    match authority.rsplit_once(':') {
        Some((host, port_str)) => match port_str.parse() {
            Ok(port) => Ok((host.to_string(), port)),
            Err(_) => Ok((authority.to_string(), default_port)),
        },
        None => Ok((authority.to_string(), default_port)),
    }
}

fn fetch_remote(url: &VerifiedRemoteUrl) -> Result<String> {
    let response = rustmotion_core::engine::renderer::http_agent()
        .get(url.as_str())
        .call()
        .map_err(|e| RustmotionError::IncludeRemoteFetch {
            url: url.as_str().to_string(),
            reason: e.to_string(),
        })?;
    response
        .into_body()
        .with_config()
        .limit(MAX_REMOTE_INCLUDE_BYTES)
        .lossy_utf8(true)
        .read_to_string()
        .map_err(|e| RustmotionError::IncludeRemoteFetch {
            url: url.as_str().to_string(),
            reason: e.to_string(),
        })
}

// --- Background template resolution ---

use std::collections::HashMap;

/// Resolve a single BackgroundEntry against the template map.
fn resolve_entry(
    entry: &BackgroundEntry,
    templates: &HashMap<String, serde_json::Value>,
) -> Result<AnimatedBackground> {
    let base = if let Some(ref name) = entry.template_ref {
        let tmpl = templates
            .get(name)
            .ok_or_else(|| RustmotionError::UnknownBackgroundTemplate { name: name.clone() })?;
        let mut base = tmpl.clone();
        deep_merge(
            &mut base,
            &serde_json::Value::Object(entry.overrides.clone()),
        );
        base
    } else {
        serde_json::Value::Object(entry.overrides.clone())
    };
    let bg: AnimatedBackground = serde_json::from_value(base)?;
    validate_animated_bg(&bg)?;
    Ok(bg)
}

/// Validate an AnimatedBackground after deserialization.
fn validate_animated_bg(bg: &AnimatedBackground) -> Result<()> {
    if let BackgroundPreset::Heropattern(cfg) = &bg.preset {
        if crate::engine::heropatterns::find_pattern(&cfg.pattern).is_none() {
            return Err(RustmotionError::UnknownHeropattern {
                name: cfg.pattern.clone(),
            });
        }
    }
    Ok(())
}

/// Resolve a BackgroundValue + legacy animated_background into a ResolvedBackground.
fn resolve_background_value(
    bg_value: Option<&BackgroundValue>,
    legacy: &[AnimatedBackground],
    templates: &HashMap<String, serde_json::Value>,
) -> Result<ResolvedBackground> {
    let mut resolved = ResolvedBackground::default();

    if let Some(bg) = bg_value {
        match bg {
            BackgroundValue::Color(s) => {
                resolved.color = Some(s.clone());
            }
            BackgroundValue::Single(entry) => {
                resolved.animated.push(resolve_entry(entry, templates)?);
                resolved.transition = entry.transition.clone();
            }
            BackgroundValue::Multiple(entries) => {
                for entry in entries {
                    resolved.animated.push(resolve_entry(entry, templates)?);
                    if resolved.transition.is_none() {
                        resolved.transition = entry.transition.clone();
                    }
                }
            }
        }
    }

    // Append legacy animated-background entries (backward compat)
    for bg in legacy {
        validate_animated_bg(bg)?;
    }
    resolved.animated.extend_from_slice(legacy);

    Ok(resolved)
}

/// Resolve the background for a scene and store it in `resolved_background`.
fn resolve_scene_background(
    scene: &mut Scene,
    templates: &HashMap<String, serde_json::Value>,
) -> Result<()> {
    scene.resolved_background = resolve_background_value(
        scene.background.as_ref(),
        &scene.animated_background,
        templates,
    )?;
    Ok(())
}

/// Deep-merge overlay into base (overlay values win). Skips `$ref` and `transition` keys.
fn deep_merge(base: &mut serde_json::Value, overlay: &serde_json::Value) {
    if let (serde_json::Value::Object(b), serde_json::Value::Object(o)) = (base, overlay) {
        for (k, v) in o {
            if k == "$ref" || k == "transition" {
                continue;
            }
            match b.get_mut(k) {
                Some(existing) if existing.is_object() && v.is_object() => {
                    deep_merge(existing, v);
                }
                _ => {
                    b.insert(k.clone(), v.clone());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "rm_include_test_{name}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn scenario_from_file(path: &Path) -> Scenario {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn local_include_with_an_absolute_path_cannot_escape_the_scenario_directory() {
        let scenario_dir = scratch_dir("escape-abs-scenario");
        std::fs::create_dir_all(&scenario_dir).unwrap();
        let witness_dir = scratch_dir("escape-abs-witness");
        std::fs::create_dir_all(&witness_dir).unwrap();
        let witness_path = witness_dir.join("witness.json");
        std::fs::write(
            &witness_path,
            r#"{"video": {"width": 10, "height": 10}, "scenes": [{"duration": 1.0, "children": []}]}"#,
        )
        .unwrap();

        let top_path = scenario_dir.join("top.json");
        let top_body = serde_json::json!({
            "video": {"width": 10, "height": 10},
            "scenes": [{"include": witness_path.to_str().unwrap()}]
        });
        std::fs::write(&top_path, serde_json::to_string(&top_body).unwrap()).unwrap();

        let scenario = scenario_from_file(&top_path);
        let result = resolve_includes(scenario, &IncludeSource::File(top_path.clone()));

        assert!(
            result.is_err(),
            "an absolute include path outside the scenario's own directory must be a named, \
             refused error, not a silent read — got {:?}",
            result.map(|r| r.all_scenes().count())
        );

        let _ = std::fs::remove_dir_all(&scenario_dir);
        let _ = std::fs::remove_dir_all(&witness_dir);
    }

    #[test]
    fn local_include_with_dot_dot_cannot_escape_the_scenario_directory() {
        let base = scratch_dir("escape-dotdot");
        let scenario_dir = base.join("project");
        std::fs::create_dir_all(&scenario_dir).unwrap();
        let witness_path = base.join("witness.json");
        std::fs::write(
            &witness_path,
            r#"{"video": {"width": 10, "height": 10}, "scenes": [{"duration": 1.0, "children": []}]}"#,
        )
        .unwrap();

        let top_path = scenario_dir.join("top.json");
        let top_body = serde_json::json!({
            "video": {"width": 10, "height": 10},
            "scenes": [{"include": "../witness.json"}]
        });
        std::fs::write(&top_path, serde_json::to_string(&top_body).unwrap()).unwrap();

        let scenario = scenario_from_file(&top_path);
        let result = resolve_includes(scenario, &IncludeSource::File(top_path.clone()));

        assert!(
            result.is_err(),
            "a '..' include path escaping the scenario's own directory must be a named, \
             refused error, not a silent read — got {:?}",
            result.map(|r| r.all_scenes().count())
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn local_include_within_the_scenario_directory_still_resolves() {
        let base = scratch_dir("include-legit");
        let scenario_dir = base.join("project");
        let sub_dir = scenario_dir.join("shared");
        std::fs::create_dir_all(&sub_dir).unwrap();

        let included_path = sub_dir.join("header.json");
        std::fs::write(
            &included_path,
            r#"{"video": {"width": 10, "height": 10}, "scenes": [{"duration": 1.0, "children": []}]}"#,
        )
        .unwrap();

        let top_path = scenario_dir.join("top.json");
        let top_body = serde_json::json!({
            "video": {"width": 10, "height": 10},
            "scenes": [{"include": "shared/header.json"}]
        });
        std::fs::write(&top_path, serde_json::to_string(&top_body).unwrap()).unwrap();

        let scenario = scenario_from_file(&top_path);
        let result = resolve_includes(scenario, &IncludeSource::File(top_path.clone()));

        let resolved = result.expect("an include within the scenario's own directory must resolve");
        assert_eq!(resolved.all_scenes().count(), 1);

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_for_each_bound_src_inside_an_included_file_is_still_rebased_against_that_file() {
        let dir = scratch_dir("foreach-src-rebase");
        let child_dir = dir.join("child");
        std::fs::create_dir_all(&child_dir).unwrap();
        std::fs::write(child_dir.join("logo.png"), b"x").unwrap();

        let child = serde_json::json!({
            "video": { "width": 10, "height": 10 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "for-each": [ { "path": "logo.png" } ],
                    "template": { "type": "image", "src": "$path" }
                }]
            }]
        });
        std::fs::write(child_dir.join("child.json"), child.to_string()).unwrap();

        let top_path = dir.join("top.json");
        let top_body = serde_json::json!({
            "video": { "width": 10, "height": 10 },
            "scenes": [ { "include": "child/child.json" } ]
        });
        std::fs::write(&top_path, top_body.to_string()).unwrap();

        let scenario = scenario_from_file(&top_path);
        let resolved = resolve_includes(scenario, &IncludeSource::File(top_path.clone()))
            .expect("include resolves");

        let src = resolved.views[0].scenes[0].children[0]["src"]
            .as_str()
            .expect("src")
            .to_string();
        assert!(
            Path::new(&src).is_absolute() && Path::new(&src).is_file(),
            "a for-each-bound src inside an included file must still be rebased against that \
             file's own directory, got: {src}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_repeated_include_scene_index_produces_one_scene_per_occurrence() {
        let dir = scratch_dir("include-repeat-index");
        std::fs::create_dir_all(&dir).unwrap();

        let child = serde_json::json!({
            "video": { "width": 10, "height": 10 },
            "scenes": [
                { "duration": 1.0, "children": [] },
                { "duration": 2.0, "children": [] }
            ]
        });
        let child_path = dir.join("child.json");
        std::fs::write(&child_path, child.to_string()).unwrap();

        let top_path = dir.join("top.json");
        let top_body = serde_json::json!({
            "video": { "width": 10, "height": 10 },
            "scenes": [ { "include": "child.json", "scenes": [0, 0, 1] } ]
        });
        std::fs::write(&top_path, top_body.to_string()).unwrap();

        let scenario = scenario_from_file(&top_path);
        let resolved = resolve_includes(scenario, &IncludeSource::File(top_path.clone()))
            .expect("include resolves");

        assert_eq!(
            resolved.all_scenes().count(),
            3,
            "'scenes': [0, 0, 1] must produce 3 scenes (the repeated index reused twice), not \
             silently drop the second occurrence"
        );
        let durations: Vec<f64> = resolved.all_scenes().map(|s| s.duration).collect();
        assert_eq!(durations, vec![1.0, 1.0, 2.0]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn include_fan_out_across_depth_is_bounded() {
        let dir = scratch_dir("fanout");
        std::fs::create_dir_all(&dir).unwrap();

        let leaf_path = dir.join("leaf.json");
        std::fs::write(
            &leaf_path,
            r#"{"video": {"width": 10, "height": 10}, "scenes": [{"duration": 1.0, "children": []}]}"#,
        )
        .unwrap();

        const BRANCHING: usize = 10;
        let mut current = "leaf.json".to_string();
        for level in 1..=4 {
            let includes: Vec<serde_json::Value> = (0..BRANCHING)
                .map(|_| serde_json::json!({ "include": current.clone() }))
                .collect();
            let body = serde_json::json!({
                "video": {"width": 10, "height": 10},
                "scenes": includes
            });
            let file_name = format!("level{level}.json");
            std::fs::write(dir.join(&file_name), serde_json::to_string(&body).unwrap()).unwrap();
            current = file_name;
        }

        let top_path = dir.join("top.json");
        let top_body = serde_json::json!({
            "video": {"width": 10, "height": 10},
            "scenes": [{"include": current}]
        });
        std::fs::write(&top_path, serde_json::to_string(&top_body).unwrap()).unwrap();

        let scenario = scenario_from_file(&top_path);
        let result = resolve_includes(scenario, &IncludeSource::File(top_path.clone()));

        assert!(
            result.is_err(),
            "10^4 fan-out through 4 include levels (all within MAX_INCLUDE_DEPTH) must be \
             refused by a total-expansion bound — got {:?} scenes",
            result.map(|r| r.all_scenes().count())
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
