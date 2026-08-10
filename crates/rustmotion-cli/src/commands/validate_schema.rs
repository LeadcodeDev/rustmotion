//! Schema-level scenario checks (file existence, dimensions, durations, etc.).
//! Returns (errors, warnings); errors block rendering, warnings are advisory only.

use rustmotion::components::{ChildComponent, Component};
use rustmotion::core::css::style::{
    Background, BackgroundLayer, Color, CssStyle, Display as CssDisplay,
};
use rustmotion::schema::{AnimationEffect, CharAnimationTiming, ResolvedScenario, SpringConfig};

pub fn validate_scenario(scenario: &ResolvedScenario) -> (Vec<String>, Vec<String>) {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    if scenario.video.width == 0 || scenario.video.height == 0 {
        errors.push("video.width and video.height must be > 0".to_string());
    }
    if !scenario.video.width.is_multiple_of(2) || !scenario.video.height.is_multiple_of(2) {
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
            errors.push(format!("audio[{}].src: file not found '{}'", i, audio.src));
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

        // Properties the CSS engine accepts but does not render yet — warn
        // instead of staying silent so authors don't rely on a no-op.
        let style = child.component.as_styled().style_config();
        if style.overflow_wrap.is_some() {
            warnings.push(format!(
                "{}: style.overflow-wrap is accepted but not rendered yet (text wraps per style.wrap)",
                p
            ));
        }
        if style.text_overflow.is_some() {
            warnings.push(format!(
                "{}: style.text-overflow is accepted but not rendered yet (no ellipsis clipping)",
                p
            ));
        }
        if style.backdrop_blur.is_some() {
            warnings.push(format!(
                "{}: style.backdrop-blur is accepted but not rendered — use \
                 backdrop-filter: [{{\"fn\": \"blur\", \"radius\": N}}]",
                p
            ));
        }
        if style.inner_shadow.is_some() {
            warnings.push(format!(
                "{}: style.inner-shadow is accepted but not rendered — use \
                 box-shadow with \"inset\": true",
                p
            ));
        }

        // C2 completion (issue #110 / #102): a colour that `parse_css_color`
        // can't resolve used to fall back to black silently; wave 1 made
        // that loud at render time (opaque magenta + stderr warning) via the
        // same frozen `parse_css_color` entry point. This closes the loop by
        // catching it before anyone renders.
        check_style_colors(style, &p, errors);

        if let Some(timed) = child.component.as_timed() {
            let (start, end) = timed.timing();
            if let (Some(s), Some(e)) = (start, end) {
                if s >= e {
                    errors.push(format!("{}: start_at ({}) must be < end_at ({})", p, s, e));
                }
            }
        }

        // Container time remapping: time_scale must be strictly positive
        // (0 would freeze the subtree, negative would run it backwards —
        // neither is supported; the builder clamps defensively but the
        // author must be told).
        if let Some(scale) = container_time_scale(&child.component) {
            if scale <= 0.0 {
                errors.push(format!(
                    "{}: time_scale must be > 0 (got {}). Use a small positive value \
                     (e.g. 0.1) to slow the subtree down.",
                    p, scale
                ));
            }
        }

        // Animation completion budget check: ensure entrance animations finish within the scene.
        //
        // Constat #4: `start_at` is a *visibility* window, not a time
        // origin — the engine resolves `animation.delay`/`duration` in
        // absolute scene time regardless of `start_at` (PR #27's frozen
        // semantics; `geometry.rs`'s `walk_anim` already states and relies
        // on the same rule). The budget used to add `start_at` in here,
        // which contradicts that: a scenario where the entrance genuinely
        // finishes well inside the scene (just before the node becomes
        // visible, so it appears already-settled) was rejected as if the
        // animation ran late.
        if let Some(anim) = child.component.as_animatable() {
            for effect in anim.animation_effects() {
                if let Some((delay, duration)) = entrance_budget(effect) {
                    let finishes_at = delay + duration;
                    // 50ms tolerance for floating-point edge cases.
                    if finishes_at > scene_duration + 0.05 {
                        let suggested = ((finishes_at + 0.5) * 10.0).ceil() / 10.0;
                        errors.push(format!(
                            "{}: animation finishes at {:.2}s (delay {:.2} + duration {:.2}) \
                             but scene_duration is {:.2}s — it will be truncated. \
                             Increase scene duration to at least {:.1}s or reduce animation delay/duration.",
                            p, finishes_at, delay, duration, scene_duration, suggested
                        ));
                    }
                }

                // Constat #6: `SpringConfig` accepts any f64 unchecked — a
                // preset's own `spring` override (`AnimationTiming::spring`,
                // reachable via `as_preset()`) or a `keyframes` effect's
                // per-`Animation` `spring` (used when that segment's easing
                // is `spring`) both feed `engine::animator::spring_value`,
                // where `mass <= 0`/`stiffness <= 0` produce NaN and negative
                // `damping` diverges. Reject both regimes here so a bad
                // config never reaches the solver.
                if let Some((_, timing)) = effect.as_preset() {
                    if let Some(spring) = &timing.spring {
                        check_spring_config(spring, &p, errors);
                    }
                }
                if let AnimationEffect::Keyframes(k) = effect {
                    for kf_anim in &k.keyframes {
                        if let Some(spring) = &kf_anim.spring {
                            check_spring_config(spring, &p, errors);
                        }
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
                let from_len =
                    counter_display_len(c.from, c.decimals, &c.separator, &c.prefix, &c.suffix);
                let to_len =
                    counter_display_len(c.to, c.decimals, &c.separator, &c.prefix, &c.suffix);
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

/// C2 completion: walk every colour reachable from a component's `style`
/// (foreground `color`, `background` fills/gradient stops, box/text shadow
/// colours, border colours, gradient-border colours) and report any
/// `Color::String` that `parse_css_color` — the single frozen colour-parsing
/// entry point (`engine/renderer/colors.rs`) — cannot resolve. At render
/// time an unresolved colour already falls back to opaque magenta with a
/// stderr warning (wave 1); this makes the same failure a blocking
/// validation error so it's caught before anyone renders.
///
/// Scope: only `CssStyle`'s `Color`-typed fields, which is exactly the
/// surface `paint_pass::parse_color` resolves through `parse_css_color` for
/// every component. Component-specific plain-`String` colour fields (e.g.
/// `Kbd.background_color`, `Table.header_color`, `Marquee.color`,
/// `Notification.accent_color`) go through a different, more lenient path
/// (`parse_hex_color`'s bare-hex retry) and are deliberately not covered
/// here — see the workstream report for the full list.
fn check_style_colors(style: &CssStyle, path: &str, errors: &mut Vec<String>) {
    if let Some(c) = &style.color {
        check_color(c, "color", path, errors);
    }
    if let Some(bg) = &style.background {
        check_background_colors(bg, path, errors);
    }
    if let Some(shadows) = &style.box_shadow {
        for (i, s) in shadows.iter().enumerate() {
            if let Some(c) = &s.color {
                check_color(c, &format!("box-shadow[{i}].color"), path, errors);
            }
        }
    }
    if let Some(shadows) = &style.text_shadow {
        for (i, s) in shadows.iter().enumerate() {
            if let Some(c) = &s.color {
                check_color(c, &format!("text-shadow[{i}].color"), path, errors);
            }
        }
    }
    if let Some(border) = &style.border {
        if let Some(c) = &border.color {
            check_color(c, "border.color", path, errors);
        }
        for (name, side) in [
            ("top", &border.top),
            ("right", &border.right),
            ("bottom", &border.bottom),
            ("left", &border.left),
        ] {
            if let Some(side) = side {
                if let Some(c) = &side.color {
                    check_color(c, &format!("border.{name}.color"), path, errors);
                }
            }
        }
    }
    if let Some(gb) = &style.gradient_border {
        for (i, s) in gb.colors.iter().enumerate() {
            check_color_str(s, &format!("gradient-border.colors[{i}]"), path, errors);
        }
    }
}

fn check_background_colors(bg: &Background, path: &str, errors: &mut Vec<String>) {
    match bg {
        Background::Color(c) => check_color(c, "background", path, errors),
        Background::Single(layer) => check_background_layer_colors(layer, path, errors),
        Background::Layers(layers) => {
            for layer in layers {
                check_background_layer_colors(layer, path, errors);
            }
        }
    }
}

fn check_background_layer_colors(layer: &BackgroundLayer, path: &str, errors: &mut Vec<String>) {
    match layer {
        BackgroundLayer::Color { color } => check_color(color, "background", path, errors),
        BackgroundLayer::LinearGradient { stops, .. }
        | BackgroundLayer::RadialGradient { stops, .. }
        | BackgroundLayer::ConicGradient { stops, .. } => {
            for (i, stop) in stops.iter().enumerate() {
                check_color(
                    &stop.color,
                    &format!("background gradient stop[{i}]"),
                    path,
                    errors,
                );
            }
        }
        BackgroundLayer::Image { .. } => {}
    }
}

fn check_color(color: &Color, label: &str, path: &str, errors: &mut Vec<String>) {
    if let Color::String(s) = color {
        check_color_str(s, label, path, errors);
    }
}

fn check_color_str(s: &str, label: &str, path: &str, errors: &mut Vec<String>) {
    if rustmotion::engine::renderer::parse_css_color(s).is_none() {
        errors.push(format!(
            "{path}: {label} '{s}' is not a recognized CSS color (expected hex #rgb/#rrggbb[aa], \
             rgb()/rgba(...), hsl()/hsla(...), or a CSS named color) — it would render as opaque \
             magenta instead of {label}",
        ));
    }
}

/// Constat #6: reject `SpringConfig` values that would make
/// `engine::animator::spring_value` produce NaN (`mass <= 0`, `stiffness <=
/// 0`) or diverge instead of settle (`damping < 0`). The solver itself also
/// floors these defensively (belt and suspenders — see `spring_value`'s doc
/// comment), but catching it here gives the author an actionable error
/// instead of a silently broken render.
///
/// Issue #167 lot E adds `duration`/`rest_threshold`: a non-positive
/// `duration` would make `spring_value`'s remap divide by zero or invert
/// time (both silently ignored by the solver rather than rejected — see its
/// `Some(duration) if duration > 0.0` guard), and a `rest_threshold` outside
/// `(0.0, 1.0)` is either meaningless (<=0: never satisfied except in the
/// limit) or vacuous (>=1.0: satisfied from t=0, before the spring has
/// moved at all — the whole 0→1 travel is "close enough").
fn check_spring_config(spring: &SpringConfig, path: &str, errors: &mut Vec<String>) {
    if spring.mass <= 0.0 {
        errors.push(format!(
            "{path}: spring.mass must be > 0 (got {}) — zero or negative mass makes the spring \
             solver divide by zero and produce NaN",
            spring.mass
        ));
    }
    if spring.stiffness <= 0.0 {
        errors.push(format!(
            "{path}: spring.stiffness must be > 0 (got {}) — zero or negative stiffness makes \
             the spring solver produce NaN",
            spring.stiffness
        ));
    }
    if spring.damping < 0.0 {
        errors.push(format!(
            "{path}: spring.damping must be >= 0 (got {}) — negative damping makes the spring \
             diverge instead of settle",
            spring.damping
        ));
    }
    if let Some(duration) = spring.duration {
        if duration <= 0.0 {
            errors.push(format!(
                "{path}: spring.duration must be > 0 when set (got {duration}) — a zero or \
                 negative duration cannot be mapped to a settle time"
            ));
        }
    }
    if let Some(rest_threshold) = spring.rest_threshold {
        if rest_threshold <= 0.0 {
            errors.push(format!(
                "{path}: spring.rest_threshold must be > 0 when set (got {rest_threshold}) — a \
                 zero or negative threshold is never satisfied, so the spring would never be \
                 considered at rest"
            ));
        } else if rest_threshold >= 1.0 {
            errors.push(format!(
                "{path}: spring.rest_threshold must be < 1.0 when set (got {rest_threshold}) — \
                 a threshold this large is satisfied at t=0, before the spring has moved"
            ));
        }
    }
}

/// The `time_scale` declared on a container component, if any.
fn container_time_scale(component: &Component) -> Option<f64> {
    match component {
        Component::Card(c) => c.time_scale,
        Component::Flex(c) => c.time_scale,
        Component::Grid(c) => c.time_scale,
        Component::Container(c) => c.time_scale,
        Component::Positioned(c) => c.time_scale,
        _ => None,
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
        | AnimationEffect::CharSlideUp(t)
        | AnimationEffect::CharBlurIn(t) => char_budget(t),

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
        | AnimationEffect::MotionBlur(_)
        | AnimationEffect::Trail(_) => None,
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
    let decimal_chars = if decimals > 0 {
        1 + decimals as usize
    } else {
        0
    };
    let prefix_len = prefix.as_deref().map(str::len).unwrap_or(0);
    let suffix_len = suffix.as_deref().map(str::len).unwrap_or(0);

    sign + integer_digits + separator_chars + decimal_chars + prefix_len + suffix_len
}

#[cfg(test)]
mod style_warning_tests {
    use super::*;

    #[test]
    fn warns_on_accepted_but_unrendered_css_properties() {
        // overflow-wrap and text-overflow parse into CssStyle but are not
        // rendered yet; validate must say so instead of staying silent.
        let child: ChildComponent = serde_json::from_value(serde_json::json!({
            "type": "text",
            "content": "hi",
            "style": { "overflow-wrap": "break-word", "text-overflow": "ellipsis" }
        }))
        .unwrap();
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        validate_children(&[child], "test", 4.0, &mut errors, &mut warnings);
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        assert!(
            warnings.iter().any(|w| w.contains("overflow-wrap")),
            "missing overflow-wrap warning: {warnings:?}"
        );
        assert!(
            warnings.iter().any(|w| w.contains("text-overflow")),
            "missing text-overflow warning: {warnings:?}"
        );
    }

    #[test]
    fn warns_on_legacy_backdrop_blur_and_inner_shadow() {
        // Legacy glassmorphism fields are accepted for compat but never
        // rendered; validate must point at the working CSS equivalents.
        let child: ChildComponent = serde_json::from_value(serde_json::json!({
            "type": "card",
            "style": {
                "backdrop-blur": 20,
                "inner-shadow": { "color": "#000000", "offset_y": 2, "blur": 8 }
            }
        }))
        .unwrap();
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        validate_children(&[child], "test", 4.0, &mut errors, &mut warnings);
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("backdrop-blur") && w.contains("backdrop-filter")),
            "missing backdrop-blur warning: {warnings:?}"
        );
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("inner-shadow") && w.contains("inset")),
            "missing inner-shadow warning: {warnings:?}"
        );
    }

    #[test]
    fn time_scale_zero_is_an_error() {
        let child: ChildComponent = serde_json::from_value(serde_json::json!({
            "type": "flex",
            "time_scale": 0.0,
            "children": [{ "type": "text", "content": "hi" }]
        }))
        .unwrap();
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        validate_children(&[child], "test", 4.0, &mut errors, &mut warnings);
        assert!(
            errors.iter().any(|e| e.contains("time_scale must be > 0")),
            "missing time_scale error: {errors:?}"
        );
    }

    #[test]
    fn negative_spring_damping_is_an_error() {
        // Constat #6: damping < 0 makes the spring solver diverge instead of
        // settle (a `SpringConfig` accepts any f64 — nothing in
        // `rustmotion-cli` checked `damping`/`stiffness` before this).
        let child: ChildComponent = serde_json::from_value(serde_json::json!({
            "type": "text",
            "content": "hi",
            "style": {
                "animation": [{ "name": "fade_in_up", "duration": 0.6, "spring": { "damping": -5, "stiffness": 100, "mass": 1 } }]
            }
        }))
        .unwrap();
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        validate_children(&[child], "test", 4.0, &mut errors, &mut warnings);
        assert!(
            errors.iter().any(|e| e.contains("damping")),
            "missing spring damping error: {errors:?}"
        );
    }

    #[test]
    fn zero_spring_stiffness_is_an_error() {
        // stiffness <= 0 makes `spring_value`'s omega = sqrt(stiffness/mass) NaN.
        let child: ChildComponent = serde_json::from_value(serde_json::json!({
            "type": "text",
            "content": "hi",
            "style": {
                "animation": [{ "name": "bounce_in", "duration": 0.6, "spring": { "damping": 10, "stiffness": 0, "mass": 1 } }]
            }
        }))
        .unwrap();
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        validate_children(&[child], "test", 4.0, &mut errors, &mut warnings);
        assert!(
            errors.iter().any(|e| e.contains("stiffness")),
            "missing spring stiffness error: {errors:?}"
        );
    }

    #[test]
    fn zero_spring_mass_is_an_error() {
        // mass <= 0 makes omega = sqrt(stiffness/mass) divide by zero -> NaN.
        let child: ChildComponent = serde_json::from_value(serde_json::json!({
            "type": "text",
            "content": "hi",
            "style": {
                "animation": [{ "name": "fade_in_up", "duration": 0.6, "spring": { "damping": 10, "stiffness": 100, "mass": 0 } }]
            }
        }))
        .unwrap();
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        validate_children(&[child], "test", 4.0, &mut errors, &mut warnings);
        assert!(
            errors.iter().any(|e| e.contains("mass")),
            "missing spring mass error: {errors:?}"
        );
    }

    #[test]
    fn spring_inside_a_keyframes_effect_is_also_checked() {
        // Springs aren't only on presets: a `keyframes` effect's per-Animation
        // `spring` field feeds the exact same solver.
        let child: ChildComponent = serde_json::from_value(serde_json::json!({
            "type": "text",
            "content": "hi",
            "style": {
                "animation": [{
                    "name": "keyframes",
                    "keyframes": [{
                        "property": "scale",
                        "easing": "spring",
                        "spring": { "damping": -1, "stiffness": 100, "mass": 1 },
                        "keyframes": [{ "time": 0.0, "value": 0.0 }, { "time": 1.0, "value": 1.0 }]
                    }]
                }]
            }
        }))
        .unwrap();
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        validate_children(&[child], "test", 4.0, &mut errors, &mut warnings);
        assert!(
            errors.iter().any(|e| e.contains("damping")),
            "missing spring damping error inside a keyframes effect: {errors:?}"
        );
    }

    #[test]
    fn positive_spring_values_are_accepted() {
        let child: ChildComponent = serde_json::from_value(serde_json::json!({
            "type": "text",
            "content": "hi",
            "style": {
                "animation": [{ "name": "fade_in_up", "duration": 0.6, "spring": { "damping": 15, "stiffness": 100, "mass": 1 } }]
            }
        }))
        .unwrap();
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        validate_children(&[child], "test", 4.0, &mut errors, &mut warnings);
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
    }

    // ---- issue #167 lot E: `spring.duration`/`spring.rest_threshold` ----

    #[test]
    fn zero_spring_duration_is_an_error() {
        let child: ChildComponent = serde_json::from_value(serde_json::json!({
            "type": "text",
            "content": "hi",
            "style": {
                "animation": [{ "name": "fade_in_up", "duration": 0.6, "spring": { "damping": 15, "stiffness": 100, "mass": 1, "duration": 0.0 } }]
            }
        }))
        .unwrap();
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        validate_children(&[child], "test", 4.0, &mut errors, &mut warnings);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("spring.duration") && e.contains("> 0")),
            "missing spring.duration error: {errors:?}"
        );
    }

    #[test]
    fn negative_spring_duration_is_an_error() {
        let child: ChildComponent = serde_json::from_value(serde_json::json!({
            "type": "text",
            "content": "hi",
            "style": {
                "animation": [{ "name": "bounce_in", "duration": 0.6, "spring": { "damping": 15, "stiffness": 100, "mass": 1, "duration": -0.5 } }]
            }
        }))
        .unwrap();
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        validate_children(&[child], "test", 4.0, &mut errors, &mut warnings);
        assert!(
            errors.iter().any(|e| e.contains("spring.duration")),
            "missing spring.duration error: {errors:?}"
        );
    }

    #[test]
    fn zero_or_negative_rest_threshold_is_an_error() {
        let child: ChildComponent = serde_json::from_value(serde_json::json!({
            "type": "text",
            "content": "hi",
            "style": {
                "animation": [{ "name": "fade_in_up", "duration": 0.6, "spring": { "damping": 15, "stiffness": 100, "mass": 1, "rest_threshold": -0.01 } }]
            }
        }))
        .unwrap();
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        validate_children(&[child], "test", 4.0, &mut errors, &mut warnings);
        assert!(
            errors.iter().any(|e| e.contains("spring.rest_threshold")),
            "missing spring.rest_threshold error: {errors:?}"
        );
    }

    #[test]
    fn absurdly_large_rest_threshold_is_an_error() {
        // >= 1.0 is satisfied at t=0, before the spring has moved at all —
        // "at rest" from the first frame is not a meaningful measurement.
        let child: ChildComponent = serde_json::from_value(serde_json::json!({
            "type": "text",
            "content": "hi",
            "style": {
                "animation": [{ "name": "fade_in_up", "duration": 0.6, "spring": { "damping": 15, "stiffness": 100, "mass": 1, "rest_threshold": 1.0 } }]
            }
        }))
        .unwrap();
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        validate_children(&[child], "test", 4.0, &mut errors, &mut warnings);
        assert!(
            errors.iter().any(|e| e.contains("spring.rest_threshold")),
            "missing spring.rest_threshold error: {errors:?}"
        );
    }

    #[test]
    fn positive_spring_duration_and_rest_threshold_are_accepted() {
        let child: ChildComponent = serde_json::from_value(serde_json::json!({
            "type": "text",
            "content": "hi",
            "style": {
                "animation": [{
                    "name": "fade_in_up",
                    "duration": 0.8,
                    "spring": { "damping": 15, "stiffness": 100, "mass": 1, "duration": 0.8, "rest_threshold": 0.01 }
                }]
            }
        }))
        .unwrap();
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        validate_children(&[child], "test", 4.0, &mut errors, &mut warnings);
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
    }

    #[test]
    fn positive_time_scale_is_accepted() {
        let child: ChildComponent = serde_json::from_value(serde_json::json!({
            "type": "flex",
            "time_scale": 0.5,
            "time_offset": 1.0,
            "children": [{ "type": "text", "content": "hi" }]
        }))
        .unwrap();
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        validate_children(&[child], "test", 4.0, &mut errors, &mut warnings);
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
    }

    #[test]
    fn completion_budget_does_not_add_start_at_to_delay_plus_duration() {
        // Constat #4: `start_at` gates *visibility* only (PR #27) — the
        // engine resolves `animation.delay`/`duration` in absolute scene
        // time regardless of `start_at`, so an entrance that finishes at
        // delay+duration=1.0s in a 2.0s scene is fine even if the node isn't
        // visible until start_at=1.5s (it simply appears already-settled).
        // The old formula added them (`start_at + delay + duration` =
        // 1.5+0+1.0 = 2.5 > 2.0), rejecting this valid scenario.
        let child: ChildComponent = serde_json::from_value(serde_json::json!({
            "type": "shape",
            "shape": "rect",
            "position": "absolute",
            "x": 100, "y": 100,
            "start_at": 1.5,
            "style": {
                "width": "100px", "height": "100px",
                "animation": [{ "name": "slide_in_left", "delay": 0.0, "duration": 1.0 }]
            },
            "fill": "#ff0000"
        }))
        .unwrap();
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        validate_children(&[child], "test", 2.0, &mut errors, &mut warnings);
        assert!(
            errors.iter().all(|e| !e.contains("animation finishes")),
            "start_at must not be added to the completion budget: {errors:?}"
        );
    }
}

/// C2 completion (issue #110 / #102): an unresolved colour must fail
/// validation, not just render as opaque magenta with a stderr warning.
#[cfg(test)]
mod color_validation_tests {
    use super::*;

    #[test]
    fn unresolvable_text_color_is_a_blocking_error() {
        let child: ChildComponent = serde_json::from_value(serde_json::json!({
            "type": "text",
            "content": "hi",
            "style": { "color": "not-a-real-color" }
        }))
        .unwrap();
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        validate_children(&[child], "test", 4.0, &mut errors, &mut warnings);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("not-a-real-color") && e.contains("color")),
            "expected an unresolved-color error: {errors:?}"
        );
    }

    #[test]
    fn valid_colors_of_every_recognized_form_do_not_error() {
        let child: ChildComponent = serde_json::from_value(serde_json::json!({
            "type": "text",
            "content": "hi",
            "style": {
                "color": "#fff",
                "background": "rgba(10, 20, 30, 0.5)",
                "box-shadow": [{ "offset-x": 0, "offset-y": 2, "color": "hsl(200, 50%, 50%)" }],
                "text-shadow": [{ "offset-x": 0, "offset-y": 1, "color": "cornflowerblue" }],
                "border": { "color": "rebeccapurple" }
            }
        }))
        .unwrap();
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        validate_children(&[child], "test", 4.0, &mut errors, &mut warnings);
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
    }

    #[test]
    fn unresolvable_background_gradient_stop_color_is_reported() {
        let child: ChildComponent = serde_json::from_value(serde_json::json!({
            "type": "card",
            "style": {
                "background": {
                    "kind": "linear-gradient",
                    "angle": 45,
                    "stops": [
                        { "color": "#111111", "offset": 0.0 },
                        { "color": "totally-bogus", "offset": 1.0 }
                    ]
                }
            }
        }))
        .unwrap();
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        validate_children(&[child], "test", 4.0, &mut errors, &mut warnings);
        assert!(
            errors.iter().any(|e| e.contains("totally-bogus")),
            "expected the bad gradient-stop color to be reported: {errors:?}"
        );
    }

    #[test]
    fn unresolvable_border_color_is_reported_with_side() {
        let child: ChildComponent = serde_json::from_value(serde_json::json!({
            "type": "card",
            "style": {
                "border": { "top": { "color": "bogus-border-color" } }
            }
        }))
        .unwrap();
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        validate_children(&[child], "test", 4.0, &mut errors, &mut warnings);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("bogus-border-color") && e.contains("border.top")),
            "expected a border.top-labelled error: {errors:?}"
        );
    }

    #[test]
    fn rgba_object_color_form_never_errors() {
        // Color::Rgba{r,g,b,a} is always valid by construction — only
        // Color::String can fail to parse.
        let child: ChildComponent = serde_json::from_value(serde_json::json!({
            "type": "text",
            "content": "hi",
            "style": { "color": { "r": 10, "g": 20, "b": 30, "a": 0.5 } }
        }))
        .unwrap();
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        validate_children(&[child], "test", 4.0, &mut errors, &mut warnings);
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
    }
}
