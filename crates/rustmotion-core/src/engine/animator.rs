use crate::schema::{
    Animation, AnimationEffect, AnimationPreset, CharAnimPreset, EasingType, GlowConfig, Keyframe,
    KeyframeValue, OrbitConfig, PresetConfig, SpringConfig, TextAnimGranularity, WiggleConfig,
};

/// Safe division that returns `fallback` when the denominator is too small to
/// produce a meaningful result (within 1e-9). Use this for any calculation
/// where a zero-or-near-zero duration could otherwise produce NaN/∞ that
/// silently propagates into transforms or opacity.
#[inline]
pub fn safe_div(num: f64, denom: f64, fallback: f64) -> f64 {
    if denom.abs() < 1e-9 {
        fallback
    } else {
        num / denom
    }
}

/// Same as `safe_div` but for f32. Useful in render-side hot paths.
#[inline]
pub fn safe_div_f32(num: f32, denom: f32, fallback: f32) -> f32 {
    if denom.abs() < 1e-6 {
        fallback
    } else {
        num / denom
    }
}

// ─── Effect extraction ──────────────────────────────────────────────────────

/// Resolved char animation config ready for the text renderer.
#[derive(Debug, Clone)]
pub struct ResolvedCharAnimation {
    pub preset: CharAnimPreset,
    pub granularity: TextAnimGranularity,
    pub stagger: f32,
    pub duration: f32,
    pub easing: EasingType,
    pub delay: f32,
    pub overshoot: f32,
}

/// Extracted and categorized animation effects from an AnimationEffect slice.
pub struct ExtractedEffects<'a> {
    pub presets: Vec<(AnimationPreset, PresetConfig)>,
    /// Every `keyframes`/`tilt_in` effect's animations, in the order their
    /// source effects appear in `style.animation` (constat #5: this used to
    /// be split into two buckets — routed purely by whether the effect's
    /// `delay` happened to be nonzero — resolved and merged separately,
    /// which made "sum vs last-wins" on a shared property depend on that
    /// unrelated field. Now there is one bucket, resolved in one
    /// `resolve_animations` call, so the composition rule is always
    /// "last effect in the array wins on a shared property" — a CSS-cascade
    /// rule, independent of `delay`).
    pub keyframe_animations: Vec<Animation>,
    /// True when any contributing `keyframes`/`tilt_in` effect requested
    /// `"loop": true` (constat #7). Applied uniformly to the whole
    /// `keyframe_animations` bucket — see the doc comment on
    /// `resolve_props_for_effects` for the same caveat presets already have
    /// (multiple effects with different loop settings on the same property
    /// is an unsupported edge case, not new to this fix).
    pub keyframes_loop: bool,
    pub wiggles: Vec<&'a WiggleConfig>,
    pub orbits: Vec<&'a OrbitConfig>,
    pub glow: Option<&'a GlowConfig>,
    pub motion_blur: Option<f32>,
    pub char_animation: Option<ResolvedCharAnimation>,
}

/// M3: find the first `glow` effect in a list, if present.
///
/// `glow` is a static (non-time-varying) coloured halo — unlike every other
/// effect `resolve_props_for_effects` resolves, it deliberately is *not*
/// folded into `AnimatedProperties`: `GlowConfig.color` has no corresponding
/// field there, and extending `AnimatedProperties`'s public shape is out of
/// scope for this workstream. Callers apply the returned config directly as
/// a CSS `filter: drop-shadow(...)` (see
/// `rustmotion_components::box_builder::apply_glow_effect`), which is the
/// only place that needs the raw colour string.
pub fn find_glow_effect(effects: &[AnimationEffect]) -> Option<&GlowConfig> {
    effects.iter().find_map(|e| match e {
        AnimationEffect::Glow(cfg) => Some(cfg),
        _ => None,
    })
}

/// Split a slice of AnimationEffect into categorized buckets for the renderer.
pub fn extract_effects(effects: &[AnimationEffect]) -> ExtractedEffects<'_> {
    let mut result = ExtractedEffects {
        presets: Vec::new(),
        keyframe_animations: Vec::new(),
        keyframes_loop: false,
        wiggles: Vec::new(),
        orbits: Vec::new(),
        glow: None,
        motion_blur: None,
        char_animation: None,
    };

    for effect in effects {
        if let Some((preset, timing)) = effect.as_preset() {
            result.presets.push((preset, timing.to_preset_config()));
        } else {
            match effect {
                AnimationEffect::CharScaleIn(t)
                | AnimationEffect::CharFadeIn(t)
                | AnimationEffect::CharWave(t)
                | AnimationEffect::CharBounce(t)
                | AnimationEffect::CharRotateIn(t)
                | AnimationEffect::CharSlideUp(t) => {
                    let preset = match effect {
                        AnimationEffect::CharScaleIn(_) => CharAnimPreset::ScaleIn,
                        AnimationEffect::CharFadeIn(_) => CharAnimPreset::FadeIn,
                        AnimationEffect::CharWave(_) => CharAnimPreset::Wave,
                        AnimationEffect::CharBounce(_) => CharAnimPreset::Bounce,
                        AnimationEffect::CharRotateIn(_) => CharAnimPreset::RotateIn,
                        AnimationEffect::CharSlideUp(_) => CharAnimPreset::SlideUp,
                        _ => unreachable!(),
                    };
                    result.char_animation = Some(ResolvedCharAnimation {
                        preset,
                        granularity: t.granularity.clone(),
                        stagger: t.stagger as f32,
                        duration: t.duration as f32,
                        easing: t.easing.clone(),
                        delay: t.delay as f32,
                        overshoot: t.overshoot.unwrap_or(0.08) as f32,
                    });
                }
                AnimationEffect::Glow(config) => {
                    result.glow = Some(config);
                }
                AnimationEffect::Wiggle(config) => {
                    result.wiggles.push(config);
                }
                AnimationEffect::Orbit(config) => {
                    result.orbits.push(config);
                }
                AnimationEffect::Keyframes(config) => {
                    // Keyframe times are absolute scene seconds; the
                    // config-level delay shifts them (applied unconditionally
                    // — a no-op when `delay == 0` — so every `keyframes`
                    // effect lands in the same bucket regardless of its
                    // delay; see the `ExtractedEffects::keyframe_animations`
                    // doc comment for why that used to matter).
                    result
                        .keyframe_animations
                        .extend(config.keyframes.iter().map(|anim| {
                            let mut a = anim.clone();
                            for kf in &mut a.keyframes {
                                kf.time += config.delay;
                            }
                            a
                        }));
                    if config.repeat {
                        result.keyframes_loop = true;
                    }
                }
                AnimationEffect::TiltIn(config) => {
                    let delay = config.delay;
                    let end = delay + config.duration;
                    let rx = config.rotate_x.unwrap_or(15.0);
                    let ry = config.rotate_y.unwrap_or(-15.0);
                    let persp = config.perspective.unwrap_or(1000.0);
                    let sc = config.scale_from.unwrap_or(0.9);
                    result.keyframe_animations.extend([
                        kf_anim(
                            "opacity",
                            delay,
                            0.0,
                            delay + config.duration * 0.3,
                            1.0,
                            EasingType::EaseOut,
                        ),
                        kf_anim("rotate_x", delay, rx, end, 0.0, EasingType::EaseOutCubic),
                        kf_anim("rotate_y", delay, ry, end, 0.0, EasingType::EaseOutCubic),
                        kf_anim("perspective", delay, persp, end, persp, EasingType::Linear),
                        kf_anim("scale", delay, sc, end, 1.0, EasingType::EaseOutCubic),
                    ]);
                    if config.repeat {
                        result.keyframes_loop = true;
                    }
                }
                AnimationEffect::MotionBlur(config) => {
                    result.motion_blur = Some(config.intensity);
                }
                _ => {} // preset variants already handled above
            }
        }
    }

    result
}

// ─── Easing functions ───────────────────────────────────────────────────────

/// Apply easing function to a normalized time t (0.0..1.0)
pub fn ease(t: f64, easing: &EasingType) -> f64 {
    let t = t.clamp(0.0, 1.0);
    match easing {
        EasingType::Linear => t,
        EasingType::EaseIn => ease_in_cubic(t),
        EasingType::EaseOut => ease_out_cubic(t),
        EasingType::EaseInOut => ease_in_out_cubic(t),
        EasingType::EaseInQuad => t * t,
        EasingType::EaseOutQuad => 1.0 - (1.0 - t) * (1.0 - t),
        EasingType::EaseInCubic => ease_in_cubic(t),
        EasingType::EaseOutCubic => ease_out_cubic(t),
        EasingType::EaseInExpo => {
            if t == 0.0 {
                0.0
            } else {
                (2.0f64).powf(10.0 * (t - 1.0))
            }
        }
        EasingType::EaseOutExpo => {
            if t == 1.0 {
                1.0
            } else {
                1.0 - (2.0f64).powf(-10.0 * t)
            }
        }
        EasingType::EaseInOutQuad => {
            if t < 0.5 {
                2.0 * t * t
            } else {
                1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
            }
        }
        EasingType::EaseInOutExpo => {
            if t == 0.0 {
                0.0
            } else if t == 1.0 {
                1.0
            } else if t < 0.5 {
                (2.0f64).powf(20.0 * t - 10.0) / 2.0
            } else {
                (2.0 - (2.0f64).powf(-20.0 * t + 10.0)) / 2.0
            }
        }
        EasingType::EaseInBack => {
            let c1 = 1.70158;
            let c3 = c1 + 1.0;
            c3 * t * t * t - c1 * t * t
        }
        EasingType::EaseOutBack => {
            let c1 = 1.70158;
            let c3 = c1 + 1.0;
            1.0 + c3 * (t - 1.0).powi(3) + c1 * (t - 1.0).powi(2)
        }
        EasingType::EaseOutElastic => {
            if t == 0.0 {
                0.0
            } else if t == 1.0 {
                1.0
            } else {
                let c4 = (2.0 * std::f64::consts::PI) / 3.0;
                (2.0f64).powf(-10.0 * t) * ((t * 10.0 - 0.75) * c4).sin() + 1.0
            }
        }
        EasingType::Bounce => bounce_ease_out(t),
        EasingType::Spring => t, // Spring handled separately
        EasingType::CubicBezier { x1, y1, x2, y2 } => cubic_bezier_ease(t, *x1, *y1, *x2, *y2),
    }
}

/// Evaluate a cubic-bezier curve at parameter t using Newton's method
/// Control points: P0=(0,0), P1=(x1,y1), P2=(x2,y2), P3=(1,1)
fn cubic_bezier_ease(t: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    // Find the parameter t_curve such that bezier_x(t_curve) = t
    // Then return bezier_y(t_curve)
    let t_curve = find_bezier_t_for_x(t, x1, x2);
    bezier_component(t_curve, y1, y2)
}

fn bezier_component(t: f64, p1: f64, p2: f64) -> f64 {
    // B(t) = 3(1-t)^2*t*p1 + 3(1-t)*t^2*p2 + t^3
    let t2 = t * t;
    let t3 = t2 * t;
    let mt = 1.0 - t;
    let mt2 = mt * mt;
    3.0 * mt2 * t * p1 + 3.0 * mt * t2 * p2 + t3
}

fn bezier_component_derivative(t: f64, p1: f64, p2: f64) -> f64 {
    let mt = 1.0 - t;
    3.0 * mt * mt * p1 + 6.0 * mt * t * (p2 - p1) + 3.0 * t * t * (1.0 - p2)
}

fn find_bezier_t_for_x(x: f64, x1: f64, x2: f64) -> f64 {
    // Newton-Raphson to solve bezier_x(t) = x
    let mut t = x; // Initial guess
    for _ in 0..8 {
        let current_x = bezier_component(t, x1, x2);
        let dx = bezier_component_derivative(t, x1, x2);
        if dx.abs() < 1e-10 {
            break;
        }
        t -= (current_x - x) / dx;
        t = t.clamp(0.0, 1.0);
    }
    t
}

fn bounce_ease_out(t: f64) -> f64 {
    let n1 = 7.5625;
    let d1 = 2.75;
    if t < 1.0 / d1 {
        n1 * t * t
    } else if t < 2.0 / d1 {
        let t = t - 1.5 / d1;
        n1 * t * t + 0.75
    } else if t < 2.5 / d1 {
        let t = t - 2.25 / d1;
        n1 * t * t + 0.9375
    } else {
        let t = t - 2.625 / d1;
        n1 * t * t + 0.984375
    }
}

fn ease_in_cubic(t: f64) -> f64 {
    t * t * t
}

fn ease_out_cubic(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(3)
}

fn ease_in_out_cubic(t: f64) -> f64 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

// ─── Spring solver ──────────────────────────────────────────────────────────

/// Solve spring animation at time t (seconds).
/// Returns a value between 0.0 and 1.0 representing progress.
///
/// Constat #6: `SpringConfig` accepts any `f64` (it's schema-level, not
/// range-checked at parse time), and `rustmotion validate` used to check
/// nothing about it either. `mass <= 0` or `stiffness <= 0` fed straight into
/// `sqrt`/division below produced NaN (sqrt of a negative/undefined ratio,
/// or division by zero), and negative `damping` flipped the decay
/// exponent's sign so the "settling" oscillation diverged to +-infinity
/// instead. Either poisons every transform/opacity value downstream once it
/// merges into `AnimatedProperties`. `validate_schema.rs` now rejects these
/// combinations as errors (belt), and this floor keeps the solver itself
/// finite and bounded even if an out-of-band caller skips validation
/// (suspenders) — see `spring_robustness_tests` below.
pub fn spring_value(t: f64, config: &SpringConfig) -> f64 {
    let damping = config.damping.max(0.0);
    let stiffness = config.stiffness.max(1e-6);
    let mass = config.mass.max(1e-6);

    let omega = (stiffness / mass).sqrt();
    let zeta = damping / (2.0 * (stiffness * mass).sqrt());

    if zeta < 1.0 {
        // Underdamped
        let omega_d = omega * (1.0 - zeta * zeta).sqrt();
        let decay = (-zeta * omega * t).exp();
        1.0 - decay
            * ((zeta * omega * t / omega_d).sin() * (zeta * omega / omega_d) + (omega_d * t).cos())
    } else if (zeta - 1.0).abs() < 1e-6 {
        // Critically damped
        let decay = (-omega * t).exp();
        1.0 - decay * (1.0 + omega * t)
    } else {
        // Overdamped
        let s1 = -omega * (zeta - (zeta * zeta - 1.0).sqrt());
        let s2 = -omega * (zeta + (zeta * zeta - 1.0).sqrt());
        let c2 = -s1 / (s2 - s1);
        let c1 = 1.0 - c2;
        1.0 - (c1 * (s1 * t).exp() + c2 * (s2 * t).exp())
    }
}

// ─── Animation resolver ─────────────────────────────────────────────────────

/// Resolved animated properties for a single layer at a specific frame
#[derive(Debug, Clone)]
pub struct AnimatedProperties {
    pub opacity: f32,
    pub translate_x: f32,
    pub translate_y: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub rotation: f32,
    pub blur: f32,
    /// For typewriter effect: number of visible characters (-1 = all)
    pub visible_chars: i32,
    /// For typewriter effect: progress 0.0→1.0 (-1.0 = unused, shows all)
    pub visible_chars_progress: f32,
    /// Animated color override (hex string)
    pub color: Option<String>,
    // Extended animatable properties
    pub border_radius: f32,
    pub font_size: f32,
    pub width: f32,
    pub height: f32,
    pub gap: f32,
    pub padding: f32,
    pub stroke_width: f32,
    pub shadow_blur: f32,
    pub glow_radius: f32,
    pub glow_intensity: f32,
    // 3D perspective transforms
    pub rotate_x: f32,
    pub rotate_y: f32,
    pub perspective: f32,
    // Path animation
    pub draw_progress: f32,
    // Motion path progress (0.0 = start, 1.0 = end)
    pub motion_progress: f32,
    // Char animation (from style.animation char_* variants)
    pub char_animation: Option<ResolvedCharAnimation>,
}

impl Default for AnimatedProperties {
    fn default() -> Self {
        Self {
            opacity: 1.0,
            translate_x: 0.0,
            translate_y: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            rotation: 0.0,
            blur: 0.0,
            visible_chars: -1,
            visible_chars_progress: -1.0,
            color: None,
            border_radius: -1.0,
            font_size: -1.0,
            width: -1.0,
            height: -1.0,
            gap: -1.0,
            padding: -1.0,
            stroke_width: -1.0,
            shadow_blur: -1.0,
            glow_radius: -1.0,
            glow_intensity: -1.0,
            rotate_x: 0.0,
            rotate_y: 0.0,
            perspective: -1.0,
            draw_progress: -1.0,
            motion_progress: -1.0,
            char_animation: None,
        }
    }
}

impl AnimatedProperties {
    /// Merge another AnimatedProperties into self. Properties that have been
    /// explicitly set in `other` (not sentinel -1.0) override values in self.
    pub fn merge(&mut self, other: &AnimatedProperties) {
        // opacity: default is 1.0, so only override if other explicitly animated to non-1.0
        // For opacity we multiply (both presets contribute)
        if (other.opacity - 1.0).abs() > 0.001 {
            self.opacity *= other.opacity;
        }
        if other.translate_x.abs() > 0.001 {
            self.translate_x += other.translate_x;
        }
        if other.translate_y.abs() > 0.001 {
            self.translate_y += other.translate_y;
        }
        if (other.scale_x - 1.0).abs() > 0.001 {
            self.scale_x *= other.scale_x;
        }
        if (other.scale_y - 1.0).abs() > 0.001 {
            self.scale_y *= other.scale_y;
        }
        if other.rotation.abs() > 0.01 {
            self.rotation += other.rotation;
        }
        if other.blur > 0.001 {
            self.blur = other.blur;
        }
        if other.visible_chars >= 0 {
            self.visible_chars = other.visible_chars;
        }
        if other.visible_chars_progress >= 0.0 {
            self.visible_chars_progress = other.visible_chars_progress;
        }
        if other.color.is_some() {
            self.color = other.color.clone();
        }
        // Sentinel-based fields (-1.0 = not set)
        if other.border_radius >= 0.0 {
            self.border_radius = other.border_radius;
        }
        if other.font_size >= 0.0 {
            self.font_size = other.font_size;
        }
        if other.width >= 0.0 {
            self.width = other.width;
        }
        if other.height >= 0.0 {
            self.height = other.height;
        }
        if other.gap >= 0.0 {
            self.gap = other.gap;
        }
        if other.padding >= 0.0 {
            self.padding = other.padding;
        }
        if other.stroke_width >= 0.0 {
            self.stroke_width = other.stroke_width;
        }
        if other.shadow_blur >= 0.0 {
            self.shadow_blur = other.shadow_blur;
        }
        if other.glow_radius >= 0.0 {
            self.glow_radius = other.glow_radius;
        }
        if other.glow_intensity >= 0.0 {
            self.glow_intensity = other.glow_intensity;
        }
        // 3D perspective transforms (additive like rotation)
        if other.rotate_x.abs() > 0.01 {
            self.rotate_x += other.rotate_x;
        }
        if other.rotate_y.abs() > 0.01 {
            self.rotate_y += other.rotate_y;
        }
        if other.perspective >= 0.0 {
            self.perspective = other.perspective;
        }
        if other.draw_progress >= 0.0 {
            self.draw_progress = other.draw_progress;
        }
        if other.motion_progress >= 0.0 {
            self.motion_progress = other.motion_progress;
        }
        if other.char_animation.is_some() {
            self.char_animation = other.char_animation.clone();
        }
    }
}

/// High-level helper: extract `effects`, resolve presets/keyframes/wiggles/orbits,
/// and propagate the char animation. Returns the resolved [`AnimatedProperties`]
/// at `time` within a scene of `scene_duration` seconds.
///
/// Used by both the legacy render pipeline and the new paint-tree dispatcher
/// so they share the exact same animation semantics.
pub fn resolve_props_for_effects(
    effects: &[AnimationEffect],
    time: f64,
    scene_duration: f64,
) -> AnimatedProperties {
    let mut props = AnimatedProperties::default();
    if effects.is_empty() {
        return props;
    }
    let extracted = extract_effects(effects);

    for (preset, preset_config) in &extracted.presets {
        let p = resolve_animations(&[], Some(preset), Some(preset_config), time, scene_duration);
        props.merge(&p);
    }
    // Every `keyframes`/`tilt_in` effect is resolved together in one call
    // (constat #5): within a single `resolve_animations` call, multiple
    // `Animation`s targeting the same property are applied in list order via
    // `apply_property` (assignment, not addition), so the *last* effect in
    // `style.animation` wins on a shared property — deterministic, and
    // independent of any effect's `delay`. `keyframes_loop` (constat #7)
    // carries `"loop": true` from any contributing effect into the solver,
    // which `resolve_animations` used to never see (it was always called
    // with `preset_config = None`, i.e. `repeat = false`).
    if !extracted.keyframe_animations.is_empty() {
        let loop_cfg = PresetConfig {
            repeat: extracted.keyframes_loop,
            ..Default::default()
        };
        let kp = resolve_animations(
            &extracted.keyframe_animations,
            None,
            Some(&loop_cfg),
            time,
            scene_duration,
        );
        props.merge(&kp);
    }
    if !extracted.wiggles.is_empty() {
        let wiggles: Vec<_> = extracted.wiggles.iter().copied().cloned().collect();
        apply_wiggles(&mut props, &wiggles, time);
    }
    if !extracted.orbits.is_empty() {
        let orbits: Vec<_> = extracted.orbits.iter().copied().cloned().collect();
        apply_orbits(&mut props, &orbits, time);
    }
    if extracted.char_animation.is_some() {
        props.char_animation = extracted.char_animation;
    }
    props
}

/// Resolve animations for a layer at a specific time (seconds) within the scene
pub fn resolve_animations(
    animations: &[Animation],
    preset: Option<&AnimationPreset>,
    preset_config: Option<&PresetConfig>,
    time: f64,
    scene_duration: f64,
) -> AnimatedProperties {
    let mut props = AnimatedProperties::default();

    let config = preset_config.cloned().unwrap_or_default();
    let should_loop = config.repeat;

    // First, expand preset into animations
    let preset_animations = preset.map(|p| expand_preset(p, &config, scene_duration));

    // Merge preset animations with explicit animations (explicit wins on conflict)
    let all_animations: Vec<&Animation> = preset_animations
        .as_ref()
        .map(|pa| pa.iter().collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .chain(animations.iter())
        .collect();

    for anim in all_animations {
        let anim_time = if should_loop {
            loop_time(anim, time)
        } else {
            time
        };
        let resolved = resolve_animation_value_full(anim, anim_time);
        match resolved {
            ResolvedValue::Number(value) => apply_property(&mut props, &anim.property, value),
            ResolvedValue::Color(color) => {
                if anim.property == "color" {
                    props.color = Some(color);
                }
            }
        }
    }

    props
}

/// Wrap time within the animation's keyframe range for looping
fn loop_time(anim: &Animation, time: f64) -> f64 {
    let keyframes = &anim.keyframes;
    if keyframes.len() < 2 {
        return time;
    }
    let start = keyframes.first().unwrap().time;
    let end = keyframes.last().unwrap().time;
    let duration = end - start;
    if duration < 1e-9 || time < start {
        return time;
    }
    start + ((time - start) % duration)
}

/// Result of resolving an animation value — either a number or a color
enum ResolvedValue {
    Number(f64),
    Color(String),
}

fn resolve_animation_value_full(anim: &Animation, time: f64) -> ResolvedValue {
    let keyframes = &anim.keyframes;
    if keyframes.is_empty() {
        return ResolvedValue::Number(0.0);
    }
    if keyframes.len() == 1 {
        return match &keyframes[0].value {
            KeyframeValue::Color(c) => ResolvedValue::Color(c.clone()),
            KeyframeValue::Number(n) => ResolvedValue::Number(*n),
        };
    }

    if time <= keyframes[0].time {
        return match &keyframes[0].value {
            KeyframeValue::Color(c) => ResolvedValue::Color(c.clone()),
            KeyframeValue::Number(n) => ResolvedValue::Number(*n),
        };
    }
    if time >= keyframes.last().unwrap().time {
        return match &keyframes.last().unwrap().value {
            KeyframeValue::Color(c) => ResolvedValue::Color(c.clone()),
            KeyframeValue::Number(n) => ResolvedValue::Number(*n),
        };
    }

    for i in 0..keyframes.len() - 1 {
        let kf0 = &keyframes[i];
        let kf1 = &keyframes[i + 1];

        if time >= kf0.time && time <= kf1.time {
            let segment_duration = kf1.time - kf0.time;
            if segment_duration < 1e-9 {
                return match &kf1.value {
                    KeyframeValue::Color(c) => ResolvedValue::Color(c.clone()),
                    KeyframeValue::Number(n) => ResolvedValue::Number(*n),
                };
            }

            let local_t = (time - kf0.time) / segment_duration;

            // Use per-keyframe easing if specified, otherwise fall back to animation-level easing
            let segment_easing = kf0.easing.as_ref().unwrap_or(&anim.easing);

            let progress = match segment_easing {
                EasingType::Spring => {
                    let spring_config = anim.spring.clone().unwrap_or_default();
                    spring_value(local_t * segment_duration, &spring_config)
                }
                other => ease(local_t, other),
            };

            // Check if both keyframes are colors
            if let (KeyframeValue::Color(c0), KeyframeValue::Color(c1)) = (&kf0.value, &kf1.value) {
                return ResolvedValue::Color(lerp_color(c0, c1, progress));
            }

            let v0 = kf0.value.as_f64();
            let v1 = kf1.value.as_f64();
            return ResolvedValue::Number(v0 + (v1 - v0) * progress);
        }
    }

    match &keyframes.last().unwrap().value {
        KeyframeValue::Color(c) => ResolvedValue::Color(c.clone()),
        KeyframeValue::Number(n) => ResolvedValue::Number(*n),
    }
}

/// Parse hex color to (r, g, b, a) as f64 components (0-255)
fn parse_hex_components(hex: &str) -> (f64, f64, f64, f64) {
    let (r, g, b, a) = super::renderer::parse_hex_color(hex);
    (r as f64, g as f64, b as f64, a as f64)
}

/// Interpolate between two hex colors
fn lerp_color(c1: &str, c2: &str, t: f64) -> String {
    let (r1, g1, b1, a1) = parse_hex_components(c1);
    let (r2, g2, b2, a2) = parse_hex_components(c2);
    let r = (r1 + (r2 - r1) * t).clamp(0.0, 255.0) as u8;
    let g = (g1 + (g2 - g1) * t).clamp(0.0, 255.0) as u8;
    let b = (b1 + (b2 - b1) * t).clamp(0.0, 255.0) as u8;
    let a = (a1 + (a2 - a1) * t).clamp(0.0, 255.0) as u8;
    if a == 255 {
        format!("#{:02X}{:02X}{:02X}", r, g, b)
    } else {
        format!("#{:02X}{:02X}{:02X}{:02X}", r, g, b, a)
    }
}

fn apply_property(props: &mut AnimatedProperties, property: &str, value: f64) {
    match property {
        "opacity" => props.opacity = value as f32,
        "position.x" | "translate_x" => props.translate_x = value as f32,
        "position.y" | "translate_y" => props.translate_y = value as f32,
        "scale" => {
            props.scale_x = value as f32;
            props.scale_y = value as f32;
        }
        "scale.x" => props.scale_x = value as f32,
        "scale.y" => props.scale_y = value as f32,
        "rotation" => props.rotation = value as f32,
        "blur" => props.blur = value as f32,
        "visible_chars" => props.visible_chars = value as i32,
        "visible_chars_progress" => props.visible_chars_progress = value as f32,
        "border_radius" => props.border_radius = value as f32,
        "font_size" => props.font_size = value as f32,
        "width" => props.width = value as f32,
        "height" => props.height = value as f32,
        "gap" => props.gap = value as f32,
        "padding" => props.padding = value as f32,
        "stroke_width" => props.stroke_width = value as f32,
        "shadow_blur" => props.shadow_blur = value as f32,
        "glow_radius" => props.glow_radius = value as f32,
        "glow_intensity" => props.glow_intensity = value as f32,
        "rotate_x" => props.rotate_x = value as f32,
        "rotate_y" => props.rotate_y = value as f32,
        "perspective" => props.perspective = value as f32,
        "draw_progress" => props.draw_progress = value as f32,
        "motion_progress" => props.motion_progress = value as f32,
        _ => {} // Unknown property, ignore
    }
}

// ─── Wiggle resolution ──────────────────────────────────────────────────────

/// Simple noise function based on sine waves with seed for pseudo-random behavior
fn simplex_noise_1d(x: f64, seed: u64) -> f64 {
    use std::f64::consts::TAU;
    let s = seed as f64;

    (x * TAU + s * 0.1234).sin() * 0.6
        + (x * TAU * 1.7 + s * 0.5678).sin() * 0.3
        + (x * TAU * 2.9 + s * 0.9012).sin() * 0.1 // roughly -1..1
}

/// Parameterized noise function with configurable octaves
fn simplex_noise_1d_ext(x: f64, seed: u64, octaves: u32) -> f64 {
    use std::f64::consts::TAU;
    let s = seed as f64;
    let mut value = 0.0;
    let mut amplitude = 0.5;
    let mut total_amplitude = 0.0;
    for i in 0..octaves {
        let freq = 1.0 + i as f64 * 1.3;
        let phase_offset = s * (0.1234 + i as f64 * 0.4444);
        value += (x * TAU * freq + phase_offset).sin() * amplitude;
        total_amplitude += amplitude;
        amplitude *= 0.5;
    }
    if total_amplitude > 0.0 {
        value / total_amplitude
    } else {
        0.0
    }
}

/// Apply wiggle offsets additively to animated properties
pub fn apply_wiggles(props: &mut AnimatedProperties, wiggles: &[WiggleConfig], time: f64) {
    for wiggle in wiggles {
        let has_extras = wiggle.octaves.is_some()
            || wiggle.phase.is_some()
            || wiggle.decay.is_some()
            || wiggle.easing.is_some();

        let phase = wiggle.phase.unwrap_or(0.0);
        let input = time * wiggle.frequency + phase;

        let is_sine = wiggle.mode.as_deref() == Some("sine");

        let mut noise_val = if is_sine {
            input.sin()
        } else if has_extras {
            let octaves = wiggle.octaves.unwrap_or(3);
            simplex_noise_1d_ext(input, wiggle.seed, octaves)
        } else {
            simplex_noise_1d(input, wiggle.seed)
        };

        // Apply easing: normalize [-1,1] → [0,1], ease, remap to [-1,1]
        if let Some(ref easing) = wiggle.easing {
            let normalized = (noise_val + 1.0) * 0.5;
            let eased = ease(normalized, easing);
            noise_val = eased * 2.0 - 1.0;
        }

        let mut amp = wiggle.amplitude;

        // Apply exponential decay
        if let Some(decay) = wiggle.decay {
            amp *= (-decay * time).exp();
        }

        let offset = amp * noise_val;
        apply_property(
            props,
            &wiggle.property,
            get_property_value(props, &wiggle.property) + offset,
        );
    }
}

/// Apply orbit effects additively to animated properties.
/// Creates circular/elliptical motion with pseudo-3D depth via scale and opacity modulation.
pub fn apply_orbits(props: &mut AnimatedProperties, orbits: &[OrbitConfig], time: f64) {
    use std::f64::consts::{PI, TAU};

    for orbit in orbits {
        let angle_offset = orbit.start_angle * PI / 180.0;
        let phase_offset = orbit.phase * TAU;
        let tilt_rad = orbit.tilt * PI / 180.0;

        let theta = TAU * orbit.speed * time + angle_offset + phase_offset;

        // Elliptical orbit position
        let raw_x = orbit.radius_x * theta.cos();
        let raw_y = orbit.radius_y * theta.sin();

        // Apply tilt: compress Y axis and add depth effect
        let x_offset = raw_x;
        let y_offset = raw_y * tilt_rad.cos();

        props.translate_x += x_offset as f32;
        props.translate_y += y_offset as f32;

        // Pseudo-depth: when "behind" (sin < 0), scale down and reduce opacity
        if orbit.depth > 0.0 {
            // depth_factor goes from (1 - depth) to (1 + depth) based on orbit position
            let depth_sin = if tilt_rad.abs() > 0.01 {
                // With tilt, depth is based on the untilted Y (how far "back" the object is)
                theta.sin()
            } else {
                // Without tilt, use Y component for depth
                theta.sin()
            };
            let scale_factor = 1.0 + orbit.depth * depth_sin;
            props.scale_x *= scale_factor as f32;
            props.scale_y *= scale_factor as f32;
        }

        // Opacity modulation for depth
        if orbit.opacity_depth > 0.0 {
            let depth_sin = theta.sin();
            let opacity_factor = 1.0 - orbit.opacity_depth * (1.0 - depth_sin) * 0.5;
            props.opacity *= opacity_factor as f32;
        }
    }
}

fn get_property_value(props: &AnimatedProperties, property: &str) -> f64 {
    match property {
        "opacity" => props.opacity as f64,
        "position.x" | "translate_x" => props.translate_x as f64,
        "position.y" | "translate_y" => props.translate_y as f64,
        "scale" => props.scale_x as f64,
        "scale.x" => props.scale_x as f64,
        "scale.y" => props.scale_y as f64,
        "rotation" => props.rotation as f64,
        "blur" => props.blur as f64,
        "border_radius" => props.border_radius as f64,
        "font_size" => props.font_size as f64,
        "width" => props.width as f64,
        "height" => props.height as f64,
        "gap" => props.gap as f64,
        "padding" => props.padding as f64,
        "stroke_width" => props.stroke_width as f64,
        "shadow_blur" => props.shadow_blur as f64,
        "glow_radius" => props.glow_radius as f64,
        "glow_intensity" => props.glow_intensity as f64,
        "rotate_x" => props.rotate_x as f64,
        "rotate_y" => props.rotate_y as f64,
        "perspective" => props.perspective as f64,
        "draw_progress" => props.draw_progress as f64,
        "motion_progress" => props.motion_progress as f64,
        _ => 0.0,
    }
}

// ─── Preset expansion ───────────────────────────────────────────────────────

/// Properties eligible for the preset-level `spring` override: motion only.
/// Opacity keeps its ease (an alpha overshoot flashes), blur/draw_progress
/// would go out of range on overshoot.
fn is_motion_property(property: &str) -> bool {
    matches!(
        property,
        "position.x"
            | "position.y"
            | "translate_x"
            | "translate_y"
            | "scale"
            | "scale.x"
            | "scale.y"
            | "rotation"
            | "rotate_x"
            | "rotate_y"
    )
}

/// Apply a user-provided spring to a preset's motion animations (issue #88).
///
/// Implementation note: a single generic post-processing pass was chosen over
/// editing each of the ~40 preset builders — the eligibility rules are uniform
/// and the builders stay oblivious to springs. Rules per animation:
/// - non-motion property (opacity, blur, …): untouched;
/// - 2 keyframes: easing → `Spring` with the given config. For `bounce_in` /
///   `elastic_in` this *overrides* their built-in spring, which thereby acts
///   as the default when no user config is provided;
/// - more than 2 keyframes with different endpoints (manual-overshoot
///   entrances like `scale_in`): collapsed to [first, last] + spring — the
///   spring supplies the overshoot itself, keeping the manual peak would
///   double it;
/// - more than 2 keyframes with identical endpoints (continuous oscillators:
///   pulse, shake, float): untouched — a spring toward the same value is a
///   no-op and would freeze the effect.
fn apply_spring_to_motion(animations: &mut [Animation], spring: &SpringConfig) {
    for anim in animations.iter_mut() {
        if !is_motion_property(&anim.property) || anim.keyframes.len() < 2 {
            continue;
        }
        if anim.keyframes.len() > 2 {
            let first = anim.keyframes.first().unwrap().clone();
            let last = anim.keyframes.last().unwrap().clone();
            if (first.value.as_f64() - last.value.as_f64()).abs() < 1e-9 {
                continue; // oscillator — leave its shape alone
            }
            anim.keyframes = vec![first, last];
        }
        anim.easing = EasingType::Spring;
        anim.spring = Some(spring.clone());
    }
}

fn expand_preset(
    preset: &AnimationPreset,
    config: &PresetConfig,
    _scene_duration: f64,
) -> Vec<Animation> {
    let mut animations = expand_preset_inner(preset, config);
    if let Some(spring) = &config.spring {
        apply_spring_to_motion(&mut animations, spring);
    }
    animations
}

fn expand_preset_inner(preset: &AnimationPreset, config: &PresetConfig) -> Vec<Animation> {
    let delay = config.delay;
    let dur = config.duration;
    let end = delay + dur;

    match preset {
        // ── Entrées ──────────────────────────────────────────────────────
        AnimationPreset::FadeIn => vec![kf_anim(
            "opacity",
            delay,
            0.0,
            end,
            1.0,
            EasingType::EaseOut,
        )],
        AnimationPreset::FadeInUp => vec![
            kf_anim("opacity", delay, 0.0, end, 1.0, EasingType::EaseOut),
            kf_anim(
                "position.y",
                delay,
                60.0,
                end,
                0.0,
                EasingType::EaseOutCubic,
            ),
        ],
        AnimationPreset::FadeInDown => vec![
            kf_anim("opacity", delay, 0.0, end, 1.0, EasingType::EaseOut),
            kf_anim(
                "position.y",
                delay,
                -60.0,
                end,
                0.0,
                EasingType::EaseOutCubic,
            ),
        ],
        AnimationPreset::FadeInLeft => vec![
            kf_anim("opacity", delay, 0.0, end, 1.0, EasingType::EaseOut),
            kf_anim(
                "position.x",
                delay,
                -60.0,
                end,
                0.0,
                EasingType::EaseOutCubic,
            ),
        ],
        AnimationPreset::FadeInRight => vec![
            kf_anim("opacity", delay, 0.0, end, 1.0, EasingType::EaseOut),
            kf_anim(
                "position.x",
                delay,
                60.0,
                end,
                0.0,
                EasingType::EaseOutCubic,
            ),
        ],
        AnimationPreset::SlideInLeft => vec![
            kf_anim(
                "opacity",
                delay,
                0.0,
                delay + dur * 0.3,
                1.0,
                EasingType::EaseOut,
            ),
            kf_anim(
                "position.x",
                delay,
                -200.0,
                end,
                0.0,
                EasingType::EaseOutCubic,
            ),
        ],
        AnimationPreset::SlideInRight => vec![
            kf_anim(
                "opacity",
                delay,
                0.0,
                delay + dur * 0.3,
                1.0,
                EasingType::EaseOut,
            ),
            kf_anim(
                "position.x",
                delay,
                200.0,
                end,
                0.0,
                EasingType::EaseOutCubic,
            ),
        ],
        AnimationPreset::SlideInUp => vec![
            kf_anim(
                "opacity",
                delay,
                0.0,
                delay + dur * 0.3,
                1.0,
                EasingType::EaseOut,
            ),
            kf_anim(
                "position.y",
                delay,
                200.0,
                end,
                0.0,
                EasingType::EaseOutCubic,
            ),
        ],
        AnimationPreset::SlideInDown => vec![
            kf_anim(
                "opacity",
                delay,
                0.0,
                delay + dur * 0.3,
                1.0,
                EasingType::EaseOut,
            ),
            kf_anim(
                "position.y",
                delay,
                -200.0,
                end,
                0.0,
                EasingType::EaseOutCubic,
            ),
        ],
        AnimationPreset::ScaleIn => {
            let overshoot = config.overshoot.unwrap_or(0.08);
            vec![
                kf_anim(
                    "opacity",
                    delay,
                    0.0,
                    delay + dur * 0.3,
                    1.0,
                    EasingType::EaseOut,
                ),
                Animation {
                    property: "scale".to_string(),
                    keyframes: vec![
                        kf(delay, 0.0),
                        kf(delay + dur * 0.7, 1.0 + overshoot),
                        kf(end, 1.0),
                    ],
                    easing: EasingType::EaseOutCubic,
                    spring: None,
                },
            ]
        }
        AnimationPreset::BounceIn => vec![
            kf_anim(
                "opacity",
                delay,
                0.0,
                delay + dur * 0.2,
                1.0,
                EasingType::EaseOut,
            ),
            kf_anim_spring("scale", delay, 0.3, end, 1.0),
        ],
        AnimationPreset::BlurIn => vec![
            kf_anim("opacity", delay, 0.0, end, 1.0, EasingType::EaseOut),
            kf_anim("blur", delay, 20.0, end, 0.0, EasingType::EaseOutCubic),
        ],
        AnimationPreset::RotateIn => vec![
            kf_anim("opacity", delay, 0.0, end, 1.0, EasingType::EaseOut),
            kf_anim("rotation", delay, -90.0, end, 0.0, EasingType::EaseOutCubic),
            kf_anim("scale", delay, 0.5, end, 1.0, EasingType::EaseOutCubic),
        ],
        AnimationPreset::ElasticIn => {
            vec![kf_anim_spring_underdamped("scale", delay, 0.0, end, 1.0)]
        }

        // ── Sorties ──────────────────────────────────────────────────────
        AnimationPreset::FadeOut => {
            vec![kf_anim("opacity", delay, 1.0, end, 0.0, EasingType::EaseIn)]
        }
        AnimationPreset::FadeOutUp => vec![
            kf_anim("opacity", delay, 1.0, end, 0.0, EasingType::EaseIn),
            kf_anim(
                "position.y",
                delay,
                0.0,
                end,
                -60.0,
                EasingType::EaseInCubic,
            ),
        ],
        AnimationPreset::FadeOutDown => vec![
            kf_anim("opacity", delay, 1.0, end, 0.0, EasingType::EaseIn),
            kf_anim("position.y", delay, 0.0, end, 60.0, EasingType::EaseInCubic),
        ],
        AnimationPreset::SlideOutLeft => vec![
            kf_anim(
                "opacity",
                delay + dur * 0.7,
                1.0,
                end,
                0.0,
                EasingType::EaseIn,
            ),
            kf_anim(
                "position.x",
                delay,
                0.0,
                end,
                -200.0,
                EasingType::EaseInCubic,
            ),
        ],
        AnimationPreset::SlideOutRight => vec![
            kf_anim(
                "opacity",
                delay + dur * 0.7,
                1.0,
                end,
                0.0,
                EasingType::EaseIn,
            ),
            kf_anim(
                "position.x",
                delay,
                0.0,
                end,
                200.0,
                EasingType::EaseInCubic,
            ),
        ],
        AnimationPreset::SlideOutUp => vec![
            kf_anim(
                "opacity",
                delay + dur * 0.7,
                1.0,
                end,
                0.0,
                EasingType::EaseIn,
            ),
            kf_anim(
                "position.y",
                delay,
                0.0,
                end,
                -200.0,
                EasingType::EaseInCubic,
            ),
        ],
        AnimationPreset::SlideOutDown => vec![
            kf_anim(
                "opacity",
                delay + dur * 0.7,
                1.0,
                end,
                0.0,
                EasingType::EaseIn,
            ),
            kf_anim(
                "position.y",
                delay,
                0.0,
                end,
                200.0,
                EasingType::EaseInCubic,
            ),
        ],
        AnimationPreset::ScaleOut => {
            let overshoot = config.overshoot.unwrap_or(0.08);
            vec![
                kf_anim(
                    "opacity",
                    delay + dur * 0.7,
                    1.0,
                    end,
                    0.0,
                    EasingType::EaseIn,
                ),
                Animation {
                    property: "scale".to_string(),
                    keyframes: vec![
                        kf(delay, 1.0),
                        kf(delay + dur * 0.2, 1.0 + overshoot),
                        kf(end, 0.0),
                    ],
                    easing: EasingType::EaseInCubic,
                    spring: None,
                },
            ]
        }
        AnimationPreset::BounceOut => vec![
            kf_anim(
                "opacity",
                delay + dur * 0.8,
                1.0,
                end,
                0.0,
                EasingType::EaseIn,
            ),
            kf_anim_spring("scale", delay, 1.0, end, 0.3),
        ],
        AnimationPreset::BlurOut => vec![
            kf_anim("opacity", delay, 1.0, end, 0.0, EasingType::EaseIn),
            kf_anim("blur", delay, 0.0, end, 20.0, EasingType::EaseInCubic),
        ],
        AnimationPreset::RotateOut => vec![
            kf_anim("opacity", delay, 1.0, end, 0.0, EasingType::EaseIn),
            kf_anim("rotation", delay, 0.0, end, 90.0, EasingType::EaseInCubic),
            kf_anim("scale", delay, 1.0, end, 0.5, EasingType::EaseInCubic),
        ],

        // ── Effets continus ──────────────────────────────────────────────
        // `delay`/`duration` used to be decorative here: the keyframes were
        // pinned to literal times 0.0/0.25/0.5/1.0 regardless of what the
        // scenario authored (constat #2), so every pulsing/floating/shaking/
        // spinning element in a scene shared one hardcoded 1-second cycle
        // starting at t=0. `delay` now shifts the cycle's start and
        // `duration` sets its length, exactly like every other preset.
        AnimationPreset::Pulse => vec![kf_anim_3kf_over(
            "scale",
            delay,
            end,
            0.95,
            1.05,
            0.95,
            EasingType::EaseInOut,
        )],
        AnimationPreset::Float => vec![kf_anim_3kf_over(
            "position.y",
            delay,
            end,
            0.0,
            -10.0,
            0.0,
            EasingType::EaseInOut,
        )],
        AnimationPreset::Shake => vec![kf_anim_4kf_over(
            "position.x",
            delay,
            end,
            0.0,
            10.0,
            -10.0,
            0.0,
            EasingType::EaseInOut,
        )],
        AnimationPreset::Spin => vec![kf_anim(
            "rotation",
            delay,
            0.0,
            end,
            360.0,
            EasingType::Linear,
        )],

        // ── 3D ───────────────────────────────────────────────────────────
        AnimationPreset::FlipInX => vec![
            kf_anim(
                "opacity",
                delay,
                0.0,
                delay + dur * 0.3,
                1.0,
                EasingType::EaseOut,
            ),
            kf_anim("rotate_x", delay, 90.0, end, 0.0, EasingType::EaseOutCubic),
            kf_anim("perspective", delay, 800.0, end, 800.0, EasingType::Linear),
        ],
        AnimationPreset::FlipInY => vec![
            kf_anim(
                "opacity",
                delay,
                0.0,
                delay + dur * 0.3,
                1.0,
                EasingType::EaseOut,
            ),
            kf_anim("rotate_y", delay, 90.0, end, 0.0, EasingType::EaseOutCubic),
            kf_anim("perspective", delay, 800.0, end, 800.0, EasingType::Linear),
        ],
        AnimationPreset::FlipOutX => vec![
            kf_anim(
                "opacity",
                delay + dur * 0.7,
                1.0,
                end,
                0.0,
                EasingType::EaseIn,
            ),
            kf_anim("rotate_x", delay, 0.0, end, -90.0, EasingType::EaseInCubic),
            kf_anim("perspective", delay, 800.0, end, 800.0, EasingType::Linear),
        ],
        AnimationPreset::FlipOutY => vec![
            kf_anim(
                "opacity",
                delay + dur * 0.7,
                1.0,
                end,
                0.0,
                EasingType::EaseIn,
            ),
            kf_anim("rotate_y", delay, 0.0, end, -90.0, EasingType::EaseInCubic),
            kf_anim("perspective", delay, 800.0, end, 800.0, EasingType::Linear),
        ],
        AnimationPreset::TiltIn => vec![
            kf_anim(
                "opacity",
                delay,
                0.0,
                delay + dur * 0.3,
                1.0,
                EasingType::EaseOut,
            ),
            kf_anim("rotate_x", delay, 15.0, end, 0.0, EasingType::EaseOutCubic),
            kf_anim("rotate_y", delay, -15.0, end, 0.0, EasingType::EaseOutCubic),
            kf_anim(
                "perspective",
                delay,
                1000.0,
                end,
                1000.0,
                EasingType::Linear,
            ),
            kf_anim("scale", delay, 0.9, end, 1.0, EasingType::EaseOutCubic),
        ],

        // ── Floating/orbit ────────────────────────────────────────────
        AnimationPreset::Float3d => {
            // The cycle spans delay..delay+duration, so `duration` sets the
            // period and `delay` shifts the phase.
            //
            // Both were previously inert: the keyframes were pinned to 0.0 /
            // 0.5 / 1.0 seconds, so every floating element in a scene shared
            // one 1-second cycle and moved in lockstep no matter what the
            // scenario asked for. A row of cards bobbing in unison reads as a
            // dance; the same cards on different phases and travels read as
            // depth, which is the point of the preset.
            let amp = config.amplitude.unwrap_or(12.0);
            let tilt = amp / 12.0;
            vec![
                kf_anim_3kf_over(
                    "position.y",
                    delay,
                    end,
                    0.0,
                    -amp,
                    0.0,
                    EasingType::EaseInOut,
                ),
                kf_anim_3kf_over(
                    "rotate_x",
                    delay,
                    end,
                    0.0,
                    5.0 * tilt,
                    0.0,
                    EasingType::EaseInOut,
                ),
                kf_anim_3kf_over(
                    "rotate_y",
                    delay,
                    end,
                    0.0,
                    -8.0 * tilt,
                    0.0,
                    EasingType::EaseInOut,
                ),
                kf_anim(
                    "perspective",
                    delay,
                    1000.0,
                    end,
                    1000.0,
                    EasingType::Linear,
                ),
            ]
        }

        // ── Spéciaux ────────────────────────────────────────────────────
        AnimationPreset::DrawIn => vec![kf_anim(
            "draw_progress",
            delay,
            0.0,
            end,
            1.0,
            EasingType::EaseInOut,
        )],
        AnimationPreset::StrokeReveal => vec![
            kf_anim("draw_progress", delay, 0.0, end, 1.0, EasingType::EaseOut),
            kf_anim(
                "opacity",
                delay,
                0.0,
                delay + dur * 0.2,
                1.0,
                EasingType::EaseOut,
            ),
        ],
        AnimationPreset::Typewriter => vec![kf_anim(
            "visible_chars_progress",
            delay,
            0.0,
            end,
            1.0,
            EasingType::Linear,
        )],
        AnimationPreset::WipeLeft => vec![
            kf_anim(
                "opacity",
                delay,
                0.0,
                delay + dur * 0.3,
                1.0,
                EasingType::EaseOut,
            ),
            kf_anim("position.x", delay, -200.0, end, 0.0, EasingType::EaseInOut),
        ],
        AnimationPreset::WipeRight => vec![
            kf_anim(
                "opacity",
                delay,
                0.0,
                delay + dur * 0.3,
                1.0,
                EasingType::EaseOut,
            ),
            kf_anim("position.x", delay, 200.0, end, 0.0, EasingType::EaseInOut),
        ],
    }
}

fn kf(time: f64, value: f64) -> Keyframe {
    Keyframe {
        time,
        value: KeyframeValue::Number(value),
        easing: None,
    }
}

fn kf_anim(property: &str, t0: f64, v0: f64, t1: f64, v1: f64, easing: EasingType) -> Animation {
    Animation {
        property: property.to_string(),
        keyframes: vec![kf(t0, v0), kf(t1, v1)],
        easing,
        spring: None,
    }
}

fn kf_anim_spring(property: &str, t0: f64, v0: f64, t1: f64, v1: f64) -> Animation {
    Animation {
        property: property.to_string(),
        keyframes: vec![kf(t0, v0), kf(t1, v1)],
        easing: EasingType::Spring,
        spring: Some(SpringConfig {
            damping: 12.0,
            stiffness: 100.0,
            mass: 1.0,
        }),
    }
}

fn kf_anim_spring_underdamped(property: &str, t0: f64, v0: f64, t1: f64, v1: f64) -> Animation {
    Animation {
        property: property.to_string(),
        keyframes: vec![kf(t0, v0), kf(t1, v1)],
        easing: EasingType::Spring,
        spring: Some(SpringConfig {
            damping: 6.0,
            stiffness: 120.0,
            mass: 1.0,
        }),
    }
}

/// Three-keyframe oscillation laid out over an explicit `start..end` window,
/// so the caller controls both when it begins and how long one cycle lasts.
fn kf_anim_3kf_over(
    property: &str,
    start: f64,
    end: f64,
    v0: f64,
    v1: f64,
    v2: f64,
    easing: EasingType,
) -> Animation {
    Animation {
        property: property.to_string(),
        keyframes: vec![kf(start, v0), kf((start + end) / 2.0, v1), kf(end, v2)],
        easing,
        spring: None,
    }
}

/// Four-keyframe oscillation (quarter/half/end split) laid out over an
/// explicit `start..end` window — the `shake` counterpart to
/// `kf_anim_3kf_over`.
#[allow(clippy::too_many_arguments)]
fn kf_anim_4kf_over(
    property: &str,
    start: f64,
    end: f64,
    v0: f64,
    v1: f64,
    v2: f64,
    v3: f64,
    easing: EasingType,
) -> Animation {
    let quarter = (end - start) / 4.0;
    Animation {
        property: property.to_string(),
        keyframes: vec![
            kf(start, v0),
            kf(start + quarter, v1),
            kf(start + quarter * 2.0, v2),
            kf(end, v3),
        ],
        easing,
        spring: None,
    }
}

#[cfg(test)]
mod spring_preset_tests {
    //! TDD tests for issue #88: spring easing on any preset via
    //! `AnimationTiming.spring`.

    use super::*;
    use crate::schema::AnimationEffect;
    use crate::schema::AnimationTiming;

    fn timing(duration: f64, spring: Option<SpringConfig>) -> AnimationTiming {
        AnimationTiming {
            duration,
            spring,
            ..Default::default()
        }
    }

    fn underdamped() -> SpringConfig {
        SpringConfig {
            damping: 8.0,
            stiffness: 120.0,
            mass: 1.0,
        }
    }

    /// Sample translate_y and opacity over the animation window.
    fn sample(effects: &[AnimationEffect], duration: f64) -> Vec<(f64, f64, f64)> {
        let steps = 80;
        (0..=steps)
            .map(|i| {
                let t = duration * i as f64 / steps as f64;
                let p = resolve_props_for_effects(effects, t, 5.0);
                (t, p.translate_y as f64, p.opacity as f64)
            })
            .collect()
    }

    #[test]
    fn fade_in_up_spring_overshoots_position() {
        // Without spring: translate_y eases 60 → 0, never negative.
        let plain = sample(&[AnimationEffect::FadeInUp(timing(0.8, None))], 0.8);
        let min_plain = plain.iter().map(|(_, y, _)| *y).fold(f64::MAX, f64::min);
        assert!(
            min_plain >= -0.01,
            "without spring translate_y must never overshoot below 0, got min {min_plain}"
        );

        // With an underdamped spring: the position overshoots past the final
        // value (goes measurably negative) somewhere inside the window.
        let sprung = sample(
            &[AnimationEffect::FadeInUp(timing(0.8, Some(underdamped())))],
            0.8,
        );
        let min_sprung = sprung.iter().map(|(_, y, _)| *y).fold(f64::MAX, f64::min);
        assert!(
            min_sprung < -0.5,
            "with spring translate_y must overshoot below 0, got min {min_sprung}"
        );

        // At ~70% of the duration the two positions differ measurably.
        let y_plain_70 = plain[56].1;
        let y_sprung_70 = sprung[56].1;
        assert!(
            (y_plain_70 - y_sprung_70).abs() > 0.5,
            "at 70% duration spring vs plain must differ: {y_plain_70} vs {y_sprung_70}"
        );
    }

    #[test]
    fn fade_in_up_spring_does_not_touch_opacity() {
        let plain = sample(&[AnimationEffect::FadeInUp(timing(0.8, None))], 0.8);
        let sprung = sample(
            &[AnimationEffect::FadeInUp(timing(0.8, Some(underdamped())))],
            0.8,
        );
        for (i, ((_, _, a_plain), (_, _, a_sprung))) in plain.iter().zip(sprung.iter()).enumerate()
        {
            assert!(
                (a_plain - a_sprung).abs() < 1e-6,
                "opacity must be identical with/without spring at sample {i}: {a_plain} vs {a_sprung}"
            );
        }
        // And alpha stays monotone non-decreasing (no overshoot flashes).
        for w in sprung.windows(2) {
            assert!(
                w[1].2 >= w[0].2 - 1e-6,
                "opacity must be monotone, got {} then {}",
                w[0].2,
                w[1].2
            );
        }
    }

    #[test]
    fn bounce_in_custom_spring_differs_from_default() {
        let scale_at = |spring: Option<SpringConfig>, t: f64| -> f64 {
            let fx = [AnimationEffect::BounceIn(timing(0.8, spring))];
            resolve_props_for_effects(&fx, t, 5.0).scale_x as f64
        };
        // Default (damping 12/stiffness 100) vs a heavily overdamped custom
        // spring must produce different scales mid-flight.
        let overdamped = SpringConfig {
            damping: 40.0,
            stiffness: 100.0,
            mass: 1.0,
        };
        let d = scale_at(None, 0.3);
        let c = scale_at(Some(overdamped), 0.3);
        assert!(
            (d - c).abs() > 0.01,
            "custom spring must change bounce_in: default {d} vs custom {c}"
        );
    }

    #[test]
    fn scale_in_spring_collapses_manual_overshoot() {
        // ScaleIn's 3-keyframe manual overshoot (0 → 1.08 → 1) collapses to a
        // 2-keyframe spring (0 → 1): the spring provides the overshoot itself,
        // so scale must exceed 1.0 somewhere (underdamped) and converge to 1.
        let fx = [AnimationEffect::ScaleIn(timing(0.8, Some(underdamped())))];
        let mut max_scale = f64::MIN;
        for i in 0..=80 {
            let t = 0.8 * i as f64 / 80.0;
            let s = resolve_props_for_effects(&fx, t, 5.0).scale_x as f64;
            max_scale = max_scale.max(s);
        }
        // The manual overshoot keyframe peaks at exactly 1.08; the collapsed
        // underdamped spring (damping 8 / stiffness 120) peaks well above it —
        // this discriminates the spring path from the manual keyframe path.
        assert!(
            max_scale > 1.12,
            "spring scale_in must overshoot past the manual 1.08 peak, got max {max_scale}"
        );
        let end = resolve_props_for_effects(&fx, 0.8, 5.0).scale_x as f64;
        assert!(
            (end - 1.0).abs() < 1e-3,
            "scale must converge to 1.0 at window end, got {end}"
        );
    }

    #[test]
    fn pulse_oscillator_ignores_spring() {
        // Pulse's scale loop (1 → 1.05 → 1) has identical endpoints — a
        // spring toward the same value would freeze the effect, so the
        // oscillator keeps its own shape.
        let at = |spring: Option<SpringConfig>, t: f64| -> f64 {
            let fx = [AnimationEffect::Pulse(timing(1.0, spring))];
            resolve_props_for_effects(&fx, t, 5.0).scale_x as f64
        };
        for i in 0..=20 {
            let t = i as f64 / 20.0;
            let plain = at(None, t);
            let sprung = at(Some(underdamped()), t);
            assert!(
                (plain - sprung).abs() < 1e-9,
                "pulse must be unaffected by spring at t={t}: {plain} vs {sprung}"
            );
        }
    }

    #[test]
    fn animation_timing_spring_serde_round_trip() {
        let json = r#"{ "name": "fade_in_up", "duration": 0.6, "spring": { "damping": 8, "stiffness": 120 } }"#;
        let fx: AnimationEffect = serde_json::from_str(json).unwrap();
        let AnimationEffect::FadeInUp(t) = &fx else {
            panic!("wrong variant");
        };
        let s = t.spring.as_ref().expect("spring parsed");
        assert_eq!(s.damping, 8.0);
        assert_eq!(s.stiffness, 120.0);
        assert_eq!(s.mass, 1.0, "mass defaults to 1");

        // Round-trip.
        let re = serde_json::to_string(&fx).unwrap();
        let back: AnimationEffect = serde_json::from_str(&re).unwrap();
        assert_eq!(fx, back);

        // Absent spring stays absent.
        let plain: AnimationEffect = serde_json::from_str(r#"{ "name": "fade_in_up" }"#).unwrap();
        let AnimationEffect::FadeInUp(t) = &plain else {
            panic!("wrong variant");
        };
        assert!(t.spring.is_none());
    }
}

#[cfg(test)]
mod glow_tests {
    //! M3: `find_glow_effect` extraction (issue #109). The colour/filter
    //! side of the fix lives in
    //! `rustmotion_components::box_builder::apply_glow_effect`, which is
    //! covered in that crate's own tests since it needs `CssStyle`/`FilterFn`
    //! (not available to this crate).

    use super::*;
    use crate::schema::{AnimationEffect, AnimationTiming, GlowConfig};

    fn glow(color: &str, radius: f32, intensity: f32) -> AnimationEffect {
        AnimationEffect::Glow(GlowConfig {
            color: color.to_string(),
            radius,
            intensity,
        })
    }

    #[test]
    fn finds_glow_among_other_effects() {
        let effects = vec![
            AnimationEffect::FadeIn(AnimationTiming::default()),
            glow("#5C39EE", 12.0, 1.0),
        ];
        let found = find_glow_effect(&effects).expect("glow effect present");
        assert_eq!(found.color, "#5C39EE");
        assert_eq!(found.radius, 12.0);
    }

    #[test]
    fn returns_none_without_a_glow_effect() {
        let effects = vec![AnimationEffect::FadeIn(AnimationTiming::default())];
        assert!(find_glow_effect(&effects).is_none());
    }

    #[test]
    fn resolve_props_for_effects_does_not_touch_glow_radius_or_intensity() {
        // Deliberate: the named `glow` effect is applied directly as a CSS
        // filter by `box_builder::apply_glow_effect` (using the raw
        // `GlowConfig.color`, which `AnimatedProperties` has no field for),
        // not through this resolver. This guards against a future change
        // accidentally routing it through `AnimatedProperties` too, which
        // would double up into two stacked drop-shadows (see the doc comment
        // on `apply_glow_effect`).
        let effects = vec![glow("#5C39EE", 12.0, 1.0)];
        let props = resolve_props_for_effects(&effects, 0.0, 1.0);
        assert_eq!(
            props.glow_radius,
            AnimatedProperties::default().glow_radius,
            "glow_radius must stay at its sentinel; the named `glow` effect must not set it"
        );
        assert_eq!(
            props.glow_intensity,
            AnimatedProperties::default().glow_intensity
        );
    }
}

#[cfg(test)]
mod float3d_amplitude_tests {
    //! Constat #1: `PresetConfig::amplitude` is read by `expand_preset_inner`
    //! (`config.amplitude.unwrap_or(12.0)`) but `AnimationTiming::to_preset_config`
    //! used to hardcode `amplitude: None`, so any author-supplied amplitude on
    //! a `float_3d` effect never reached the solver — every element bobbed by
    //! the same hardcoded 12px regardless of what was authored.
    use super::*;
    use crate::schema::AnimationEffect;

    /// Peak absolute `translate_y` reached while sampling densely across one
    /// cycle — proxy for the oscillation's amplitude actually resolved.
    fn peak_translate_y(effects: &[AnimationEffect], window: f64) -> f64 {
        let mut peak = 0.0f64;
        let steps = 200;
        for i in 0..=steps {
            let t = window * i as f64 / steps as f64;
            let y = resolve_props_for_effects(effects, t, window + 1.0).translate_y as f64;
            if y.abs() > peak.abs() {
                peak = y;
            }
        }
        peak
    }

    #[test]
    fn author_supplied_amplitude_reaches_the_solver() {
        // Parsed from raw JSON, not built in Rust — proves the value survives
        // serde all the way to the resolver, not merely that the struct has a
        // field for it.
        let default_fx: AnimationEffect =
            serde_json::from_str(r#"{ "name": "float_3d", "duration": 1.0 }"#).unwrap();
        let big_fx: AnimationEffect =
            serde_json::from_str(r#"{ "name": "float_3d", "duration": 1.0, "amplitude": 60 }"#)
                .unwrap();

        let default_peak = peak_translate_y(&[default_fx], 1.0);
        let big_peak = peak_translate_y(&[big_fx], 1.0);

        assert!(
            (default_peak.abs() - 12.0).abs() < 0.5,
            "default float_3d amplitude must stay ~12px, got {default_peak}"
        );
        assert!(
            big_peak.abs() > 50.0,
            "amplitude=60 must reach the solver (peak translate_y near 60px), got {big_peak} \
             (default was {default_peak})"
        );
    }
}

#[cfg(test)]
mod continuous_preset_timing_tests {
    //! Constat #2: `pulse` / `float` / `shake` / `spin` used to fabricate
    //! keyframes at literal times 0.0/0.25/0.5/1.0, ignoring `config.delay`
    //! and `config.duration` entirely — every element sharing one of these
    //! presets moved in lockstep on a fixed 1-second cycle no matter what the
    //! scenario authored.
    use super::*;
    use crate::schema::AnimationEffect;

    fn timing(delay: f64, duration: f64) -> AnimationTimingFixture {
        AnimationTimingFixture { delay, duration }
    }

    /// Minimal JSON round-trip helper — keeps every case going through serde,
    /// like the author's JSON would.
    struct AnimationTimingFixture {
        delay: f64,
        duration: f64,
    }

    impl AnimationTimingFixture {
        fn json(&self, name: &str) -> String {
            format!(
                r#"{{ "name": "{}", "delay": {}, "duration": {} }}"#,
                name, self.delay, self.duration
            )
        }
    }

    #[test]
    fn pulse_honours_delay_and_duration() {
        let t = timing(1.0, 2.0);
        let fx: AnimationEffect = serde_json::from_str(&t.json("pulse")).unwrap();
        // Before its delay, the cycle has not started: the resolver clamps to
        // the first keyframe's value (the 0.95 trough) at every pre-delay
        // instant — it must be identical at two different pre-delay times,
        // not moving. Before the fix, delay/duration were ignored and the
        // preset ran its own literal 0..1s cycle regardless, so t=0.1 and
        // t=0.9 fell in different oscillation phases and disagreed.
        let early = resolve_props_for_effects(std::slice::from_ref(&fx), 0.1, 10.0).scale_x as f64;
        let late = resolve_props_for_effects(std::slice::from_ref(&fx), 0.9, 10.0).scale_x as f64;
        assert!(
            (early - late).abs() < 1e-6,
            "pulse must be frozen before its delay=1.0 (not yet oscillating): \
             t=0.1 -> {early}, t=0.9 -> {late}"
        );
        assert!(
            (early - 0.95).abs() < 0.01,
            "pulse before its delay must clamp to the first keyframe (0.95), got {early}"
        );
        // At the midpoint of its cycle (delay + duration/2 = 2.0): near the peak (1.05).
        let mid = resolve_props_for_effects(&[fx], 2.0, 10.0).scale_x as f64;
        assert!(
            mid > 1.03,
            "pulse at t=2.0 (cycle midpoint) must be near peak scale ~1.05, got {mid}"
        );
    }

    #[test]
    fn float_honours_delay_and_duration() {
        let t = timing(1.0, 2.0);
        let fx: AnimationEffect = serde_json::from_str(&t.json("float")).unwrap();
        let before =
            resolve_props_for_effects(std::slice::from_ref(&fx), 0.5, 10.0).translate_y as f64;
        assert!(
            before.abs() < 0.1,
            "float at t=0.5 (before delay=1.0) must be at rest y=0, got {before}"
        );
        let mid = resolve_props_for_effects(&[fx], 2.0, 10.0).translate_y as f64;
        assert!(
            mid < -8.0,
            "float at t=2.0 (cycle midpoint) must be near peak y=-10, got {mid}"
        );
    }

    #[test]
    fn shake_honours_delay_and_duration() {
        let t = timing(1.0, 2.0);
        let fx: AnimationEffect = serde_json::from_str(&t.json("shake")).unwrap();
        let before =
            resolve_props_for_effects(std::slice::from_ref(&fx), 0.5, 10.0).translate_x as f64;
        assert!(
            before.abs() < 0.1,
            "shake at t=0.5 (before delay=1.0) must be at rest x=0, got {before}"
        );
        // Quarter point of the cycle (delay + duration/4 = 1.5): near +10 peak.
        let quarter = resolve_props_for_effects(&[fx], 1.5, 10.0).translate_x as f64;
        assert!(
            quarter > 8.0,
            "shake at t=1.5 (cycle quarter) must be near peak x=+10, got {quarter}"
        );
    }

    #[test]
    fn spin_honours_delay_and_duration() {
        let t = timing(1.0, 2.0);
        let fx: AnimationEffect = serde_json::from_str(&t.json("spin")).unwrap();
        let before =
            resolve_props_for_effects(std::slice::from_ref(&fx), 0.5, 10.0).rotation as f64;
        assert!(
            before.abs() < 0.1,
            "spin at t=0.5 (before delay=1.0) must be at rest rotation=0, got {before}"
        );
        // Halfway through its own cycle (delay + duration/2 = 2.0): ~180deg.
        let mid = resolve_props_for_effects(&[fx], 2.0, 10.0).rotation as f64;
        assert!(
            (mid - 180.0).abs() < 5.0,
            "spin at t=2.0 (cycle midpoint) must be near 180deg, got {mid}"
        );
    }
}

#[cfg(test)]
mod keyframes_composition_tests {
    //! Constat #5: two `keyframes` effects targeting the same property used
    //! to be routed into one of two buckets purely by whether `delay != 0`
    //! (`owned_keyframes` vs `keyframes` in `extract_effects`), each bucket
    //! resolved by its own `resolve_animations` call and combined via
    //! `AnimatedProperties::merge` — which *sums* additive properties like
    //! `translate_x` across buckets, while two effects landing in the *same*
    //! bucket instead overwrite (last one in the list wins, since
    //! `apply_property` assigns rather than adds). So the composition rule
    //! depended entirely on an incidental field (`delay`) with no relation to
    //! authoring intent.
    //!
    //! Chosen semantic: every `keyframes`/`tilt_in` effect is resolved
    //! together in one `resolve_animations` call, in the order the effects
    //! appear in `style.animation` — like a CSS cascade, the *last* effect
    //! in the array wins on a shared property. This is deterministic and
    //! independent of `delay`.
    use super::*;
    use crate::schema::{Animation, AnimationEffect, Keyframe, KeyframeValue, KeyframesConfig};

    /// A `keyframes` effect with one property ramping `0 -> value` over
    /// `[0, 1]` (pre-shift), then shifted by `delay`.
    fn ramp(property: &str, value: f64, delay: f64) -> AnimationEffect {
        AnimationEffect::Keyframes(KeyframesConfig {
            keyframes: vec![Animation {
                property: property.to_string(),
                keyframes: vec![
                    Keyframe {
                        time: 0.0,
                        value: KeyframeValue::Number(0.0),
                        easing: None,
                    },
                    Keyframe {
                        time: 1.0,
                        value: KeyframeValue::Number(value),
                        easing: None,
                    },
                ],
                easing: EasingType::Linear,
                spring: None,
            }],
            delay,
            duration: 0.8,
            repeat: false,
        })
    }

    #[test]
    fn last_declared_effect_wins_regardless_of_which_one_carries_the_delay() {
        // Case 1: A (delay=0) declared first, B (delay=0.5) declared second.
        let a1 = ramp("translate_x", 100.0, 0.0);
        let b1 = ramp("translate_x", 40.0, 0.5);
        let combined_1 = resolve_props_for_effects(&[a1, b1.clone()], 1.0, 5.0).translate_x as f64;
        let b1_alone = resolve_props_for_effects(&[b1], 1.0, 5.0).translate_x as f64;
        assert!(
            (combined_1 - b1_alone).abs() < 1e-4,
            "B (declared last) must alone determine translate_x at t=1.0: combined={combined_1}, B-alone={b1_alone}"
        );

        // Case 2: swap which one carries the delay, keep declaration order
        // (A first, B second) — the outcome must be identical in shape: B
        // (still last) wins alone, this time using B's own (now delay=0)
        // timing.
        let a2 = ramp("translate_x", 100.0, 0.5);
        let b2 = ramp("translate_x", 40.0, 0.0);
        let combined_2 = resolve_props_for_effects(&[a2, b2.clone()], 1.0, 5.0).translate_x as f64;
        let b2_alone = resolve_props_for_effects(&[b2], 1.0, 5.0).translate_x as f64;
        assert!(
            (combined_2 - b2_alone).abs() < 1e-4,
            "B (declared last) must alone determine translate_x at t=1.0 even with delay swapped: \
             combined={combined_2}, B-alone={b2_alone}"
        );

        // The two cases must NOT collapse to the same number (sanity check
        // that this test isn't vacuous — B's own resolved value genuinely
        // differs between the two delay assignments).
        assert!(
            (combined_1 - combined_2).abs() > 1.0,
            "sanity: the two cases must differ (B's own timing changed): {combined_1} vs {combined_2}"
        );
    }
}

#[cfg(test)]
mod keyframes_loop_tests {
    //! Constat #7: `"loop": true` on a `keyframes` effect or on `tilt_in`
    //! never reached the solver. `resolve_props_for_effects` always called
    //! `resolve_animations(&kfs, None, None, ...)` for both keyframe buckets
    //! — passing `preset_config = None` means `resolve_animations` falls back
    //! to `PresetConfig::default()`, whose `repeat` is `false`, so
    //! `loop_time` was never invoked no matter what `KeyframesConfig::repeat`
    //! / `TiltInConfig::repeat` said.
    use super::*;
    use crate::schema::{Animation, AnimationEffect, Keyframe, KeyframeValue, KeyframesConfig};

    #[test]
    fn keyframes_loop_true_wraps_time_past_the_last_keyframe() {
        let looping = AnimationEffect::Keyframes(KeyframesConfig {
            keyframes: vec![Animation {
                property: "opacity".to_string(),
                keyframes: vec![
                    Keyframe {
                        time: 0.0,
                        value: KeyframeValue::Number(0.0),
                        easing: None,
                    },
                    Keyframe {
                        time: 1.0,
                        value: KeyframeValue::Number(1.0),
                        easing: None,
                    },
                ],
                easing: EasingType::Linear,
                spring: None,
            }],
            delay: 0.0,
            duration: 0.8,
            repeat: true,
        });
        // t=2.5 is past the keyframe's own last time (1.0). Without looping,
        // the resolver clamps to the last keyframe's value (1.0) forever.
        // With looping (start=0, end=1, duration=1), t=2.5 wraps to 0.5 ->
        // opacity should be ~0.5, not 1.0.
        let opacity = resolve_props_for_effects(&[looping], 2.5, 5.0).opacity as f64;
        assert!(
            (opacity - 0.5).abs() < 0.05,
            "looping keyframes at t=2.5 must wrap to local t=0.5 (opacity ~0.5), got {opacity}"
        );
    }

    #[test]
    fn tilt_in_loop_true_keeps_tilting_past_its_settle_time() {
        let looping_tilt: AnimationEffect = serde_json::from_str(
            r#"{ "name": "tilt_in", "delay": 0.0, "duration": 0.4, "loop": true }"#,
        )
        .unwrap();
        let settled: AnimationEffect =
            serde_json::from_str(r#"{ "name": "tilt_in", "delay": 0.0, "duration": 0.4 }"#)
                .unwrap();

        // Well past the settle time (0.4s): without loop, scale is pinned at
        // the final resting value (1.0). With loop (cycle 0..0.4), t=1.0
        // wraps to local t=0.2 (t=1.0 % 0.4 = 0.2), mid-tilt, scale != 1.0.
        let settled_scale = resolve_props_for_effects(&[settled], 1.0, 5.0).scale_x as f64;
        let looping_scale = resolve_props_for_effects(&[looping_tilt], 1.0, 5.0).scale_x as f64;

        assert!(
            (settled_scale - 1.0).abs() < 1e-3,
            "non-looping tilt_in at t=1.0 (past settle) must be resting at scale 1.0, got {settled_scale}"
        );
        assert!(
            (looping_scale - 1.0).abs() > 0.01,
            "looping tilt_in at t=1.0 must still be mid-cycle (scale != 1.0 rest), got {looping_scale}"
        );
    }
}

#[cfg(test)]
mod spring_robustness_tests {
    //! Constat #6: `spring_value` fed `mass`/`stiffness`/`damping` straight
    //! into `sqrt`/division with no floor, so `mass <= 0` or `stiffness <= 0`
    //! produced NaN (division by zero or sqrt of a negative number), and
    //! negative `damping` flipped the decay exponent's sign, diverging to
    //! +-infinity instead of settling. A NaN/inf progress value then flows
    //! into transform math (translate/scale) and contaminates the whole
    //! subtree it touches.
    use super::*;

    #[test]
    fn zero_mass_does_not_produce_nan() {
        let config = SpringConfig {
            damping: 10.0,
            stiffness: 100.0,
            mass: 0.0,
        };
        for i in 0..=20 {
            let t = i as f64 * 0.25;
            let v = spring_value(t, &config);
            assert!(
                v.is_finite(),
                "spring_value(t={t}) with mass=0 must be finite, got {v}"
            );
        }
    }

    #[test]
    fn zero_stiffness_does_not_produce_nan() {
        let config = SpringConfig {
            damping: 10.0,
            stiffness: 0.0,
            mass: 1.0,
        };
        for i in 0..=20 {
            let t = i as f64 * 0.25;
            let v = spring_value(t, &config);
            assert!(
                v.is_finite(),
                "spring_value(t={t}) with stiffness=0 must be finite, got {v}"
            );
        }
    }

    #[test]
    fn negative_damping_stays_bounded_instead_of_diverging() {
        let config = SpringConfig {
            damping: -20.0,
            stiffness: 100.0,
            mass: 1.0,
        };
        let v_at_5s = spring_value(5.0, &config);
        assert!(
            v_at_5s.is_finite() && v_at_5s.abs() < 100.0,
            "spring_value(t=5.0) with damping=-20 must stay bounded (finite and reasonably \
             small), got {v_at_5s} — negative damping must not diverge to +-infinity"
        );
    }
}
