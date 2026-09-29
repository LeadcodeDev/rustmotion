use super::style::CssStyle;

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

pub fn overlay_resolved_typography(resolved: &CssStyle, own: &mut CssStyle) {
    if resolved.color.is_some() {
        own.color = resolved.color.clone();
    }
    if resolved.font_family.is_some() {
        own.font_family = resolved.font_family.clone();
    }
    if resolved.font_size.is_some() {
        own.font_size = resolved.font_size.clone();
    }
    if resolved.font_weight.is_some() {
        own.font_weight = resolved.font_weight.clone();
    }
    if resolved.font_style.is_some() {
        own.font_style = resolved.font_style;
    }
    if resolved.line_height.is_some() {
        own.line_height = resolved.line_height.clone();
    }
    if resolved.letter_spacing.is_some() {
        own.letter_spacing = resolved.letter_spacing.clone();
    }
    if resolved.text_align.is_some() {
        own.text_align = resolved.text_align;
    }
    if resolved.white_space.is_some() {
        own.white_space = resolved.white_space;
    }
    if resolved.overflow_wrap.is_some() {
        own.overflow_wrap = resolved.overflow_wrap;
    }
    if resolved.visibility.is_some() {
        own.visibility = resolved.visibility;
    }
    if resolved.text_decoration.is_some() {
        own.text_decoration = resolved.text_decoration.clone();
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

    #[test]
    fn overlay_resolved_typography_replaces_a_component_own_static_font_size() {
        use crate::css::units::Length;

        let resolved = CssStyle {
            font_size: Some(Length::Px(120.0)),
            ..Default::default()
        };
        let mut own = CssStyle {
            font_size: Some(Length::Px(20.0)),
            ..Default::default()
        };
        overlay_resolved_typography(&resolved, &mut own);
        assert_eq!(
            own.font_size,
            Some(Length::Px(120.0)),
            "the box's own already-animated resolved font-size must win over the component's \
             declared static one, unlike inherit_from's fill-only-if-none rule"
        );
    }

    #[test]
    fn overlay_resolved_typography_replaces_a_component_own_static_letter_spacing() {
        use crate::css::units::Length;

        let resolved = CssStyle {
            letter_spacing: Some(Length::Px(18.0)),
            ..Default::default()
        };
        let mut own = CssStyle {
            letter_spacing: Some(Length::Px(0.0)),
            ..Default::default()
        };
        overlay_resolved_typography(&resolved, &mut own);
        assert_eq!(own.letter_spacing, Some(Length::Px(18.0)));
    }

    #[test]
    fn overlay_resolved_typography_leaves_a_field_alone_when_resolved_lacks_it() {
        let resolved = CssStyle::default();
        let mut own = CssStyle {
            color: Some(Color::String("#00ff00".into())),
            ..Default::default()
        };
        overlay_resolved_typography(&resolved, &mut own);
        match own.color {
            Some(Color::String(s)) => assert_eq!(s, "#00ff00"),
            other => panic!("resolved had no color; own's must be left alone, got {other:?}"),
        }
    }
}
