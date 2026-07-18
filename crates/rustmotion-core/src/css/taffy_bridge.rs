//! `CssStyle` → `taffy::Style` converter.
//!
//! Inspired by `stylo_taffy` (Servo) but kept minimal: we only translate the
//! layout-affecting properties. Paint properties (color, background, transform,
//! filter, etc.) are read separately by the paint pass.
//!
//! Currently a stub — fleshed out in step 3 of the migration plan.

use taffy::prelude as tf;

use super::style::{
    AlignContent, AlignItems, AlignSelf, CssStyle, Display, Edges, FlexDirection, FlexWrap, Gap,
    JustifyContent, Overflow, Position, Size,
};
use super::units::{LengthContext, LengthPercentage, ParsedLength};

#[derive(Debug, Clone, Copy, Default)]
pub struct ConversionContext {
    pub length: LengthContext,
}

/// Convert a [`CssStyle`] into a [`taffy::Style`]. Properties not relevant to
/// layout are ignored. Unsupported / unset properties fall back to taffy
/// defaults (which match CSS initial values).
pub fn to_taffy_style(css: &CssStyle, ctx: &ConversionContext) -> tf::Style {
    let mut style = tf::Style::DEFAULT;

    // Display
    style.display = match css.display {
        Some(Display::None) => tf::Display::None,
        Some(Display::Block) => tf::Display::Block,
        Some(Display::Flex) => tf::Display::Flex,
        Some(Display::Grid) => tf::Display::Grid,
        // inline-block / contents → fall back to block in our scope
        _ => tf::Display::Block,
    };

    // Position
    style.position = match css.position {
        Some(Position::Absolute) => tf::Position::Absolute,
        _ => tf::Position::Relative,
    };

    // Inset
    style.inset = tf::Rect {
        top: lp_to_lp_auto(css.top.as_ref(), ctx),
        right: lp_to_lp_auto(css.right.as_ref(), ctx),
        bottom: lp_to_lp_auto(css.bottom.as_ref(), ctx),
        left: lp_to_lp_auto(css.left.as_ref(), ctx),
    };

    // Sizing
    style.size = tf::Size {
        width: size_to_dim(css.width.as_ref(), ctx),
        height: size_to_dim(css.height.as_ref(), ctx),
    };
    style.min_size = tf::Size {
        width: size_to_dim(css.min_width.as_ref(), ctx),
        height: size_to_dim(css.min_height.as_ref(), ctx),
    };
    style.max_size = tf::Size {
        width: size_to_dim(css.max_width.as_ref(), ctx),
        height: size_to_dim(css.max_height.as_ref(), ctx),
    };
    style.aspect_ratio = css.aspect_ratio;

    // Margin / padding / border (border WIDTH only — border style/color are paint props)
    style.margin = edges_to_rect_lpa(css.margin.as_ref(), ctx);
    style.padding = edges_to_rect_lp(css.padding.as_ref(), ctx);
    style.border = border_widths(css.border.as_ref(), ctx);

    // Flex
    if let Some(d) = css.flex_direction {
        style.flex_direction = match d {
            FlexDirection::Row => tf::FlexDirection::Row,
            FlexDirection::RowReverse => tf::FlexDirection::RowReverse,
            FlexDirection::Column => tf::FlexDirection::Column,
            FlexDirection::ColumnReverse => tf::FlexDirection::ColumnReverse,
        };
    }
    if let Some(w) = css.flex_wrap {
        style.flex_wrap = match w {
            FlexWrap::Nowrap => tf::FlexWrap::NoWrap,
            FlexWrap::Wrap => tf::FlexWrap::Wrap,
            FlexWrap::WrapReverse => tf::FlexWrap::WrapReverse,
        };
    }
    if let Some(j) = css.justify_content {
        style.justify_content = Some(match j {
            JustifyContent::FlexStart | JustifyContent::Start => tf::JustifyContent::Start,
            JustifyContent::FlexEnd | JustifyContent::End => tf::JustifyContent::End,
            JustifyContent::Center => tf::JustifyContent::Center,
            JustifyContent::SpaceBetween => tf::JustifyContent::SpaceBetween,
            JustifyContent::SpaceAround => tf::JustifyContent::SpaceAround,
            JustifyContent::SpaceEvenly => tf::JustifyContent::SpaceEvenly,
        });
    }
    if let Some(a) = css.align_items {
        style.align_items = Some(align_items_to_taffy(a));
    }
    if let Some(a) = css.align_self {
        style.align_self = align_self_to_taffy(a);
    }
    if let Some(a) = css.align_content {
        style.align_content = Some(align_content_to_taffy(a));
    }
    if let Some(grow) = css.flex_grow {
        style.flex_grow = grow;
    }
    if let Some(shrink) = css.flex_shrink {
        style.flex_shrink = shrink;
    }
    if let Some(basis) = css.flex_basis.as_ref() {
        style.flex_basis = size_to_dim(Some(basis), ctx);
    }

    // Gap
    if let Some(gap) = css.gap.as_ref() {
        let (row, col) = match gap {
            Gap::Uniform(v) => (lp_to_lp(v, ctx), lp_to_lp(v, ctx)),
            Gap::RowColumn { row, column } => (lp_to_lp(row, ctx), lp_to_lp(column, ctx)),
        };
        style.gap = tf::Size {
            width: col,
            height: row,
        };
    }

    // Overflow
    if let Some(o) = css.overflow {
        let v = overflow_to_taffy(o);
        style.overflow = taffy::Point { x: v, y: v };
    }
    if let Some(o) = css.overflow_x {
        style.overflow.x = overflow_to_taffy(o);
    }
    if let Some(o) = css.overflow_y {
        style.overflow.y = overflow_to_taffy(o);
    }

    style
}

fn align_items_to_taffy(a: AlignItems) -> tf::AlignItems {
    match a {
        AlignItems::Stretch => tf::AlignItems::Stretch,
        AlignItems::FlexStart | AlignItems::Start => tf::AlignItems::Start,
        AlignItems::FlexEnd | AlignItems::End => tf::AlignItems::End,
        AlignItems::Center => tf::AlignItems::Center,
        AlignItems::Baseline => tf::AlignItems::Baseline,
    }
}

fn align_self_to_taffy(a: AlignSelf) -> Option<tf::AlignSelf> {
    Some(match a {
        AlignSelf::Auto => return None,
        AlignSelf::Stretch => tf::AlignSelf::Stretch,
        AlignSelf::FlexStart | AlignSelf::Start => tf::AlignSelf::Start,
        AlignSelf::FlexEnd | AlignSelf::End => tf::AlignSelf::End,
        AlignSelf::Center => tf::AlignSelf::Center,
        AlignSelf::Baseline => tf::AlignSelf::Baseline,
    })
}

fn align_content_to_taffy(a: AlignContent) -> tf::AlignContent {
    match a {
        AlignContent::Stretch => tf::AlignContent::Stretch,
        AlignContent::FlexStart | AlignContent::Start => tf::AlignContent::Start,
        AlignContent::FlexEnd | AlignContent::End => tf::AlignContent::End,
        AlignContent::Center => tf::AlignContent::Center,
        AlignContent::SpaceBetween => tf::AlignContent::SpaceBetween,
        AlignContent::SpaceAround => tf::AlignContent::SpaceAround,
        AlignContent::SpaceEvenly => tf::AlignContent::SpaceEvenly,
    }
}

fn overflow_to_taffy(o: Overflow) -> taffy::Overflow {
    match o {
        Overflow::Visible => taffy::Overflow::Visible,
        Overflow::Hidden | Overflow::Clip => taffy::Overflow::Hidden,
        Overflow::Auto | Overflow::Scroll => taffy::Overflow::Scroll,
    }
}

/// Convert `LengthPercentage` → taffy `LengthPercentage`.
fn lp_to_lp(v: &LengthPercentage, ctx: &ConversionContext) -> tf::LengthPercentage {
    match v.parse() {
        ParsedLength::Px(p) => tf::LengthPercentage::length(p),
        ParsedLength::Percent(p) => tf::LengthPercentage::percent(p / 100.0),
        ParsedLength::Em(em) => tf::LengthPercentage::length(em * ctx.length.font_size),
        ParsedLength::Rem(r) => tf::LengthPercentage::length(r * ctx.length.root_font_size),
        ParsedLength::Vw(p) => tf::LengthPercentage::length(p / 100.0 * ctx.length.viewport_width),
        ParsedLength::Vh(p) => tf::LengthPercentage::length(p / 100.0 * ctx.length.viewport_height),
        ParsedLength::Fr(_) | ParsedLength::Auto => tf::LengthPercentage::length(0.0),
    }
}

/// Convert `LengthPercentage` → taffy `LengthPercentageAuto`. None → auto.
fn lp_to_lp_auto(
    v: Option<&LengthPercentage>,
    ctx: &ConversionContext,
) -> tf::LengthPercentageAuto {
    let Some(v) = v else {
        return tf::LengthPercentageAuto::auto();
    };
    match v.parse() {
        ParsedLength::Auto => tf::LengthPercentageAuto::auto(),
        ParsedLength::Px(p) => tf::LengthPercentageAuto::length(p),
        ParsedLength::Percent(p) => tf::LengthPercentageAuto::percent(p / 100.0),
        ParsedLength::Em(em) => tf::LengthPercentageAuto::length(em * ctx.length.font_size),
        ParsedLength::Rem(r) => tf::LengthPercentageAuto::length(r * ctx.length.root_font_size),
        ParsedLength::Vw(p) => {
            tf::LengthPercentageAuto::length(p / 100.0 * ctx.length.viewport_width)
        }
        ParsedLength::Vh(p) => {
            tf::LengthPercentageAuto::length(p / 100.0 * ctx.length.viewport_height)
        }
        ParsedLength::Fr(_) => tf::LengthPercentageAuto::auto(),
    }
}

/// Convert `Size` → taffy `Dimension`.
fn size_to_dim(s: Option<&Size>, ctx: &ConversionContext) -> tf::Dimension {
    let Some(s) = s else {
        return tf::Dimension::auto();
    };
    match s {
        Size::Auto(_) => tf::Dimension::auto(),
        Size::Length(lp) => match lp.parse() {
            ParsedLength::Auto => tf::Dimension::auto(),
            ParsedLength::Px(p) => tf::Dimension::length(p),
            ParsedLength::Percent(p) => tf::Dimension::percent(p / 100.0),
            ParsedLength::Em(em) => tf::Dimension::length(em * ctx.length.font_size),
            ParsedLength::Rem(r) => tf::Dimension::length(r * ctx.length.root_font_size),
            ParsedLength::Vw(p) => tf::Dimension::length(p / 100.0 * ctx.length.viewport_width),
            ParsedLength::Vh(p) => tf::Dimension::length(p / 100.0 * ctx.length.viewport_height),
            ParsedLength::Fr(_) => tf::Dimension::auto(),
        },
        Size::Keyword(_) => {
            // taffy 0.10 supports max-content / min-content / fit-content via Dimension.
            // We map them to `auto` for now; refine later if needed.
            tf::Dimension::auto()
        }
    }
}

fn edges_to_rect_lp(e: Option<&Edges>, ctx: &ConversionContext) -> tf::Rect<tf::LengthPercentage> {
    let Some(e) = e else {
        return tf::Rect {
            top: tf::LengthPercentage::length(0.0),
            right: tf::LengthPercentage::length(0.0),
            bottom: tf::LengthPercentage::length(0.0),
            left: tf::LengthPercentage::length(0.0),
        };
    };
    let (top, right, bottom, left) = e.resolve();
    tf::Rect {
        top: lp_to_lp(&top, ctx),
        right: lp_to_lp(&right, ctx),
        bottom: lp_to_lp(&bottom, ctx),
        left: lp_to_lp(&left, ctx),
    }
}

fn edges_to_rect_lpa(
    e: Option<&Edges>,
    ctx: &ConversionContext,
) -> tf::Rect<tf::LengthPercentageAuto> {
    let Some(e) = e else {
        return tf::Rect {
            top: tf::LengthPercentageAuto::length(0.0),
            right: tf::LengthPercentageAuto::length(0.0),
            bottom: tf::LengthPercentageAuto::length(0.0),
            left: tf::LengthPercentageAuto::length(0.0),
        };
    };
    let (top, right, bottom, left) = e.resolve();
    tf::Rect {
        top: lp_to_lp_auto(Some(&top), ctx),
        right: lp_to_lp_auto(Some(&right), ctx),
        bottom: lp_to_lp_auto(Some(&bottom), ctx),
        left: lp_to_lp_auto(Some(&left), ctx),
    }
}

fn border_widths(
    b: Option<&super::style::BorderEdges>,
    ctx: &ConversionContext,
) -> tf::Rect<tf::LengthPercentage> {
    let Some(b) = b else {
        return tf::Rect {
            top: tf::LengthPercentage::length(0.0),
            right: tf::LengthPercentage::length(0.0),
            bottom: tf::LengthPercentage::length(0.0),
            left: tf::LengthPercentage::length(0.0),
        };
    };
    // Per-side overrides take precedence over the uniform `width`.
    let uniform = b.width.as_ref().map(|e| e.resolve());
    let pick_side = |side: Option<&super::style::BorderSide>, idx: usize| -> tf::LengthPercentage {
        if let Some(side) = side {
            if let Some(w) = side.width.as_ref() {
                let lp = LengthPercentage::Px(w.resolve(&ctx.length));
                return lp_to_lp(&lp, ctx);
            }
        }
        if let Some((t, r, btm, l)) = uniform.as_ref() {
            let pick = match idx {
                0 => t,
                1 => r,
                2 => btm,
                3 => l,
                _ => t,
            };
            return lp_to_lp(pick, ctx);
        }
        tf::LengthPercentage::length(0.0)
    };
    tf::Rect {
        top: pick_side(b.top.as_ref(), 0),
        right: pick_side(b.right.as_ref(), 1),
        bottom: pick_side(b.bottom.as_ref(), 2),
        left: pick_side(b.left.as_ref(), 3),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::style::*;

    fn ctx() -> ConversionContext {
        ConversionContext::default()
    }

    #[test]
    fn empty_style_yields_taffy_default() {
        let css = CssStyle::default();
        let s = to_taffy_style(&css, &ctx());
        assert_eq!(s.display, tf::Display::Block);
        assert_eq!(s.flex_grow, 0.0);
    }

    #[test]
    fn flex_column_with_gap() {
        let css = CssStyle {
            display: Some(Display::Flex),
            flex_direction: Some(FlexDirection::Column),
            gap: Some(Gap::Uniform(LengthPercentage::Px(16.0))),
            align_items: Some(AlignItems::Center),
            ..Default::default()
        };
        let s = to_taffy_style(&css, &ctx());
        assert_eq!(s.display, tf::Display::Flex);
        assert_eq!(s.flex_direction, tf::FlexDirection::Column);
        assert_eq!(s.align_items, Some(tf::AlignItems::Center));
        assert_eq!(s.gap.height, tf::LengthPercentage::length(16.0));
    }

    #[test]
    fn padding_uniform_resolved() {
        let css = CssStyle {
            padding: Some(Edges::Uniform(LengthPercentage::Px(24.0))),
            ..Default::default()
        };
        let s = to_taffy_style(&css, &ctx());
        assert_eq!(s.padding.top, tf::LengthPercentage::length(24.0));
        assert_eq!(s.padding.left, tf::LengthPercentage::length(24.0));
    }

    #[test]
    fn width_percent() {
        let css = CssStyle {
            width: Some(Size::Length(LengthPercentage::String("50%".into()))),
            ..Default::default()
        };
        let s = to_taffy_style(&css, &ctx());
        assert_eq!(s.size.width, tf::Dimension::percent(0.5));
    }

    #[test]
    fn position_absolute_inset() {
        let css = CssStyle {
            position: Some(Position::Absolute),
            top: Some(LengthPercentage::Px(10.0)),
            left: Some(LengthPercentage::Px(20.0)),
            ..Default::default()
        };
        let s = to_taffy_style(&css, &ctx());
        assert_eq!(s.position, tf::Position::Absolute);
        assert_eq!(s.inset.top, tf::LengthPercentageAuto::length(10.0));
        assert_eq!(s.inset.left, tf::LengthPercentageAuto::length(20.0));
    }

    #[test]
    fn flex_grow_shrink_basis() {
        let css = CssStyle {
            flex_grow: Some(2.0),
            flex_shrink: Some(0.5),
            flex_basis: Some(Size::Length(LengthPercentage::Px(100.0))),
            ..Default::default()
        };
        let s = to_taffy_style(&css, &ctx());
        assert_eq!(s.flex_grow, 2.0);
        assert_eq!(s.flex_shrink, 0.5);
        assert_eq!(s.flex_basis, tf::Dimension::length(100.0));
    }

    #[test]
    fn overflow_hidden() {
        let css = CssStyle {
            overflow: Some(Overflow::Hidden),
            ..Default::default()
        };
        let s = to_taffy_style(&css, &ctx());
        assert_eq!(s.overflow.x, taffy::Overflow::Hidden);
        assert_eq!(s.overflow.y, taffy::Overflow::Hidden);
    }
}
