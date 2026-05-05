//! Adapter from the legacy `LayerStyle` (Flutter-like) to the new `CssStyle`.
//!
//! Used during the transition period so existing scenarios + components can be
//! routed through the new browser-mode pipeline without rewriting every JSON
//! file at once. Each component's `style: LayerStyle` is converted on the fly
//! when building the box tree.
//!
//! Mappings that have no direct CSS analogue (`text_background`,
//! `gradient_border`, `motion_path`, `text_gradient`, `clip_path` strings)
//! are dropped here and continue to be handled by the component's painter.

use super::style::*;
use super::units::{Length as CLength, LengthPercentage as CLP};
use crate::schema::style::{
    CardAlign, CardDirection, CardDisplay, CardJustify, FontStyleType as LFontStyle,
    FontWeight as LFontWeight, GridTrack as LGridTrack, LayerStyle, Overflow as LOverflow,
    Spacing, TextAlign as LTextAlign,
};
use crate::schema::video::{
    BlendMode as LBlendMode, CardBorder, CardShadow, DropShadow, FilterConfig, InnerShadow,
};

/// Convert a [`LayerStyle`] (legacy schema) into the equivalent [`CssStyle`].
pub fn layer_to_css(layer: &LayerStyle) -> CssStyle {
    let mut css = CssStyle::default();

    if (layer.opacity - 1.0).abs() > f32::EPSILON {
        css.opacity = Some(layer.opacity);
    }

    if let Some(p) = &layer.padding {
        css.padding = Some(spacing_to_edges(p));
    }
    if let Some(m) = &layer.margin {
        css.margin = Some(spacing_to_edges(m));
    }

    if let Some(bg) = &layer.background {
        css.background = Some(parse_background_string(bg));
    }

    if let Some(r) = layer.border_radius {
        css.border_radius = Some(BorderRadius::Uniform(CLP::Px(r)));
    }
    if let Some(b) = &layer.border {
        css.border = Some(card_border_to_css(b));
    }

    if let Some(s) = &layer.box_shadow {
        css.box_shadow = Some(vec![card_shadow_to_css(s, false)]);
    }
    if let Some(s) = &layer.inner_shadow {
        let mut shadows = css.box_shadow.unwrap_or_default();
        shadows.push(inner_shadow_to_css(s));
        css.box_shadow = Some(shadows);
    }

    if let Some(ts) = &layer.text_shadow {
        css.text_shadow = Some(vec![TextShadow {
            offset_x: CLength::Px(ts.offset_x),
            offset_y: CLength::Px(ts.offset_y),
            blur: Some(CLength::Px(ts.blur)),
            color: Some(Color::String(ts.color.clone())),
        }]);
    }

    if let Some(fs) = layer.font_size {
        css.font_size = Some(CLength::Px(fs));
    }
    if let Some(c) = &layer.color {
        css.color = Some(Color::String(c.clone()));
    }
    if let Some(ff) = &layer.font_family {
        css.font_family = Some(ff.clone());
    }
    if let Some(fw) = &layer.font_weight {
        css.font_weight = Some(font_weight_to_css(fw));
    }
    if let Some(fs) = &layer.font_style {
        css.font_style = Some(match fs {
            LFontStyle::Normal => FontStyle::Normal,
            LFontStyle::Italic => FontStyle::Italic,
            LFontStyle::Oblique => FontStyle::Oblique,
        });
    }
    if let Some(ta) = &layer.text_align {
        css.text_align = Some(match ta {
            LTextAlign::Left => TextAlign::Left,
            LTextAlign::Center => TextAlign::Center,
            LTextAlign::Right => TextAlign::Right,
        });
    }
    if let Some(ls) = layer.letter_spacing {
        css.letter_spacing = Some(CLength::Px(ls));
    }
    if let Some(lh) = layer.line_height {
        // Legacy convention: <= 10 means unitless multiplier, else px.
        if lh <= 10.0 {
            css.line_height = Some(LineHeight::Number(lh));
        } else {
            css.line_height = Some(LineHeight::Length(CLP::Px(lh)));
        }
    }

    // ---- Flex / grid ----
    if let Some(d) = &layer.display {
        css.display = Some(match d {
            CardDisplay::Flex => Display::Flex,
            CardDisplay::Grid => Display::Grid,
        });
    }
    if let Some(fd) = &layer.flex_direction {
        css.flex_direction = Some(match fd {
            CardDirection::Row => FlexDirection::Row,
            CardDirection::RowReverse => FlexDirection::RowReverse,
            CardDirection::Column => FlexDirection::Column,
            CardDirection::ColumnReverse => FlexDirection::ColumnReverse,
        });
    }
    if let Some(g) = layer.gap {
        css.gap = Some(Gap::Uniform(CLP::Px(g)));
    }
    if let Some(ai) = &layer.align_items {
        css.align_items = Some(card_align_to_items(ai));
    }
    if let Some(jc) = &layer.justify_content {
        css.justify_content = Some(card_justify_to_css(jc));
    }
    if let Some(fw) = layer.flex_wrap {
        css.flex_wrap = Some(if fw { FlexWrap::Wrap } else { FlexWrap::Nowrap });
    }
    if let Some(grow) = layer.flex_grow {
        css.flex_grow = Some(grow);
    }
    if let Some(shrink) = layer.flex_shrink {
        css.flex_shrink = Some(shrink);
    }
    if let Some(basis) = layer.flex_basis {
        css.flex_basis = Some(Size::Length(CLP::Px(basis)));
    }
    if let Some(asf) = &layer.align_self {
        css.align_self = Some(card_align_to_self(asf));
    }

    if let Some(cols) = &layer.grid_template_columns {
        css.grid_template_columns = Some(grid_tracks_to_css(cols));
    }
    if let Some(rows) = &layer.grid_template_rows {
        css.grid_template_rows = Some(grid_tracks_to_css(rows));
    }

    // ---- Visual effects ----
    if let Some(bb) = layer.backdrop_blur {
        css.backdrop_filter = Some(vec![FilterFn::Blur { radius: CLength::Px(bb) }]);
    }
    if let Some(f) = &layer.filter {
        let fns = filter_config_to_css(f);
        if !fns.is_empty() {
            css.filter = Some(fns);
        }
    }
    if let Some(ds) = &layer.drop_shadow {
        let mut fns = css.filter.unwrap_or_default();
        fns.push(drop_shadow_to_css(ds));
        css.filter = Some(fns);
    }
    if let Some(bm) = &layer.blend_mode {
        css.mix_blend_mode = Some(blend_mode_to_css(bm));
    }
    if let Some(ar) = layer.aspect_ratio {
        css.aspect_ratio = Some(ar);
    }

    if let Some(ov) = &layer.overflow {
        css.overflow = Some(match ov {
            LOverflow::Visible => Overflow::Visible,
            LOverflow::Hidden => Overflow::Hidden,
        });
    }
    if let Some(wrap) = layer.wrap {
        if wrap {
            css.overflow_wrap = Some(OverflowWrap::BreakWord);
        } else {
            css.white_space = Some(WhiteSpace::Nowrap);
        }
    }

    css
}

// ---- helpers ----

fn spacing_to_edges(s: &Spacing) -> Edges {
    match s {
        Spacing::Uniform(v) => Edges::Uniform(CLP::Px(*v)),
        Spacing::Sides { top, right, bottom, left } => Edges::Sides {
            top: CLP::Px(*top),
            right: CLP::Px(*right),
            bottom: CLP::Px(*bottom),
            left: CLP::Px(*left),
        },
    }
}

fn card_border_to_css(b: &CardBorder) -> BorderEdges {
    BorderEdges {
        width: Some(Edges::Uniform(CLP::Px(b.width))),
        style: Some(BorderStyle::Solid),
        color: Some(Color::String(b.color.clone())),
        ..Default::default()
    }
}

fn card_shadow_to_css(s: &CardShadow, inset: bool) -> BoxShadow {
    BoxShadow {
        offset_x: CLength::Px(s.offset_x),
        offset_y: CLength::Px(s.offset_y),
        blur: Some(CLength::Px(s.blur)),
        spread: None,
        color: Some(Color::String(s.color.clone())),
        inset: Some(inset),
    }
}

fn inner_shadow_to_css(s: &InnerShadow) -> BoxShadow {
    BoxShadow {
        offset_x: CLength::Px(s.offset_x),
        offset_y: CLength::Px(s.offset_y),
        blur: Some(CLength::Px(s.blur)),
        spread: None,
        color: Some(Color::String(s.color.clone())),
        inset: Some(true),
    }
}

fn font_weight_to_css(w: &LFontWeight) -> FontWeight {
    match w {
        LFontWeight::Normal => FontWeight::Keyword(FontWeightKw::Normal),
        LFontWeight::Bold => FontWeight::Keyword(FontWeightKw::Bold),
        LFontWeight::Weight(n) => FontWeight::Number(*n),
    }
}

fn card_align_to_items(a: &CardAlign) -> AlignItems {
    match a {
        CardAlign::Start => AlignItems::FlexStart,
        CardAlign::Center => AlignItems::Center,
        CardAlign::End => AlignItems::FlexEnd,
        CardAlign::Stretch => AlignItems::Stretch,
    }
}

fn card_align_to_self(a: &CardAlign) -> AlignSelf {
    match a {
        CardAlign::Start => AlignSelf::FlexStart,
        CardAlign::Center => AlignSelf::Center,
        CardAlign::End => AlignSelf::FlexEnd,
        CardAlign::Stretch => AlignSelf::Stretch,
    }
}

fn card_justify_to_css(j: &CardJustify) -> JustifyContent {
    match j {
        CardJustify::Start => JustifyContent::FlexStart,
        CardJustify::Center => JustifyContent::Center,
        CardJustify::End => JustifyContent::FlexEnd,
        CardJustify::SpaceBetween => JustifyContent::SpaceBetween,
        CardJustify::SpaceAround => JustifyContent::SpaceAround,
        CardJustify::SpaceEvenly => JustifyContent::SpaceEvenly,
    }
}

fn grid_tracks_to_css(tracks: &[LGridTrack]) -> Vec<GridTrack> {
    tracks
        .iter()
        .map(|t| match t {
            LGridTrack::Px(v) => GridTrack::Length(CLP::Px(*v)),
            LGridTrack::Fr(v) => GridTrack::Fr(*v),
            LGridTrack::Auto => GridTrack::Keyword(GridTrackKeyword::Auto),
        })
        .collect()
}

fn filter_config_to_css(f: &FilterConfig) -> Vec<FilterFn> {
    let mut out = Vec::new();
    if let Some(v) = f.brightness {
        out.push(FilterFn::Brightness { value: v });
    }
    if let Some(v) = f.contrast {
        out.push(FilterFn::Contrast { value: v });
    }
    if let Some(v) = f.grayscale {
        out.push(FilterFn::Grayscale { value: v });
    }
    if let Some(v) = f.hue_rotate {
        out.push(FilterFn::HueRotate { deg: v });
    }
    if let Some(v) = f.saturate {
        out.push(FilterFn::Saturate { value: v });
    }
    if let Some(v) = f.sepia {
        out.push(FilterFn::Sepia { value: v });
    }
    out
}

fn drop_shadow_to_css(ds: &DropShadow) -> FilterFn {
    FilterFn::DropShadow {
        offset_x: CLength::Px(ds.dx),
        offset_y: CLength::Px(ds.dy),
        blur: Some(CLength::Px(ds.blur)),
        color: Some(Color::String(ds.color.clone())),
    }
}

fn blend_mode_to_css(bm: &LBlendMode) -> BlendMode {
    match bm {
        LBlendMode::Multiply => BlendMode::Multiply,
        LBlendMode::Screen => BlendMode::Screen,
        LBlendMode::Overlay => BlendMode::Overlay,
        LBlendMode::Darken => BlendMode::Darken,
        LBlendMode::Lighten => BlendMode::Lighten,
        LBlendMode::ColorDodge => BlendMode::ColorDodge,
        LBlendMode::ColorBurn => BlendMode::ColorBurn,
        LBlendMode::HardLight => BlendMode::HardLight,
        LBlendMode::SoftLight => BlendMode::SoftLight,
        LBlendMode::Difference => BlendMode::Difference,
        LBlendMode::Exclusion => BlendMode::Exclusion,
        LBlendMode::Hue => BlendMode::Hue,
        LBlendMode::Saturation => BlendMode::Saturation,
        LBlendMode::Color => BlendMode::Color,
        LBlendMode::Luminosity => BlendMode::Luminosity,
    }
}

/// Parse the legacy `background: String` field. Recognises:
/// - solid hex/named colors (`#fff`, `red`)
/// - `linear-gradient(<angle>, <stop>, ...)` and `radial-gradient(<stop>, ...)`
///   in their basic forms. Anything else falls back to `Color::String(s)`.
fn parse_background_string(s: &str) -> Background {
    let trimmed = s.trim();
    if let Some(layer) = parse_gradient(trimmed) {
        return Background::Single(layer);
    }
    Background::Color(Color::String(trimmed.to_string()))
}

fn parse_gradient(s: &str) -> Option<BackgroundLayer> {
    if let Some(rest) = strip_fn(s, "linear-gradient") {
        return parse_linear_gradient_args(rest);
    }
    if let Some(rest) = strip_fn(s, "radial-gradient") {
        return parse_radial_gradient_args(rest);
    }
    None
}

fn strip_fn<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    let s = s.trim();
    if !s.to_ascii_lowercase().starts_with(name) {
        return None;
    }
    let rest = &s[name.len()..];
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('(')?;
    rest.strip_suffix(')').map(|r| r.trim())
}

fn parse_linear_gradient_args(args: &str) -> Option<BackgroundLayer> {
    let parts = split_top_level_commas(args);
    if parts.is_empty() {
        return None;
    }
    let (angle, stop_parts) = if let Some(deg) = parse_angle(parts[0]) {
        (Some(deg), &parts[1..])
    } else {
        (None, &parts[..])
    };
    let stops = parse_color_stops(stop_parts);
    if stops.is_empty() {
        return None;
    }
    Some(BackgroundLayer::LinearGradient { angle, stops })
}

fn parse_radial_gradient_args(args: &str) -> Option<BackgroundLayer> {
    let parts = split_top_level_commas(args);
    if parts.is_empty() {
        return None;
    }
    // Skip leading shape/position descriptor if present (e.g. "circle",
    // "ellipse", "circle at center"). Anything starting with a shape keyword
    // or `at`/`closest-`/`farthest-` is treated as a descriptor.
    let stop_parts: &[&str] = if is_radial_descriptor(parts[0]) {
        &parts[1..]
    } else {
        &parts[..]
    };
    let stops = parse_color_stops(stop_parts);
    if stops.is_empty() {
        return None;
    }
    Some(BackgroundLayer::RadialGradient { shape: None, position: None, stops })
}

fn is_radial_descriptor(s: &str) -> bool {
    let lower = s.trim().to_ascii_lowercase();
    matches!(lower.as_str(), "circle" | "ellipse")
        || lower.starts_with("circle ")
        || lower.starts_with("ellipse ")
        || lower.starts_with("at ")
        || lower.starts_with("closest-")
        || lower.starts_with("farthest-")
}

fn parse_angle(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(num) = s.strip_suffix("deg") {
        return num.trim().parse::<f32>().ok();
    }
    None
}

fn parse_color_stops(parts: &[&str]) -> Vec<GradientStop> {
    let mut out = Vec::new();
    for raw in parts {
        let p = raw.trim();
        if p.is_empty() {
            continue;
        }
        // "color offset%" or just "color"
        if let Some(idx) = p.rfind(' ') {
            let (color, off) = p.split_at(idx);
            let color = color.trim();
            let off = off.trim();
            if let Some(n) = off.strip_suffix('%') {
                if let Ok(pct) = n.trim().parse::<f32>() {
                    out.push(GradientStop {
                        color: Color::String(color.to_string()),
                        offset: Some(pct / 100.0),
                    });
                    continue;
                }
            }
        }
        out.push(GradientStop { color: Color::String(p.to_string()), offset: None });
    }
    out
}

/// Split a comma-separated list while respecting nested parentheses (so
/// `rgba(0,0,0,1)` stays in one piece).
fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth: i32 = 0;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                out.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::style::{LayerStyle, Spacing};

    #[test]
    fn empty_layer_yields_default_css() {
        let css = layer_to_css(&LayerStyle::default());
        assert!(css.padding.is_none());
        assert!(css.background.is_none());
        // opacity default 1.0 is dropped (None means "not set")
        assert!(css.opacity.is_none());
    }

    #[test]
    fn padding_uniform() {
        let mut l = LayerStyle::default();
        l.padding = Some(Spacing::Uniform(24.0));
        let css = layer_to_css(&l);
        assert!(matches!(css.padding, Some(Edges::Uniform(_))));
    }

    #[test]
    fn flex_column_with_gap() {
        let l = LayerStyle {
            display: Some(CardDisplay::Flex),
            flex_direction: Some(CardDirection::Column),
            gap: Some(16.0),
            align_items: Some(CardAlign::Center),
            ..Default::default()
        };
        let css = layer_to_css(&l);
        assert_eq!(css.display, Some(Display::Flex));
        assert_eq!(css.flex_direction, Some(FlexDirection::Column));
        assert_eq!(css.align_items, Some(AlignItems::Center));
        assert!(matches!(css.gap, Some(Gap::Uniform(_))));
    }

    #[test]
    fn background_solid_color() {
        let mut l = LayerStyle::default();
        l.background = Some("#FF00FF".into());
        let css = layer_to_css(&l);
        assert!(matches!(css.background, Some(Background::Color(Color::String(_)))));
    }

    #[test]
    fn background_linear_gradient() {
        let mut l = LayerStyle::default();
        l.background = Some("linear-gradient(45deg, #fff, #000)".into());
        let css = layer_to_css(&l);
        match css.background {
            Some(Background::Single(BackgroundLayer::LinearGradient { angle, stops })) => {
                assert_eq!(angle, Some(45.0));
                assert_eq!(stops.len(), 2);
            }
            other => panic!("expected linear gradient, got {:?}", other),
        }
    }

    #[test]
    fn background_radial_gradient_with_offsets() {
        let mut l = LayerStyle::default();
        l.background =
            Some("radial-gradient(circle, #fff 0%, #000 100%)".into());
        let css = layer_to_css(&l);
        match css.background {
            Some(Background::Single(BackgroundLayer::RadialGradient { stops, .. })) => {
                assert_eq!(stops.len(), 2);
                assert_eq!(stops[0].offset, Some(0.0));
                assert_eq!(stops[1].offset, Some(1.0));
            }
            _ => panic!("expected radial gradient"),
        }
    }

    #[test]
    fn wrap_false_sets_white_space_nowrap() {
        let mut l = LayerStyle::default();
        l.wrap = Some(false);
        let css = layer_to_css(&l);
        assert_eq!(css.white_space, Some(WhiteSpace::Nowrap));
    }

    #[test]
    fn wrap_true_sets_overflow_wrap_break_word() {
        let mut l = LayerStyle::default();
        l.wrap = Some(true);
        let css = layer_to_css(&l);
        assert_eq!(css.overflow_wrap, Some(OverflowWrap::BreakWord));
    }

    #[test]
    fn font_weight_numeric() {
        let mut l = LayerStyle::default();
        l.font_weight = Some(LFontWeight::Weight(600));
        let css = layer_to_css(&l);
        assert!(matches!(css.font_weight, Some(FontWeight::Number(600))));
    }
}
