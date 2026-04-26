//! Schema-level scenario checks (file existence, dimensions, durations, etc.).
//! Returns a list of human-readable error messages; an empty list means clean.

use rustmotion::components::{ChildComponent, Component};
use rustmotion::schema::{CardDisplay, ResolvedScenario};

pub fn validate_scenario(scenario: &ResolvedScenario) -> Vec<String> {
    let mut errors = Vec::new();

    if scenario.video.width == 0 || scenario.video.height == 0 {
        errors.push("video.width and video.height must be > 0".to_string());
    }
    if scenario.video.width % 2 != 0 || scenario.video.height % 2 != 0 {
        errors.push(
            "video.width and video.height must be even (required for H.264)".to_string(),
        );
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
                errors.push(format!(
                    "views[{}].scenes[{}].duration must be > 0",
                    vi, si
                ));
            }

            let children = rustmotion::engine::render::deserialize_children(scene);
            validate_children(
                &children,
                &format!("views[{}].scenes[{}]", vi, si),
                &mut errors,
            );
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

fn validate_children(
    children: &[ChildComponent],
    path: &str,
    errors: &mut Vec<String>,
) {
    for (j, child) in children.iter().enumerate() {
        let p = format!("{}.children[{}]", path, j);

        if let Some(timed) = child.component.as_timed() {
            let (start, end) = timed.timing();
            if let (Some(s), Some(e)) = (start, end) {
                if s >= e {
                    errors.push(format!(
                        "{}: start_at ({}) must be < end_at ({})",
                        p, s, e
                    ));
                }
            }
        }

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
