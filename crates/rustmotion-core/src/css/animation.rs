//! Bridge from the legacy `AnimatedProperties` (Flutter-style) to CSS-style
//! overrides on `CssStyle`. Used by the new pipeline so animations resolved
//! through `animator::resolve_props_for_effects` flow into `transform`,
//! `opacity`, `filter`, etc. before layout/paint.
//!
//! This is a transitional module: once all animation surfaces are CSS-native,
//! the animator can produce `CssStyle` overrides directly and this bridge
//! disappears.

use crate::css::style::CssStyle;
use crate::css::style::{FilterFn, TransformFn};
use crate::css::units::{Length, LengthPercentage};
use crate::engine::animator::AnimatedProperties;

/// Apply the resolved animation properties as a partial CSS override on top
/// of an existing `CssStyle`. Only properties that the animator actually
/// touched (vs. their `Default` sentinels) are written.
///
/// Order matters: the resulting `transform` list mirrors the legacy paint
/// order (translate → scale → rotate → 3D rotate). Matching `transform-origin`
/// is the box centre, which the paint pass already defaults to.
pub fn apply_animated_props(css: &mut CssStyle, props: &AnimatedProperties) {
    // ---- transform list ----
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
        // Append rather than replace so a CSS-defined transform composes with
        // the animation-derived one (CSS transforms are post-multiplied).
        match css.transform.as_mut() {
            Some(existing) => existing.extend(tx),
            None => css.transform = Some(tx),
        }
    }

    // ---- opacity ----
    // Only write if it differs from the default (1.0). The animator sets
    // `opacity = 1.0` as default, so any other value is meaningful.
    if (props.opacity - 1.0).abs() > 1e-4 {
        let base = css.opacity.unwrap_or(1.0);
        css.opacity = Some(base * props.opacity);
    }

    // ---- filter (blur + glow) ----
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

    // ---- perspective ----
    if props.perspective > 0.0 {
        css.perspective = Some(Length::Px(props.perspective));
    }

    // Note: `border_radius`, `font_size`, `width`, `height`, `gap`, `padding`,
    // `stroke_width`, `shadow_blur`, `draw_progress`, `motion_progress`,
    // `visible_chars*`, `char_animation`, `color` are NOT translated to CSS
    // here. Those are component-internal animations and remain accessible
    // through the legacy props on the dispatcher path. They will move into
    // CSS once each component's painter is migrated.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translate_props_become_transform_translate() {
        let mut css = CssStyle::default();
        let mut props = AnimatedProperties::default();
        props.translate_x = 10.0;
        props.translate_y = -5.0;
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
        let mut props = AnimatedProperties::default();
        props.opacity = 0.5;
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
    fn blur_and_glow_compose_into_filter_list() {
        let mut css = CssStyle::default();
        let mut props = AnimatedProperties::default();
        props.blur = 4.0;
        props.glow_radius = 8.0;
        props.glow_intensity = 1.0;
        apply_animated_props(&mut css, &props);

        let filters = css.filter.expect("filter list created");
        assert_eq!(filters.len(), 2);
        assert!(matches!(filters[0], FilterFn::Blur { .. }));
        assert!(matches!(filters[1], FilterFn::DropShadow { .. }));
    }
}
