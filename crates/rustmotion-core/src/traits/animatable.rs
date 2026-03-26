use crate::schema::AnimationEffect;

/// Trait for components that support animation.
pub trait Animatable {
    fn animation_effects(&self) -> &[AnimationEffect];
}
