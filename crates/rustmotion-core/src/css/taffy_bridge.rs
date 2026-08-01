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
    GridAutoFlow, GridLine, GridLineEnd, GridTrack, GridTrackKeyword, JustifyContent, Overflow,
    Position, Size,
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

    // Grid
    if let Some(tracks) = css.grid_template_columns.as_ref() {
        style.grid_template_columns = tracks
            .iter()
            .map(|t| tf::GridTemplateComponent::Single(grid_track_sizing(t, ctx)))
            .collect();
    }
    if let Some(tracks) = css.grid_template_rows.as_ref() {
        style.grid_template_rows = tracks
            .iter()
            .map(|t| tf::GridTemplateComponent::Single(grid_track_sizing(t, ctx)))
            .collect();
    }
    if let Some(flow) = css.grid_auto_flow {
        style.grid_auto_flow = match flow {
            GridAutoFlow::Row => tf::GridAutoFlow::Row,
            GridAutoFlow::Column => tf::GridAutoFlow::Column,
            GridAutoFlow::RowDense => tf::GridAutoFlow::RowDense,
            GridAutoFlow::ColumnDense => tf::GridAutoFlow::ColumnDense,
        };
    }
    if let Some(gc) = css.grid_column.as_ref() {
        style.grid_column = grid_placement_line(gc);
    }
    if let Some(gr) = css.grid_row.as_ref() {
        style.grid_row = grid_placement_line(gr);
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

/// Convert a [`GridTrack`] into a taffy `TrackSizingFunction` (used for both
/// the min and max sizing function, except `fr` which uses taffy's `flex()`
/// helper — `minmax(0, Nfr)`. This gives *exactly* evenly-sized tracks
/// regardless of child content, matching the `flex-direction: row` control
/// (`flex-grow: 1` siblings) rather than CSS's stricter `minmax(auto, Nfr)`
/// default, which lets content push a track wider than its fair share.
fn grid_track_sizing(t: &GridTrack, ctx: &ConversionContext) -> tf::TrackSizingFunction {
    match t {
        GridTrack::Fr(n) => tf::flex(*n),
        GridTrack::Keyword(k) => grid_keyword_sizing(*k),
        GridTrack::Length(lp) => grid_length_sizing(lp, ctx),
        GridTrack::Minmax { min, max } => {
            tf::minmax(grid_track_min(min, ctx), grid_track_max(max, ctx))
        }
    }
}

fn grid_keyword_sizing(k: GridTrackKeyword) -> tf::TrackSizingFunction {
    match k {
        GridTrackKeyword::Auto => tf::auto(),
        GridTrackKeyword::MinContent => tf::min_content(),
        GridTrackKeyword::MaxContent => tf::max_content(),
    }
}

fn grid_length_sizing(lp: &LengthPercentage, ctx: &ConversionContext) -> tf::TrackSizingFunction {
    match lp.parse() {
        ParsedLength::Fr(n) => tf::flex(n),
        ParsedLength::Auto => tf::auto(),
        ParsedLength::Px(p) => tf::length(p),
        ParsedLength::Percent(p) => tf::percent(p / 100.0),
        ParsedLength::Em(em) => tf::length(em * ctx.length.font_size),
        ParsedLength::Rem(r) => tf::length(r * ctx.length.root_font_size),
        ParsedLength::Vw(p) => tf::length(p / 100.0 * ctx.length.viewport_width),
        ParsedLength::Vh(p) => tf::length(p / 100.0 * ctx.length.viewport_height),
    }
}

/// `min` side of an explicit `minmax(min, max)`. `fr` is not a valid CSS
/// minimum sizing function, so it falls back to `auto` (matches taffy's own
/// `MaxTrackSizingFunction -> MinTrackSizingFunction` conversion for `fr`).
fn grid_track_min(t: &GridTrack, ctx: &ConversionContext) -> tf::MinTrackSizingFunction {
    match t {
        GridTrack::Fr(_) => tf::auto(),
        GridTrack::Keyword(GridTrackKeyword::Auto) => tf::auto(),
        GridTrack::Keyword(GridTrackKeyword::MinContent) => tf::min_content(),
        GridTrack::Keyword(GridTrackKeyword::MaxContent) => tf::max_content(),
        GridTrack::Length(lp) => grid_length_min(lp, ctx),
        // A `minmax` nested inside a `minmax` isn't valid CSS; degrade
        // gracefully by taking the inner track's own min side.
        GridTrack::Minmax { min, .. } => grid_track_min(min, ctx),
    }
}

fn grid_length_min(lp: &LengthPercentage, ctx: &ConversionContext) -> tf::MinTrackSizingFunction {
    match lp.parse() {
        ParsedLength::Fr(_) | ParsedLength::Auto => tf::auto(),
        ParsedLength::Px(p) => tf::length(p),
        ParsedLength::Percent(p) => tf::percent(p / 100.0),
        ParsedLength::Em(em) => tf::length(em * ctx.length.font_size),
        ParsedLength::Rem(r) => tf::length(r * ctx.length.root_font_size),
        ParsedLength::Vw(p) => tf::length(p / 100.0 * ctx.length.viewport_width),
        ParsedLength::Vh(p) => tf::length(p / 100.0 * ctx.length.viewport_height),
    }
}

/// `max` side of an explicit `minmax(min, max)`.
fn grid_track_max(t: &GridTrack, ctx: &ConversionContext) -> tf::MaxTrackSizingFunction {
    match t {
        GridTrack::Fr(n) => tf::fr(*n),
        GridTrack::Keyword(GridTrackKeyword::Auto) => tf::auto(),
        GridTrack::Keyword(GridTrackKeyword::MinContent) => tf::min_content(),
        GridTrack::Keyword(GridTrackKeyword::MaxContent) => tf::max_content(),
        GridTrack::Length(lp) => grid_length_max(lp, ctx),
        GridTrack::Minmax { max, .. } => grid_track_max(max, ctx),
    }
}

fn grid_length_max(lp: &LengthPercentage, ctx: &ConversionContext) -> tf::MaxTrackSizingFunction {
    match lp.parse() {
        ParsedLength::Fr(n) => tf::fr(n),
        ParsedLength::Auto => tf::auto(),
        ParsedLength::Px(p) => tf::length(p),
        ParsedLength::Percent(p) => tf::percent(p / 100.0),
        ParsedLength::Em(em) => tf::length(em * ctx.length.font_size),
        ParsedLength::Rem(r) => tf::length(r * ctx.length.root_font_size),
        ParsedLength::Vw(p) => tf::length(p / 100.0 * ctx.length.viewport_width),
        ParsedLength::Vh(p) => tf::length(p / 100.0 * ctx.length.viewport_height),
    }
}

/// Convert a `grid-column` / `grid-row` placement (`{ start, end, span }`)
/// into a taffy `Line<GridPlacement>`. Named lines / grid areas are out of
/// scope (issue #105) — only numeric line indices and spans are supported.
fn grid_placement_line(g: &GridLine) -> tf::Line<tf::GridPlacement> {
    let start = g.start.map(|GridLineEnd::Index(i)| i as i16);
    let end = g.end.map(|GridLineEnd::Index(i)| i as i16);
    match (start, end, g.span) {
        (Some(s), Some(e), _) => tf::Line {
            start: tf::line(s),
            end: tf::line(e),
        },
        (Some(s), None, Some(n)) => tf::Line {
            start: tf::line(s),
            end: tf::span(n),
        },
        (Some(s), None, None) => tf::Line {
            start: tf::line(s),
            end: tf::auto(),
        },
        (None, Some(e), Some(n)) => tf::Line {
            start: tf::span(n),
            end: tf::line(e),
        },
        (None, Some(e), None) => tf::Line {
            start: tf::auto(),
            end: tf::line(e),
        },
        (None, None, Some(n)) => tf::Line {
            start: tf::span(n),
            end: tf::auto(),
        },
        (None, None, None) => tf::Line::auto(),
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

    // ---- Grid (issue #105) ----

    #[test]
    fn grid_display_translated() {
        let css = CssStyle {
            display: Some(Display::Grid),
            ..Default::default()
        };
        let s = to_taffy_style(&css, &ctx());
        assert_eq!(s.display, tf::Display::Grid);
    }

    #[test]
    fn grid_template_columns_fr_tracks_translated() {
        let css = CssStyle {
            grid_template_columns: Some(vec![GridTrack::Fr(1.0), GridTrack::Fr(2.0)]),
            ..Default::default()
        };
        let s = to_taffy_style(&css, &ctx());
        assert_eq!(s.grid_template_columns.len(), 2);
    }

    #[test]
    fn grid_template_rows_string_fr_tracks_translated() {
        // Same string-encoded form used by examples/mega-showcase.json.
        let css = CssStyle {
            grid_template_rows: Some(vec![
                GridTrack::Length(LengthPercentage::String("1fr".into())),
                GridTrack::Length(LengthPercentage::String("1fr".into())),
            ]),
            ..Default::default()
        };
        let s = to_taffy_style(&css, &ctx());
        assert_eq!(s.grid_template_rows.len(), 2);
    }

    #[test]
    fn grid_auto_flow_column_translated() {
        let css = CssStyle {
            grid_auto_flow: Some(GridAutoFlow::Column),
            ..Default::default()
        };
        let s = to_taffy_style(&css, &ctx());
        assert_eq!(s.grid_auto_flow, tf::GridAutoFlow::Column);
    }

    #[test]
    fn grid_column_start_and_span_translated() {
        let css = CssStyle {
            grid_column: Some(GridLine {
                start: Some(GridLineEnd::Index(2)),
                span: Some(3),
                ..Default::default()
            }),
            ..Default::default()
        };
        let s = to_taffy_style(&css, &ctx());
        assert!(matches!(s.grid_column.start, tf::GridPlacement::Line(_)));
        assert!(matches!(s.grid_column.end, tf::GridPlacement::Span(3)));
    }

    #[test]
    fn grid_row_start_end_translated() {
        let css = CssStyle {
            grid_row: Some(GridLine {
                start: Some(GridLineEnd::Index(1)),
                end: Some(GridLineEnd::Index(3)),
                ..Default::default()
            }),
            ..Default::default()
        };
        let s = to_taffy_style(&css, &ctx());
        assert!(matches!(s.grid_row.start, tf::GridPlacement::Line(_)));
        assert!(matches!(s.grid_row.end, tf::GridPlacement::Line(_)));
    }

    #[test]
    fn grid_gap_applies_to_both_axes() {
        let css = CssStyle {
            display: Some(Display::Grid),
            gap: Some(Gap::Uniform(LengthPercentage::Px(24.0))),
            ..Default::default()
        };
        let s = to_taffy_style(&css, &ctx());
        assert_eq!(s.gap.width, tf::LengthPercentage::length(24.0));
        assert_eq!(s.gap.height, tf::LengthPercentage::length(24.0));
    }

    /// The audit's core repro: a grid with three `1fr` columns must lay its
    /// children out side-by-side (distinct x-offsets), matching a
    /// `flex-direction: row` control — NOT stacked as three full-width rows,
    /// which is what the engine did before `grid-template-columns` was wired
    /// through to taffy (grid fell back to taffy's single-implicit-column
    /// default because it never received any track definitions).
    #[test]
    fn grid_three_fr_columns_produce_distinct_x_offsets() {
        let root_css = CssStyle {
            display: Some(Display::Grid),
            // The grid container needs a definite size: an `auto`-sized root
            // (the CssStyle default) has no intrinsic content of its own, so
            // it collapses to 0×0 and every column ends up at x=0.
            width: Some(Size::Length(LengthPercentage::Px(900.0))),
            height: Some(Size::Length(LengthPercentage::Px(300.0))),
            grid_template_columns: Some(vec![
                GridTrack::Fr(1.0),
                GridTrack::Fr(1.0),
                GridTrack::Fr(1.0),
            ]),
            ..Default::default()
        };
        let root_style = to_taffy_style(&root_css, &ctx());
        let leaf_style = to_taffy_style(&CssStyle::default(), &ctx());

        let mut tree: tf::TaffyTree = tf::TaffyTree::new();
        let c1 = tree.new_leaf(leaf_style.clone()).unwrap();
        let c2 = tree.new_leaf(leaf_style.clone()).unwrap();
        let c3 = tree.new_leaf(leaf_style).unwrap();
        let root = tree.new_with_children(root_style, &[c1, c2, c3]).unwrap();

        tree.compute_layout(
            root,
            tf::Size {
                width: tf::AvailableSpace::Definite(900.0),
                height: tf::AvailableSpace::Definite(300.0),
            },
        )
        .unwrap();

        let x1 = tree.layout(c1).unwrap().location.x;
        let x2 = tree.layout(c2).unwrap().location.x;
        let x3 = tree.layout(c3).unwrap().location.x;
        let w1 = tree.layout(c1).unwrap().size.width;

        assert_ne!(x1, x2, "columns 1 and 2 must not share an x-offset");
        assert_ne!(x2, x3, "columns 2 and 3 must not share an x-offset");
        assert!((x1 - 0.0).abs() < 0.5, "column 1 should start at x=0, got {x1}");
        assert!((x2 - 300.0).abs() < 1.0, "column 2 should start at ~300px, got {x2}");
        assert!((x3 - 600.0).abs() < 1.0, "column 3 should start at ~600px, got {x3}");
        assert!((w1 - 300.0).abs() < 1.0, "each 1fr column should be ~300px wide, got {w1}");
    }
}
