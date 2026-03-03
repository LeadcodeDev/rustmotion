use crate::schema::{
    Animation, AnimationPreset, EasingType, Keyframe, KeyframeValue, PresetConfig, SpringConfig,
    WiggleConfig,
};

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
pub fn spring_value(t: f64, config: &SpringConfig) -> f64 {
    let damping = config.damping;
    let stiffness = config.stiffness;
    let mass = config.mass;

    let omega = (stiffness / mass).sqrt();
    let zeta = damping / (2.0 * (stiffness * mass).sqrt());

    if zeta < 1.0 {
        // Underdamped
        let omega_d = omega * (1.0 - zeta * zeta).sqrt();
        let decay = (-zeta * omega * t).exp();
        1.0 - decay * ((zeta * omega * t / omega_d).sin() * (zeta * omega / omega_d) + (omega_d * t).cos())
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
        }
    }
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
    let preset_animations = preset.map(|p| {
        expand_preset(p, &config, scene_duration)
    });

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
        _ => {} // Unknown property, ignore
    }
}

// ─── Wiggle resolution ──────────────────────────────────────────────────────

/// Simple noise function based on sine waves with seed for pseudo-random behavior
fn simplex_noise_1d(x: f64, seed: u64) -> f64 {
    let s = seed as f64;
    let v = (x * 1.0 + s * 0.1234).sin() * 0.5
        + (x * 2.3 + s * 0.5678).sin() * 0.25
        + (x * 4.7 + s * 0.9012).sin() * 0.125;
    v / 0.875 // normalize to roughly -1..1
}

/// Apply wiggle offsets additively to animated properties
pub fn apply_wiggles(props: &mut AnimatedProperties, wiggles: &[WiggleConfig], time: f64) {
    for wiggle in wiggles {
        let noise_val = simplex_noise_1d(time * wiggle.frequency, wiggle.seed);
        let offset = wiggle.amplitude * noise_val;
        apply_property(props, &wiggle.property, get_property_value(props, &wiggle.property) + offset);
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
        _ => 0.0,
    }
}

// ─── Preset expansion ───────────────────────────────────────────────────────

fn expand_preset(preset: &AnimationPreset, config: &PresetConfig, _scene_duration: f64) -> Vec<Animation> {
    let delay = config.delay;
    let dur = config.duration;
    let end = delay + dur;

    match preset {
        // ── Entrées ──────────────────────────────────────────────────────
        AnimationPreset::FadeIn => vec![
            kf_anim("opacity", delay, 0.0, end, 1.0, EasingType::EaseOut),
        ],
        AnimationPreset::FadeInUp => vec![
            kf_anim("opacity", delay, 0.0, end, 1.0, EasingType::EaseOut),
            kf_anim("position.y", delay, 60.0, end, 0.0, EasingType::EaseOutCubic),
        ],
        AnimationPreset::FadeInDown => vec![
            kf_anim("opacity", delay, 0.0, end, 1.0, EasingType::EaseOut),
            kf_anim("position.y", delay, -60.0, end, 0.0, EasingType::EaseOutCubic),
        ],
        AnimationPreset::FadeInLeft => vec![
            kf_anim("opacity", delay, 0.0, end, 1.0, EasingType::EaseOut),
            kf_anim("position.x", delay, -60.0, end, 0.0, EasingType::EaseOutCubic),
        ],
        AnimationPreset::FadeInRight => vec![
            kf_anim("opacity", delay, 0.0, end, 1.0, EasingType::EaseOut),
            kf_anim("position.x", delay, 60.0, end, 0.0, EasingType::EaseOutCubic),
        ],
        AnimationPreset::SlideInLeft => vec![
            kf_anim("opacity", delay, 0.0, delay + dur * 0.3, 1.0, EasingType::EaseOut),
            kf_anim("position.x", delay, -200.0, end, 0.0, EasingType::EaseOutCubic),
        ],
        AnimationPreset::SlideInRight => vec![
            kf_anim("opacity", delay, 0.0, delay + dur * 0.3, 1.0, EasingType::EaseOut),
            kf_anim("position.x", delay, 200.0, end, 0.0, EasingType::EaseOutCubic),
        ],
        AnimationPreset::SlideInUp => vec![
            kf_anim("opacity", delay, 0.0, delay + dur * 0.3, 1.0, EasingType::EaseOut),
            kf_anim("position.y", delay, 200.0, end, 0.0, EasingType::EaseOutCubic),
        ],
        AnimationPreset::SlideInDown => vec![
            kf_anim("opacity", delay, 0.0, delay + dur * 0.3, 1.0, EasingType::EaseOut),
            kf_anim("position.y", delay, -200.0, end, 0.0, EasingType::EaseOutCubic),
        ],
        AnimationPreset::ScaleIn => vec![
            kf_anim("opacity", delay, 0.0, delay + dur * 0.3, 1.0, EasingType::EaseOut),
            kf_anim_spring("scale", delay, 0.0, end, 1.0),
        ],
        AnimationPreset::BounceIn => vec![
            kf_anim("opacity", delay, 0.0, delay + dur * 0.2, 1.0, EasingType::EaseOut),
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
        AnimationPreset::ElasticIn => vec![
            kf_anim_spring_underdamped("scale", delay, 0.0, end, 1.0),
        ],

        // ── Sorties ──────────────────────────────────────────────────────
        AnimationPreset::FadeOut => vec![
            kf_anim("opacity", delay, 1.0, end, 0.0, EasingType::EaseIn),
        ],
        AnimationPreset::FadeOutUp => vec![
            kf_anim("opacity", delay, 1.0, end, 0.0, EasingType::EaseIn),
            kf_anim("position.y", delay, 0.0, end, -60.0, EasingType::EaseInCubic),
        ],
        AnimationPreset::FadeOutDown => vec![
            kf_anim("opacity", delay, 1.0, end, 0.0, EasingType::EaseIn),
            kf_anim("position.y", delay, 0.0, end, 60.0, EasingType::EaseInCubic),
        ],
        AnimationPreset::SlideOutLeft => vec![
            kf_anim("opacity", delay + dur * 0.7, 1.0, end, 0.0, EasingType::EaseIn),
            kf_anim("position.x", delay, 0.0, end, -200.0, EasingType::EaseInCubic),
        ],
        AnimationPreset::SlideOutRight => vec![
            kf_anim("opacity", delay + dur * 0.7, 1.0, end, 0.0, EasingType::EaseIn),
            kf_anim("position.x", delay, 0.0, end, 200.0, EasingType::EaseInCubic),
        ],
        AnimationPreset::SlideOutUp => vec![
            kf_anim("opacity", delay + dur * 0.7, 1.0, end, 0.0, EasingType::EaseIn),
            kf_anim("position.y", delay, 0.0, end, -200.0, EasingType::EaseInCubic),
        ],
        AnimationPreset::SlideOutDown => vec![
            kf_anim("opacity", delay + dur * 0.7, 1.0, end, 0.0, EasingType::EaseIn),
            kf_anim("position.y", delay, 0.0, end, 200.0, EasingType::EaseInCubic),
        ],
        AnimationPreset::ScaleOut => vec![
            kf_anim("opacity", delay + dur * 0.7, 1.0, end, 0.0, EasingType::EaseIn),
            kf_anim("scale", delay, 1.0, end, 0.0, EasingType::EaseInCubic),
        ],
        AnimationPreset::BounceOut => vec![
            kf_anim("opacity", delay + dur * 0.8, 1.0, end, 0.0, EasingType::EaseIn),
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
        AnimationPreset::Pulse => vec![
            kf_anim_loop("scale", 0.95, 1.05),
        ],
        AnimationPreset::Float => vec![
            kf_anim_3kf("position.y", 0.0, -10.0, 0.0, EasingType::EaseInOut),
        ],
        AnimationPreset::Shake => vec![
            kf_anim_4kf("position.x", 0.0, 10.0, -10.0, 0.0, EasingType::EaseInOut),
        ],
        AnimationPreset::Spin => vec![
            kf_anim("rotation", 0.0, 0.0, 1.0, 360.0, EasingType::Linear),
        ],

        // ── Spéciaux ────────────────────────────────────────────────────
        AnimationPreset::Typewriter => vec![
            kf_anim("visible_chars_progress", delay, 0.0, end, 1.0, EasingType::Linear),
        ],
        AnimationPreset::WipeLeft => vec![
            kf_anim("opacity", delay, 0.0, delay + dur * 0.3, 1.0, EasingType::EaseOut),
            kf_anim("position.x", delay, -200.0, end, 0.0, EasingType::EaseInOut),
        ],
        AnimationPreset::WipeRight => vec![
            kf_anim("opacity", delay, 0.0, delay + dur * 0.3, 1.0, EasingType::EaseOut),
            kf_anim("position.x", delay, 200.0, end, 0.0, EasingType::EaseInOut),
        ],
    }
}

fn kf(time: f64, value: f64) -> Keyframe {
    Keyframe { time, value: KeyframeValue::Number(value), easing: None }
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

fn kf_anim_3kf(property: &str, v0: f64, v1: f64, v2: f64, easing: EasingType) -> Animation {
    Animation {
        property: property.to_string(),
        keyframes: vec![kf(0.0, v0), kf(0.5, v1), kf(1.0, v2)],
        easing,
        spring: None,
    }
}

fn kf_anim_4kf(property: &str, v0: f64, v1: f64, v2: f64, v3: f64, easing: EasingType) -> Animation {
    Animation {
        property: property.to_string(),
        keyframes: vec![kf(0.0, v0), kf(0.25, v1), kf(0.5, v2), kf(1.0, v3)],
        easing,
        spring: None,
    }
}

fn kf_anim_loop(property: &str, min: f64, max: f64) -> Animation {
    Animation {
        property: property.to_string(),
        keyframes: vec![kf(0.0, min), kf(0.5, max), kf(1.0, min)],
        easing: EasingType::EaseInOut,
        spring: None,
    }
}
