use crate::css::CssStyle;

/// Object-safe trait for components that carry a `CssStyle`.
/// Used by the box-tree builder to read styling.
pub trait Styled {
    fn style_config(&self) -> &CssStyle;
}

/// Mutable access to style config.
pub trait StyledMut: Styled {
    fn style_config_mut(&mut self) -> &mut CssStyle;
}
