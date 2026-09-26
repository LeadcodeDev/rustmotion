use skia_safe::{Font, FontStyle as SkFontStyle, Typeface};

use rustmotion_core::css::style::{
    CssStyle, FontStyle as CssFontStyle, FontWeight as CssFontWeight, FontWeightKw, LineHeight,
    TextAlign as CssTextAlign, WhiteSpace, TEXT_AUTOFIT_MIN_FONT_PX,
};
use rustmotion_core::engine::box_tree::{AvailableSpace, IntrinsicMeasure};
use rustmotion_core::engine::deps::{TextMetrics, TextMetricsProvider};
use rustmotion_core::engine::renderer::{
    compute_glyph_metrics, emoji_typeface, format_counter_value, measure_text_with_fallback,
    typeface_with_fallback, wrap_text_with_tracking, GlyphMetric,
};

use crate::badge::{Badge, BadgeSize};
use crate::caption::Caption;
use crate::counter::Counter;
use crate::gradient_text::GradientText;
use crate::kbd::Kbd;
use crate::text::Text;

use rustmotion_core::css::units::LengthContext;

pub fn font_size_ctx(viewport_width: f32, viewport_height: f32, parent_size: f32) -> LengthContext {
    LengthContext {
        viewport_width,
        viewport_height,
        parent_size,
        font_size: 16.0,
        root_font_size: 16.0,
    }
}

pub fn measure_time_font_size_ctx(parent_size: f32) -> LengthContext {
    font_size_ctx(1920.0, 1080.0, parent_size)
}

pub struct TextIntrinsic {
    content: String,
    font_family: Option<String>,
    font_size: f32,
    line_height_resolved: f32,
    weight: u16,
    italic: bool,
    letter_spacing: f32,
    max_width: Option<f32>,
    wrap: bool,
    text_align: CssTextAlign,
    text_autofit: bool,
}

impl TextIntrinsic {
    pub fn from_text(text: &Text) -> Self {
        let wrap = !matches!(
            text.style.white_space,
            Some(WhiteSpace::Nowrap | WhiteSpace::Pre)
        );
        let widest = text
            .all_labels()
            .max_by_key(|label| label.chars().count())
            .unwrap_or(&text.content);
        Self::from_parts_with_wrap(widest, &text.style, text.max_width, wrap)
            .with_autofit(matches!(text.style.text_autofit, Some(true)))
    }

    pub fn with_autofit(mut self, on: bool) -> Self {
        self.text_autofit = on;
        self
    }

    pub fn from_parts(content: &str, style: &CssStyle, max_width: Option<f32>) -> Self {
        let base_ctx = measure_time_font_size_ctx(0.0);
        let (font_size, letter_spacing, line_height_resolved) =
            style.typography_px_ctx(&base_ctx, 48.0);
        let text_align = match style.text_align {
            Some(CssTextAlign::Center) => CssTextAlign::Center,
            Some(CssTextAlign::Right | CssTextAlign::End) => CssTextAlign::Right,
            _ => CssTextAlign::Left,
        };
        Self {
            content: content.to_string(),
            font_family: style.font_family.clone(),
            font_size,
            line_height_resolved,
            weight: weight_to_u16(style.font_weight.as_ref()),
            italic: matches!(style.font_style, Some(CssFontStyle::Italic)),
            letter_spacing,
            max_width,
            wrap: true,
            text_align,
            text_autofit: false,
        }
    }

    pub fn from_parts_with_wrap(
        content: &str,
        style: &CssStyle,
        max_width: Option<f32>,
        wrap: bool,
    ) -> Self {
        let mut t = Self::from_parts(content, style, max_width);
        t.wrap = wrap;
        t
    }
}

impl IntrinsicMeasure for TextIntrinsic {
    fn measure(
        &self,
        known: (Option<f32>, Option<f32>),
        available: (AvailableSpace, AvailableSpace),
    ) -> (f32, f32) {
        let max_width = if let Some(w) = known.0 {
            Some(w)
        } else {
            let avail_w = match available.0 {
                AvailableSpace::Definite(w) => Some(w),
                AvailableSpace::MaxContent => None,
                AvailableSpace::MinContent => Some(0.0),
            };
            match (self.max_width, avail_w) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            }
        };

        let Some(typeface) = self.typeface() else {
            return (0.0, 0.0);
        };
        let wrap_at = if self.wrap { max_width } else { None };
        let (base_w, base_h) = wrap_and_measure(
            &self.content,
            &typeface,
            self.font_size,
            wrap_at,
            self.letter_spacing,
            self.line_height_resolved,
        );

        if !self.text_autofit {
            return (base_w, base_h);
        }

        let target_height = match known.1 {
            Some(h) => Some(h),
            None => match available.1 {
                AvailableSpace::Definite(h) => Some(h),
                AvailableSpace::MaxContent => None,
                AvailableSpace::MinContent => Some(0.0),
            },
        };

        let (final_size, final_ls, final_lh) = resolve_text_autofit(
            &self.content,
            &typeface,
            self.font_size,
            self.letter_spacing,
            self.line_height_resolved,
            self.wrap,
            max_width,
            target_height,
        );

        if final_size >= self.font_size {
            return (base_w, base_h);
        }
        let wrap_at = if self.wrap { max_width } else { None };
        wrap_and_measure(
            &self.content,
            &typeface,
            final_size,
            wrap_at,
            final_ls,
            final_lh,
        )
    }
}

impl TextIntrinsic {
    fn sk_font_style(&self) -> SkFontStyle {
        let slant = if self.italic {
            skia_safe::font_style::Slant::Italic
        } else {
            skia_safe::font_style::Slant::Upright
        };
        let weight = skia_safe::font_style::Weight::from(self.weight as i32);
        SkFontStyle::new(weight, skia_safe::font_style::Width::NORMAL, slant)
    }

    fn typeface(&self) -> Option<Typeface> {
        let family = self.font_family.as_deref().unwrap_or("Inter");
        typeface_with_fallback(family, self.sk_font_style()).ok()
    }

    pub fn text_metrics(&self, content_box_width: f32) -> Option<TextMetrics> {
        let typeface = self.typeface()?;
        let font = Font::from_typeface(typeface, self.font_size);
        let emoji_font = emoji_typeface().map(|tf| Font::from_typeface(tf, self.font_size));

        let wrap_at = if self.wrap {
            Some(
                self.max_width
                    .map(|m| m.min(content_box_width))
                    .unwrap_or(content_box_width),
            )
        } else {
            None
        };
        let lines = wrap_text_with_tracking(
            &self.content,
            &font,
            &emoji_font,
            wrap_at,
            self.letter_spacing,
        );

        let mut text_width = 0.0f32;
        let mut glyphs: Vec<GlyphMetric> = Vec::new();
        for line in &lines {
            let advance = measure_text_with_fallback(line, &font, &emoji_font, self.letter_spacing);
            text_width = text_width.max(advance);
            let line_x = match self.text_align {
                CssTextAlign::Center => (content_box_width - advance) / 2.0,
                CssTextAlign::Right => content_box_width - advance,
                _ => 0.0,
            };
            let line_glyphs = compute_glyph_metrics(line, &font, &emoji_font, self.letter_spacing);
            glyphs.extend(line_glyphs.into_iter().map(|g| GlyphMetric {
                x: g.x + line_x,
                width: g.width,
            }));
        }

        let (_, metrics) = font.metrics();
        let ascender = -metrics.ascent;
        let descender = metrics.descent;
        let baseline = (self.line_height_resolved + ascender - descender) / 2.0;

        Some(TextMetrics {
            text_width,
            cap_height: metrics.cap_height,
            ascender,
            baseline,
            glyphs,
        })
    }
}

pub struct ComponentTextMetrics;

impl TextMetricsProvider for ComponentTextMetrics {
    fn text_metrics(
        &self,
        payload: &(dyn std::any::Any + Send + Sync),
        content_box_width: f32,
    ) -> Option<TextMetrics> {
        if let Some(t) = payload.downcast_ref::<Text>() {
            return TextIntrinsic::from_text(t).text_metrics(content_box_width);
        }
        if let Some(g) = payload.downcast_ref::<GradientText>() {
            return GradientTextIntrinsic::from_gradient_text(g)
                .0
                .text_metrics(content_box_width);
        }
        None
    }
}

fn wrap_and_measure(
    content: &str,
    typeface: &Typeface,
    font_size: f32,
    wrap_at: Option<f32>,
    letter_spacing: f32,
    line_height: f32,
) -> (f32, f32) {
    let font = Font::from_typeface(typeface.clone(), font_size);
    let emoji_font = emoji_typeface().map(|tf| Font::from_typeface(tf, font_size));
    let lines = wrap_text_with_tracking(content, &font, &emoji_font, wrap_at, letter_spacing);
    let mut max_w = 0.0f32;
    for line in &lines {
        max_w = max_w.max(measure_text_with_fallback(
            line,
            &font,
            &emoji_font,
            letter_spacing,
        ));
    }
    let line_count = lines.len().max(1) as f32;
    (max_w, line_count * line_height)
}

#[allow(clippy::too_many_arguments)]
pub fn resolve_text_autofit(
    content: &str,
    typeface: &Typeface,
    requested_font_size: f32,
    requested_letter_spacing: f32,
    requested_line_height: f32,
    wrap: bool,
    box_width: Option<f32>,
    declared_height: Option<f32>,
) -> (f32, f32, f32) {
    if requested_font_size <= 0.0 || (box_width.is_none() && declared_height.is_none()) {
        return (
            requested_font_size,
            requested_letter_spacing,
            requested_line_height,
        );
    }
    let wrap_at = if wrap { box_width } else { None };
    let measure_at = |size: f32| -> (f32, f32) {
        let ratio = size / requested_font_size;
        wrap_and_measure(
            content,
            typeface,
            size,
            wrap_at,
            requested_letter_spacing * ratio,
            requested_line_height * ratio,
        )
    };
    let floor = TEXT_AUTOFIT_MIN_FONT_PX.min(requested_font_size);
    let final_size = shrink_to_fit(
        requested_font_size,
        floor,
        box_width,
        declared_height,
        measure_at,
    );
    if final_size >= requested_font_size {
        (
            requested_font_size,
            requested_letter_spacing,
            requested_line_height,
        )
    } else {
        let ratio = final_size / requested_font_size;
        (
            final_size,
            requested_letter_spacing * ratio,
            requested_line_height * ratio,
        )
    }
}

fn shrink_to_fit(
    requested_font_size: f32,
    floor_px: f32,
    target_width: Option<f32>,
    target_height: Option<f32>,
    mut measure_at: impl FnMut(f32) -> (f32, f32),
) -> f32 {
    let eps = 0.5;
    let fits = |w: f32, h: f32| {
        target_width.is_none_or(|tw| w <= tw + eps) && target_height.is_none_or(|th| h <= th + eps)
    };

    let (w0, h0) = measure_at(requested_font_size);
    if fits(w0, h0) {
        return requested_font_size;
    }

    let floor_px = floor_px.min(requested_font_size).max(0.1);
    if floor_px >= requested_font_size {
        return requested_font_size;
    }

    let (mut lo, mut hi) = (floor_px, requested_font_size);
    let (w_floor, h_floor) = measure_at(lo);
    if !fits(w_floor, h_floor) {
        return lo;
    }
    for _ in 0..16 {
        let mid = (lo + hi) / 2.0;
        let (w, h) = measure_at(mid);
        if fits(w, h) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

fn weight_to_u16(w: Option<&CssFontWeight>) -> u16 {
    match w {
        Some(CssFontWeight::Keyword(FontWeightKw::Bold)) => 700,
        Some(CssFontWeight::Keyword(FontWeightKw::Bolder)) => 800,
        Some(CssFontWeight::Keyword(FontWeightKw::Lighter)) => 300,
        Some(CssFontWeight::Keyword(FontWeightKw::Normal)) | None => 400,
        Some(CssFontWeight::Number(n)) => (*n).clamp(1, 1000),
    }
}

pub struct GradientTextIntrinsic(TextIntrinsic);

impl GradientTextIntrinsic {
    pub fn from_gradient_text(t: &GradientText) -> Self {
        use rustmotion_core::css::style::Size as CSize;
        use rustmotion_core::css::units::LengthPercentage;
        let max_width = match &t.style.width {
            Some(CSize::Length(LengthPercentage::Px(v))) => Some(*v),
            _ => None,
        };
        let wrap = !matches!(
            t.style.white_space,
            Some(WhiteSpace::Nowrap | WhiteSpace::Pre)
        );
        Self(
            TextIntrinsic::from_parts_with_wrap(&t.content, &t.style, max_width, wrap)
                .with_autofit(matches!(t.style.text_autofit, Some(true))),
        )
    }
}

impl IntrinsicMeasure for GradientTextIntrinsic {
    fn measure(
        &self,
        known: (Option<f32>, Option<f32>),
        available: (AvailableSpace, AvailableSpace),
    ) -> (f32, f32) {
        self.0.measure(known, available)
    }
}

pub struct CaptionIntrinsic(TextIntrinsic);

impl CaptionIntrinsic {
    pub fn from_caption(c: &Caption) -> Self {
        let joined = c
            .words
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        let wrap = !matches!(
            c.style.white_space,
            Some(WhiteSpace::Nowrap | WhiteSpace::Pre)
        );
        Self(TextIntrinsic::from_parts_with_wrap(
            &joined,
            &c.style,
            c.max_width,
            wrap,
        ))
    }
}

impl IntrinsicMeasure for CaptionIntrinsic {
    fn measure(
        &self,
        known: (Option<f32>, Option<f32>),
        available: (AvailableSpace, AvailableSpace),
    ) -> (f32, f32) {
        self.0.measure(known, available)
    }
}

pub struct KbdIntrinsic {
    text: TextIntrinsic,
    h_padding: f32,
    v_padding: f32,
    min_width: f32,
}

impl KbdIntrinsic {
    pub fn from_kbd(k: &Kbd) -> Self {
        let fs = k
            .style
            .font_size_px_ctx(&measure_time_font_size_ctx(0.0), k.font_size);
        let synthetic_style = synthesize_text_style(&k.style, fs, "SF Mono");
        Self {
            text: TextIntrinsic::from_parts_with_wrap(&k.key, &synthetic_style, None, false),
            h_padding: fs * 0.7,
            v_padding: fs * 0.4,
            min_width: fs * 1.8,
        }
    }
}

impl IntrinsicMeasure for KbdIntrinsic {
    fn measure(
        &self,
        known: (Option<f32>, Option<f32>),
        available: (AvailableSpace, AvailableSpace),
    ) -> (f32, f32) {
        let (tw, th) = self.text.measure(known, available);
        let w = (tw + self.h_padding * 2.0).max(self.min_width);
        let h = th + self.v_padding * 2.0;
        (w, h)
    }
}

pub struct CounterIntrinsic(TextIntrinsic);

impl CounterIntrinsic {
    pub fn from_counter(c: &Counter) -> Self {
        let absmax = c.from.abs().max(c.to.abs());
        let signed = if c.from < 0.0 || c.to < 0.0 {
            -absmax
        } else {
            absmax
        };
        let display = format_counter_value(signed, c.decimals, &c.separator, &c.prefix, &c.suffix);
        Self(TextIntrinsic::from_parts_with_wrap(
            &display, &c.style, None, false,
        ))
    }
}

impl IntrinsicMeasure for CounterIntrinsic {
    fn measure(
        &self,
        known: (Option<f32>, Option<f32>),
        available: (AvailableSpace, AvailableSpace),
    ) -> (f32, f32) {
        self.0.measure(known, available)
    }
}

pub struct NumberWheelIntrinsic(TextIntrinsic);

impl NumberWheelIntrinsic {
    pub fn from_number_wheel(w: &crate::number_wheel::NumberWheel) -> Self {
        let widest = (0..10)
            .map(|d| {
                let ch = char::from_digit(d, 10).expect("0..10 is a digit");
                w.value
                    .chars()
                    .map(|c| if c.is_ascii_digit() { ch } else { c })
                    .collect::<String>()
            })
            .max_by(|a, b| {
                let measure = |s: &str| {
                    TextIntrinsic::from_parts_with_wrap(s, &w.style, None, false)
                        .measure(
                            (None, None),
                            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
                        )
                        .0
                };
                measure(a)
                    .partial_cmp(&measure(b))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap_or_else(|| w.value.clone());
        Self(TextIntrinsic::from_parts_with_wrap(
            &widest, &w.style, None, false,
        ))
    }
}

impl IntrinsicMeasure for NumberWheelIntrinsic {
    fn measure(
        &self,
        known: (Option<f32>, Option<f32>),
        available: (AvailableSpace, AvailableSpace),
    ) -> (f32, f32) {
        self.0.measure(known, available)
    }
}

pub struct BadgeIntrinsic {
    text: TextIntrinsic,
    h_padding: f32,
    v_padding: f32,
    icon_extra: f32,
    font_size: f32,
}

impl BadgeIntrinsic {
    pub fn from_badge(b: &Badge) -> Self {
        let (default_fs, h_pad, v_pad, icon_size) = badge_size_params(&b.badge_size);
        let font_size = b
            .style
            .font_size_px_ctx(&measure_time_font_size_ctx(0.0), default_fs);
        let ratio = font_size / default_fs;
        let h_padding = h_pad * ratio;
        let v_padding = v_pad * ratio;
        let icon_extra = if b.icon.is_some() {
            icon_size * ratio + 6.0 * ratio
        } else {
            0.0
        };

        let synthetic_style = synthesize_text_style(&b.style, font_size, "Inter");

        Self {
            text: TextIntrinsic::from_parts_with_wrap(&b.text, &synthetic_style, None, false),
            h_padding,
            v_padding,
            icon_extra,
            font_size,
        }
    }
}

impl IntrinsicMeasure for BadgeIntrinsic {
    fn measure(
        &self,
        known: (Option<f32>, Option<f32>),
        available: (AvailableSpace, AvailableSpace),
    ) -> (f32, f32) {
        let (tw, _th) = self.text.measure(known, available);
        let w = self.h_padding * 2.0 + tw + self.icon_extra;
        let h = self.v_padding * 2.0 + self.font_size * 1.3;
        (w, h)
    }
}

fn badge_size_params(s: &BadgeSize) -> (f32, f32, f32, f32) {
    match s {
        BadgeSize::Sm => (12.0, 8.0, 4.0, 14.0),
        BadgeSize::Md => (14.0, 12.0, 6.0, 18.0),
        BadgeSize::Lg => (18.0, 16.0, 8.0, 22.0),
    }
}

fn synthesize_text_style(src: &CssStyle, font_size: f32, default_family: &str) -> CssStyle {
    use rustmotion_core::css::Length;
    let family = src
        .font_family
        .clone()
        .unwrap_or_else(|| default_family.to_string());
    CssStyle {
        font_size: Some(Length::Px(font_size)),
        font_family: Some(family),
        font_weight: src.font_weight.clone(),
        font_style: src.font_style,
        letter_spacing: src.letter_spacing.clone(),
        line_height: src.line_height.clone(),
        ..CssStyle::default()
    }
}

#[allow(dead_code)]
fn _line_height_unused(_: Option<&LineHeight>) {}

use crate::table::{Table, DEFAULT_FONT_SIZE as TABLE_FONT_SIZE, DEFAULT_ROW_HEIGHT_RATIO};

pub struct TableIntrinsic {
    row_height: f32,
    row_count: usize,
    total_width: f32,
}

impl TableIntrinsic {
    pub fn from_table(t: &Table) -> Self {
        let font_size = t
            .style
            .font_size_px_ctx(&measure_time_font_size_ctx(0.0), TABLE_FONT_SIZE);
        let row_height = font_size * DEFAULT_ROW_HEIGHT_RATIO;

        let total_width = Self::compute_width(t, font_size);

        Self {
            row_height,
            row_count: t.rows.len(),
            total_width,
        }
    }

    fn compute_width(t: &Table, font_size: f32) -> f32 {
        if let Some(widths) = &t.column_widths {
            if !widths.is_empty() {
                return widths.iter().sum();
            }
        }
        t.natural_column_widths(font_size).iter().sum()
    }
}

impl IntrinsicMeasure for TableIntrinsic {
    fn measure(
        &self,
        known: (Option<f32>, Option<f32>),
        _available: (AvailableSpace, AvailableSpace),
    ) -> (f32, f32) {
        let w = known.0.unwrap_or(self.total_width);
        let h = known
            .1
            .unwrap_or((1 + self.row_count) as f32 * self.row_height);
        (w, h)
    }
}

use crate::rich_text::{RichText, RichTextSpan};

pub struct RichTextIntrinsic {
    spans: Vec<RichTextSpan>,
    style: CssStyle,
    max_width: Option<f32>,
}

impl RichTextIntrinsic {
    pub fn from_rich_text(rt: &RichText) -> Self {
        Self {
            spans: rt.spans.clone(),
            style: rt.style.clone(),
            max_width: rt.max_width,
        }
    }
}

impl IntrinsicMeasure for RichTextIntrinsic {
    fn measure(
        &self,
        known: (Option<f32>, Option<f32>),
        available: (AvailableSpace, AvailableSpace),
    ) -> (f32, f32) {
        let max_width = if let Some(w) = known.0 {
            Some(w)
        } else {
            let avail_w = match available.0 {
                AvailableSpace::Definite(w) => Some(w),
                AvailableSpace::MaxContent => None,
                AvailableSpace::MinContent => Some(0.0),
            };
            match (self.max_width, avail_w) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            }
        };

        let layout =
            RichText::compute_layout(&self.spans, &self.style, 1920.0, 1080.0, max_width, -1.0);
        let line_count = layout.lines.len().max(1) as f32;
        (layout.max_width, line_count * layout.line_height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmotion_core::css::style::CssStyle;
    use rustmotion_core::css::Length;
    use rustmotion_core::engine::box_tree::AvailableSpace;

    #[test]
    fn measure_returns_positive_size_for_non_empty_text() {
        let text = Text {
            content: "Hello World".into(),
            max_width: None,
            timing: Default::default(),
            style: CssStyle {
                font_size: Some(Length::Px(32.0)),
                ..Default::default()
            },
            timeline: Vec::new(),
            stagger: None,
            text_shadow: None,
            stroke: None,
            text_background: None,
            caret: None,
            states: Vec::new(),
            swap: None,
        };
        let m = TextIntrinsic::from_text(&text);
        let (w, h) = m.measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        );
        assert!(w > 0.0, "width should be > 0, got {}", w);
        assert!(
            h > 30.0,
            "height should be roughly font_size * line_height, got {}",
            h
        );
    }

    #[test]
    fn wrapping_grows_height_when_max_width_constrained() {
        let text = Text {
            content: "the quick brown fox jumps over the lazy dog".into(),
            max_width: None,
            timing: Default::default(),
            style: CssStyle {
                font_size: Some(Length::Px(20.0)),
                ..Default::default()
            },
            timeline: Vec::new(),
            stagger: None,
            text_shadow: None,
            stroke: None,
            text_background: None,
            caret: None,
            states: Vec::new(),
            swap: None,
        };
        let m = TextIntrinsic::from_text(&text);
        let (_w_unwrapped, h_unwrapped) = m.measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        );
        let (_w_wrapped, h_wrapped) = m.measure(
            (None, None),
            (AvailableSpace::Definite(80.0), AvailableSpace::MaxContent),
        );
        assert!(
            h_wrapped > h_unwrapped,
            "wrapped height ({}) should exceed unwrapped ({})",
            h_wrapped,
            h_unwrapped,
        );
    }

    #[test]
    fn empty_text_has_zero_width_but_one_line_height() {
        let text = Text {
            content: "".into(),
            max_width: None,
            timing: Default::default(),
            style: CssStyle {
                font_size: Some(Length::Px(24.0)),
                ..Default::default()
            },
            timeline: Vec::new(),
            stagger: None,
            text_shadow: None,
            stroke: None,
            text_background: None,
            caret: None,
            states: Vec::new(),
            swap: None,
        };
        let m = TextIntrinsic::from_text(&text);
        let (w, h) = m.measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        );
        assert_eq!(w, 0.0);
        assert!(h > 0.0);
    }

    fn nowrap_text(content: &str, white_space: Option<WhiteSpace>) -> Text {
        Text {
            content: content.into(),
            max_width: None,
            timing: Default::default(),
            style: CssStyle {
                font_size: Some(Length::Px(20.0)),
                white_space,
                ..Default::default()
            },
            timeline: Vec::new(),
            stagger: None,
            text_shadow: None,
            stroke: None,
            text_background: None,
            caret: None,
            states: Vec::new(),
            swap: None,
        }
    }

    #[test]
    fn nowrap_ignores_a_constrained_width_and_stays_one_line() {
        let text = nowrap_text(
            "the quick brown fox jumps over the lazy dog",
            Some(WhiteSpace::Nowrap),
        );
        let m = TextIntrinsic::from_text(&text);
        let (w_unconstrained, h_unconstrained) = m.measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        );
        let (w_constrained, h_constrained) = m.measure(
            (None, None),
            (AvailableSpace::Definite(80.0), AvailableSpace::MaxContent),
        );
        assert!(
            w_constrained > 80.0,
            "nowrap must ignore the 80px constraint, got width {}",
            w_constrained
        );
        assert_eq!(
            w_constrained, w_unconstrained,
            "nowrap width must equal the natural (unconstrained) width regardless of available space"
        );
        assert_eq!(
            h_constrained, h_unconstrained,
            "nowrap must always report a single line's height, constrained or not"
        );
    }

    #[test]
    fn pre_disables_wrap_exactly_like_nowrap() {
        let text = nowrap_text("this string is too long to fit", Some(WhiteSpace::Pre));
        let m = TextIntrinsic::from_text(&text);
        let (w, _h) = m.measure(
            (None, None),
            (AvailableSpace::Definite(80.0), AvailableSpace::MaxContent),
        );
        assert!(
            w > 80.0,
            "white-space: pre must also ignore the width constraint, got {}",
            w
        );
    }

    #[test]
    fn normal_white_space_still_wraps_at_a_constrained_width() {
        let wrapped = nowrap_text(
            "the quick brown fox jumps over the lazy dog",
            Some(WhiteSpace::Normal),
        );
        let unset = nowrap_text("the quick brown fox jumps over the lazy dog", None);
        for text in [wrapped, unset] {
            let m = TextIntrinsic::from_text(&text);
            let (w, _h) = m.measure(
                (None, None),
                (AvailableSpace::Definite(80.0), AvailableSpace::MaxContent),
            );
            assert!(
                w <= 80.0 + 0.5,
                "white-space: normal (or unset) must still wrap at an 80px constraint, got {}",
                w
            );
        }
    }

    fn span(text: &str) -> RichTextSpan {
        RichTextSpan {
            text: text.into(),
            color: None,
            font_size: None,
            font_weight: None,
            font_family: None,
            font_style: None,
            letter_spacing: None,
        }
    }

    #[test]
    fn rich_text_intrinsic_is_non_zero_without_explicit_size() {
        let spans = vec![span("Hello "), span("world")];
        let style = CssStyle {
            font_size: Some(Length::Px(32.0)),
            ..Default::default()
        };
        let intrinsic = RichTextIntrinsic {
            spans,
            style,
            max_width: None,
        };
        let (w, h) = intrinsic.measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        );
        assert!(w > 0.0, "rich_text natural width must be > 0, got {}", w);
        assert!(h > 0.0, "rich_text natural height must be > 0, got {}", h);
    }

    #[test]
    fn rich_text_intrinsic_wraps_a_single_long_span_internally() {
        let spans = vec![span(
            "the quick brown fox jumps over the lazy dog and keeps going",
        )];
        let style = CssStyle {
            font_size: Some(Length::Px(24.0)),
            ..Default::default()
        };
        let intrinsic = RichTextIntrinsic {
            spans,
            style,
            max_width: None,
        };
        let (_w_unconstrained, h_unconstrained) = intrinsic.measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        );
        let (w_constrained, h_constrained) = intrinsic.measure(
            (None, None),
            (AvailableSpace::Definite(150.0), AvailableSpace::MaxContent),
        );
        assert!(
            w_constrained <= 150.0 + 0.5,
            "wrapped width must fit the 150px constraint, got {}",
            w_constrained
        );
        assert!(
            h_constrained > h_unconstrained,
            "constraining width must add lines (wrap within the single span): {} vs {}",
            h_constrained,
            h_unconstrained
        );
    }

    #[test]
    fn rich_text_intrinsic_matches_compute_layout_used_by_the_painter() {
        let spans = vec![span("Total: "), span("42"), span(" items")];
        let style = CssStyle::default();
        let intrinsic = RichTextIntrinsic {
            spans: spans.clone(),
            style: style.clone(),
            max_width: None,
        };
        let (w, h) = intrinsic.measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        );
        let layout = RichText::compute_layout(&spans, &style, 1920.0, 1080.0, None, -1.0);
        assert_eq!(w, layout.max_width);
        assert_eq!(h, layout.lines.len().max(1) as f32 * layout.line_height);
    }

    #[test]
    fn gradient_text_intrinsic_ignores_constrained_width_when_nowrap() {
        let gt = GradientText {
            content: "the quick brown fox jumps over the lazy dog".into(),
            colors: vec!["#3B82F6".into(), "#8B5CF6".into()],
            angle: 90.0,
            animate_angle: false,
            speed: 0.5,
            timing: Default::default(),
            style: CssStyle {
                font_size: Some(Length::Px(20.0)),
                white_space: Some(WhiteSpace::Nowrap),
                ..Default::default()
            },
            timeline: Vec::new(),
            stagger: None,
        };
        let m = GradientTextIntrinsic::from_gradient_text(&gt);
        let (w, h) = m.measure(
            (None, None),
            (AvailableSpace::Definite(80.0), AvailableSpace::MaxContent),
        );
        assert!(
            w > 80.0,
            "nowrap gradient_text must ignore the 80px constraint, got {}",
            w
        );
        let (_, h_unconstrained) = m.measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        );
        assert_eq!(h, h_unconstrained);
    }

    #[test]
    fn gradient_text_intrinsic_wraps_by_default() {
        let gt = GradientText {
            content: "the quick brown fox jumps over the lazy dog".into(),
            colors: vec!["#3B82F6".into(), "#8B5CF6".into()],
            angle: 90.0,
            animate_angle: false,
            speed: 0.5,
            timing: Default::default(),
            style: CssStyle {
                font_size: Some(Length::Px(20.0)),
                ..Default::default()
            },
            timeline: Vec::new(),
            stagger: None,
        };
        let m = GradientTextIntrinsic::from_gradient_text(&gt);
        let (_w, h_unconstrained) = m.measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        );
        let (w_constrained, h_constrained) = m.measure(
            (None, None),
            (AvailableSpace::Definite(80.0), AvailableSpace::MaxContent),
        );
        assert!(w_constrained <= 80.0 + 0.5);
        assert!(h_constrained > h_unconstrained);
    }

    #[test]
    fn caption_intrinsic_ignores_constrained_width_when_nowrap() {
        let caption = Caption {
            words: "the quick brown fox jumps over the lazy dog"
                .split_whitespace()
                .map(|w| rustmotion_core::schema::CaptionWord {
                    text: w.to_string(),
                    start: 0.0,
                    end: 10.0,
                })
                .collect(),
            active_color: "#FFFF00".into(),
            mode: Default::default(),
            max_width: Some(80.0),
            pill_color: None,
            style: CssStyle {
                font_size: Some(Length::Px(20.0)),
                white_space: Some(WhiteSpace::Nowrap),
                ..Default::default()
            },
            timing: Default::default(),
            timeline: Vec::new(),
            stagger: None,
        };
        let m = CaptionIntrinsic::from_caption(&caption);
        let (w, _h) = m.measure(
            (None, None),
            (AvailableSpace::Definite(80.0), AvailableSpace::MaxContent),
        );
        assert!(
            w > 80.0,
            "nowrap caption intrinsic must ignore the 80px constraint, got {}",
            w
        );
    }

    fn text_with_style(content: &str, style: CssStyle) -> Text {
        Text {
            content: content.into(),
            max_width: None,
            timing: Default::default(),
            style,
            timeline: Vec::new(),
            stagger: None,
            text_shadow: None,
            stroke: None,
            text_background: None,
            caret: None,
            states: Vec::new(),
            swap: None,
        }
    }

    #[test]
    fn line_height_percent_no_longer_collapses_the_box_to_zero_height() {
        use rustmotion_core::css::units::LengthPercentage;
        let text = text_with_style(
            "VISIBLE?",
            CssStyle {
                font_size: Some(Length::Px(60.0)),
                line_height: Some(LineHeight::Length(LengthPercentage::String("150%".into()))),
                ..Default::default()
            },
        );
        let m = TextIntrinsic::from_text(&text);
        let (_w, h) = m.measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        );
        assert!(
            (h - 90.0).abs() < 0.5,
            "line-height: 150% of a 60px font-size must resolve to 90px (own font-size, per \
             CSS), got {h}"
        );
    }

    #[test]
    fn line_height_em_no_longer_collapses_the_box_to_zero_height() {
        use rustmotion_core::css::units::LengthPercentage;
        let text = text_with_style(
            "VISIBLE?",
            CssStyle {
                font_size: Some(Length::Px(60.0)),
                line_height: Some(LineHeight::Length(LengthPercentage::String("1.5em".into()))),
                ..Default::default()
            },
        );
        let m = TextIntrinsic::from_text(&text);
        let (_w, h) = m.measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        );
        assert!(
            (h - 90.0).abs() < 0.5,
            "line-height: 1.5em of a 60px font-size must resolve to 90px, got {h}"
        );
        let numeric = text_with_style(
            "VISIBLE?",
            CssStyle {
                font_size: Some(Length::Px(60.0)),
                line_height: Some(LineHeight::Number(1.5)),
                ..Default::default()
            },
        );
        let (_w, h_numeric) = TextIntrinsic::from_text(&numeric).measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        );
        assert_eq!(h, h_numeric);
    }

    #[test]
    fn letter_spacing_em_matches_the_equivalent_px_measurement() {
        let em_style = CssStyle {
            font_size: Some(Length::Px(200.0)),
            letter_spacing: Some(Length::String("1.2em".into())),
            white_space: Some(WhiteSpace::Nowrap),
            ..Default::default()
        };
        let px_style = CssStyle {
            font_size: Some(Length::Px(200.0)),
            letter_spacing: Some(Length::Px(240.0)),
            white_space: Some(WhiteSpace::Nowrap),
            ..Default::default()
        };
        let w_em = TextIntrinsic::from_text(&text_with_style("TRACKING", em_style))
            .measure(
                (None, None),
                (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
            )
            .0;
        let w_px = TextIntrinsic::from_text(&text_with_style("TRACKING", px_style))
            .measure(
                (None, None),
                (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
            )
            .0;
        assert!(
            (w_em - w_px).abs() < 1.0,
            "letter-spacing: 1.2em (font-size 200) must measure the same as the equivalent \
             240px value: em={w_em}, px={w_px}"
        );
        let w_zero_tracking = TextIntrinsic::from_text(&text_with_style(
            "TRACKING",
            CssStyle {
                font_size: Some(Length::Px(200.0)),
                white_space: Some(WhiteSpace::Nowrap),
                ..Default::default()
            },
        ))
        .measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        )
        .0;
        assert!(
            w_em > w_zero_tracking + 100.0,
            "em tracking must measurably widen the line versus zero tracking: em={w_em}, \
             zero={w_zero_tracking}"
        );
    }

    #[test]
    fn shrink_to_fit_is_a_noop_when_content_already_fits() {
        let calls = std::cell::RefCell::new(Vec::new());
        let size = shrink_to_fit(48.0, 12.0, Some(200.0), Some(100.0), |s| {
            calls.borrow_mut().push(s);
            (150.0, 80.0)
        });
        assert_eq!(size, 48.0);
        assert_eq!(
            *calls.borrow(),
            vec![48.0],
            "must measure only once (at the requested size) when it already fits"
        );
    }

    #[test]
    fn shrink_to_fit_is_a_noop_when_nothing_to_fit_against() {
        let size = shrink_to_fit(48.0, 12.0, None, None, |_| (99999.0, 99999.0));
        assert_eq!(size, 48.0);
    }

    #[test]
    fn shrink_to_fit_finds_a_size_that_fits_the_width_target() {
        let target = 100.0;
        let size = shrink_to_fit(120.0, 5.0, Some(target), None, |s| (s * 2.0, 10.0));
        assert!(size < 120.0, "must have shrunk, got {size}");
        assert!(size * 2.0 <= target + 0.5, "resolved size must fit: {size}");
        assert!(
            (size + 1.0) * 2.0 > target + 0.5,
            "resolved size should be close to the fitting boundary, got {size}"
        );
    }

    #[test]
    fn shrink_to_fit_respects_both_axes_jointly() {
        let size = shrink_to_fit(100.0, 5.0, Some(1000.0), Some(20.0), |s| (s, s * 2.0));
        assert!(size * 2.0 <= 20.5, "must respect the height target: {size}");
        assert!(
            (size + 0.5) * 2.0 > 20.5,
            "should converge close to the height boundary, got {size}"
        );
    }

    #[test]
    fn shrink_to_fit_never_returns_below_the_floor() {
        let size = shrink_to_fit(120.0, 20.0, Some(10.0), None, |s| (s * 5.0, 10.0));
        assert_eq!(size, 20.0, "must stop exactly at the floor, not lower");
    }

    #[test]
    fn shrink_to_fit_is_deterministic_across_repeated_calls() {
        let run = || shrink_to_fit(90.0, 10.0, Some(137.0), Some(64.0), |s| (s * 1.7, s * 0.9));
        let a = run();
        let b = run();
        assert_eq!(a, b);
    }

    fn inter_typeface() -> Typeface {
        typeface_with_fallback("Inter", SkFontStyle::normal()).expect("Inter resolves in tests")
    }

    #[test]
    fn resolve_text_autofit_shrinks_to_fit_a_width_target() {
        let typeface = inter_typeface();
        let content = "A very long headline that will not fit in this box";
        let requested = 80.0;
        let box_width = 300.0;
        let (fs, ls, lh) = resolve_text_autofit(
            content,
            &typeface,
            requested,
            0.0,
            requested * 1.3,
            false,
            Some(box_width),
            None,
        );
        assert!(fs < requested, "must shrink, got {fs}");
        assert!(
            fs >= TEXT_AUTOFIT_MIN_FONT_PX - 0.01,
            "must not shrink past the calibrated floor, got {fs}"
        );
        let (w, _) = wrap_and_measure(content, &typeface, fs, None, ls, lh);
        assert!(
            w <= box_width + 0.5,
            "resolved size must actually fit: w={w}, target={box_width}"
        );
    }

    #[test]
    fn resolve_text_autofit_is_a_noop_when_it_already_fits() {
        let typeface = inter_typeface();
        let (fs, ls, lh) = resolve_text_autofit(
            "hi",
            &typeface,
            24.0,
            1.0,
            30.0,
            true,
            Some(1000.0),
            Some(1000.0),
        );
        assert_eq!(fs, 24.0);
        assert_eq!(ls, 1.0);
        assert_eq!(lh, 30.0);
    }

    #[test]
    fn resolve_text_autofit_never_goes_below_the_calibrated_floor() {
        let typeface = inter_typeface();
        let (fs, _, _) = resolve_text_autofit(
            "This sentence is far too long for a ten pixel wide box",
            &typeface,
            80.0,
            0.0,
            104.0,
            true,
            Some(10.0),
            Some(10.0),
        );
        assert!(
            (fs - TEXT_AUTOFIT_MIN_FONT_PX).abs() < 0.01,
            "expected exactly the floor ({TEXT_AUTOFIT_MIN_FONT_PX}), got {fs}"
        );
    }

    #[test]
    fn resolve_text_autofit_rescales_letter_spacing_and_line_height_proportionally() {
        let typeface = inter_typeface();
        let (fs, ls, lh) = resolve_text_autofit(
            "SHRINK ME PLEASE, THIS LINE IS QUITE LONG",
            &typeface,
            100.0,
            5.0,
            130.0,
            false,
            Some(150.0),
            None,
        );
        assert!(fs < 100.0, "sanity: must have shrunk, got {fs}");
        let ratio = fs / 100.0;
        assert!((ls - 5.0 * ratio).abs() < 1e-3);
        assert!((lh - 130.0 * ratio).abs() < 1e-3);
    }

    fn autofit_text(content: &str, font_size: f32) -> Text {
        Text {
            content: content.into(),
            max_width: None,
            timing: Default::default(),
            style: CssStyle {
                font_size: Some(Length::Px(font_size)),
                text_autofit: Some(true),
                white_space: Some(WhiteSpace::Nowrap),
                ..Default::default()
            },
            timeline: Vec::new(),
            stagger: None,
            text_shadow: None,
            stroke: None,
            text_background: None,
            caret: None,
            states: Vec::new(),
            swap: None,
        }
    }

    #[test]
    fn text_intrinsic_shrinks_when_autofit_is_on_and_the_box_is_too_narrow() {
        let text = autofit_text("the quick brown fox jumps over the lazy dog", 60.0);
        let m = TextIntrinsic::from_text(&text);
        let (w_unconstrained, _) = m.measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        );
        let target = w_unconstrained / 2.0;
        let (w_constrained, _) = m.measure(
            (None, None),
            (AvailableSpace::Definite(target), AvailableSpace::MaxContent),
        );
        assert!(
            w_constrained <= target + 0.5,
            "autofit must shrink the nowrap line to fit {target}px, got {w_constrained}"
        );
        assert!(
            w_constrained < w_unconstrained,
            "must have actually shrunk from the natural width ({w_unconstrained}), got {w_constrained}"
        );
    }

    #[test]
    fn text_intrinsic_ignores_autofit_target_when_the_flag_is_off() {
        let mut text = autofit_text("the quick brown fox jumps over the lazy dog", 60.0);
        text.style.text_autofit = None;
        let m = TextIntrinsic::from_text(&text);
        let (w, _) = m.measure(
            (None, None),
            (AvailableSpace::Definite(200.0), AvailableSpace::MaxContent),
        );
        assert!(
            w > 200.0,
            "without text-autofit, nowrap must still bleed past the box exactly as before, got {w}"
        );
    }

    #[test]
    fn text_intrinsic_autofit_still_overflows_when_even_the_floor_does_not_fit() {
        let text = autofit_text(
            "This is an extremely long sentence that will not fit no matter how much the font shrinks",
            80.0,
        );
        let m = TextIntrinsic::from_text(&text);
        let (w, _) = m.measure(
            (None, None),
            (AvailableSpace::Definite(5.0), AvailableSpace::MaxContent),
        );
        assert!(
            w > 5.0,
            "must not silently report a fit that never actually happened, got {w}"
        );
    }

    #[test]
    fn caption_intrinsic_never_autofits_even_if_style_declares_it() {
        let caption = Caption {
            words: "the quick brown fox jumps over the lazy dog"
                .split_whitespace()
                .map(|w| rustmotion_core::schema::CaptionWord {
                    text: w.to_string(),
                    start: 0.0,
                    end: 10.0,
                })
                .collect(),
            active_color: "#FFFF00".into(),
            mode: Default::default(),
            max_width: None,
            pill_color: None,
            style: CssStyle {
                font_size: Some(Length::Px(60.0)),
                text_autofit: Some(true),
                white_space: Some(WhiteSpace::Nowrap),
                ..Default::default()
            },
            timing: Default::default(),
            timeline: Vec::new(),
            stagger: None,
        };
        let m = CaptionIntrinsic::from_caption(&caption);
        let (w_unconstrained, _) = m.measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        );
        let (w_constrained, _) = m.measure(
            (None, None),
            (AvailableSpace::Definite(200.0), AvailableSpace::MaxContent),
        );
        assert_eq!(
            w_constrained, w_unconstrained,
            "caption must ignore text-autofit entirely (nowrap bleeds exactly as before)"
        );
    }

    fn plain_text(content: &str, font_size: f32) -> Text {
        Text {
            content: content.into(),
            max_width: None,
            timing: Default::default(),
            style: CssStyle {
                font_size: Some(Length::Px(font_size)),
                ..Default::default()
            },
            timeline: Vec::new(),
            stagger: None,
            text_shadow: None,
            stroke: None,
            text_background: None,
            caret: None,
            states: Vec::new(),
            swap: None,
        }
    }

    #[test]
    fn text_metrics_reports_glyph_count_matching_content_for_one_line() {
        let text = plain_text("Sentence", 32.0);
        let metrics = TextIntrinsic::from_text(&text)
            .text_metrics(500.0)
            .expect("host must have a fallback typeface");
        assert_eq!(metrics.glyphs.len(), "Sentence".chars().count());
        assert!(metrics.text_width > 0.0);
        assert!(metrics.cap_height > 0.0);
        assert!(metrics.ascender > 0.0);
    }

    #[test]
    fn text_metrics_last_glyph_sits_before_where_a_detached_char_would_go() {
        let text = plain_text("Sentence", 32.0);
        let metrics = TextIntrinsic::from_text(&text).text_metrics(500.0).unwrap();
        let last = *metrics.glyphs.last().unwrap();
        let detached_question_mark_x = last.x + last.width;
        assert!(detached_question_mark_x > last.x);
        assert!(detached_question_mark_x <= metrics.text_width + 0.5);
    }

    #[test]
    fn text_metrics_width_matches_measure_when_unwrapped() {
        let text = plain_text("no wrap needed", 24.0);
        let intrinsic = TextIntrinsic::from_text(&text);
        let (measured_w, _measured_h) = intrinsic.measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        );
        let metrics = intrinsic.text_metrics(10_000.0).unwrap();
        assert!(
            (metrics.text_width - measured_w).abs() < 0.5,
            "text_metrics width {} should match measure() width {}",
            metrics.text_width,
            measured_w
        );
    }

    #[test]
    fn text_metrics_centers_glyphs_when_text_align_is_center() {
        let mut text = plain_text("Hi", 32.0);
        text.style.text_align = Some(rustmotion_core::css::style::TextAlign::Center);
        let intrinsic = TextIntrinsic::from_text(&text);
        let centered = intrinsic.text_metrics(400.0).unwrap();
        let left = plain_text("Hi", 32.0);
        let left_metrics = TextIntrinsic::from_text(&left).text_metrics(400.0).unwrap();
        assert!(
            centered.glyphs[0].x > left_metrics.glyphs[0].x,
            "centered first glyph ({}) should start further right than left-aligned ({})",
            centered.glyphs[0].x,
            left_metrics.glyphs[0].x
        );
    }

    #[test]
    fn component_text_metrics_downcasts_text() {
        let text = plain_text("Hello", 28.0);
        let provider = ComponentTextMetrics;
        let payload: &(dyn std::any::Any + Send + Sync) = &text;
        let metrics = provider
            .text_metrics(payload, 500.0)
            .expect("Text must resolve through ComponentTextMetrics");
        assert_eq!(metrics.glyphs.len(), "Hello".chars().count());
    }

    #[test]
    fn component_text_metrics_downcasts_gradient_text() {
        let gt = GradientText {
            content: "Gradient".into(),
            colors: vec!["#3B82F6".into(), "#8B5CF6".into()],
            angle: 90.0,
            animate_angle: false,
            speed: 0.5,
            timing: Default::default(),
            style: CssStyle {
                font_size: Some(Length::Px(28.0)),
                ..Default::default()
            },
            timeline: Vec::new(),
            stagger: None,
        };
        let provider = ComponentTextMetrics;
        let payload: &(dyn std::any::Any + Send + Sync) = &gt;
        let metrics = provider
            .text_metrics(payload, 500.0)
            .expect("GradientText must resolve through ComponentTextMetrics");
        assert_eq!(metrics.glyphs.len(), "Gradient".chars().count());
    }

    #[test]
    fn component_text_metrics_returns_none_for_a_non_text_component() {
        let payload: &(dyn std::any::Any + Send + Sync) = &42i32;
        assert!(ComponentTextMetrics.text_metrics(payload, 500.0).is_none());
    }
}
