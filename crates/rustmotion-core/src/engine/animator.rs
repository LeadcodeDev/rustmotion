use crate::schema::{
    Animation, AnimationEffect, AnimationPreset, BurstConfig, CharAnimPreset, EasingType,
    GlowConfig, Keyframe, KeyframeValue, MotionPathConfig, OrbitConfig, PresetConfig, SpringConfig,
    TextAnimDirection, TextAnimGranularity, WiggleConfig,
};

pub const DEFAULT_CHAR_BLUR_SIGMA: f32 = 14.0;

#[inline]
pub fn safe_div(num: f64, denom: f64, fallback: f64) -> f64 {
    if denom.abs() < 1e-9 {
        fallback
    } else {
        num / denom
    }
}

#[inline]
pub fn safe_div_f32(num: f32, denom: f32, fallback: f32) -> f32 {
    if denom.abs() < 1e-6 {
        fallback
    } else {
        num / denom
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedCharAnimation {
    pub preset: CharAnimPreset,
    pub granularity: TextAnimGranularity,
    pub stagger: f32,
    pub duration: f32,
    pub easing: EasingType,
    pub delay: f32,
    pub overshoot: f32,
    pub blur: f32,
    pub direction: TextAnimDirection,
    pub distance: f32,
    pub scale_from: Option<f32>,
    pub jitter: f32,
    pub seed: u32,
    pub ink_from: Option<String>,
}

impl ResolvedCharAnimation {
    pub fn unit_start(&self, idx: usize) -> f64 {
        let even = self.delay as f64 + idx as f64 * self.stagger as f64;
        if self.jitter.abs() < 1e-6 || self.stagger.abs() < 1e-6 {
            return even;
        }
        let mut h = (idx as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (self.seed as u64);
        h ^= h >> 30;
        h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        h ^= h >> 27;
        h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
        h ^= h >> 31;
        let unit = (h >> 11) as f64 / (1u64 << 53) as f64;
        let nudge = (unit * 2.0 - 1.0) * self.jitter as f64 * self.stagger as f64;
        (even + nudge).max(self.delay as f64)
    }
}

pub struct ExtractedEffects<'a> {
    pub presets: Vec<(AnimationPreset, PresetConfig)>,
    pub keyframe_animations: Vec<Animation>,
    pub keyframes_loop: bool,
    pub wiggles: Vec<&'a WiggleConfig>,
    pub orbits: Vec<&'a OrbitConfig>,
    pub motion_paths: Vec<&'a MotionPathConfig>,
    pub glow: Option<&'a GlowConfig>,
    pub motion_blur: Option<f32>,
    pub char_animation: Option<ResolvedCharAnimation>,
}

pub fn find_glow_effect(effects: &[AnimationEffect]) -> Option<&GlowConfig> {
    effects.iter().find_map(|e| match e {
        AnimationEffect::Glow(cfg) => Some(cfg),
        _ => None,
    })
}

pub fn extract_effects(effects: &[AnimationEffect]) -> ExtractedEffects<'_> {
    let mut result = ExtractedEffects {
        presets: Vec::new(),
        keyframe_animations: Vec::new(),
        keyframes_loop: false,
        wiggles: Vec::new(),
        orbits: Vec::new(),
        motion_paths: Vec::new(),
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
                | AnimationEffect::CharSlideUp(t)
                | AnimationEffect::CharBlurIn(t) => {
                    let preset = match effect {
                        AnimationEffect::CharScaleIn(_) => CharAnimPreset::ScaleIn,
                        AnimationEffect::CharFadeIn(_) => CharAnimPreset::FadeIn,
                        AnimationEffect::CharWave(_) => CharAnimPreset::Wave,
                        AnimationEffect::CharBounce(_) => CharAnimPreset::Bounce,
                        AnimationEffect::CharRotateIn(_) => CharAnimPreset::RotateIn,
                        AnimationEffect::CharSlideUp(_) => CharAnimPreset::SlideUp,
                        AnimationEffect::CharBlurIn(_) => CharAnimPreset::BlurIn,
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
                        blur: t.blur.map(|b| b as f32).unwrap_or(DEFAULT_CHAR_BLUR_SIGMA),
                        direction: t.direction,
                        distance: t.distance.unwrap_or(1.0) as f32,
                        scale_from: t.scale_from.map(|s| s as f32),
                        jitter: t.jitter.unwrap_or(0.0) as f32,
                        seed: t.seed.unwrap_or(0),
                        ink_from: t.ink_from.clone(),
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
                AnimationEffect::MotionPath(config) => {
                    result.motion_paths.push(config);
                }
                _ => {}
            }
        }
    }

    result
}

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
        EasingType::Spring => t,
        EasingType::CubicBezier { x1, y1, x2, y2 } => cubic_bezier_ease(t, *x1, *y1, *x2, *y2),
        EasingType::Steps(n) => {
            let n = (*n).max(1) as f64;
            if t >= 1.0 {
                1.0
            } else {
                (t * n).floor() / n
            }
        }
    }
}

pub fn chromatic_aberration_shift(
    cfg: &crate::schema::ChromaticAberrationConfig,
    progress: f32,
) -> f32 {
    let eased = ease(progress as f64, &cfg.easing) as f32;
    cfg.amount * (1.0 - eased)
}

#[cfg(test)]
mod chromatic_aberration_shift_tests {
    use super::*;
    use crate::schema::{ChromaticAberrationConfig, EasingType};

    fn cfg(amount: f32) -> ChromaticAberrationConfig {
        ChromaticAberrationConfig {
            delay: 0.0,
            duration: 0.6,
            amount,
            easing: EasingType::Linear,
        }
    }

    #[test]
    fn peaks_at_the_full_amount_when_progress_is_zero() {
        assert_eq!(chromatic_aberration_shift(&cfg(6.0), 0.0), 6.0);
    }

    #[test]
    fn decays_to_exactly_zero_when_progress_reaches_one() {
        assert_eq!(chromatic_aberration_shift(&cfg(6.0), 1.0), 0.0);
    }

    #[test]
    fn is_between_zero_and_the_amount_mid_flight() {
        let shift = chromatic_aberration_shift(&cfg(6.0), 0.5);
        assert!(shift > 0.0 && shift < 6.0, "got {shift}");
    }
}

pub fn shatter_progress(cfg: &crate::schema::ShatterConfig, time: f64) -> Option<f32> {
    use crate::schema::ShatterMode;

    if cfg.duration <= 0.0 {
        return None;
    }
    let elapsed = time - cfg.delay;
    if elapsed < 0.0 {
        return None;
    }
    match cfg.mode {
        ShatterMode::Hold => Some((elapsed / cfg.duration).min(1.0) as f32),
        ShatterMode::Out => {
            if elapsed >= cfg.duration {
                None
            } else {
                Some((elapsed / cfg.duration) as f32)
            }
        }
        ShatterMode::In => {
            if elapsed >= cfg.duration {
                None
            } else {
                Some((1.0 - elapsed / cfg.duration) as f32)
            }
        }
    }
}

pub fn burst_progress(cfg: &BurstConfig, time: f64) -> Option<f32> {
    if cfg.duration <= 0.0 {
        return None;
    }
    let elapsed = time - cfg.delay;
    if elapsed < 0.0 || elapsed >= cfg.duration {
        return None;
    }
    Some((elapsed / cfg.duration) as f32)
}

pub fn burst_stroke_span(progress: f32, phase: f32) -> (f32, f32) {
    let phase = phase.clamp(0.0, 0.9);
    let local = ((progress - phase) / (1.0 - phase)).clamp(0.0, 1.0);
    let head = ease_out_cubic((local * 2.0).min(1.0) as f64) as f32;
    let tail = ease_in_cubic((local * 2.0 - 1.0).clamp(0.0, 1.0) as f64) as f32;
    (tail, head)
}

#[cfg(test)]
mod shatter_progress_tests {
    use super::*;
    use crate::schema::{ShatterConfig, ShatterMode};

    fn cfg(mode: ShatterMode) -> ShatterConfig {
        ShatterConfig {
            delay: 1.0,
            duration: 0.5,
            mode,
            pieces: 12,
            seed: 3,
            origin: Default::default(),
            spread: 1.0,
            spin: 90.0,
            depth: 0.4,
            fade: true,
        }
    }

    #[test]
    fn out_is_none_before_delay_and_at_or_after_the_end() {
        let c = cfg(ShatterMode::Out);
        assert_eq!(shatter_progress(&c, 0.0), None);
        assert_eq!(shatter_progress(&c, 0.999), None);
        assert_eq!(shatter_progress(&c, 1.5), None);
        assert_eq!(shatter_progress(&c, 10.0), None);
    }

    #[test]
    fn out_climbs_from_zero_to_just_under_one_across_the_window() {
        let c = cfg(ShatterMode::Out);
        assert_eq!(shatter_progress(&c, 1.0), Some(0.0));
        let mid = shatter_progress(&c, 1.25).unwrap();
        assert!(mid > 0.0 && mid < 1.0, "got {mid}");
    }

    #[test]
    fn in_is_the_mirror_of_out() {
        let c = cfg(ShatterMode::In);
        assert_eq!(shatter_progress(&c, 0.0), None);
        assert_eq!(shatter_progress(&c, 1.5), None);
        assert_eq!(shatter_progress(&c, 1.0), Some(1.0));
        let mid = shatter_progress(&c, 1.25).unwrap();
        assert!(mid > 0.0 && mid < 1.0, "got {mid}");
    }

    #[test]
    fn hold_freezes_at_one_past_the_window_instead_of_going_back_to_none() {
        let c = cfg(ShatterMode::Hold);
        assert_eq!(shatter_progress(&c, 0.0), None);
        assert_eq!(shatter_progress(&c, 1.0), Some(0.0));
        assert_eq!(shatter_progress(&c, 1.5), Some(1.0));
        assert_eq!(
            shatter_progress(&c, 100.0),
            Some(1.0),
            "hold must never converge back to None, unlike out/in"
        );
    }
}

fn cubic_bezier_ease(t: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let t_curve = find_bezier_t_for_x(t, x1, x2);
    bezier_component(t_curve, y1, y2)
}

fn bezier_component(t: f64, p1: f64, p2: f64) -> f64 {
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
    let mut t = x;
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

pub const DEFAULT_SPRING_REST_THRESHOLD: f64 = 0.005;

pub const MAX_SPRING_SEARCH_SECONDS: f64 = 30.0;

thread_local! {
    static SPRING_SETTLE_TIME_CACHE: std::cell::RefCell<std::collections::HashMap<(u64, u64, u64, u64), f64>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

fn spring_settle_time_cached(damping: f64, stiffness: f64, mass: f64, threshold: f64) -> f64 {
    let key = (
        damping.to_bits(),
        stiffness.to_bits(),
        mass.to_bits(),
        threshold.to_bits(),
    );
    SPRING_SETTLE_TIME_CACHE.with(|cache| {
        if let Some(&cached) = cache.borrow().get(&key) {
            return cached;
        }
        let settled = spring_settle_time(
            damping,
            stiffness,
            mass,
            threshold,
            MAX_SPRING_SEARCH_SECONDS,
        );
        cache.borrow_mut().insert(key, settled);
        settled
    })
}

pub fn spring_value(t: f64, config: &SpringConfig) -> f64 {
    let damping = config.damping.max(0.0);
    let stiffness = config.stiffness.max(1e-6);
    let mass = config.mass.max(1e-6);

    match config.duration {
        Some(duration) if duration > 0.0 => {
            let threshold = spring_rest_threshold(config);
            let natural_rest = spring_settle_time_cached(damping, stiffness, mass, threshold);
            if natural_rest < 1e-9 {
                spring_value_raw(t, damping, stiffness, mass)
            } else {
                let time_scale = natural_rest / duration;
                spring_value_raw(t * time_scale, damping, stiffness, mass)
            }
        }
        _ => spring_value_raw(t, damping, stiffness, mass),
    }
}

fn spring_value_raw(t: f64, damping: f64, stiffness: f64, mass: f64) -> f64 {
    let omega = (stiffness / mass).sqrt();
    let zeta = damping / (2.0 * (stiffness * mass).sqrt());

    if zeta < 1.0 {
        let omega_d = omega * (1.0 - zeta * zeta).sqrt();
        let decay = (-zeta * omega * t).exp();
        1.0 - decay * ((omega_d * t).sin() * (zeta * omega / omega_d) + (omega_d * t).cos())
    } else if (zeta - 1.0).abs() < 1e-6 {
        let decay = (-omega * t).exp();
        1.0 - decay * (1.0 + omega * t)
    } else {
        let s1 = -omega * (zeta - (zeta * zeta - 1.0).sqrt());
        let s2 = -omega * (zeta + (zeta * zeta - 1.0).sqrt());
        let c2 = -s1 / (s2 - s1);
        let c1 = 1.0 - c2;
        1.0 - (c1 * (s1 * t).exp() + c2 * (s2 * t).exp())
    }
}

const SPRING_SETTLE_MIN_SAMPLES: usize = 2_000;
const SPRING_SETTLE_MAX_SAMPLES: usize = 20_000;
const SPRING_SETTLE_SAMPLES_PER_PERIOD: f64 = 48.0;

fn spring_settle_time(damping: f64, stiffness: f64, mass: f64, threshold: f64, max_t: f64) -> f64 {
    let threshold = threshold.max(1e-9);
    let omega = (stiffness / mass).sqrt();
    let period = if omega > 1e-9 {
        std::f64::consts::TAU / omega
    } else {
        max_t
    };
    let desired_steps = (max_t / (period / SPRING_SETTLE_SAMPLES_PER_PERIOD)).ceil() as usize;
    let steps = desired_steps.clamp(SPRING_SETTLE_MIN_SAMPLES, SPRING_SETTLE_MAX_SAMPLES);
    let dt = max_t / steps as f64;

    let mut last_exceed_idx: usize = 0;
    for i in 0..=steps {
        let t = i as f64 * dt;
        if (spring_value_raw(t, damping, stiffness, mass) - 1.0).abs() > threshold {
            last_exceed_idx = i;
        }
    }

    if last_exceed_idx >= steps {
        return max_t;
    }

    let mut lo = last_exceed_idx as f64 * dt;
    let mut hi = (lo + dt).min(max_t);
    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        if (spring_value_raw(mid, damping, stiffness, mass) - 1.0).abs() > threshold {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    hi
}

fn spring_rest_threshold(config: &SpringConfig) -> f64 {
    config
        .rest_threshold
        .unwrap_or(DEFAULT_SPRING_REST_THRESHOLD)
        .max(1e-9)
}

pub fn spring_rest_time(config: &SpringConfig) -> f64 {
    match config.duration {
        Some(d) if d > 0.0 => d,
        _ => {
            let damping = config.damping.max(0.0);
            let stiffness = config.stiffness.max(1e-6);
            let mass = config.mass.max(1e-6);
            let threshold = spring_rest_threshold(config);
            spring_settle_time_cached(damping, stiffness, mass, threshold)
        }
    }
}

#[derive(Debug, Clone)]
pub struct AnimatedProperties {
    pub opacity: f32,
    pub translate_x: f32,
    pub translate_y: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub rotation: f32,
    pub blur: f32,
    pub blur_x: f32,
    pub blur_y: f32,
    pub letter_spacing: f32,
    pub draw_start: f32,
    pub visible_chars: i32,
    pub visible_chars_progress: f32,
    pub color: Option<String>,
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
    pub rotate_x: f32,
    pub rotate_y: f32,
    pub perspective: f32,
    pub draw_progress: f32,
    pub motion_progress: f32,
    pub clip_path_progress: f32,
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
            blur: -1.0,
            blur_x: -1.0,
            letter_spacing: f32::NAN,
            draw_start: -1.0,
            blur_y: -1.0,
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
            clip_path_progress: -1.0,
            char_animation: None,
        }
    }
}

impl AnimatedProperties {
    pub fn merge(&mut self, other: &AnimatedProperties) {
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
        if other.blur >= 0.0 {
            self.blur = other.blur;
        }
        if other.blur_x >= 0.0 {
            self.blur_x = other.blur_x;
        }

        if other.letter_spacing.is_finite() {
            self.letter_spacing = other.letter_spacing;
        }
        if other.draw_start >= 0.0 {
            self.draw_start = other.draw_start;
        }
        if other.blur_y >= 0.0 {
            self.blur_y = other.blur_y;
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
        if other.clip_path_progress >= 0.0 {
            self.clip_path_progress = other.clip_path_progress;
        }
        if other.char_animation.is_some() {
            self.char_animation = other.char_animation.clone();
        }
    }
}

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
    if !extracted.motion_paths.is_empty() {
        let motion_paths: Vec<_> = extracted.motion_paths.iter().copied().cloned().collect();
        apply_motion_paths(&mut props, &motion_paths, time);
    }
    if extracted.char_animation.is_some() {
        props.char_animation = extracted.char_animation;
    }
    props
}

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

    let preset_animations = preset.map(|p| expand_preset(p, &config, scene_duration));

    let all_animations: Vec<&Animation> = preset_animations
        .as_ref()
        .map(|pa| pa.iter().collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .chain(animations.iter())
        .collect();

    for anim in all_animations {
        let anim_time = if should_loop {
            cycle_time(anim, time, &config)
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

fn cycle_time(anim: &Animation, time: f64, config: &PresetConfig) -> f64 {
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

    let period = duration + config.repeat_delay.max(0.0);
    if period < 1e-9 {
        return time;
    }

    let elapsed = time - start;
    let mut cycle_index = (elapsed / period).floor() as i64;
    if cycle_index < 0 {
        cycle_index = 0;
    }

    if let Some(count) = config.repeat_count {
        let last_index = (count.max(1) - 1) as i64;
        if cycle_index > last_index {
            cycle_index = last_index;
        }
    }

    let within_cycle = (elapsed - cycle_index as f64 * period)
        .min(duration)
        .max(0.0);

    let backward = config.yoyo && cycle_index % 2 == 1;
    if backward {
        end - within_cycle
    } else {
        start + within_cycle
    }
}

enum ResolvedValue {
    Number(f64),
    Color(String),
}

pub fn resolve_keyframe_track(anim: &Animation, time: f64) -> KeyframeValue {
    match resolve_animation_value_full(anim, time) {
        ResolvedValue::Number(n) => KeyframeValue::Number(n),
        ResolvedValue::Color(c) => KeyframeValue::Color(c),
    }
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

            let segment_easing = kf0.easing.as_ref().unwrap_or(&anim.easing);

            let progress = match segment_easing {
                EasingType::Spring => {
                    let spring_config = anim.spring.clone().unwrap_or_default();
                    spring_value(local_t * segment_duration, &spring_config)
                }
                other => ease(local_t, other),
            };

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

fn parse_hex_components(hex: &str) -> (f64, f64, f64, f64) {
    let (r, g, b, a) = super::renderer::parse_hex_color(hex);
    (r as f64, g as f64, b as f64, a as f64)
}

pub fn lerp_color(c1: &str, c2: &str, t: f64) -> String {
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
        "blur_x" => props.blur_x = value as f32,
        "letter_spacing" => props.letter_spacing = value as f32,
        "draw_start" => props.draw_start = value as f32,
        "blur_y" => props.blur_y = value as f32,
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
        "clip_path_progress" => props.clip_path_progress = value as f32,
        _ => {}
    }
}

fn simplex_noise_1d(x: f64, seed: u64) -> f64 {
    use std::f64::consts::TAU;
    let s = seed as f64;

    (x * TAU + s * 0.1234).sin() * 0.6
        + (x * TAU * 1.7 + s * 0.5678).sin() * 0.3
        + (x * TAU * 2.9 + s * 0.9012).sin() * 0.1
}

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

        if let Some(ref easing) = wiggle.easing {
            let normalized = (noise_val + 1.0) * 0.5;
            let eased = ease(normalized, easing);
            noise_val = eased * 2.0 - 1.0;
        }

        let mut amp = wiggle.amplitude;

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

pub fn apply_orbits(props: &mut AnimatedProperties, orbits: &[OrbitConfig], time: f64) {
    use std::f64::consts::{PI, TAU};

    for orbit in orbits {
        let angle_offset = orbit.start_angle * PI / 180.0;
        let phase_offset = orbit.phase * TAU;
        let tilt_rad = orbit.tilt * PI / 180.0;

        let theta = TAU * orbit.speed * time + angle_offset + phase_offset;

        let raw_x = orbit.radius_x * theta.cos();
        let raw_y = orbit.radius_y * theta.sin();

        let x_offset = raw_x;
        let y_offset = raw_y * tilt_rad.cos();

        props.translate_x += x_offset as f32;
        props.translate_y += y_offset as f32;

        if orbit.depth > 0.0 {
            let depth_sin = theta.sin();
            let scale_factor = 1.0 + orbit.depth * depth_sin;
            props.scale_x *= scale_factor as f32;
            props.scale_y *= scale_factor as f32;
        }

        if orbit.opacity_depth > 0.0 {
            let depth_sin = theta.sin();
            let opacity_factor = 1.0 - orbit.opacity_depth * (1.0 - depth_sin) * 0.5;
            props.opacity *= opacity_factor as f32;
        }
    }
}

pub const MOTION_PATH_MIN_LENGTH: f32 = 1e-3;

pub fn motion_path_length(path_data: &str) -> Option<f32> {
    let path = skia_safe::Path::from_svg(path_data)?;
    if path.count_points() == 0 {
        return None;
    }
    let mut measure = skia_safe::PathMeasure::new(&path, false, None);
    Some(measure.length())
}

fn motion_path_progress(cfg: &MotionPathConfig, time: f64) -> f64 {
    let elapsed = time - cfg.delay;
    if elapsed <= 0.0 {
        return 0.0;
    }
    let raw = safe_div(elapsed, cfg.duration, 1.0);
    let progress = if cfg.repeat {
        raw.rem_euclid(1.0)
    } else {
        raw.clamp(0.0, 1.0)
    };
    ease(progress, &cfg.easing)
}

struct MotionPathSample {
    dx: f32,
    dy: f32,
    angle_deg: f32,
}

fn motion_path_sample(cfg: &MotionPathConfig, time: f64) -> MotionPathSample {
    let zero = MotionPathSample {
        dx: 0.0,
        dy: 0.0,
        angle_deg: 0.0,
    };
    let Some(path) = skia_safe::Path::from_svg(&cfg.path) else {
        return zero;
    };
    if path.count_points() == 0 {
        return zero;
    }

    let mut measure = skia_safe::PathMeasure::new(&path, false, None);
    let length = measure.length();

    if length <= MOTION_PATH_MIN_LENGTH {
        let (x, y) = path.points().first().map_or((0.0, 0.0), |p| (p.x, p.y));
        return MotionPathSample {
            dx: x,
            dy: y,
            angle_deg: 0.0,
        };
    }

    let progress = motion_path_progress(cfg, time) as f32;
    let distance = (length * progress).clamp(0.0, length);

    match measure.pos_tan(distance) {
        Some((pos, tangent)) => {
            let angle_deg = if cfg.orient {
                tangent.y.atan2(tangent.x).to_degrees() + cfg.orient_offset as f32
            } else {
                0.0
            };
            MotionPathSample {
                dx: pos.x,
                dy: pos.y,
                angle_deg,
            }
        }
        None => {
            let (x, y) = path.points().first().map_or((0.0, 0.0), |p| (p.x, p.y));
            MotionPathSample {
                dx: x,
                dy: y,
                angle_deg: 0.0,
            }
        }
    }
}

pub fn apply_motion_paths(props: &mut AnimatedProperties, paths: &[MotionPathConfig], time: f64) {
    for cfg in paths {
        let sample = motion_path_sample(cfg, time);
        props.translate_x += sample.dx;
        props.translate_y += sample.dy;
        props.rotation += sample.angle_deg;
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
        "blur_x" => props.blur_x as f64,
        "letter_spacing" => props.letter_spacing as f64,
        "draw_start" => props.draw_start as f64,
        "blur_y" => props.blur_y as f64,
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
        "clip_path_progress" => props.clip_path_progress as f64,
        _ => 0.0,
    }
}

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

fn apply_spring_to_motion(animations: &mut [Animation], spring: &SpringConfig) {
    for anim in animations.iter_mut() {
        if !is_motion_property(&anim.property) || anim.keyframes.len() < 2 {
            continue;
        }
        if anim.keyframes.len() > 2 {
            let first = anim.keyframes.first().unwrap().clone();
            let last = anim.keyframes.last().unwrap().clone();
            if (first.value.as_f64() - last.value.as_f64()).abs() < 1e-9 {
                continue;
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
        AnimationPreset::PopIn => {
            let pulse = 1.0 + config.overshoot.unwrap_or(0.18);
            let placed = delay + dur * 0.6;
            let peak = delay + dur * 0.8;
            vec![
                kf_anim(
                    "opacity",
                    delay,
                    0.0,
                    delay + dur * 0.25,
                    1.0,
                    EasingType::EaseOut,
                ),
                Animation {
                    property: "scale".to_string(),
                    keyframes: vec![
                        Keyframe {
                            time: delay,
                            value: KeyframeValue::Number(0.0),
                            easing: Some(EasingType::EaseOutBack),
                        },
                        Keyframe {
                            time: placed,
                            value: KeyframeValue::Number(1.0),
                            easing: Some(EasingType::EaseOutQuad),
                        },
                        Keyframe {
                            time: peak,
                            value: KeyframeValue::Number(pulse),
                            easing: Some(EasingType::EaseOutElastic),
                        },
                        Keyframe {
                            time: end,
                            value: KeyframeValue::Number(1.0),
                            easing: None,
                        },
                    ],
                    easing: EasingType::EaseOut,
                    spring: None,
                },
            ]
        }

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

        AnimationPreset::Float3d => {
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
            ..Default::default()
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
            ..Default::default()
        }),
    }
}

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
            ..Default::default()
        }
    }

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
        let plain = sample(&[AnimationEffect::FadeInUp(timing(0.8, None))], 0.8);
        let min_plain = plain.iter().map(|(_, y, _)| *y).fold(f64::MAX, f64::min);
        assert!(
            min_plain >= -0.01,
            "without spring translate_y must never overshoot below 0, got min {min_plain}"
        );

        let sprung = sample(
            &[AnimationEffect::FadeInUp(timing(0.8, Some(underdamped())))],
            0.8,
        );
        let min_sprung = sprung.iter().map(|(_, y, _)| *y).fold(f64::MAX, f64::min);
        assert!(
            min_sprung < -0.5,
            "with spring translate_y must overshoot below 0, got min {min_sprung}"
        );

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
        let overdamped = SpringConfig {
            damping: 40.0,
            stiffness: 100.0,
            mass: 1.0,
            ..Default::default()
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
        let fx = [AnimationEffect::ScaleIn(timing(0.8, Some(underdamped())))];
        let mut max_scale = f64::MIN;
        for i in 0..=80 {
            let t = 0.8 * i as f64 / 80.0;
            let s = resolve_props_for_effects(&fx, t, 5.0).scale_x as f64;
            max_scale = max_scale.max(s);
        }
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

        let re = serde_json::to_string(&fx).unwrap();
        let back: AnimationEffect = serde_json::from_str(&re).unwrap();
        assert_eq!(fx, back);

        let plain: AnimationEffect = serde_json::from_str(r#"{ "name": "fade_in_up" }"#).unwrap();
        let AnimationEffect::FadeInUp(t) = &plain else {
            panic!("wrong variant");
        };
        assert!(t.spring.is_none());
    }
}

#[cfg(test)]
mod glow_tests {

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
    use super::*;
    use crate::schema::AnimationEffect;

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
    use super::*;
    use crate::schema::AnimationEffect;

    fn timing(delay: f64, duration: f64) -> AnimationTimingFixture {
        AnimationTimingFixture { delay, duration }
    }

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
        let mid = resolve_props_for_effects(&[fx], 2.0, 10.0).rotation as f64;
        assert!(
            (mid - 180.0).abs() < 5.0,
            "spin at t=2.0 (cycle midpoint) must be near 180deg, got {mid}"
        );
    }
}

#[cfg(test)]
mod keyframes_composition_tests {
    use super::*;
    use crate::schema::{Animation, AnimationEffect, Keyframe, KeyframeValue, KeyframesConfig};

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
        let a1 = ramp("translate_x", 100.0, 0.0);
        let b1 = ramp("translate_x", 40.0, 0.5);
        let combined_1 = resolve_props_for_effects(&[a1, b1.clone()], 1.0, 5.0).translate_x as f64;
        let b1_alone = resolve_props_for_effects(&[b1], 1.0, 5.0).translate_x as f64;
        assert!(
            (combined_1 - b1_alone).abs() < 1e-4,
            "B (declared last) must alone determine translate_x at t=1.0: combined={combined_1}, B-alone={b1_alone}"
        );

        let a2 = ramp("translate_x", 100.0, 0.5);
        let b2 = ramp("translate_x", 40.0, 0.0);
        let combined_2 = resolve_props_for_effects(&[a2, b2.clone()], 1.0, 5.0).translate_x as f64;
        let b2_alone = resolve_props_for_effects(&[b2], 1.0, 5.0).translate_x as f64;
        assert!(
            (combined_2 - b2_alone).abs() < 1e-4,
            "B (declared last) must alone determine translate_x at t=1.0 even with delay swapped: \
             combined={combined_2}, B-alone={b2_alone}"
        );

        assert!(
            (combined_1 - combined_2).abs() > 1.0,
            "sanity: the two cases must differ (B's own timing changed): {combined_1} vs {combined_2}"
        );
    }
}

#[cfg(test)]
mod keyframes_loop_tests {
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
    use super::*;

    #[test]
    fn zero_mass_does_not_produce_nan() {
        let config = SpringConfig {
            damping: 10.0,
            stiffness: 100.0,
            mass: 0.0,
            ..Default::default()
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
            ..Default::default()
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
            ..Default::default()
        };
        let v_at_5s = spring_value(5.0, &config);
        assert!(
            v_at_5s.is_finite() && v_at_5s.abs() < 100.0,
            "spring_value(t=5.0) with damping=-20 must stay bounded (finite and reasonably \
             small), got {v_at_5s} — negative damping must not diverge to +-infinity"
        );
    }
}

#[cfg(test)]
mod spring_duration_tests {
    use super::*;

    fn brute_force_settle_time(
        damping: f64,
        stiffness: f64,
        mass: f64,
        threshold: f64,
        max_t: f64,
        steps: usize,
    ) -> f64 {
        let dt = max_t / steps as f64;
        let mut last_exceed = 0.0;
        for i in 0..=steps {
            let t = i as f64 * dt;
            if (spring_value_raw(t, damping, stiffness, mass) - 1.0).abs() > threshold {
                last_exceed = t;
            }
        }
        last_exceed
    }

    #[test]
    fn red_phase_duration_is_ignored_by_the_raw_physical_solver() {
        let v = spring_value_raw(0.8, 6.0, 120.0, 1.0);
        assert!(
            (v - 1.027616).abs() < 1e-5,
            "captured red-phase reference value drifted: got {v}, expected ~1.027616"
        );
        assert!(
            (v - 1.0).abs() > 0.02,
            "red-phase claim: at t=duration the unscaled spring must still be far from rest \
             (got diff {:.6}, expected > 0.02)",
            (v - 1.0).abs()
        );
    }

    #[test]
    fn duration_makes_the_spring_settle_exactly_there() {
        let config = SpringConfig {
            damping: 6.0,
            stiffness: 120.0,
            mass: 1.0,
            duration: Some(0.8),
            rest_threshold: None,
        };
        let threshold = DEFAULT_SPRING_REST_THRESHOLD;

        let v_at_duration = spring_value(0.8, &config);
        assert!(
            (v_at_duration - 1.0).abs() <= threshold,
            "spring_value(0.8, ..) with duration=Some(0.8) must be within {threshold} of rest, \
             got {v_at_duration} (diff {})",
            (v_at_duration - 1.0).abs()
        );

        let v_at_half = spring_value(0.4, &config);
        assert!(
            (v_at_half - 1.0).abs() > threshold,
            "sanity: spring must not already be at rest at half of duration, got diff {}",
            (v_at_half - 1.0).abs()
        );
    }

    #[test]
    fn spring_rest_time_returns_duration_verbatim_when_set() {
        let config = SpringConfig {
            damping: 6.0,
            stiffness: 120.0,
            mass: 1.0,
            duration: Some(0.8),
            rest_threshold: None,
        };
        assert_eq!(spring_rest_time(&config), 0.8);
    }

    #[test]
    fn spring_rest_time_matches_a_brute_force_reference_without_duration() {
        let cases: [(f64, f64, f64, &str); 5] = [
            (15.0, 100.0, 1.0, "default"),
            (12.0, 100.0, 1.0, "kf_anim_spring"),
            (6.0, 120.0, 1.0, "underdamped elastic_in-like"),
            (
                20.0,
                100.0,
                1.0,
                "critically damped (damping = 2*sqrt(stiffness*mass))",
            ),
            (60.0, 100.0, 1.0, "overdamped"),
        ];
        for (damping, stiffness, mass, label) in cases {
            let config = SpringConfig {
                damping,
                stiffness,
                mass,
                duration: None,
                rest_threshold: None,
            };
            let threshold = DEFAULT_SPRING_REST_THRESHOLD;
            let got = spring_rest_time(&config);
            let reference = brute_force_settle_time(
                damping,
                stiffness,
                mass,
                threshold,
                MAX_SPRING_SEARCH_SECONDS,
                400_000,
            );
            let abs_err = (got - reference).abs();
            assert!(
                abs_err < 0.05,
                "{label}: spring_rest_time={got:.5}s vs brute-force reference={reference:.5}s \
                 (|err|={abs_err:.5}s, expected < 0.05s)"
            );
        }
    }

    #[test]
    fn overdamped_spring_never_reaches_target_exactly_but_settle_time_is_found() {
        let config = SpringConfig {
            damping: 200.0,
            stiffness: 100.0,
            mass: 1.0,
            duration: None,
            rest_threshold: None,
        };
        let t = spring_rest_time(&config);
        assert!(
            t > 0.0 && t < MAX_SPRING_SEARCH_SECONDS,
            "expected a finite, non-degenerate settle time, got {t}"
        );

        for i in 1..=200 {
            let sample_t = t + i as f64 * 0.1;
            let v = spring_value_raw(sample_t, 200.0, 100.0, 1.0);
            assert_ne!(
                v, 1.0,
                "an overdamped spring must never hit its target exactly (t={sample_t})"
            );
        }
    }

    #[test]
    fn undamped_spring_is_capped_not_infinite() {
        let config = SpringConfig {
            damping: 0.0,
            stiffness: 100.0,
            mass: 1.0,
            duration: None,
            rest_threshold: None,
        };
        let t = spring_rest_time(&config);
        assert_eq!(
            t, MAX_SPRING_SEARCH_SECONDS,
            "an undamped spring must be reported as capped at the search bound, got {t}"
        );
    }

    #[test]
    fn very_lightly_damped_spring_is_also_capped_when_beyond_the_bound() {
        let config = SpringConfig {
            damping: 0.05,
            stiffness: 100.0,
            mass: 1.0,
            duration: None,
            rest_threshold: None,
        };
        let t = spring_rest_time(&config);
        assert_eq!(
            t, MAX_SPRING_SEARCH_SECONDS,
            "expected the search to hit its cap, got {t}"
        );
    }

    #[test]
    fn duration_remap_preserves_shape() {
        let damping = 6.0;
        let stiffness = 120.0;
        let mass = 1.0;
        let natural = SpringConfig {
            damping,
            stiffness,
            mass,
            duration: None,
            rest_threshold: None,
        };
        let natural_rest = spring_rest_time(&natural);

        let pinned_duration = 2.5;
        let pinned = SpringConfig {
            damping,
            stiffness,
            mass,
            duration: Some(pinned_duration),
            rest_threshold: None,
        };

        let mut natural_overshoots = 0;
        let mut pinned_overshoots = 0;
        let mut max_natural_overshoot = 0.0_f64;
        let mut max_pinned_overshoot = 0.0_f64;
        let mut prev_natural_over = false;
        let mut prev_pinned_over = false;

        for i in 0..=1000 {
            let frac = i as f64 / 1000.0;
            let v_natural = spring_value(frac * natural_rest, &natural);
            let v_pinned = spring_value(frac * pinned_duration, &pinned);

            assert!(
                (v_natural - v_pinned).abs() < 1e-9,
                "shape mismatch at fraction {frac}: natural={v_natural} pinned={v_pinned}"
            );

            let natural_over = v_natural > 1.0;
            if natural_over && !prev_natural_over {
                natural_overshoots += 1;
            }
            prev_natural_over = natural_over;
            max_natural_overshoot = max_natural_overshoot.max(v_natural - 1.0);

            let pinned_over = v_pinned > 1.0;
            if pinned_over && !prev_pinned_over {
                pinned_overshoots += 1;
            }
            prev_pinned_over = pinned_over;
            max_pinned_overshoot = max_pinned_overshoot.max(v_pinned - 1.0);
        }

        assert!(
            natural_overshoots > 0,
            "expected this underdamped spring to overshoot at least once"
        );
        assert_eq!(
            natural_overshoots, pinned_overshoots,
            "oscillation count must be identical with/without duration"
        );
        assert!(
            (max_natural_overshoot - max_pinned_overshoot).abs() < 1e-9,
            "overshoot amplitude must be identical with/without duration: natural={max_natural_overshoot} pinned={max_pinned_overshoot}"
        );
    }

    #[test]
    fn duration_does_not_change_delay_semantics() {
        let config = SpringConfig {
            damping: 6.0,
            stiffness: 120.0,
            mass: 1.0,
            duration: Some(0.8),
            rest_threshold: None,
        };
        assert_eq!(
            spring_value(0.0, &config),
            spring_value_raw(0.0, 6.0, 120.0, 1.0)
        );
    }
}

#[cfg(test)]
mod motion_path_tests {
    use super::*;

    fn cfg(path: &str) -> MotionPathConfig {
        MotionPathConfig {
            path: path.to_string(),
            delay: 0.0,
            duration: 1.0,
            repeat: false,
            orient: false,
            orient_offset: 0.0,
            easing: EasingType::Linear,
        }
    }

    #[test]
    fn mid_path_progress_lands_on_the_curve_not_on_the_endpoint_chord() {
        let c = cfg("M0,0 L100,0 L100,100");
        let sample = motion_path_sample(&c, 0.5);

        assert!(
            (sample.dx - 100.0).abs() < 0.5,
            "expected dx≈100 (on the path's corner), got {}",
            sample.dx
        );
        assert!(
            (sample.dy - 0.0).abs() < 0.5,
            "expected dy≈0 (on the path's corner), got {}",
            sample.dy
        );

        let chord_x = 50.0f32;
        let chord_y = 50.0f32;
        let dist_from_chord_midpoint =
            ((sample.dx - chord_x).powi(2) + (sample.dy - chord_y).powi(2)).sqrt();
        assert!(
            dist_from_chord_midpoint > 40.0,
            "t=0.5 must not land near the endpoint-to-endpoint chord midpoint (50,50) — got \
             ({}, {}), which would also pass under a plain linear-interpolation bug",
            sample.dx,
            sample.dy
        );
    }

    #[test]
    fn progress_zero_and_one_land_on_the_paths_own_endpoints() {
        let c = cfg("M10,20 L310,20 L310,220");
        let start = motion_path_sample(&c, 0.0);
        assert!((start.dx - 10.0).abs() < 0.5 && (start.dy - 20.0).abs() < 0.5);

        let end = motion_path_sample(&c, 1.0);
        assert!((end.dx - 310.0).abs() < 0.5 && (end.dy - 220.0).abs() < 0.5);
    }

    #[test]
    fn path_coordinates_are_used_literally_as_the_translate_delta() {
        let c = cfg("M100,50 L300,50");
        let sample = motion_path_sample(&c, 0.0);
        assert!(
            (sample.dx - 100.0).abs() < 0.5 && (sample.dy - 50.0).abs() < 0.5,
            "expected the raw path start point (100, 50) as the delta, got ({}, {})",
            sample.dx,
            sample.dy
        );
    }

    #[test]
    fn apply_motion_paths_writes_translate_and_rotation_additively() {
        let mut props = AnimatedProperties {
            translate_x: 5.0,
            translate_y: -5.0,
            ..AnimatedProperties::default()
        };
        let mut c = cfg("M0,0 L100,0");
        c.orient = true;
        apply_motion_paths(&mut props, &[c], 0.0);

        assert!((props.translate_x - 5.0).abs() < 0.5);
        assert!((props.translate_y - (-5.0)).abs() < 0.5);
        assert!(props.rotation.abs() < 0.5, "got {}", props.rotation);
    }

    #[test]
    fn orient_false_never_touches_rotation() {
        let c = cfg("M0,0 L0,100");
        let mut props = AnimatedProperties::default();
        apply_motion_paths(&mut props, &[c], 0.5);
        assert_eq!(props.rotation, 0.0);
    }

    #[test]
    fn orient_true_rotates_toward_the_tangent_and_offset_is_additive() {
        let mut vertical = cfg("M0,0 L0,100");
        vertical.orient = true;
        let sample = motion_path_sample(&vertical, 0.5);
        assert!(
            (sample.angle_deg - 90.0).abs() < 1.0,
            "got {}",
            sample.angle_deg
        );

        let mut with_offset = vertical.clone();
        with_offset.orient_offset = 10.0;
        let offset_sample = motion_path_sample(&with_offset, 0.5);
        assert!(
            (offset_sample.angle_deg - 100.0).abs() < 1.0,
            "orient_offset must add on top of the tangent angle, got {}",
            offset_sample.angle_deg
        );
    }

    #[test]
    fn single_point_path_holds_position_and_never_produces_nan() {
        let mut c = cfg("M50,50");
        c.orient = true;
        for t in [-1.0, 0.0, 0.3, 0.5, 1.0, 2.0] {
            let sample = motion_path_sample(&c, t);
            assert!(sample.dx.is_finite() && sample.dy.is_finite() && sample.angle_deg.is_finite());
            assert!((sample.dx - 50.0).abs() < 0.5 && (sample.dy - 50.0).abs() < 0.5);
            assert_eq!(
                sample.angle_deg, 0.0,
                "orientation is undefined at zero length and must default to 0, not NaN"
            );
        }
    }

    #[test]
    fn coincident_points_zero_length_path_holds_without_nan() {
        let mut c = cfg("M10,10 L10,10 L10,10");
        c.orient = true;
        let sample = motion_path_sample(&c, 0.5);
        assert!(sample.dx.is_finite() && sample.dy.is_finite() && sample.angle_deg.is_finite());
        assert!((sample.dx - 10.0).abs() < 0.5 && (sample.dy - 10.0).abs() < 0.5);
        assert_eq!(sample.angle_deg, 0.0);
    }

    #[test]
    fn empty_path_data_never_panics_or_produces_nan() {
        let c = cfg("");
        let sample = motion_path_sample(&c, 0.5);
        assert_eq!((sample.dx, sample.dy, sample.angle_deg), (0.0, 0.0, 0.0));
    }

    #[test]
    fn unparsable_path_data_never_panics_or_produces_nan() {
        let c = cfg("definitely not svg path data");
        let sample = motion_path_sample(&c, 0.5);
        assert!(sample.dx.is_finite() && sample.dy.is_finite() && sample.angle_deg.is_finite());
    }

    #[test]
    fn zero_or_negative_duration_never_produces_nan() {
        for duration in [0.0, -1.0, -0.5] {
            let mut c = cfg("M0,0 L100,0");
            c.duration = duration;
            for t in [0.0, 0.5, 1.0, 5.0] {
                let sample = motion_path_sample(&c, t);
                assert!(
                    sample.dx.is_finite() && sample.dy.is_finite() && sample.angle_deg.is_finite(),
                    "duration={duration} time={t} produced a non-finite sample: dx={} dy={}",
                    sample.dx,
                    sample.dy
                );
            }
        }
    }

    #[test]
    fn motion_path_length_reports_none_for_empty_or_unparsable_input() {
        assert_eq!(motion_path_length(""), None);
        assert_eq!(motion_path_length("not a path"), None);
    }

    #[test]
    fn motion_path_length_reports_near_zero_for_a_single_point() {
        let len = motion_path_length("M50,50").expect("single point is a valid, parseable path");
        assert!(len <= MOTION_PATH_MIN_LENGTH, "got {len}");
    }

    #[test]
    fn motion_path_length_reports_the_real_length_for_a_real_path() {
        let len = motion_path_length("M0,0 L100,0").expect("valid path");
        assert!((len - 100.0).abs() < 0.5, "got {len}");
    }

    #[test]
    fn looping_wraps_progress_back_toward_the_start() {
        let mut c = cfg("M0,0 L100,0 L100,100");
        c.repeat = true;
        c.duration = 1.0;
        let sample = motion_path_sample(&c, 1.5);
        assert!((sample.dx - 100.0).abs() < 0.5 && (sample.dy - 0.0).abs() < 0.5);
    }

    #[test]
    fn non_looping_holds_at_the_end_past_delay_plus_duration() {
        let c = cfg("M0,0 L100,0 L100,100");
        let at_end = motion_path_sample(&c, 1.0);
        let past_end = motion_path_sample(&c, 5.0);
        assert_eq!(at_end.dx, past_end.dx);
        assert_eq!(at_end.dy, past_end.dy);
    }

    #[test]
    fn sampling_is_deterministic_across_repeated_calls() {
        let c = cfg("M0,0 C50,-100 150,-100 200,0");
        let first = motion_path_sample(&c, 0.37);
        for _ in 0..25 {
            let again = motion_path_sample(&c, 0.37);
            assert_eq!(first.dx, again.dx);
            assert_eq!(first.dy, again.dy);
            assert_eq!(first.angle_deg, again.angle_deg);
        }
    }

    #[test]
    fn resolve_props_for_effects_is_deterministic_and_reaches_translate() {
        let effect = AnimationEffect::MotionPath(cfg("M0,0 L400,0"));
        let effects = vec![effect];
        let a = resolve_props_for_effects(&effects, 0.5, 1.0);
        let b = resolve_props_for_effects(&effects, 0.5, 1.0);
        assert_eq!(a.translate_x, b.translate_x);
        assert_eq!(a.translate_y, b.translate_y);
        assert!(
            (a.translate_x - 200.0).abs() < 1.0,
            "expected ~halfway along a straight 400px path, got {}",
            a.translate_x
        );
    }
}

#[cfg(test)]
mod char_animation_tuning_tests {
    use super::*;

    fn anim(stagger: f32, jitter: f32, seed: u32) -> ResolvedCharAnimation {
        ResolvedCharAnimation {
            preset: CharAnimPreset::SlideUp,
            granularity: TextAnimGranularity::Word,
            stagger,
            duration: 0.4,
            easing: EasingType::Linear,
            delay: 0.5,
            overshoot: 0.08,
            blur: DEFAULT_CHAR_BLUR_SIGMA,
            direction: TextAnimDirection::Up,
            distance: 1.0,
            scale_from: None,
            jitter,
            seed,
            ink_from: None,
        }
    }

    #[test]
    fn without_jitter_units_are_evenly_spaced() {
        let a = anim(0.2, 0.0, 0);
        for i in 0..6 {
            let expected = 0.5 + i as f64 * 0.2;
            assert!(
                (a.unit_start(i) - expected).abs() < 1e-6,
                "unit {i} should start at {expected}, got {}",
                a.unit_start(i)
            );
        }
    }

    #[test]
    fn jitter_is_a_pure_function_of_index_and_seed() {
        let a = anim(0.2, 0.6, 42);
        let b = anim(0.2, 0.6, 42);
        for i in 0..32 {
            assert_eq!(
                a.unit_start(i).to_bits(),
                b.unit_start(i).to_bits(),
                "unit {i} must resolve bit-identically for the same seed"
            );
        }
    }

    #[test]
    fn a_different_seed_reshuffles_the_rhythm() {
        let a = anim(0.2, 0.6, 1);
        let b = anim(0.2, 0.6, 2);
        let differing = (0..32)
            .filter(|&i| a.unit_start(i) != b.unit_start(i))
            .count();
        assert!(
            differing > 24,
            "changing the seed should move nearly every unit, but only {differing}/32 moved"
        );
    }

    #[test]
    fn jitter_actually_perturbs_the_even_spacing() {
        let even = anim(0.2, 0.0, 7);
        let jittered = anim(0.2, 0.8, 7);
        let moved = (1..32)
            .filter(|&i| (even.unit_start(i) - jittered.unit_start(i)).abs() > 1e-6)
            .count();
        assert!(
            moved > 20,
            "jitter should visibly perturb the march, but only {moved}/31 units moved"
        );
    }

    #[test]
    fn no_unit_starts_before_the_effects_own_delay() {
        let a = anim(0.2, 2.0, 99);
        for i in 0..64 {
            assert!(
                a.unit_start(i) >= 0.5 - 1e-9,
                "unit {i} started at {} — before the effect's own 0.5s delay",
                a.unit_start(i)
            );
        }
    }

    #[test]
    fn a_zero_stagger_is_unaffected_by_jitter() {
        let a = anim(0.0, 1.0, 3);
        for i in 0..8 {
            assert!((a.unit_start(i) - 0.5).abs() < 1e-9);
        }
    }
}

#[cfg(test)]
mod easing_steps_tests {
    use super::*;

    #[test]
    fn steps_one_holds_the_start_value_until_the_very_end() {
        let s = EasingType::Steps(1);
        for t in [0.0, 0.1, 0.5, 0.9, 0.999_999] {
            assert_eq!(
                ease(t, &s),
                0.0,
                "steps(1) must hold at 0.0 for the entire segment, t={t}"
            );
        }
        assert_eq!(ease(1.0, &s), 1.0, "steps(1) jumps to 1.0 exactly at t=1.0");
    }

    #[test]
    fn steps_four_holds_four_discrete_levels() {
        let s = EasingType::Steps(4);
        assert_eq!(ease(0.0, &s), 0.0);
        assert_eq!(ease(0.1, &s), 0.0);
        assert_eq!(ease(0.24, &s), 0.0);
        assert_eq!(ease(0.25, &s), 0.25);
        assert_eq!(ease(0.49, &s), 0.25);
        assert_eq!(ease(0.50, &s), 0.50);
        assert_eq!(ease(0.75, &s), 0.75);
        assert_eq!(ease(0.999, &s), 0.75);
        assert_eq!(ease(1.0, &s), 1.0);
    }

    #[test]
    fn steps_zero_does_not_panic_or_divide_by_zero() {
        let s = EasingType::Steps(0);
        for t in [0.0, 0.5, 1.0] {
            assert!(ease(t, &s).is_finite());
        }
    }
}

#[cfg(test)]
mod repeat_cycle_tests {
    use super::*;

    fn ramp(duration: f64) -> Animation {
        kf_anim("x", 0.0, 0.0, duration, 1.0, EasingType::Linear)
    }

    fn config(repeat_count: Option<u32>, yoyo: bool, repeat_delay: f64) -> PresetConfig {
        PresetConfig {
            repeat: true,
            repeat_count,
            yoyo,
            repeat_delay,
            ..Default::default()
        }
    }

    #[test]
    fn repeat_true_with_no_new_fields_is_byte_identical_to_legacy_loop_time() {
        fn legacy_loop_time(start: f64, duration: f64, time: f64) -> f64 {
            if duration < 1e-9 || time < start {
                return time;
            }
            start + ((time - start) % duration)
        }

        let duration = 2.0;
        let anim = ramp(duration);
        let cfg = config(None, false, 0.0);
        for t in [
            -1.0, 0.0, 0.3, 0.999_999, 1.0, 1.5, 1.999_999, 2.0, 2.000_001, 2.5, 3.999_999, 4.0,
            4.1, 10.3, 100.7,
        ] {
            let got = cycle_time(&anim, t, &cfg);
            let legacy = legacy_loop_time(0.0, duration, t);
            assert!(
                (got - legacy).abs() < 1e-12,
                "t={t}: cycle_time={got}, legacy loop_time={legacy}"
            );
        }
    }

    #[test]
    fn a_finite_repeat_count_freezes_on_the_last_plays_resting_value() {
        let duration = 1.0;
        let anim = ramp(duration);
        let cfg = config(Some(3), false, 0.0);
        let frozen_at = cycle_time(&anim, 3.0, &cfg);
        for t in [3.0, 3.5, 10.0, 1_000.0] {
            let got = cycle_time(&anim, t, &cfg);
            assert!(
                (got - frozen_at).abs() < 1e-9,
                "t={t} must stay frozen at the last play's end ({frozen_at}), got {got}"
            );
        }
        assert!((cycle_time(&anim, 0.5, &cfg) - 0.5).abs() < 1e-9);
        assert!((cycle_time(&anim, 1.5, &cfg) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn repeat_count_of_one_behaves_like_a_single_play() {
        let duration = 1.0;
        let anim = ramp(duration);
        let looping = config(Some(1), false, 0.0);
        for t in [0.0, 0.5, 1.0, 2.0, 5.0] {
            let got = cycle_time(&anim, t, &looping);
            let single_play = t.min(duration);
            assert!((got - single_play).abs() < 1e-9, "t={t}: got {got}");
        }
    }

    #[test]
    fn yoyo_reverses_every_other_cycle() {
        let duration = 1.0;
        let anim = ramp(duration);
        let cfg = config(None, true, 0.0);
        assert!((cycle_time(&anim, 0.25, &cfg) - 0.25).abs() < 1e-9);
        assert!((cycle_time(&anim, 1.25, &cfg) - 0.75).abs() < 1e-9);
        assert!((cycle_time(&anim, 1.75, &cfg) - 0.25).abs() < 1e-9);
        assert!((cycle_time(&anim, 2.25, &cfg) - 0.25).abs() < 1e-9);
    }

    #[test]
    fn yoyo_produces_a_continuous_value_at_every_cycle_boundary() {
        let duration = 1.0;
        let anim = ramp(duration);
        let cfg = config(None, true, 0.0);
        for boundary in [1.0, 2.0, 3.0] {
            let just_before = cycle_time(&anim, boundary - 1e-6, &cfg);
            let at = cycle_time(&anim, boundary, &cfg);
            assert!(
                (just_before - at).abs() < 1e-3,
                "boundary {boundary}: just_before={just_before}, at={at}"
            );
        }
    }

    #[test]
    fn repeat_delay_holds_the_resting_value_between_plays() {
        let duration = 1.0;
        let anim = ramp(duration);
        let cfg = config(None, false, 0.5);
        assert!((cycle_time(&anim, 0.5, &cfg) - 0.5).abs() < 1e-9);
        assert!((cycle_time(&anim, 1.0, &cfg) - 1.0).abs() < 1e-9);
        assert!((cycle_time(&anim, 1.3, &cfg) - 1.0).abs() < 1e-9);
        assert!((cycle_time(&anim, 1.5, &cfg) - 0.0).abs() < 1e-9);
        assert!((cycle_time(&anim, 2.0, &cfg) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn resolve_animations_actually_applies_yoyo_and_repeat_count_end_to_end() {
        let animations = vec![kf_anim(
            "translate_x",
            0.0,
            0.0,
            1.0,
            1.0,
            EasingType::Linear,
        )];
        let cfg = config(Some(2), true, 0.0);
        let at = |t: f64| -> f64 {
            resolve_animations(&animations, None, Some(&cfg), t, 10.0).translate_x as f64
        };
        assert!((at(0.25) - 0.25).abs() < 1e-4, "forward play: {}", at(0.25));
        assert!(
            (at(1.25) - 0.75).abs() < 1e-4,
            "yoyo'd second play: {}",
            at(1.25)
        );
        let frozen = at(2.0);
        assert!(
            (at(5.0) - frozen).abs() < 1e-4,
            "must stay frozen: {}",
            at(5.0)
        );
    }
}

#[cfg(test)]
mod burst_progress_tests {
    use super::*;

    fn cfg() -> BurstConfig {
        BurstConfig {
            delay: 1.0,
            duration: 0.5,
            count: 8,
            length: 40.0,
            gap: 12.0,
            width: 4.0,
            color: "#FFB020".to_string(),
            seed: 3,
            jitter: 0.2,
        }
    }

    #[test]
    fn nothing_before_the_delay_and_nothing_at_or_after_the_end() {
        let c = cfg();
        assert_eq!(burst_progress(&c, c.delay - 0.01), None);
        assert_eq!(burst_progress(&c, c.delay + c.duration), None);
        assert_eq!(burst_progress(&c, 9.0), None);
        assert!(burst_progress(&c, c.delay).is_some());
        assert!(burst_progress(&c, c.delay + c.duration * 0.99).is_some());
    }

    #[test]
    fn a_frame_landing_one_ulp_inside_the_window_still_paints_nothing() {
        let mut c = cfg();
        c.duration = 0.4;
        let at_end = c.delay + c.duration;
        let progress = burst_progress(&c, at_end)
            .expect("0.4 is not exactly representable: delay + duration - delay < duration");
        let (tail, head) = burst_stroke_span(progress, 0.0);
        assert_eq!(
            tail, head,
            "the window test cannot be exact for every duration, so the guarantee is \
             geometric: a stroke at progress 1.0 has zero length and paints nothing"
        );
    }

    #[test]
    fn a_zero_or_negative_duration_disables_the_effect_entirely() {
        let mut c = cfg();
        c.duration = 0.0;
        assert_eq!(burst_progress(&c, 1.0), None);
        c.duration = -1.0;
        assert_eq!(burst_progress(&c, 1.0), None);
    }

    #[test]
    fn the_stroke_has_zero_length_at_both_ends_of_its_own_window() {
        let (tail, head) = burst_stroke_span(0.0, 0.0);
        assert_eq!(tail, head, "at the start the head has not left the tail");
        let (tail, head) = burst_stroke_span(1.0, 0.0);
        assert_eq!(tail, head, "at the end the tail has caught the head");
    }

    #[test]
    fn the_head_reaches_the_far_end_halfway_through_while_the_tail_waits() {
        let (tail, head) = burst_stroke_span(0.5, 0.0);
        assert!((head - 1.0).abs() < 1e-6, "head at the far end, got {head}");
        assert_eq!(tail, 0.0, "tail has not started");
    }

    #[test]
    fn the_head_never_overtakes_the_far_end_nor_the_tail_the_head() {
        for step in 0..=100 {
            let p = step as f32 / 100.0;
            for phase in [0.0_f32, 0.15, 0.4] {
                let (tail, head) = burst_stroke_span(p, phase);
                assert!((0.0..=1.0).contains(&head), "head {head} at p={p}");
                assert!(tail <= head + 1e-6, "tail {tail} past head {head} at p={p}");
            }
        }
    }

    #[test]
    fn a_phase_delays_the_stroke_without_letting_it_outlive_the_window() {
        let (tail, head) = burst_stroke_span(0.3, 0.4);
        assert_eq!(tail, head, "a phased stroke has not started at p=0.3");
        let (tail, head) = burst_stroke_span(1.0, 0.4);
        assert_eq!(tail, head, "a phased stroke still ends with the window");
    }
}

#[cfg(test)]
mod merge_contract_tests {
    use super::*;

    fn bucket() -> AnimatedProperties {
        AnimatedProperties::default()
    }

    #[test]
    fn opacity_and_scale_multiply_across_buckets() {
        let mut props = bucket();
        props.opacity = 0.5;
        props.scale_x = 2.0;
        props.scale_y = 2.0;

        let mut other = bucket();
        other.opacity = 0.8;
        other.scale_x = 1.5;
        other.scale_y = 1.5;
        props.merge(&other);

        assert!((props.opacity - 0.4).abs() < 1e-6, "got {}", props.opacity);
        assert!((props.scale_x - 3.0).abs() < 1e-6, "got {}", props.scale_x);
        assert!((props.scale_y - 3.0).abs() < 1e-6, "got {}", props.scale_y);
    }

    #[test]
    fn translation_and_rotation_add_across_buckets() {
        let mut props = bucket();
        props.translate_x = 10.0;
        props.translate_y = -4.0;
        props.rotation = 30.0;

        let mut other = bucket();
        other.translate_x = 5.0;
        other.translate_y = 4.0;
        other.rotation = 15.0;
        props.merge(&other);

        assert!((props.translate_x - 15.0).abs() < 1e-6);
        assert!((props.translate_y - 0.0).abs() < 1e-6);
        assert!((props.rotation - 45.0).abs() < 1e-6);
    }

    #[test]
    fn a_neutral_value_in_a_composing_property_is_a_no_op_by_arithmetic() {
        let mut props = bucket();
        props.opacity = 0.5;
        props.scale_x = 2.0;
        props.translate_x = 10.0;

        let mut other = bucket();
        other.opacity = 1.0;
        other.scale_x = 1.0;
        other.translate_x = 0.0;
        props.merge(&other);

        assert!(
            (props.opacity - 0.5).abs() < 1e-6
                && (props.scale_x - 2.0).abs() < 1e-6
                && (props.translate_x - 10.0).abs() < 1e-6,
            "1 is the identity for a product and 0 for a sum, so skipping a neutral value and \
             applying it are the same answer — the guard here is an optimisation, not a rule"
        );
    }

    #[test]
    fn a_later_bucket_can_take_blur_back_to_zero() {
        let mut props = bucket();
        props.blur = 5.0;

        let mut other = bucket();
        other.blur = 0.0;
        props.merge(&other);

        assert_eq!(
            props.blur, 0.0,
            "blur is last-wins, not a product, so zero is a value and not an absence — guarding \
             on `> 0.001` left an element blurred for the rest of the scene"
        );
    }

    #[test]
    fn a_bucket_that_never_touched_blur_leaves_an_earlier_one_alone() {
        let mut props = bucket();
        props.blur = 5.0;
        props.blur_x = 3.0;
        props.blur_y = 2.0;

        props.merge(&bucket());

        assert_eq!(
            (props.blur, props.blur_x, props.blur_y),
            (5.0, 3.0, 2.0),
            "the resting value is negative precisely so that `not animated` and `animated to \
             zero` are two different things"
        );
    }
}
