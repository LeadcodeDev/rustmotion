use crate::schema::{
    Animation, AnimationPreset, EasingType, Keyframe, KeyframeValue, PresetConfig, SpringConfig,
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

    // First, expand preset into animations
    let preset_animations = preset.map(|p| {
        let config = preset_config.cloned().unwrap_or_default();
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
        let value = resolve_animation_value(anim, time);
        apply_property(&mut props, &anim.property, value);
    }

    props
}

fn resolve_animation_value(anim: &Animation, time: f64) -> f64 {
    let keyframes = &anim.keyframes;
    if keyframes.is_empty() {
        return 0.0;
    }
    if keyframes.len() == 1 {
        return keyframes[0].value.as_f64();
    }

    // Find the two surrounding keyframes
    if time <= keyframes[0].time {
        return keyframes[0].value.as_f64();
    }
    if time >= keyframes.last().unwrap().time {
        return keyframes.last().unwrap().value.as_f64();
    }

    for i in 0..keyframes.len() - 1 {
        let kf0 = &keyframes[i];
        let kf1 = &keyframes[i + 1];

        if time >= kf0.time && time <= kf1.time {
            let segment_duration = kf1.time - kf0.time;
            if segment_duration < 1e-9 {
                return kf1.value.as_f64();
            }

            let local_t = (time - kf0.time) / segment_duration;

            let progress = match &anim.easing {
                EasingType::Spring => {
                    let spring_config = anim.spring.clone().unwrap_or_default();
                    spring_value(local_t * segment_duration, &spring_config)
                }
                other => ease(local_t, other),
            };

            let v0 = kf0.value.as_f64();
            let v1 = kf1.value.as_f64();
            return v0 + (v1 - v0) * progress;
        }
    }

    keyframes.last().unwrap().value.as_f64()
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
        _ => {} // Unknown property, ignore
    }
}

// ─── Preset expansion ───────────────────────────────────────────────────────

fn expand_preset(preset: &AnimationPreset, config: &PresetConfig, _scene_duration: f64) -> Vec<Animation> {
    let delay = config.delay;
    let dur = config.duration;
    let end = delay + dur;

    match preset {
        AnimationPreset::FadeIn => vec![kf_anim("opacity", delay, 0.0, end, 1.0, EasingType::EaseOut)],
        AnimationPreset::FadeOut => vec![kf_anim("opacity", delay, 1.0, end, 0.0, EasingType::EaseIn)],
        AnimationPreset::FadeInUp => vec![
            kf_anim("opacity", delay, 0.0, end, 1.0, EasingType::EaseOut),
            kf_anim("position.y", delay, 60.0, end, 0.0, EasingType::EaseOutCubic),
        ],
        AnimationPreset::FadeInDown => vec![
            kf_anim("opacity", delay, 0.0, end, 1.0, EasingType::EaseOut),
            kf_anim("position.y", delay, -60.0, end, 0.0, EasingType::EaseOutCubic),
        ],
        AnimationPreset::SlideInLeft => vec![
            kf_anim("opacity", delay, 0.0, delay + dur * 0.3, 1.0, EasingType::EaseOut),
            kf_anim("position.x", delay, -200.0, end, 0.0, EasingType::EaseOutCubic),
        ],
        AnimationPreset::SlideInRight => vec![
            kf_anim("opacity", delay, 0.0, delay + dur * 0.3, 1.0, EasingType::EaseOut),
            kf_anim("position.x", delay, 200.0, end, 0.0, EasingType::EaseOutCubic),
        ],
        AnimationPreset::ScaleIn => vec![
            kf_anim("opacity", delay, 0.0, delay + dur * 0.3, 1.0, EasingType::EaseOut),
            kf_anim_spring("scale", delay, 0.0, end, 1.0),
        ],
        AnimationPreset::ScaleOut => vec![
            kf_anim("opacity", delay + dur * 0.7, 1.0, end, 0.0, EasingType::EaseIn),
            kf_anim("scale", delay, 1.0, end, 0.0, EasingType::EaseInCubic),
        ],
        AnimationPreset::Typewriter => vec![
            kf_anim("visible_chars", delay, 0.0, end, 1000.0, EasingType::Linear),
        ],
        AnimationPreset::BounceIn => vec![
            kf_anim("opacity", delay, 0.0, delay + dur * 0.2, 1.0, EasingType::EaseOut),
            kf_anim_spring("scale", delay, 0.3, end, 1.0),
        ],
        AnimationPreset::WipeLeft => vec![
            kf_anim("clip_progress", delay, 0.0, end, 1.0, EasingType::EaseInOut),
        ],
        AnimationPreset::WipeRight => vec![
            kf_anim("clip_progress", delay, 0.0, end, 1.0, EasingType::EaseInOut),
        ],
        AnimationPreset::Pulse => vec![
            kf_anim_loop("scale", 0.95, 1.05),
        ],
        AnimationPreset::BlurIn => vec![
            kf_anim("opacity", delay, 0.0, end, 1.0, EasingType::EaseOut),
            kf_anim("blur", delay, 20.0, end, 0.0, EasingType::EaseOutCubic),
        ],
    }
}

fn kf_anim(property: &str, t0: f64, v0: f64, t1: f64, v1: f64, easing: EasingType) -> Animation {
    Animation {
        property: property.to_string(),
        keyframes: vec![
            Keyframe { time: t0, value: KeyframeValue::Number(v0) },
            Keyframe { time: t1, value: KeyframeValue::Number(v1) },
        ],
        easing,
        spring: None,
    }
}

fn kf_anim_spring(property: &str, t0: f64, v0: f64, t1: f64, v1: f64) -> Animation {
    Animation {
        property: property.to_string(),
        keyframes: vec![
            Keyframe { time: t0, value: KeyframeValue::Number(v0) },
            Keyframe { time: t1, value: KeyframeValue::Number(v1) },
        ],
        easing: EasingType::Spring,
        spring: Some(SpringConfig {
            damping: 12.0,
            stiffness: 100.0,
            mass: 1.0,
        }),
    }
}

fn kf_anim_loop(_property: &str, _min: f64, _max: f64) -> Animation {
    // Pulse is a special case: we create a simple oscillation
    // For now, use a 3-keyframe approach
    Animation {
        property: "scale".to_string(),
        keyframes: vec![
            Keyframe { time: 0.0, value: KeyframeValue::Number(1.0) },
            Keyframe { time: 0.5, value: KeyframeValue::Number(1.05) },
            Keyframe { time: 1.0, value: KeyframeValue::Number(1.0) },
        ],
        easing: EasingType::EaseInOut,
        spring: None,
    }
}
