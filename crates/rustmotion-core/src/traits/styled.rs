use crate::css::CssStyle;

pub trait Styled {
    fn style_config(&self) -> &CssStyle;
}

pub trait StyledMut: Styled {
    fn style_config_mut(&mut self) -> &mut CssStyle;
}
