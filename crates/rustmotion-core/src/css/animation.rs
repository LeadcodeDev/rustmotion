use crate::css::style::CssStyle;
use crate::css::style::{BorderRadius, ClipPath, Edges, FilterFn, Gap, Size, TransformFn};
use crate::css::units::{Length, LengthPercentage};
use crate::engine::animator::AnimatedProperties;

pub fn apply_animated_props(css: &mut CssStyle, props: &AnimatedProperties) {
    let mut tx: Vec<TransformFn> = Vec::new();
    if props.translate_x != 0.0 || props.translate_y != 0.0 {
        tx.push(TransformFn::Translate {
            x: LengthPercentage::Px(props.translate_x),
            y: LengthPercentage::Px(props.translate_y),
        });
    }
    let sx = props.scale_x;
    let sy = props.scale_y;
    if (sx - 1.0).abs() > 1e-4 || (sy - 1.0).abs() > 1e-4 {
        tx.push(TransformFn::Scale { x: sx, y: sy });
    }
    if props.rotation.abs() > 1e-3 {
        tx.push(TransformFn::Rotate {
            deg: props.rotation,
        });
    }
    if props.rotate_x.abs() > 1e-3 {
        tx.push(TransformFn::RotateX {
            deg: props.rotate_x,
        });
    }
    if props.rotate_y.abs() > 1e-3 {
        tx.push(TransformFn::RotateY {
            deg: props.rotate_y,
        });
    }
    if !tx.is_empty() {
        match css.transform.as_mut() {
            Some(existing) => existing.extend(tx),
            None => css.transform = Some(tx),
        }
    }

    if (props.opacity - 1.0).abs() > 1e-4 {
        css.opacity = Some(props.opacity);
    }

    let mut filters: Vec<FilterFn> = Vec::new();
    if props.blur > 0.0 {
        filters.push(FilterFn::Blur {
            radius: Some(Length::Px(props.blur)),
            radius_x: None,
            radius_y: None,
        });
    }
    if props.glow_radius > 0.0 && props.glow_intensity > 0.0 {
        filters.push(FilterFn::DropShadow {
            offset_x: Length::Px(0.0),
            offset_y: Length::Px(0.0),
            blur: Some(Length::Px(props.glow_radius)),
            color: None,
        });
    }
    if !filters.is_empty() {
        match css.filter.as_mut() {
            Some(existing) => existing.extend(filters),
            None => css.filter = Some(filters),
        }
    }

    if props.perspective > 0.0 {
        css.perspective = Some(Length::Px(props.perspective));
    }

    if props.width >= 0.0 {
        css.width = Some(Size::Length(LengthPercentage::Px(props.width)));
    }
    if props.height >= 0.0 {
        css.height = Some(Size::Length(LengthPercentage::Px(props.height)));
    }

    if props.border_radius >= 0.0 {
        css.border_radius = Some(BorderRadius::Uniform(LengthPercentage::Px(
            props.border_radius,
        )));
    }

    if props.clip_path_progress >= 0.0 {
        if let Some(ClipPath::Morph { progress, .. }) = css.clip_path.as_mut() {
            *progress = props.clip_path_progress;
        }
    }

    if props.font_size >= 0.0 {
        css.font_size = Some(Length::Px(props.font_size));
    }

    if props.letter_spacing.is_finite() {
        css.letter_spacing = Some(Length::Px(props.letter_spacing));
    }

    if props.gap >= 0.0 {
        css.gap = Some(Gap::Uniform(LengthPercentage::Px(props.gap)));
    }

    if props.padding >= 0.0 {
        css.padding = Some(Edges::Uniform(LengthPercentage::Px(props.padding)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translate_props_become_transform_translate() {
        let mut css = CssStyle::default();
        let props = AnimatedProperties {
            translate_x: 10.0,
            translate_y: -5.0,
            ..AnimatedProperties::default()
        };
        apply_animated_props(&mut css, &props);

        let tx = css.transform.expect("transform list created");
        assert_eq!(tx.len(), 1);
        match &tx[0] {
            TransformFn::Translate { x, y } => {
                assert!(matches!(x, LengthPercentage::Px(v) if (*v - 10.0).abs() < 1e-6));
                assert!(matches!(y, LengthPercentage::Px(v) if (*v + 5.0).abs() < 1e-6));
            }
            other => panic!("expected Translate, got {:?}", other),
        }
    }

    #[test]
    fn an_active_opacity_animation_replaces_the_declared_static_opacity() {
        let mut css = CssStyle {
            opacity: Some(0.5),
            ..CssStyle::default()
        };
        let props = AnimatedProperties {
            opacity: 0.5,
            ..AnimatedProperties::default()
        };
        apply_animated_props(&mut css, &props);
        assert!(
            (css.opacity.unwrap() - 0.5).abs() < 1e-6,
            "an animation touching opacity must overwrite the static declaration outright, the \
             same last-value-written rule every other animated property in this function \
             follows — not multiply against it, got {:?}",
            css.opacity
        );
    }

    #[test]
    fn a_static_opacity_of_zero_is_overridden_by_an_active_fade_in_animation() {
        let mut css = CssStyle {
            opacity: Some(0.0),
            ..CssStyle::default()
        };
        let props = AnimatedProperties {
            opacity: 0.8,
            ..AnimatedProperties::default()
        };
        apply_animated_props(&mut css, &props);
        assert!(
            (css.opacity.unwrap() - 0.8).abs() < 1e-6,
            "a declared opacity: 0 authored as the fade-in's own starting point must not cancel \
             the animation for the rest of its run (issue #430); got {:?}",
            css.opacity
        );
    }

    #[test]
    fn no_op_props_leave_css_untouched() {
        let mut css = CssStyle::default();
        let props = AnimatedProperties::default();
        apply_animated_props(&mut css, &props);
        assert!(css.transform.is_none());
        assert!(css.opacity.is_none());
        assert!(css.filter.is_none());
        assert!(css.perspective.is_none());
    }

    #[test]
    fn motion_path_shaped_translate_and_rotation_compose_into_one_transform_list() {
        let mut css = CssStyle::default();
        let props = AnimatedProperties {
            translate_x: 120.0,
            translate_y: -40.0,
            rotation: 33.5,
            ..AnimatedProperties::default()
        };
        apply_animated_props(&mut css, &props);

        let tx = css.transform.expect("transform list created");
        assert_eq!(tx.len(), 2, "expected translate + rotate, got {:?}", tx);
        match &tx[0] {
            TransformFn::Translate { x, y } => {
                assert!(matches!(x, LengthPercentage::Px(v) if (*v - 120.0).abs() < 1e-6));
                assert!(matches!(y, LengthPercentage::Px(v) if (*v + 40.0).abs() < 1e-6));
            }
            other => panic!("expected Translate first, got {:?}", other),
        }
        match &tx[1] {
            TransformFn::Rotate { deg } => assert!((*deg - 33.5).abs() < 1e-6),
            other => panic!("expected Rotate second, got {:?}", other),
        }
    }

    #[test]
    fn animated_border_radius_reaches_the_css_style() {
        let mut css = CssStyle::default();
        let props = AnimatedProperties {
            border_radius: 100.0,
            ..AnimatedProperties::default()
        };
        apply_animated_props(&mut css, &props);

        match css.border_radius {
            Some(BorderRadius::Uniform(LengthPercentage::Px(v))) => {
                assert!((v - 100.0).abs() < 1e-6)
            }
            other => panic!("expected a uniform 100px border-radius, got {other:?}"),
        }
    }

    #[test]
    fn animated_clip_path_progress_writes_onto_the_morph_variant() {
        let mut css = CssStyle {
            clip_path: Some(ClipPath::Morph {
                from: Box::new(ClipPath::Circle {
                    radius: LengthPercentage::Px(10.0),
                    origin: None,
                }),
                to: Box::new(ClipPath::Circle {
                    radius: LengthPercentage::Px(50.0),
                    origin: None,
                }),
                via: Vec::new(),
                progress: 0.0,
            }),
            ..CssStyle::default()
        };
        let props = AnimatedProperties {
            clip_path_progress: 0.75,
            ..AnimatedProperties::default()
        };
        apply_animated_props(&mut css, &props);

        match css.clip_path {
            Some(ClipPath::Morph { progress, .. }) => {
                assert!((progress - 0.75).abs() < 1e-6)
            }
            other => panic!("expected the morph's progress to be updated, got {other:?}"),
        }
    }

    #[test]
    fn no_clip_path_progress_leaves_a_non_morph_clip_path_untouched() {
        let mut css = CssStyle {
            clip_path: Some(ClipPath::Circle {
                radius: LengthPercentage::Px(10.0),
                origin: None,
            }),
            ..CssStyle::default()
        };
        let props = AnimatedProperties {
            clip_path_progress: 0.5,
            ..AnimatedProperties::default()
        };
        apply_animated_props(&mut css, &props);

        assert_eq!(
            css.clip_path,
            Some(ClipPath::Circle {
                radius: LengthPercentage::Px(10.0),
                origin: None,
            }),
            "a static (non-morph) clip-path must not be mutated by clip_path_progress"
        );
    }

    #[test]
    fn blur_and_glow_compose_into_filter_list() {
        let mut css = CssStyle::default();
        let props = AnimatedProperties {
            blur: 4.0,
            glow_radius: 8.0,
            glow_intensity: 1.0,
            ..AnimatedProperties::default()
        };
        apply_animated_props(&mut css, &props);

        let filters = css.filter.expect("filter list created");
        assert_eq!(filters.len(), 2);
        assert!(matches!(filters[0], FilterFn::Blur { .. }));
        assert!(matches!(filters[1], FilterFn::DropShadow { .. }));
    }

    #[test]
    fn an_animation_touching_only_font_size_reaches_the_css_style() {
        let mut css = CssStyle::default();
        let props = AnimatedProperties {
            font_size: 120.0,
            ..AnimatedProperties::default()
        };
        apply_animated_props(&mut css, &props);
        assert_eq!(css.font_size, Some(Length::Px(120.0)));
    }

    #[test]
    fn an_animation_touching_only_letter_spacing_reaches_the_css_style() {
        let mut css = CssStyle::default();
        let props = AnimatedProperties {
            letter_spacing: 6.0,
            ..AnimatedProperties::default()
        };
        apply_animated_props(&mut css, &props);
        assert_eq!(css.letter_spacing, Some(Length::Px(6.0)));
    }

    #[test]
    fn an_untouched_letter_spacing_nan_resting_value_leaves_css_untouched() {
        let mut css = CssStyle::default();
        let props = AnimatedProperties::default();
        apply_animated_props(&mut css, &props);
        assert!(
            css.letter_spacing.is_none(),
            "letter_spacing's resting value is NaN precisely so 'not animated' can be told apart \
             from 'animated to zero'; it must not be written as a css value"
        );
    }

    #[test]
    fn an_animation_touching_only_gap_reaches_the_css_style() {
        let mut css = CssStyle::default();
        let props = AnimatedProperties {
            gap: 24.0,
            ..AnimatedProperties::default()
        };
        apply_animated_props(&mut css, &props);
        assert_eq!(css.gap, Some(Gap::Uniform(LengthPercentage::Px(24.0))));
    }

    #[test]
    fn an_animation_touching_only_padding_reaches_the_css_style() {
        let mut css = CssStyle::default();
        let props = AnimatedProperties {
            padding: 32.0,
            ..AnimatedProperties::default()
        };
        apply_animated_props(&mut css, &props);
        assert_eq!(
            css.padding,
            Some(Edges::Uniform(LengthPercentage::Px(32.0)))
        );
    }
}
