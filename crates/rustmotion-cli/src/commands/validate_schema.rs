//! Schema-level scenario checks (file existence, dimensions, durations, etc.).
//! Returns (errors, warnings); errors block rendering, warnings are advisory only.

use rustmotion::components::{ChildComponent, Component};
use rustmotion::core::css::style::{
    Background, BackgroundLayer, Color, CssStyle, Display as CssDisplay,
};
use rustmotion::schema::{AnimationEffect, CharAnimationTiming, ResolvedScenario};

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
        if let Some(anim) = child.component.as_animatable() {
            let start_at = child
                .component
                .as_timed()
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
