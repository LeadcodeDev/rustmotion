use rustmotion::components::{ChildComponent, Component};
use rustmotion::error::Result;
use rustmotion::include;
use rustmotion::schema::{CardDisplay, ResolvedScenario, Scenario};
use rustmotion::variables;
use std::path::PathBuf;

pub fn cmd_validate(input: &PathBuf) -> Result<()> {
    use rustmotion::error::RustmotionError;

    let json_str = std::fs::read_to_string(input)
        .map_err(|e| RustmotionError::FileRead { path: input.display().to_string(), source: e })?;

    // Parse as raw Value first for variable processing
    let json_value: std::result::Result<serde_json::Value, _> = serde_json::from_str(&json_str);
    let mut json_value = match json_value {
        Ok(v) => v,
        Err(e) => {
            eprintln!("JSON parse error: {}", e);
            std::process::exit(1);
        }
    };

    // Apply variable defaults
    if let Err(e) = variables::apply_defaults(&mut json_value) {
        eprintln!("Variable error: {}", e);
        std::process::exit(1);
    }

    // Check for unresolved variable references
    let unresolved = variables::find_unresolved(&json_value);
    if !unresolved.is_empty() {
        for name in &unresolved {
            eprintln!(
                "Warning: unresolved variable reference '${}' in '{}'",
                name,
                input.display()
            );
        }
    }

    // Parse JSON into Scenario
    let scenario: std::result::Result<Scenario, _> = serde_json::from_value(json_value);

    match scenario {
        Ok(scenario) => {
            // Resolve includes before validating
            let resolved = include::resolve_includes(
                scenario,
                &include::IncludeSource::File(input.clone()),
            );
            match resolved {
                Ok(resolved) => {
                    let errors = validate_scenario(&resolved);
                    let all_scenes: Vec<_> = resolved.all_scenes().collect();
                    if errors.is_empty() {
                        eprintln!("Valid scenario: {} scene(s) in {} view(s)", all_scenes.len(), resolved.views.len());
                        let total_duration: f64 =
                            all_scenes.iter().map(|s| s.duration).sum();
                        eprintln!(
                            "  Resolution: {}x{} @ {}fps",
                            resolved.video.width, resolved.video.height, resolved.video.fps
                        );
                        eprintln!("  Duration: {:.1}s", total_duration);
                        Ok(())
                    } else {
                        for err in &errors {
                            eprintln!("Error: {}", err);
                        }
                        std::process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("Include resolution error: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("JSON parse error: {}", e);
            std::process::exit(1);
        }
    }
}

fn validate_children(
    children: &[ChildComponent],
    path: &str,
    errors: &mut Vec<String>,
) {
    for (j, child) in children.iter().enumerate() {
        let p = format!("{}.children[{}]", path, j);

        // Timing validation (common to all timed components)
        if let Some(timed) = child.component.as_timed() {
            let (start, end) = timed.timing();
            if let (Some(s), Some(e)) = (start, end) {
                if s >= e {
                    errors.push(format!("{}: start_at ({}) must be < end_at ({})", p, s, e));
                }
            }
        }

        // Component-specific validation
        match &child.component {
            Component::Image(img) => {
                if !std::path::Path::new(&img.src).exists() {
                    errors.push(format!("{}.src: file not found '{}'", p, img.src));
                }
            }
            Component::Video(v) => {
                if !std::path::Path::new(&v.src).exists() {
                    errors.push(format!("{}.src: file not found '{}'", p, v.src));
                }
            }
            Component::Gif(g) => {
                if !std::path::Path::new(&g.src).exists() {
                    errors.push(format!("{}.src: file not found '{}'", p, g.src));
                }
            }
            Component::Svg(svg) => {
                if svg.src.is_none() && svg.data.is_none() {
                    errors.push(format!("{}: SVG must have 'src' or 'data'", p));
                }
                if let Some(ref src) = svg.src {
                    if !std::path::Path::new(src).exists() {
                        errors.push(format!("{}.src: file not found '{}'", p, src));
                    }
                }
            }
            Component::Icon(icon) => {
                if let Some((prefix, name)) = icon.icon.split_once(':') {
                    if prefix.is_empty() || name.is_empty() {
                        errors.push(format!(
                            "{}: icon '{}' has empty prefix or name (expected 'prefix:name')",
                            p, icon.icon
                        ));
                    }
                } else {
                    errors.push(format!(
                        "{}: invalid icon format '{}' (expected 'prefix:name', e.g. 'lucide:home')",
                        p, icon.icon
                    ));
                }
            }
            Component::QrCode(qr) => {
                if qr.content.is_empty() {
                    errors.push(format!("{}: QR code content must not be empty", p));
                }
            }
            Component::Mockup(m) => {
                if !std::path::Path::new(&m.src).exists() {
                    errors.push(format!("{}.src: file not found '{}'", p, m.src));
                }
            }
            Component::Card(card) => {
                if matches!(card.style.display, Some(CardDisplay::Grid))
                    && card.style.grid_template_columns.is_none()
                {
                    errors.push(format!("{}: grid display without grid-template-columns", p));
                }
                validate_children(&card.children, &p, errors);
            }
            Component::Flex(flex) => {
                validate_children(&flex.children, &p, errors);
            }
            Component::Grid(grid) => {
                if grid.style.grid_template_columns.is_none() {
                    errors.push(format!("{}: grid without grid-template-columns", p));
                }
                validate_children(&grid.children, &p, errors);
            }
            Component::Positioned(pos) => {
                validate_children(&pos.children, &p, errors);
            }
            Component::Container(container) => {
                validate_children(&container.children, &p, errors);
            }
            _ => {}
        }
    }
}

fn validate_scenario(scenario: &ResolvedScenario) -> Vec<String> {
    let mut errors = Vec::new();

    if scenario.video.width == 0 || scenario.video.height == 0 {
        errors.push("video.width and video.height must be > 0".to_string());
    }
    if scenario.video.width % 2 != 0 || scenario.video.height % 2 != 0 {
        errors.push("video.width and video.height must be even (required for H.264)".to_string());
    }
    if scenario.video.fps == 0 {
        errors.push("video.fps must be > 0".to_string());
    }

    let all_scenes: Vec<_> = scenario.all_scenes().collect();
    if all_scenes.is_empty() {
        errors.push("At least one scene is required".to_string());
    }

    for (vi, view) in scenario.views.iter().enumerate() {
        for (si, scene) in view.scenes.iter().enumerate() {
            if scene.duration <= 0.0 {
                errors.push(format!("views[{}].scenes[{}].duration must be > 0", vi, si));
            }

            let children = rustmotion::engine::render::deserialize_children(scene);
            validate_children(&children, &format!("views[{}].scenes[{}]", vi, si), &mut errors);
        }
    }

    for (i, audio) in scenario.audio.iter().enumerate() {
        if !std::path::Path::new(&audio.src).exists() {
            errors.push(format!(
                "audio[{}].src: file not found '{}'",
                i, audio.src
            ));
        }
    }

    errors
}
