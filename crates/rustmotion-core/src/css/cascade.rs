//! CSS cascade & inheritance.
//!
//! Walks the BoxTree and propagates inheritable properties from parent to
//! child when the child has not specified them. Properties not in the
//! inheritable list are left untouched (initial values applied at paint time).
//!
//! Inheritable properties (per CSS spec, restricted to our scope):
//! - `color`
//! - `font-family`
//! - `font-size`
//! - `font-weight`
//! - `font-style`
//! - `line-height`
//! - `letter-spacing`
//! - `text-align`
//! - `white-space`
//! - `overflow-wrap`
//! - `visibility`
//! - `text-decoration` (not strictly inheritable but used as such here)
//!
//! All other properties (background, padding, border, transform, etc.) are
//! NOT inherited.

use super::style::CssStyle;

/// Inherit unset child properties from the parent. `parent` is the already
/// resolved style (post-cascade) of the parent node.
pub fn inherit_from(parent: &CssStyle, child: &mut CssStyle) {
    if child.color.is_none() {
        child.color = parent.color.clone();
    }
    if child.font_family.is_none() {
        child.font_family = parent.font_family.clone();
    }
    if child.font_size.is_none() {
        child.font_size = parent.font_size.clone();
    }
    if child.font_weight.is_none() {
        child.font_weight = parent.font_weight.clone();
    }
    if child.font_style.is_none() {
        child.font_style = parent.font_style;
    }
    if child.line_height.is_none() {
        child.line_height = parent.line_height.clone();
    }
    if child.letter_spacing.is_none() {
        child.letter_spacing = parent.letter_spacing.clone();
    }
    if child.text_align.is_none() {
        child.text_align = parent.text_align;
    }
    if child.white_space.is_none() {
        child.white_space = parent.white_space;
    }
    if child.overflow_wrap.is_none() {
        child.overflow_wrap = parent.overflow_wrap;
    }
    if child.visibility.is_none() {
        child.visibility = parent.visibility;
    }
    if child.text_decoration.is_none() {
        child.text_decoration = parent.text_decoration.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::style::{Color, Display, FontStyle, TextAlign};

    #[test]
    fn child_inherits_color_when_unset() {
        let parent = CssStyle {
            color: Some(Color::String("#ff0000".into())),
            ..Default::default()
        };
        let mut child = CssStyle::default();
        inherit_from(&parent, &mut child);
        assert!(matches!(child.color, Some(Color::String(_))));
    }

    #[test]
    fn child_overrides_parent_color() {
        let parent = CssStyle {
            color: Some(Color::String("#ff0000".into())),
            ..Default::default()
        };
        let mut child = CssStyle {
            color: Some(Color::String("#00ff00".into())),
            ..Default::default()
        };
        inherit_from(&parent, &mut child);
        match child.color {
            Some(Color::String(s)) => assert_eq!(s, "#00ff00"),
            _ => panic!("expected child color preserved"),
        }
    }

    #[test]
    fn non_inheritable_not_propagated() {
        let parent = CssStyle {
            display: Some(Display::Flex),
            ..Default::default()
        };
        let mut child = CssStyle::default();
        inherit_from(&parent, &mut child);
        // display is not inherited
        assert!(child.display.is_none());
    }

    #[test]
    fn font_props_inherited() {
        let parent = CssStyle {
            font_family: Some("Arial".into()),
            font_style: Some(FontStyle::Italic),
            text_align: Some(TextAlign::Center),
            ..Default::default()
        };
        let mut child = CssStyle::default();
        inherit_from(&parent, &mut child);
        assert_eq!(child.font_family.as_deref(), Some("Arial"));
        assert_eq!(child.font_style, Some(FontStyle::Italic));
        assert_eq!(child.text_align, Some(TextAlign::Center));
    }
}
