//! Issue #167 lot E: `SpringConfig::duration`/`rest_threshold` and
//! `engine::animator::spring_rest_time` are meant to be used from outside
//! `rustmotion-core` (by `rustmotion`'s `info` command, in particular).
//! This is a black-box check that the public surface actually works end to
//! end — the white-box coverage (settle-time correctness against a
//! brute-force reference, over/under-damped edge cases, shape preservation)
//! lives in `crates/rustmotion-core/src/engine/animator.rs`'s
//! `spring_duration_tests` module, since it needs access to private solver
//! internals that this crate does not expose.

use rustmotion_core::engine::animator::{spring_rest_time, spring_value};
use rustmotion_core::schema::SpringConfig;

fn spring(damping: f64, stiffness: f64, mass: f64, duration: Option<f64>) -> SpringConfig {
    SpringConfig {
        damping,
        stiffness,
        mass,
        duration,
        rest_threshold: None,
    }
}

#[test]
fn spring_config_duration_and_rest_threshold_default_to_none() {
    let config = SpringConfig::default();
    assert!(config.duration.is_none());
    assert!(config.rest_threshold.is_none());
}

#[test]
fn spring_config_duration_round_trips_through_json() {
    let json = serde_json::json!({
        "damping": 8.0,
        "stiffness": 120.0,
        "mass": 1.0,
        "duration": 0.8,
        "rest_threshold": 0.01
    });
    let config: SpringConfig = serde_json::from_value(json).expect("valid SpringConfig");
    assert_eq!(config.duration, Some(0.8));
    assert_eq!(config.rest_threshold, Some(0.01));
}

#[test]
fn a_spring_without_duration_settles_on_its_own_schedule() {
    // damping=6, stiffness=120, mass=1: the same underdamped spring used
    // throughout the crate's internal tests (elastic_in / kf_anim_spring's
    // sibling). Its natural settle time is a couple seconds, not 0.8s.
    let config = spring(6.0, 120.0, 1.0, None);
    let natural_rest = spring_rest_time(&config);
    assert!(
        natural_rest > 1.0,
        "expected a natural settle time well past 0.8s for this lightly damped spring, got {natural_rest}"
    );
}

#[test]
fn pinning_duration_moves_the_settle_point_there() {
    let config = spring(6.0, 120.0, 1.0, Some(0.8));
    assert_eq!(spring_rest_time(&config), 0.8);

    let v = spring_value(0.8, &config);
    assert!(
        (v - 1.0).abs() <= 0.005,
        "expected the pinned spring to be at rest (within the default 0.5% threshold) at \
         t=duration, got value {v}"
    );
}

#[test]
fn two_different_pinned_durations_both_settle_exactly_where_asked() {
    for duration in [0.3, 0.8, 1.5, 3.0] {
        let config = spring(6.0, 120.0, 1.0, Some(duration));
        let v = spring_value(duration, &config);
        assert!(
            (v - 1.0).abs() <= 0.005,
            "duration={duration}: expected value at t=duration to be within threshold of rest, got {v}"
        );
    }
}
