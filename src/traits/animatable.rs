use crate::schema::AnimationStyle;

/// Trait for components that support animation.
pub trait Animatable {
    fn animation_style(&self) -> Option<&AnimationStyle>;
}
