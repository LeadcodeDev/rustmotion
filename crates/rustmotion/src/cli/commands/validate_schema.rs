//! Schema-level scenario checks (file existence, dimensions, durations, etc.).
//! Returns (errors, warnings); errors block rendering, warnings are advisory only.

use rustmotion::components::{ChildComponent, Component};
use rustmotion::core::css::style::{
    Background, BackgroundLayer, BorderRadius, Color, CssStyle, Display as CssDisplay,
};
use rustmotion::engine::animator::{motion_path_length, MOTION_PATH_MIN_LENGTH};
use rustmotion::schema::{
    AnimationEffect, CharAnimationTiming, MotionPathConfig, ResolvedScenario, SpringConfig,
};

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

        // Generic interpolation (issue: "interpolation générique de
        // n'importe quelle propriété"): `timeline` + `style.transition`
        // accepts a transition on any CSS property and used to silently
        // snap instead of animating it for everything except opacity and
        // color (text/counter). This turns that silence into a named
        // diagnostic — see the function's doc comment. (The sibling gap —
        // an explicit `style.animation` keyframes effect naming an
        // unrecognized `property` — turned out to already be closed at
        // deserialize time by `schema/video.rs`'s
        // `validate_motion_property`; verified, not reopened here.)
        check_transition_smoothing(&child.component, &p, warnings);

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

                if let AnimationEffect::MotionPath(cfg) = effect {
                    check_motion_path_config(cfg, &p, errors, warnings);
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

// ─── Generic interpolation of timeline/style.transition properties ────────
//
// Two independent silent-gap classes existed before this workstream, both
// rooted in the same fact: `style.animation`/`timeline` accept a transition
// on *any* named CSS/animation property, but the engine only actually knows
// how to smoothly interpolate a handful of them. Everything else either (a)
// snaps at the step's `at` instead of animating (`style.transition` +
// `timeline` style states — see `box_builder.rs::apply_style_states`'s doc
// comment), or (b) has no effect whatsoever, not even a snap (an explicit
// `style.animation: [{ "name": "keyframes", "keyframes": [{ "property":
// "…" }] }]` targeting a name `animator::apply_property` doesn't recognize).
// `validate` used to say nothing about either. These two checks do.

/// Classification of a CSS property with respect to `style.transition` +
/// `timeline` style-state smoothing (`check_transition_smoothing` below).
/// Mirrors real CSS's own interpolable/discrete split — see each variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransitionPropertyKind {
    /// Already smoothed: `opacity` (always, when `style.transition` is
    /// set), `color` (text/counter), `background`/`border-radius` (solid
    /// colour / uniform absolute px — see the finer shape check in the
    /// caller for when a *specific* value isn't one of those shapes).
    Smoothed,
    /// CSS-spec-discrete (keyword/enum-valued) — snapping is the correct,
    /// expected behaviour, exactly like real CSS `transition-property`
    /// would do. Still diagnosed (reassuring wording, not an alarm): an
    /// author who set `style.transition` and sees a hard cut on this
    /// property deserves a line saying that's expected, not silence either
    /// way — "sauter et le dire" per the workstream brief.
    Discrete,
    /// Numeric/continuous but affects the layout box (size/position/flow).
    /// Interpolating it would require the value to reach `run_layout` on
    /// every sampled frame — out of reach without changing the frozen
    /// `layout_pass.rs`/`paint_pass.rs`, and the reason this workstream
    /// draws a hard line between paint-time and layout-time properties
    /// (see the workstream report's "piège" section).
    Layout,
    /// Numeric/continuous, paint-time, but not yet wired up to interpolate
    /// (includes the empty/unrecognized-name fallback below — a future
    /// `CssStyle` field this table hasn't been taught about yet fails loud,
    /// not silent).
    UnsupportedPaint,
}

/// Classify a `CssStyle` field by its kebab-case JSON key (the wire name —
/// matches what `apply_style_states`'s own `serde_json` merge keys on, and
/// what an author actually typed under `style`/a `timeline[*].style`).
fn classify_transition_property(name: &str) -> TransitionPropertyKind {
    use TransitionPropertyKind::*;
    match name {
        "opacity" | "color" | "background" | "border-radius" => Smoothed,
        "display"
        | "position"
        | "box-sizing"
        | "flex-direction"
        | "flex-wrap"
        | "justify-content"
        | "align-items"
        | "align-self"
        | "align-content"
        | "justify-items"
        | "justify-self"
        | "grid-auto-flow"
        | "font-family"
        | "font-weight"
        | "font-style"
        | "text-align"
        | "white-space"
        | "overflow-wrap"
        | "text-overflow"
        | "text-decoration"
        | "text-autofit"
        | "mix-blend-mode"
        | "clip-path"
        | "overflow"
        | "overflow-x"
        | "overflow-y"
        | "visibility"
        | "z-index"
        | "order"
        | "grid-template-columns"
        | "grid-template-rows"
        | "grid-column"
        | "grid-row" => Discrete,
        "top" | "right" | "bottom" | "left" | "width" | "height" | "min-width" | "min-height"
        | "max-width" | "max-height" | "margin" | "padding" | "border" | "aspect-ratio" | "gap"
        | "flex-grow" | "flex-shrink" | "flex-basis" | "font-size" | "line-height"
        | "letter-spacing" => Layout,
        // "animation"/"transition"/"audio-reactive" are config, not visual
        // state, and are filtered out of the walk before this is ever
        // called (see `check_transition_smoothing`) — they never reach
        // this match. Everything else — box-shadow, text-shadow,
        // gradient-border, filter, backdrop-filter, transform,
        // transform-origin, perspective, perspective-origin, depth,
        // backdrop-blur, inner-shadow, and any `CssStyle` field added later
        // that this table hasn't been taught about — fails loud here by
        // design: an unrecognized name is treated as "known not to smooth"
        // rather than silently passed through.
        _ => UnsupportedPaint,
    }
}

/// `style.transition` promises to smooth whichever CSS properties a
/// `timeline` step changes — but `box_builder.rs`'s
/// `apply_style_states`/`resolve_transition_css_overrides`/
/// `transition_keyframes` only actually smooth `opacity`, `color`
/// (text/counter), `background` (solid colour), and `border-radius`
/// (uniform absolute px). Everything else still snaps at the step's `at`.
///
/// This walks the declared `timeline` in author order — not tied to any
/// particular render time, unlike the runtime: a static check must catch
/// every step-to-step (and base-to-first-step) diff, not just whichever one
/// happens to be "due" at some sampled `t`. Only runs when `style.transition`
/// is actually set: if it isn't, nothing was ever promised, and every
/// property snapping is exactly the documented, expected behaviour (no
/// diagnostic needed).
fn check_transition_smoothing(component: &Component, path: &str, warnings: &mut Vec<String>) {
    let style = component.as_styled().style_config();
    if style.transition.is_none() {
        return;
    }
    let Some(animatable) = component.as_animatable() else {
        return;
    };
    let mut relevant: Vec<&rustmotion::schema::TimelineStep> = animatable
        .timeline_steps()
        .iter()
        .filter(|s| s.style.is_some())
        .collect();
    if relevant.is_empty() {
        return;
    }
    relevant.sort_by(|a, b| a.at.total_cmp(&b.at));

    let Ok(mut current) = serde_json::to_value(style) else {
        return;
    };
    let mut warned: std::collections::HashSet<String> = std::collections::HashSet::new();

    for step in relevant {
        let step_style = step.style.as_deref().unwrap();
        let Ok(serde_json::Value::Object(state)) = serde_json::to_value(step_style) else {
            continue;
        };
        let serde_json::Value::Object(cur_obj) = &current else {
            continue;
        };
        let mut merged = cur_obj.clone();
        for (k, v) in &state {
            if v.is_null() || k == "animation" || k == "transition" || k == "audio-reactive" {
                continue;
            }
            let changed = merged.get(k) != Some(v);
            if changed && !warned.contains(k.as_str()) {
                match classify_transition_property(k) {
                    TransitionPropertyKind::Discrete => {
                        // Still named, not silent: `display`/`position`/etc.
                        // have no in-between value (real CSS can't animate
                        // them either), so this is expected, correct
                        // behaviour — not a gap. The wording deliberately
                        // reads as reassurance, not an alarm, but the point
                        // is that an author who set `style.transition`
                        // expecting *something* to smooth still gets a line
                        // telling them exactly which property didn't and
                        // why, instead of silence either way.
                        warnings.push(format!(
                            "{path}: style.transition is set and timeline changes `{k}`, but \
                             `{k}` is a discrete CSS property (no value exists in between the two \
                             states) — it always snaps at the step's `at`, exactly like real CSS. \
                             This is expected, not a rustmotion limitation."
                        ));
                        warned.insert(k.clone());
                    }
                    TransitionPropertyKind::Smoothed => {
                        // `background`/`border-radius` only actually smooth
                        // for a specific value shape (solid colour / uniform
                        // absolute px) — anything else in the recognized-
                        // property bucket must still be caught, or an
                        // author using per-corner radii or a gradient would
                        // get silence again, just one level deeper.
                        let resolves = match k.as_str() {
                            "border-radius" => {
                                let mut probe = merged.clone();
                                probe.insert(k.clone(), v.clone());
                                serde_json::from_value::<CssStyle>(serde_json::Value::Object(probe))
                                    .ok()
                                    .and_then(|s| s.border_radius)
                                    .and_then(|br| BorderRadius::absolute_px(&br))
                                    .is_some()
                            }
                            "background" => {
                                let mut probe = merged.clone();
                                probe.insert(k.clone(), v.clone());
                                serde_json::from_value::<CssStyle>(serde_json::Value::Object(probe))
                                    .ok()
                                    .and_then(|s| s.background)
                                    .and_then(|bg| Background::solid_hex(&bg))
                                    .is_some()
                            }
                            _ => true,
                        };
                        if !resolves {
                            let hint = if k == "border-radius" {
                                "only a single uniform px/unitless radius is smoothed today — \
                                 per-corner radii and %/em/rem/vw/vh are not"
                            } else {
                                "only a solid colour is smoothed today — gradients and image \
                                 layers are not"
                            };
                            warnings.push(format!(
                                "{path}: style.transition is set and timeline changes `{k}`, but \
                                 this value isn't a shape rustmotion can interpolate yet ({hint}) \
                                 — it will snap instead of animating."
                            ));
                            warned.insert(k.clone());
                        }
                    }
                    TransitionPropertyKind::Layout => {
                        warnings.push(format!(
                            "{path}: style.transition is set and timeline changes `{k}`, but \
                             `{k}` affects layout (box size/position/flow) — rustmotion cannot \
                             interpolate a layout property without re-running layout on every \
                             sampled frame, so it will snap instead of animating. Consider \
                             approximating the motion with `transform: translate`/`scale` \
                             instead, which is paint-time and does interpolate."
                        ));
                        warned.insert(k.clone());
                    }
                    TransitionPropertyKind::UnsupportedPaint => {
                        warnings.push(format!(
                            "{path}: style.transition is set and timeline changes `{k}`, but \
                             rustmotion does not yet know how to interpolate `{k}` — it will \
                             snap instead of animating at the step's `at`."
                        ));
                        warned.insert(k.clone());
                    }
                }
            }
            merged.insert(k.clone(), v.clone());
        }
        current = serde_json::Value::Object(merged);
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

/// Same "reject before it reaches the solver" posture as `check_spring_config`
/// (constat #6), applied to `motion_path`.
///
/// `duration <= 0` would make `engine::animator::motion_path_progress`
/// divide by (near-)zero — the solver already floors that defensively via
/// `safe_div` (belt and suspenders, the same pattern `spring_value` uses for
/// `mass`/`stiffness`), so this is an actionable author-facing error, not
/// the only thing standing between a bad config and a degenerate (if still
/// finite) render.
///
/// The path's own syntax/emptiness is already a hard parse-time error
/// (`schema/video.rs::deserialize_motion_path_data` — a scenario carrying
/// one would have failed to deserialize before ever reaching here). What
/// *can* still reach here is a syntactically valid but geometrically
/// degenerate path — (near-)zero measured length, e.g. a single point or
/// every segment collapsing onto one — which is well-defined at render time
/// (the component holds still, see `motion_path_sample`'s doc comment) but
/// is very likely a typo (duplicated/near-identical coordinates) rather
/// than an intentional "don't move" effect — especially combined with
/// `orient: true`, where the tangent is undefined and orientation silently
/// does nothing. Advisory only (`warnings`, not `errors`): nothing here is
/// unsound to render, unlike `duration <= 0`.
fn check_motion_path_config(
    cfg: &MotionPathConfig,
    path: &str,
    errors: &mut Vec<String>,
    warnings: &mut Vec<String>,
) {
    if cfg.duration <= 0.0 {
        errors.push(format!(
            "{path}: motion_path.duration must be > 0 (got {}) — a zero or negative duration \
             cannot be mapped to a point along the path",
            cfg.duration
        ));
    }
    if let Some(length) = motion_path_length(&cfg.path) {
        if length <= MOTION_PATH_MIN_LENGTH {
            let orient_note = if cfg.orient {
                ", and `orient: true` will have no effect (a tangent is undefined at zero length)"
            } else {
                ""
            };
            warnings.push(format!(
                "{path}: motion_path.path '{}' has (near-)zero measured length (a single \
                 point, or every segment collapses onto one) — the component will hold still \
                 instead of travelling{orient_note}. If this is intentional, ignore this \
                 warning; otherwise check for duplicated/typo'd coordinates.",
                cfg.path
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
        | AnimationEffect::PopIn(t)
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

        // A sweep of light is decoration over an element that is already
        // there — it has a completion time like an entrance does, so it is
        // budgeted the same way, but a looping one never completes.
        AnimationEffect::Shimmer(s) => {
            if s.repeat {
                None
            } else {
                Some((s.delay, s.duration))
            }
        }

        // Custom keyframes
        AnimationEffect::Keyframes(k) => {
            if k.repeat {
                None
            } else {
                Some((k.delay, k.duration))
            }
        }

        // A non-looping `motion_path` settles onto the path's end at
        // `delay + duration`, exactly like `Keyframes`/`TiltIn` above —
        // budget it the same way. A looping one is continuous by nature
        // (like `Orbit`), so it has no completion budget.
        AnimationEffect::MotionPath(c) => {
            if c.repeat {
                None
            } else {
                Some((c.delay, c.duration))
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
        // `rustmotion`'s CLI checked `damping`/`stiffness` before this).
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

#[cfg(test)]
mod transition_smoothing_tests {
    use super::*;

    fn warnings_for(json: serde_json::Value) -> Vec<String> {
        let child: ChildComponent = serde_json::from_value(json).unwrap();
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        validate_children(&[child], "test", 4.0, &mut errors, &mut warnings);
        warnings
    }

    #[test]
    fn layout_property_change_under_transition_is_diagnosed() {
        // `width` affects the layout box — smoothing it would require
        // `run_layout` on every sampled frame, which this workstream leaves
        // to a future one (see the "piège" in the workstream report). It
        // must still snap, but `validate` must say so instead of staying
        // silent about it, the way it always has until now.
        let warnings = warnings_for(serde_json::json!({
            "type": "div",
            "style": { "width": "100px", "transition": 0.5 },
            "timeline": [{ "at": 1.0, "style": { "width": "300px" } }]
        }));
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("`width`") && w.contains("layout") && w.contains("snap")),
            "expected a layout-property diagnostic naming `width`: {warnings:?}"
        );
    }

    #[test]
    fn unsupported_paint_property_change_under_transition_is_diagnosed() {
        // `transform` is paint-time but this workstream deliberately did not
        // implement list-shaped interpolation for it (see workstream
        // report) — it must be diagnosed, not silently accepted just
        // because it's "only" paint, not layout.
        let warnings = warnings_for(serde_json::json!({
            "type": "div",
            "style": { "transition": 0.5 },
            "timeline": [{ "at": 1.0, "style": { "transform": [{ "fn": "scale", "x": 2, "y": 2 }] } }]
        }));
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("`transform`") && w.contains("snap")),
            "expected an unsupported-paint diagnostic naming `transform`: {warnings:?}"
        );
    }

    #[test]
    fn discrete_property_change_under_transition_is_diagnosed_but_reassuringly() {
        // `display` is CSS-spec-discrete — snapping is the correct,
        // expected behaviour (real CSS can't animate it either) — but the
        // brief is explicit that a discrete property must still "sauter et
        // le dire", not sauter en silence: an author who set
        // `style.transition` and sees `display` hard-cut deserves a line
        // explaining that's expected, distinguishable from an actual gap.
        let warnings = warnings_for(serde_json::json!({
            "type": "div",
            "style": { "transition": 0.5, "display": "flex" },
            "timeline": [{ "at": 1.0, "style": { "display": "block" } }]
        }));
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("`display`") && w.contains("expected")),
            "expected a reassuring discrete-property diagnostic naming `display`: {warnings:?}"
        );
        // But it must read differently from an actual gap — never the
        // "snap instead of animating" alarm wording the Layout/
        // UnsupportedPaint branches use.
        assert!(
            warnings
                .iter()
                .filter(|w| w.contains("`display`"))
                .all(|w| !w.contains("snap instead of animating")),
            "a discrete property's diagnostic must not read like a gap: {warnings:?}"
        );
    }

    #[test]
    fn newly_smoothed_properties_are_not_diagnosed() {
        // `border-radius` (uniform, absolute px) and `background` (solid
        // colour) are exactly the two properties this workstream taught
        // `box_builder.rs` to interpolate — they must not warn.
        let warnings = warnings_for(serde_json::json!({
            "type": "div",
            "style": { "transition": 0.5, "border-radius": 0, "background": "#000000" },
            "timeline": [{ "at": 1.0, "style": { "border-radius": 40, "background": "#ffffff" } }]
        }));
        assert!(
            warnings
                .iter()
                .all(|w| !w.contains("border-radius") && !w.contains("`background`")),
            "newly-smoothed properties must not be diagnosed: {warnings:?}"
        );
    }

    #[test]
    fn per_corner_border_radius_is_diagnosed_as_an_unresolvable_shape() {
        // The interpolable set is "border-radius" by name, but only a
        // uniform, absolute-px value actually resolves
        // (`BorderRadius::absolute_px`) — a per-corner shape must still be
        // caught, not pass silently just because the property name matches.
        let warnings = warnings_for(serde_json::json!({
            "type": "div",
            "style": { "transition": 0.5, "border-radius": 0 },
            "timeline": [{
                "at": 1.0,
                "style": { "border-radius": { "top-left": 10, "top-right": 0, "bottom-right": 0, "bottom-left": 0 } }
            }]
        }));
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("border-radius") && w.contains("per-corner")),
            "expected a shape-mismatch diagnostic for per-corner border-radius: {warnings:?}"
        );
    }

    #[test]
    fn no_transition_configured_means_no_diagnostic_at_all() {
        // Without `style.transition`, nothing was ever promised — every
        // property snapping (including `width`) is exactly the documented,
        // expected behaviour. No diagnostic should fire.
        let warnings = warnings_for(serde_json::json!({
            "type": "div",
            "style": { "width": "100px" },
            "timeline": [{ "at": 1.0, "style": { "width": "300px" } }]
        }));
        assert!(
            warnings.iter().all(|w| !w.contains("width")),
            "no style.transition means no diagnostic is expected: {warnings:?}"
        );
    }

    #[test]
    fn unknown_explicit_keyframes_property_is_rejected_at_parse_time_not_validate_time() {
        // A sibling silent-gap hypothesis this workstream investigated and
        // found already closed: an explicit `style.animation` keyframes
        // effect naming a `property` `animator::apply_property` doesn't
        // recognize (e.g. a CSS-ish `"background-color"` instead of the
        // solver's `"background"`/`"color"`) used to *look* like the same
        // "known property but not wired up" gap `check_transition_smoothing`
        // covers above — but it isn't reachable that far: `schema/video.rs`'s
        // `deserialize_validated_keyframes`/`validate_motion_property`
        // (constat #4, an earlier workstream) already rejects it during
        // `Component` deserialization, with a did-you-mean suggestion, well
        // before a scenario ever reaches `validate_scenario`. This test
        // pins that down instead of re-diagnosing something `validate`
        // structurally cannot ever see.
        let err = serde_json::from_value::<ChildComponent>(serde_json::json!({
            "type": "div",
            "style": {
                "animation": [{
                    "name": "keyframes",
                    "keyframes": [{
                        "property": "background-color",
                        "keyframes": [
                            { "time": 0.0, "value": "#000000" },
                            { "time": 1.0, "value": "#ffffff" }
                        ]
                    }]
                }]
            }
        }))
        .expect_err("an unrecognized animation property must fail to deserialize");
        assert!(
            err.to_string().contains("background-color"),
            "expected the parse error to name the offending property: {err}"
        );
    }
}

#[cfg(test)]
mod motion_path_validation_tests {
    use super::*;

    fn motion_path_child(json_animation: serde_json::Value) -> ChildComponent {
        serde_json::from_value(serde_json::json!({
            "type": "shape",
            "shape": "rect",
            "style": { "animation": [json_animation] }
        }))
        .expect("valid component JSON")
    }

    #[test]
    fn zero_or_negative_duration_is_an_error() {
        for duration in [0.0, -1.0] {
            let child = motion_path_child(serde_json::json!({
                "name": "motion_path",
                "path": "M0,0 L100,0",
                "duration": duration
            }));
            let mut errors = Vec::new();
            let mut warnings = Vec::new();
            validate_children(&[child], "test", 4.0, &mut errors, &mut warnings);
            assert!(
                errors
                    .iter()
                    .any(|e| e.contains("motion_path.duration must be > 0")),
                "duration={duration}: missing error, got errors={errors:?}"
            );
        }
    }

    #[test]
    fn a_normal_traveling_path_has_no_errors_or_warnings_about_itself() {
        let child = motion_path_child(serde_json::json!({
            "name": "motion_path",
            "path": "M0,0 L100,0",
            "duration": 0.6
        }));
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        validate_children(&[child], "test", 4.0, &mut errors, &mut warnings);
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        assert!(
            warnings.iter().all(|w| !w.contains("motion_path")),
            "unexpected motion_path warning on a normal path: {warnings:?}"
        );
    }

    // ---- brief's "single point" / "zero length" degenerate cases: legal at
    // parse time, but advisory-flagged here since they are very likely a
    // typo (constat: this mirrors check_spring_config's posture, but as a
    // warning rather than an error — nothing here is unsound to render). ----

    #[test]
    fn a_single_point_path_is_a_warning_not_an_error() {
        let child = motion_path_child(serde_json::json!({
            "name": "motion_path",
            "path": "M50,50",
            "duration": 0.6
        }));
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        validate_children(&[child], "test", 4.0, &mut errors, &mut warnings);
        assert!(
            errors.iter().all(|e| !e.contains("motion_path")),
            "a single-point path must not be a hard error: {errors:?}"
        );
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("motion_path") && w.contains("measured length")),
            "missing zero-length warning: {warnings:?}"
        );
    }

    #[test]
    fn a_zero_length_path_combined_with_orient_names_that_orient_does_nothing() {
        let child = motion_path_child(serde_json::json!({
            "name": "motion_path",
            "path": "M10,10 L10,10",
            "duration": 0.6,
            "orient": true
        }));
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        validate_children(&[child], "test", 4.0, &mut errors, &mut warnings);
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("motion_path") && w.contains("orient")),
            "expected the warning to call out orient:true doing nothing here: {warnings:?}"
        );
    }

    // ---- entrance-budget completion check now covers motion_path too ----

    #[test]
    fn a_non_looping_motion_path_finishing_after_the_scene_is_an_error() {
        let child = motion_path_child(serde_json::json!({
            "name": "motion_path",
            "path": "M0,0 L100,0",
            "delay": 3.5,
            "duration": 1.0
        }));
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        validate_children(
            &[child],
            "test",
            /*scene_duration=*/ 2.0,
            &mut errors,
            &mut warnings,
        );
        assert!(
            errors.iter().any(|e| e.contains("animation finishes at")),
            "expected the entrance-budget error for delay 3.5 + duration 1.0 > scene 2.0: {errors:?}"
        );
    }

    #[test]
    fn a_looping_motion_path_has_no_completion_budget() {
        let child = motion_path_child(serde_json::json!({
            "name": "motion_path",
            "path": "M0,0 L100,0",
            "delay": 3.5,
            "duration": 1.0,
            "loop": true
        }));
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        validate_children(
            &[child],
            "test",
            /*scene_duration=*/ 2.0,
            &mut errors,
            &mut warnings,
        );
        assert!(
            errors.iter().all(|e| !e.contains("animation finishes at")),
            "a looping motion_path must not be budget-checked, like orbit/wiggle: {errors:?}"
        );
    }
}
