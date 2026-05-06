//! Schema-level scenario checks (file existence, dimensions, durations, etc.).
//! Returns (errors, warnings); errors block rendering, warnings are advisory only.

use rustmotion::components::{ChildComponent, Component};
use rustmotion::core::css::style::Display as CssDisplay;
use rustmotion::schema::{AnimationEffect, CharAnimationTiming, ResolvedScenario};

pub fn validate_scenario(scenario: &ResolvedScenario) -> (Vec<String>, Vec<String>) {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

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
                scene.duration,
                &mut errors,
                &mut warnings,
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

    (errors, warnings)
}

fn validate_children(
    children: &[ChildComponent],
    path: &str,
    scene_duration: f64,
    errors: &mut Vec<String>,
    warnings: &mut Vec<String>,
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

        // Animation completion budget check: ensure entrance animations finish within the scene.
        if let Some(anim) = child.component.as_animatable() {
            let start_at = child.component.as_timed()
                .and_then(|t| t.timing().0)
                .unwrap_or(0.0);

            for effect in anim.animation_effects() {
                if let Some((delay, duration)) = entrance_budget(effect) {
                    let finishes_at = start_at + delay + duration;
                    // 50ms tolerance for floating-point edge cases.
                    if finishes_at > scene_duration + 0.05 {
                        let suggested = ((finishes_at + 0.5) * 10.0).ceil() / 10.0;
                        errors.push(format!(
                            "{}: animation finishes at {:.2}s (start_at {:.2} + delay {:.2} + duration {:.2}) \
                             but scene_duration is {:.2}s — it will be truncated. \
                             Increase scene duration to at least {:.1}s or reduce animation delay/duration.",
                            p, finishes_at, start_at, delay, duration, scene_duration, suggested
                        ));
                    }
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
            Component::Counter(c) => {
                let from_len = counter_display_len(c.from, c.decimals, &c.separator, &c.prefix, &c.suffix);
                let to_len = counter_display_len(c.to, c.decimals, &c.separator, &c.prefix, &c.suffix);
                if from_len != to_len {
                    warnings.push(format!(
                        "{}: counter from={} to={} — display width changes from {} to {} chars. \
                         Ensure the parent container is at least wide enough for the {}-char display.",
                        p, c.from, c.to, from_len, to_len, from_len.max(to_len)
                    ));
                }
            }
            Component::Card(card) => {
                if matches!(card.style.display, Some(CssDisplay::Grid))
                    && card.style.grid_template_columns.is_none()
                {
                    errors.push(format!("{}: grid display without grid-template-columns", p));
                }
                validate_children(&card.children, &p, scene_duration, errors, warnings);
            }
            Component::Flex(flex) => {
                validate_children(&flex.children, &p, scene_duration, errors, warnings);
            }
            Component::Grid(grid) => {
                if grid.style.grid_template_columns.is_none() {
                    errors.push(format!("{}: grid without grid-template-columns", p));
                }
                validate_children(&grid.children, &p, scene_duration, errors, warnings);
            }
            Component::Positioned(pos) => {
                validate_children(&pos.children, &p, scene_duration, errors, warnings);
            }
            Component::Container(container) => {
                validate_children(&container.children, &p, scene_duration, errors, warnings);
            }
            _ => {}
        }
    }
}

/// Returns (delay, duration) for animations that have a completion budget,
/// or None for exit presets, looped animations, and non-timing effects.
fn entrance_budget(effect: &AnimationEffect) -> Option<(f64, f64)> {
    match effect {
        // Exit presets intentionally overlap with scene end — skip.
        AnimationEffect::FadeOut(_)
        | AnimationEffect::FadeOutUp(_)
        | AnimationEffect::FadeOutDown(_)
        | AnimationEffect::SlideOutLeft(_)
        | AnimationEffect::SlideOutRight(_)
        | AnimationEffect::SlideOutUp(_)
        | AnimationEffect::SlideOutDown(_)
        | AnimationEffect::ScaleOut(_)
        | AnimationEffect::BounceOut(_)
        | AnimationEffect::BlurOut(_)
        | AnimationEffect::RotateOut(_)
        | AnimationEffect::FlipOutX(_)
        | AnimationEffect::FlipOutY(_) => None,

        // All other presets with AnimationTiming: check if looped.
        AnimationEffect::FadeIn(t)
        | AnimationEffect::FadeInUp(t)
        | AnimationEffect::FadeInDown(t)
        | AnimationEffect::FadeInLeft(t)
        | AnimationEffect::FadeInRight(t)
        | AnimationEffect::SlideInLeft(t)
        | AnimationEffect::SlideInRight(t)
        | AnimationEffect::SlideInUp(t)
        | AnimationEffect::SlideInDown(t)
        | AnimationEffect::ScaleIn(t)
        | AnimationEffect::BounceIn(t)
        | AnimationEffect::BlurIn(t)
        | AnimationEffect::RotateIn(t)
        | AnimationEffect::ElasticIn(t)
        | AnimationEffect::Pulse(t)
        | AnimationEffect::Float(t)
        | AnimationEffect::Shake(t)
        | AnimationEffect::Spin(t)
        | AnimationEffect::FlipInX(t)
        | AnimationEffect::FlipInY(t)
        | AnimationEffect::DrawIn(t)
        | AnimationEffect::StrokeReveal(t)
        | AnimationEffect::Typewriter(t)
        | AnimationEffect::WipeLeft(t)
        | AnimationEffect::WipeRight(t)
        | AnimationEffect::Float3d(t) => {
            if t.repeat {
                None
            } else {
                Some((t.delay, t.duration))
            }
        }

        AnimationEffect::TiltIn(t) => Some((t.delay, t.duration)),

        // Char animations: check delay + duration (conservative — stagger not factored since
        // char count is unknown at validation time).
        AnimationEffect::CharScaleIn(t)
        | AnimationEffect::CharFadeIn(t)
        | AnimationEffect::CharWave(t)
        | AnimationEffect::CharBounce(t)
        | AnimationEffect::CharRotateIn(t)
        | AnimationEffect::CharSlideUp(t) => char_budget(t),

        // Custom keyframes
        AnimationEffect::Keyframes(k) => {
            if k.repeat {
                None
            } else {
                Some((k.delay, k.duration))
            }
        }

        // Non-timing effects: continuous by nature, no completion budget.
        AnimationEffect::Glow(_)
        | AnimationEffect::Wiggle(_)
        | AnimationEffect::Orbit(_)
        | AnimationEffect::MotionBlur(_) => None,
    }
}

fn char_budget(t: &CharAnimationTiming) -> Option<(f64, f64)> {
    Some((t.delay, t.duration))
}

/// Estimate display character count for a counter value including prefix/suffix/separators.
fn counter_display_len(
    value: f64,
    decimals: u8,
    separator: &Option<String>,
    prefix: &Option<String>,
    suffix: &Option<String>,
) -> usize {
    let abs_val = value.abs();
    let integer_digits = if abs_val < 1.0 {
        1
    } else {
        abs_val.floor().log10().floor() as usize + 1
    };

    let separator_chars = if separator.is_some() {
        integer_digits.saturating_sub(1) / 3
    } else {
        0
    };

    let sign = if value < 0.0 { 1 } else { 0 };
    let decimal_chars = if decimals > 0 { 1 + decimals as usize } else { 0 };
    let prefix_len = prefix.as_deref().map(str::len).unwrap_or(0);
    let suffix_len = suffix.as_deref().map(str::len).unwrap_or(0);

    sign + integer_digits + separator_chars + decimal_chars + prefix_len + suffix_len
}
