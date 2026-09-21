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
                    });
                }
                let scenes = fetch_and_resolve(
                    &directive,
                    source,
                    depth + 1,
                    audio,
                    included_paths,
                    remote_policy,
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
) -> Result<Vec<Scene>> {
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
        let path = resolve_local_path(&directive.include, parent_source)?;
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

    // An included file's assets are relative to *that* file, not to the parent
    // that pulled it in — otherwise moving an include would silently break
    // every path inside it.
    if let IncludeSource::File(ref p) = child_source {
        if let Some(dir) = p.parent() {
            crate::assets::rebase_relative_paths(&mut json_value, dir);
        }
    }
    // `components` (and any `for-each`/`use` inside this file's own scenes)
    // is scoped to this document: expanded here, per included file, using
    // ONLY this file's own `components` block — never the parent's, and
    // never visible to the parent's own `use` sites. See
    // `rustmotion_core::expand`'s module doc for why that scoping was
    // chosen over a cross-file component registry.
    crate::expand::expand_directives(&mut json_value, &directive.include)?;

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
            let parent_dir = parent_path.parent().unwrap_or_else(|| Path::new("."));
            Ok(parent_dir.join(relative))
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
