use crate::css::style::CssStyle;
use crate::css::style::{FilterFn, Size, TransformFn};
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
        let base = css.opacity.unwrap_or(1.0);
        css.opacity = Some(base * props.opacity);
    }

    let mut filters: Vec<FilterFn> = Vec::new();
    if props.blur > 0.0 {
        filters.push(FilterFn::Blur {
            radius: Length::Px(props.blur),
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
    fn opacity_is_multiplied_with_existing_css_opacity() {
        let mut css = CssStyle {
            opacity: Some(0.5),
            ..CssStyle::default()
        };
        let props = AnimatedProperties {
            opacity: 0.5,
            ..AnimatedProperties::default()
        };
        apply_animated_props(&mut css, &props);
        assert!((css.opacity.unwrap() - 0.25).abs() < 1e-6);
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
}
