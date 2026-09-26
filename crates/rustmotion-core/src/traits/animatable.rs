use crate::schema::{AnimationEffect, TimelineStep};

pub trait Animatable {
    fn animation_effects(&self) -> &[AnimationEffect];

    fn timeline_steps(&self) -> &[TimelineStep] {
        &[]
    }
}
