use crate::schema::{
    Animation, AnimationEffect, AnimationPreset, CharAnimPreset, EasingType, GlowConfig, Keyframe,
    KeyframeValue, MotionPathConfig, OrbitConfig, PresetConfig, SpringConfig, TextAnimDirection,
    TextAnimGranularity, WiggleConfig,
};

/// Default starting blur sigma (px) for `char_blur_in` when
/// `CharAnimationTiming.blur` is not set. Tuned against rendered output at
/// 120px display type (see issue #118's render proof): low enough that
/// individual letterforms stay ghost-legible at the start of a unit's
/// reveal (this is a *reveal*, not a smoke effect), high enough that the
/// blur is unmistakable next to the settled, sharp frame.
pub const DEFAULT_CHAR_BLUR_SIGMA: f32 = 14.0;

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
    /// Starting blur sigma in px (`char_blur_in` only; 0 elsewhere).
    pub blur: f32,
    /// Travel direction for the presets whose motion is a translate.
    pub direction: TextAnimDirection,
    /// Multiplier on the preset's own travel distance (1.0 = as tuned).
    pub distance: f32,
    /// Scale each unit starts at, or `None` for no scaling.
    pub scale_from: Option<f32>,
    /// ±fraction of `stagger` each unit's start is nudged by (0 = even).
    pub jitter: f32,
    /// Seed for the deterministic jitter offsets.
    pub seed: u32,
    /// Colour each unit starts at before settling to the text's own.
    pub ink_from: Option<String>,
}

impl ResolvedCharAnimation {
    /// When unit `idx` starts, in seconds, including its jitter nudge.
    ///
    /// The nudge is a pure function of `(idx, seed)` — deliberately not an
    /// RNG. Frames are rendered out of order, in parallel, and sometimes in
    /// separate processes (`--frames a-b` segments), so anything stateful
    /// here would make a unit jump between neighbouring frames.
    ///
    /// It is also clamped so a unit never starts before the effect's own
    /// `delay`: a negative start would make the first units appear already
    /// half-animated on frame 0.
    pub fn unit_start(&self, idx: usize) -> f64 {
        let even = self.delay as f64 + idx as f64 * self.stagger as f64;
        if self.jitter.abs() < 1e-6 || self.stagger.abs() < 1e-6 {
            return even;
        }
        // Bit-mixing hash (splitmix64's finalizer) over the unit index and
        // seed → a well-distributed value in -1.0..1.0.
        let mut h = (idx as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (self.seed as u64);
        h ^= h >> 30;
        h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
        h ^= h >> 27;
        h = h.wrapping_mul(0x94D0_49BB_1331_11EB);
        h ^= h >> 31;
        let unit = (h >> 11) as f64 / (1u64 << 53) as f64; // 0.0..1.0
        let nudge = (unit * 2.0 - 1.0) * self.jitter as f64 * self.stagger as f64;
        (even + nudge).max(self.delay as f64)
    }
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
    /// Every `motion_path` effect, resolved by `apply_motion_paths` into
    /// `translate_x`/`translate_y` (and, when `orient` is set,
    /// `rotation`) — the same additive-into-`props` treatment `orbits`
    /// already gets, and for the same reason: multiple path effects on one
    /// node compose by simple vector addition, not last-wins.
    pub motion_paths: Vec<&'a MotionPathConfig>,
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
                        // `char_blur_in` used to be resolved separately, off
                        // `style.animation` inside `text.rs`'s painter, which
                        // meant it silently missed container-level stagger
                        // shifting and `timeline`-embedded copies. It goes
                        // through the same door as its five siblings now.
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
                AnimationEffect::MotionPath(config) => {
                    result.motion_paths.push(config);
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

/// Default `rest_threshold` (fraction of the 0→1 travel) used by
/// `spring_rest_time`/the `duration` remap in `spring_value` when a
/// `SpringConfig` does not set one explicitly. 0.5% is tight enough that
/// "at rest" reads as visually still, without demanding the numeric search
/// chase an asymptote that (for a critically- or over-damped spring) is
/// never reached exactly.
pub const DEFAULT_SPRING_REST_THRESHOLD: f64 = 0.005;

/// Hard cap, in seconds, on how far into the future `spring_settle_time`
/// searches for a rest point. A very lightly damped spring can take an
/// arbitrarily long time to decay under `rest_threshold` — in the limit
/// (`damping == 0`) it never does, oscillating forever at constant
/// amplitude — so the search needs a bound or it would not terminate. When
/// the cap is hit, the spring is reported as resting at the cap itself: a
/// defined, tested "has not settled by then" answer (see
/// `spring_duration_tests::undamped_spring_is_capped_not_infinite` and
/// `spring_duration_tests::very_lightly_damped_spring_is_also_capped_when_beyond_the_bound`)
/// rather than an unbounded loop.
pub const MAX_SPRING_SEARCH_SECONDS: f64 = 30.0;

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
///
/// `duration` (issue #167 lot E, `SpringConfig::duration`): when set, `t` is
/// linearly rescaled before it reaches the physics below — not the physical
/// parameters themselves — so that `spring_rest_time` on the *unscaled*
/// spring lands exactly on `duration`. The spring's shape (oscillation
/// count, overshoot amplitude) is entirely a function of
/// `damping`/`stiffness`/`mass`, so rescaling only the time axis preserves
/// it; see `spring_duration_tests::duration_remap_preserves_shape`. This
/// does *not* resize whatever keyframe segment `spring_value` is being
/// evaluated within — see the `duration` field's doc comment on
/// `SpringConfig` for why that is a separate, author-owned concern.
pub fn spring_value(t: f64, config: &SpringConfig) -> f64 {
    let damping = config.damping.max(0.0);
    let stiffness = config.stiffness.max(1e-6);
    let mass = config.mass.max(1e-6);

    match config.duration {
        Some(duration) if duration > 0.0 => {
            let threshold = spring_rest_threshold(config);
            let natural_rest = spring_settle_time(
                damping,
                stiffness,
                mass,
                threshold,
                MAX_SPRING_SEARCH_SECONDS,
            );
            if natural_rest < 1e-9 {
                // Degenerate: the spring starts at distance 1.0 from its
                // target, so in practice `natural_rest` is never this
                // small — fall back to unscaled rather than divide by ~0.
                spring_value_raw(t, damping, stiffness, mass)
            } else {
                let time_scale = natural_rest / duration;
                spring_value_raw(t * time_scale, damping, stiffness, mass)
            }
        }
        _ => spring_value_raw(t, damping, stiffness, mass),
    }
}

/// The physics solver itself, unscaled by any `duration` remap. Takes
/// already-floored parameters (see `spring_value`'s constat #6 doc comment)
/// so `spring_settle_time`'s search can call it directly without redoing
/// the floor on every sample.
fn spring_value_raw(t: f64, damping: f64, stiffness: f64, mass: f64) -> f64 {
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

/// Lower bound on the number of samples `spring_settle_time` takes across
/// `[0, max_t]` — enough to resolve slow (critically-/over-damped) decays
/// even when the natural oscillation period doesn't drive the sample count
/// up on its own.
const SPRING_SETTLE_MIN_SAMPLES: usize = 2_000;
/// Upper bound on samples, regardless of how short the oscillation period
/// is — keeps `spring_settle_time` (called on every `spring_value` sample
/// when `duration` is set) bounded-cost for very stiff/fast springs.
const SPRING_SETTLE_MAX_SAMPLES: usize = 20_000;
/// Target sample density within one oscillation period, chosen empirically
/// (see the workstream report) to keep the coarse-then-bisect search within
/// ~0.1% of a brute-force reference across a broad random sweep of
/// damping/stiffness/mass. Shallow, near-tangential graze-and-return
/// excursions across the threshold band (a spring that dips back below the
/// line by a razor-thin margin on a secondary oscillation) can still be
/// missed — `spring_rest_time`/`spring_settle_time` are a documented
/// numeric approximation, not an exact guarantee.
const SPRING_SETTLE_SAMPLES_PER_PERIOD: f64 = 48.0;

/// First `t >= 0` from which `spring_value_raw` stays within `threshold` of
/// its target (1.0) forever after. Implements the "mesure du repos" from
/// issue #167 lot E: `spring_value_raw` is closed-form, so a coarse scan to
/// bracket the last exceedance, refined by bisection, is enough — no need
/// to integrate anything.
///
/// Two regimes get explicit handling (both required by the workstream
/// brief, both exercised in `spring_duration_tests`):
/// - an overdamped (or critically damped) spring never touches its target
///   exactly, only approaches it asymptotically — the scan terminates via
///   `threshold`, never via an exact equality check;
/// - a very lightly damped spring can take arbitrarily long to settle (an
///   undamped spring, `damping == 0`, never does — it oscillates forever at
///   constant amplitude). `max_t` bounds the search; if the last sample is
///   still outside `threshold`, `max_t` itself is returned — defined,
///   tested behaviour instead of an unbounded search.
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
        // Still exceeding at (or past) max_t: capped, "not settled".
        return max_t;
    }

    // Refine within (last_exceed, last_exceed + dt]: the coarse scan found
    // this as the last sample outside the threshold band, so bisect for the
    // point within this bracket where it steps inside for good.
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

/// The `rest_threshold` a `SpringConfig` resolves to: the author's value if
/// set, else `DEFAULT_SPRING_REST_THRESHOLD`, floored so `spring_settle_time`
/// always has a well-defined (nonzero) target — the same belt-and-suspenders
/// pattern `spring_value` already applies to `damping`/`stiffness`/`mass`.
/// the CLI's `check_spring_config` rejects non-positive or absurd
/// (`>= 1.0`) values at the author-facing layer; this floor is the
/// solver-side backstop.
fn spring_rest_threshold(config: &SpringConfig) -> f64 {
    config
        .rest_threshold
        .unwrap_or(DEFAULT_SPRING_REST_THRESHOLD)
        .max(1e-9)
}

/// Public "measure du repos" (issue #167 lot E): the instant, in seconds,
/// at which this spring settles within `rest_threshold` of its target and
/// stays there — what `rustmotion info` surfaces so an author can size a
/// scene/preset duration around a spring instead of discovering it by
/// trial and error.
///
/// When `config.duration` is set, this *is* that duration, exactly — that
/// is the point of the time remap `spring_value` performs (see its doc
/// comment). Otherwise it is the natural settle time computed from
/// `damping`/`stiffness`/`mass` alone via `spring_settle_time`.
pub fn spring_rest_time(config: &SpringConfig) -> f64 {
    match config.duration {
        Some(d) if d > 0.0 => d,
        _ => {
            let damping = config.damping.max(0.0);
            let stiffness = config.stiffness.max(1e-6);
            let mass = config.mass.max(1e-6);
            let threshold = spring_rest_threshold(config);
            spring_settle_time(
                damping,
                stiffness,
                mass,
                threshold,
                MAX_SPRING_SEARCH_SECONDS,
            )
        }
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
    if !extracted.motion_paths.is_empty() {
        let motion_paths: Vec<_> = extracted.motion_paths.iter().copied().cloned().collect();
        apply_motion_paths(&mut props, &motion_paths, time);
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

/// Public wrapper around `resolve_animation_value_full` for callers outside
/// this module that want to reuse the exact segment/easing/spring
/// interpolation math (ordering, per-keyframe easing override, clamping at
/// the ends) on a synthetic `Animation` they built themselves, without
/// routing the result through `AnimatedProperties`/`apply_property`.
///
/// This is how `box_builder.rs`'s `style.transition` smoothing for
/// `border-radius`/`background` is implemented: those two properties are
/// paint-time `CssStyle` fields that every painter already reads directly
/// (via `paint_pass.rs`, frozen) — there is no `AnimatedProperties` field
/// for them to land in that anything downstream would ever look at, so
/// resolving through the generic effects pipeline the way `opacity`/`color`
/// do would be a dead end. Calling this directly and writing the resolved
/// `CssStyle` field by hand instead reuses the proven interpolation math
/// while staying entirely inside `box_builder.rs`'s own file scope.
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

// Note: an earlier workstream (constat #4, `schema/video.rs`) already closed
// the "unrecognized `Animation.property` is a silent no-op" gap this
// function's catch-all (`_ => {}` above) would otherwise hide —
// `KeyframesConfig.keyframes` deserializes through
// `deserialize_validated_keyframes`/`validate_motion_property`, which
// rejects any `property` outside `KNOWN_MOTION_PROPERTIES` (with a
// did-you-mean suggestion) at parse time, before a scenario ever reaches
// `validate`/render. This workstream verified that gap is closed rather
// than reopening it with a second, redundant "known properties" list here;
// see the workstream report's "generic interpolation" write-up.

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

// ─── Motion path ────────────────────────────────────────────────────────────

/// Below this measured path length (in px), a `motion_path` is treated as
/// the "zero length" degenerate case: the component holds at its single
/// point instead of travelling, and `orient` contributes no rotation (a
/// tangent is undefined at zero length). Not `0.0` exactly — `PathMeasure`
/// is a numeric approximation, and a path whose segments collapse onto one
/// point within float precision (e.g. two near-coincident cubic control
/// points) should degrade the same defined way a literal single-point path
/// does, rather than pass through as a very short, jittery "real" travel.
pub const MOTION_PATH_MIN_LENGTH: f32 = 1e-3;

/// Parse `path_data` and measure its length, in px — the shared primitive
/// `apply_motion_paths` (render time) and `validate_schema.rs`'s advisory
/// zero-length check (author time) both build on, so the two never
/// disagree about what "degenerate" means.
///
/// Returns `None` when `path_data` is empty or not valid SVG path data.
/// Every `motion_path` effect reachable through `AnimationEffect` already
/// has this ruled out at JSON-parse time
/// (`schema/video.rs::deserialize_motion_path_data`), so in practice `None`
/// only fires if a caller builds a `MotionPathConfig` directly in Rust,
/// bypassing that gate. `Some(0.0)` (or a value below
/// `MOTION_PATH_MIN_LENGTH`) is returned for a syntactically valid path
/// with (near-)zero measured length — a well-defined, distinct case from
/// "invalid", per `MotionPathConfig`'s "Degenerate paths" doc section.
pub fn motion_path_length(path_data: &str) -> Option<f32> {
    let path = skia_safe::Path::from_svg(path_data)?;
    if path.count_points() == 0 {
        return None;
    }
    let mut measure = skia_safe::PathMeasure::new(&path, false, None);
    Some(measure.length())
}

/// Progress along a `motion_path` effect's own timeline, already eased, in
/// `[0, 1]`. Mirrors the delay/duration semantics every other timed effect
/// in this file uses: before `delay`, progress is pinned to `0.0` (the path
/// hasn't started — the component sits at the path's start point, the same
/// "hold at the entrance state" every preset already does before its own
/// delay elapses); at/after `delay + duration` it is pinned to `1.0` (holds
/// at the path's end) unless `repeat` wraps it back into `[0, 1)` instead.
///
/// `safe_div`'s fallback (`1.0`) makes a non-positive `duration` behave as
/// "already complete the instant `delay` elapses" — finite and defined,
/// never a NaN/∞ division — the same belt-and-suspenders posture
/// `spring_value` already takes on its own denominators.
/// `validate_schema.rs::check_motion_path_config` additionally rejects
/// `duration <= 0` as an author-facing error, so this fallback is a second
/// line of defence, not the only one.
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

/// One `motion_path` effect's contribution at `time`: a translate delta (in
/// the component-local coordinate space `MotionPathConfig` documents) and a
/// tangent-derived rotation in degrees (`0.0` when `orient` is unset, or
/// when the path is the zero-length degenerate case).
struct MotionPathSample {
    dx: f32,
    dy: f32,
    angle_deg: f32,
}

/// Sample a `motion_path` effect at `time`. Never returns a NaN/infinite
/// component, for any input — the three degenerate cases the workstream
/// brief names are each handled explicitly rather than falling through to
/// whatever the underlying float operation happens to produce:
///
/// - **empty/unparsable path**: `AnimationEffect::MotionPath` cannot carry
///   one past `schema/video.rs::deserialize_motion_path_data`'s parse-time
///   rejection, but this function stays defensive anyway (`(0.0, 0.0,
///   0.0)`, i.e. no displacement) rather than assuming that gate always ran
///   — e.g. a future direct `MotionPathConfig` construction in Rust code
///   would bypass serde entirely.
/// - **single point** (`"M50,50"`) and **zero-length** (every segment
///   collapses onto one point, e.g. `"M10,10 L10,10"`): both measure to
///   (near-)zero length. Position holds at that single point (read via
///   `Path::get_point(0)`) for the entire timeline; orientation is `0.0`
///   regardless of `orient` — a tangent is undefined at zero length, so
///   `atan2(0.0, 0.0)`'s technically-zero-but-meaningless result is never
///   computed or relied on.
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

    // Verified empirically (not just assumed): `PathMeasure::pos_tan` on a
    // zero-length contour returns `None` in this skia-safe build, which the
    // `None` arm below would also catch — this early return is kept anyway
    // as the one place the degenerate case is *named*, rather than an
    // undocumented cross-version PathMeasure behaviour a reader would have
    // to intuit, and it skips constructing/querying the measure entirely
    // for the single most common degenerate input (a single-point path).
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
        // `0 <= distance <= length` on a >0-length path should always
        // report a position; if Skia ever declines anyway, hold at the
        // path's start rather than let a missing sample surface as a jump
        // to the component's untranslated origin or a NaN.
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

/// Apply every `motion_path` effect additively to `props.translate_x`/
/// `translate_y` (and, when `orient` is set, `props.rotation`) — the same
/// treatment `apply_orbits`/`apply_wiggles` already give their own
/// continuous effects, and critically, fields `css::animation::
/// apply_animated_props` already bridges into `css.transform`'s
/// `translate`/`rotate` functions. That bridge — not a new one — is what
/// makes a `motion_path` excursion past the viewport visible to
/// `--strict-anim` (`rustmotion::cli::commands::geometry::
/// apply_static_node_transform`, which folds `css.transform` to detect
/// overflow): this function must never write position/orientation anywhere
/// else, or that detection silently stops seeing it.
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
///
/// `spring.duration` (issue #167 lot E) is *not* consulted here to resize
/// the keyframe pair's own span: the pair's `[delay, end]` still comes from
/// `AnimationTiming::delay`/`duration` (the same preset-level timing every
/// other easing uses), untouched by whatever `SpringConfig::duration` says.
/// `spring_value` — not this function — is where `duration` acts, by
/// rescaling the *physics* time axis it is fed. Consequently, if the
/// preset's own `duration` is shorter than `spring.duration`, the segment
/// still ends (and the property still snaps to its final keyframe value) at
/// the preset's `end`, before the spring has visually settled — exactly the
/// pre-existing behaviour for any other easing curve given too short a
/// segment. Pin `AnimationTiming::duration` (or the `keyframes` effect's own
/// keyframe span, for the other call site in `resolve_animation_value_full`)
/// to at least `spring_rest_time` to avoid that cutoff; `rustmotion info`
/// reports `spring_rest_time` for exactly this purpose.
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
        AnimationPreset::PopIn => {
            // Two beats, not one: the back-out scale places the element, then
            // a short pulse draws the eye back to it. Collapsing them into a
            // single overshooting curve reads as one wobble instead — the
            // second beat has to land *after* the element has visibly settled.
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
            ..Default::default()
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
    //! Issue #167 lot E: `SpringConfig::duration` forces a spring to settle
    //! (see `rest_threshold`) at exactly that many seconds by rescaling the
    //! time axis fed to the physics solver; `spring_rest_time` is the
    //! public "measure du repos" `rustmotion info` surfaces.
    use super::*;

    /// Reference settle time via a fine linear scan of the actual
    /// implemented formula (`spring_value_raw`), independent of
    /// `spring_settle_time`'s coarse-then-bisect implementation — these
    /// tests check the algorithm against ground truth, not against itself.
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
        // Captured red-phase numbers (issue #167 lot E, before `duration`
        // existed on `SpringConfig`): a spring's settle time was purely
        // emergent from damping/stiffness/mass. `spring_value_raw` is
        // exactly that pre-existing, unscaled solver — by construction it
        // does not know about `duration`.
        //
        // damping=6, stiffness=120, mass=1 (the same "underdamped" preset
        // this file already uses for elastic_in / kf_anim_spring_underdamped)
        // at t=0.8s: spring_value_raw(0.8, 6, 120, 1) ~= 1.043467 — 4.35%
        // past the target, an order of magnitude outside any reasonable
        // rest_threshold (default 0.5%). An author asking this spring to
        // "finish at 0.8s" got a value nowhere near rest.
        let v = spring_value_raw(0.8, 6.0, 120.0, 1.0);
        assert!(
            (v - 1.043467).abs() < 1e-5,
            "captured red-phase reference value drifted: got {v}, expected ~1.043467"
        );
        assert!(
            (v - 1.0).abs() > 0.04,
            "red-phase claim: at t=duration the unscaled spring must still be far from rest \
             (got diff {:.6}, expected > 0.04)",
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

        // Green phase: the same (damping, stiffness, mass) that the
        // red-phase test above showed is 4.35% off at t=0.8s without a
        // `duration` must now be within `threshold` of rest at t=0.8s.
        let v_at_duration = spring_value(0.8, &config);
        assert!(
            (v_at_duration - 1.0).abs() <= threshold,
            "spring_value(0.8, ..) with duration=Some(0.8) must be within {threshold} of rest, \
             got {v_at_duration} (diff {})",
            (v_at_duration - 1.0).abs()
        );

        // And it must not already be at rest well before `duration` —
        // this is a genuine rescale, not "duration happens to be late
        // enough not to matter".
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
        // Pitfall called out in the brief: an overdamped spring approaches
        // its target asymptotically and never touches it. The search must
        // terminate via `rest_threshold`, not by looking for an exact hit.
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

        // Confirm it genuinely never hits exactly 1.0 — the asymptotic
        // property `rest_threshold` exists to work around.
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
        // Pitfall: damping=0 means the spring oscillates forever at
        // constant amplitude — it never settles. The search must return the
        // defined cap (`MAX_SPRING_SEARCH_SECONDS`), not loop forever.
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
        // Not literally undamped, but damped so lightly it does not reach a
        // 0.5% rest threshold within the search bound — same defined-cap
        // behaviour as the fully undamped case, exercised with nonzero
        // damping so the `zeta == 0` special case isn't the only path
        // that's actually bounded.
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
        // The whole point of a spring's `duration` is to keep its shape —
        // oscillation count, overshoot amplitude — and only rescale how
        // fast it plays back. Compare the natural (no-duration) curve to a
        // duration-remapped curve of the *same* underlying spring, sampled
        // at matching fractions of each one's own settle time: if the remap
        // were instead clipping the tail (shortening, not rescaling), these
        // would diverge.
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

        let pinned_duration = 2.5; // deliberately different from natural_rest
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

            // Same fraction of each spring's own settle time must produce
            // the same progress value — that is the shape being preserved,
            // only the clock speed differs.
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
        // `spring_value`'s `t` argument is already local to the enclosing
        // segment (time since the segment/keyframe start — `delay` is
        // baked into where that segment begins, upstream of this call).
        // `duration` must not reinterpret that: t=0 must still be the
        // spring's own start regardless of `duration`.
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

    // ─── The decisive property: on the curve, not the chord ──────────────

    /// A bent two-segment polyline ("M0,0 L100,0 L100,100", total length
    /// 200) makes this trivial to prove without any bezier arithmetic:
    /// halfway along the *path* (distance 100) lands exactly on the corner
    /// (100, 0). The *chord* between the two endpoints (0,0)→(100,100) has
    /// its own midpoint at (50, 50) — a linear interpolation between
    /// endpoints (what a buggy "lerp the bounding box" implementation would
    /// produce) would land there instead. Asserting the real result is far
    /// from (50, 50) and exactly at (100, 0) is what distinguishes "walks
    /// the path" from "interpolates the endpoints" — checking only t=0/t=1
    /// bounds would pass either implementation.
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

    // ─── Endpoints, as a sanity boundary (not the decisive test on its own) ──

    #[test]
    fn progress_zero_and_one_land_on_the_paths_own_endpoints() {
        let c = cfg("M10,20 L310,20 L310,220");
        let start = motion_path_sample(&c, 0.0);
        assert!((start.dx - 10.0).abs() < 0.5 && (start.dy - 20.0).abs() < 0.5);

        let end = motion_path_sample(&c, 1.0);
        assert!((end.dx - 310.0).abs() < 0.5 && (end.dy - 220.0).abs() < 0.5);
    }

    // ─── Coordinate space: deltas relative to the laid-out position ──────

    /// The path's own coordinates are used literally as the translate delta
    /// — not normalized so the path's first point becomes (0,0). A path
    /// that starts away from the origin therefore starts the component
    /// already displaced by that much, on top of wherever layout placed it.
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

    // ─── The channel: translate_x/translate_y/rotation, additive ─────────

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

        // Path start is (0,0), so translate ends up unchanged from the
        // pre-existing (5, -5) contribution — proves this is additive, not
        // an overwrite.
        assert!((props.translate_x - 5.0).abs() < 0.5);
        assert!((props.translate_y - (-5.0)).abs() < 0.5);
        // Horizontal rightward tangent ⇒ 0 degrees.
        assert!(props.rotation.abs() < 0.5, "got {}", props.rotation);
    }

    #[test]
    fn orient_false_never_touches_rotation() {
        // A vertical segment has a 90°-ish tangent; if `orient` leaked
        // through despite being false, rotation would move off 0.
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
        // Downward tangent (Skia is Y-down): atan2(1, 0) = 90°.
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

    // ─── Degenerate cases: defined, finite, never NaN ─────────────────────

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
        // Bypasses `deserialize_motion_path_data`'s parse-time rejection on
        // purpose (constructed directly in Rust) — the runtime sampler must
        // still be safe on its own, defence in depth.
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

    // ─── Loop semantics ────────────────────────────────────────────────────

    #[test]
    fn looping_wraps_progress_back_toward_the_start() {
        let mut c = cfg("M0,0 L100,0 L100,100");
        c.repeat = true;
        c.duration = 1.0;
        // 1.5s in, with a 1s loop period, is equivalent to t=0.5 within the
        // loop — same corner-of-the-L assertion as the non-looping test.
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

    // ─── Determinism: same time in, same result out ───────────────────────

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
            // f32 fields widened to f64 — compare at f32 precision.
            assert!(
                (a.unit_start(i) - expected).abs() < 1e-6,
                "unit {i} should start at {expected}, got {}",
                a.unit_start(i)
            );
        }
    }

    #[test]
    fn jitter_is_a_pure_function_of_index_and_seed() {
        // Frames are rendered out of order, in parallel, and across separate
        // processes (`--frames a-b` segments). If the nudge came from an RNG,
        // a word would land at a different time in each of those, i.e. jump
        // between neighbouring frames of the same video.
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
        // A negative nudge on the first unit would have it appear already
        // half-animated on frame 0 — the one artefact the clamp exists for.
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
        // Nothing to spread out: every unit shares one start time, and
        // `jitter` scales off `stagger`, so it has nothing to scale.
        let a = anim(0.0, 1.0, 3);
        for i in 0..8 {
            assert!((a.unit_start(i) - 0.5).abs() < 1e-9);
        }
    }
}
