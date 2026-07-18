use crate::schema::{AnimationEffect, TimelineStep};

/// Trait for components that support animation.
pub trait Animatable {
    fn animation_effects(&self) -> &[AnimationEffect];

    /// Timed animation steps (`timeline` field): each step's animations are
    /// resolved with `delay += step.at`. Empty by default.
    fn timeline_steps(&self) -> &[TimelineStep] {
        &[]
    }
}
