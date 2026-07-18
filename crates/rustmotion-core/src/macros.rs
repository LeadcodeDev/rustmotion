/// Macro to implement common traits by delegating to embedded config fields.
///
/// Usage:
/// ```ignore
/// impl_traits!(Text {
///     Animatable => animation,  // Vec<AnimationEffect> directly
///     Timed => timing,
///     Styled => style,          // CssStyle
/// });
/// ```
///
/// This generates both the object-safe accessor trait AND the mutable
/// builder trait (e.g. `Styled` + `StyledMut`).
#[macro_export]
macro_rules! impl_traits {
    ($type:ty { $($trait_name:ident => $field:ident),* $(,)? }) => {
        $(
            $crate::impl_traits!(@single $type, $trait_name, $field);
        )*
    };

    (@single $type:ty, Animatable, $_field:ident) => {
        impl $crate::traits::Animatable for $type {
            fn animation_effects(&self) -> &[$crate::schema::AnimationEffect] {
                &self.style.animation
            }

            fn timeline_steps(&self) -> &[$crate::schema::TimelineStep] {
                &self.timeline
            }
        }
    };

    (@single $type:ty, Timed, $field:ident) => {
        impl $crate::traits::Timed for $type {
            fn timing(&self) -> (Option<f64>, Option<f64>) {
                (self.$field.start_at, self.$field.end_at)
            }
        }
    };

    (@single $type:ty, Styled, $field:ident) => {
        impl $crate::traits::Styled for $type {
            fn style_config(&self) -> &$crate::css::CssStyle {
                &self.$field
            }
        }

        impl $crate::traits::StyledMut for $type {
            fn style_config_mut(&mut self) -> &mut $crate::css::CssStyle {
                &mut self.$field
            }
        }
    };
}
