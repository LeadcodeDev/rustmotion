//! Intrinsic measurers for components whose box size depends on content.
//!
//! Uses the same Skia metrics that the painter uses, so the box reserved by
//! taffy matches the pixels actually drawn — measure-vs-paint mismatches
//! would otherwise cause text to wrap onto an extra line at paint time and
//! overflow into the next sibling.

use skia_safe::{Font, FontStyle as SkFontStyle};

use rustmotion_core::css::style::{
    CssStyle, FontStyle as CssFontStyle, FontWeight as CssFontWeight, FontWeightKw, LineHeight,
};
use rustmotion_core::engine::box_tree::{AvailableSpace, IntrinsicMeasure};
use rustmotion_core::engine::renderer::{
    emoji_typeface, format_counter_value, measure_text_with_fallback, typeface_with_fallback,
    wrap_text_with_fallback,
};

use crate::badge::{Badge, BadgeSize};
use crate::caption::Caption;
use crate::counter::Counter;
use crate::gradient_text::GradientText;
use crate::kbd::Kbd;
use crate::text::Text;

/// Cosmic-text–backed intrinsic measurer for [`Text`].
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
}

impl TextIntrinsic {
    pub fn from_text(text: &Text) -> Self {
        Self::from_parts(&text.content, &text.style, text.max_width)
    }

    pub fn from_parts(content: &str, style: &CssStyle, max_width: Option<f32>) -> Self {
        let font_size = style.font_size_px_or(48.0);
        let line_height_resolved = style.line_height_for(font_size);
        Self {
            content: content.to_string(),
            font_family: style.font_family.clone(),
            font_size,
            line_height_resolved,
            weight: weight_to_u16(style.font_weight.as_ref()),
            italic: matches!(style.font_style, Some(CssFontStyle::Italic)),
            letter_spacing: style.letter_spacing_px(),
            max_width,
            // CssStyle has no `wrap` field — use white-space/overflow-wrap heuristics.
            // For now, default to true (CSS default).
            wrap: true,
        }
    }

    /// Build with an explicit `wrap` override (used by atomic components like
    /// counter, kbd, badge that never wrap).
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

        let Some(font) = self.skia_font() else {
            return (0.0, 0.0);
        };
        let emoji_font = emoji_typeface().map(|tf| Font::from_typeface(tf, self.font_size));

        let wrap_at = if self.wrap { max_width } else { None };
        let lines = wrap_text_with_fallback(&self.content, &font, &emoji_font, wrap_at);

        let mut max_w = 0.0f32;
        for line in &lines {
            let w = measure_text_with_fallback(line, &font, &emoji_font, self.letter_spacing);
            max_w = max_w.max(w);
        }
        let line_count = lines.len().max(1) as f32;
        (max_w, line_count * self.line_height_resolved)
    }
}

impl TextIntrinsic {
    fn skia_font(&self) -> Option<Font> {
        let slant = if self.italic {
            skia_safe::font_style::Slant::Italic
        } else {
            skia_safe::font_style::Slant::Upright
        };
        let weight = skia_safe::font_style::Weight::from(self.weight as i32);
        let sk_style = SkFontStyle::new(weight, skia_safe::font_style::Width::NORMAL, slant);
        let family = self.font_family.as_deref().unwrap_or("Inter");
        let typeface = typeface_with_fallback(family, sk_style).ok()?;
        Some(Font::from_typeface(typeface, self.font_size))
    }
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

/// Cosmic-text–backed intrinsic measurer for [`GradientText`] — same content
/// model as [`Text`] (a single string + style); the gradient is purely a
/// paint-time concern and doesn't change box dimensions.
pub struct GradientTextIntrinsic(TextIntrinsic);

impl GradientTextIntrinsic {
    pub fn from_gradient_text(t: &GradientText) -> Self {
        // max_width comes from CSS style.width if set as a fixed pixel value
        use rustmotion_core::css::style::Size as CSize;
        use rustmotion_core::css::units::LengthPercentage;
        let max_width = match &t.style.width {
            Some(CSize::Length(LengthPercentage::Px(v))) => Some(*v),
            _ => None,
        };
        Self(TextIntrinsic::from_parts(&t.content, &t.style, max_width))
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

/// Intrinsic measurer for [`Caption`]. Concatenates the words with single
/// spaces and measures the result like a regular text run.
pub struct CaptionIntrinsic(TextIntrinsic);

impl CaptionIntrinsic {
    pub fn from_caption(c: &Caption) -> Self {
        let joined = c
            .words
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        Self(TextIntrinsic::from_parts(&joined, &c.style, c.max_width))
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

/// Intrinsic measurer for [`Kbd`] — measures the key text plus the legacy
/// keyboard-cap padding (h ≈ font_size × 0.7, v ≈ font_size × 0.4) and
/// enforces a min-width of `font_size × 1.8`.
pub struct KbdIntrinsic {
    text: TextIntrinsic,
    h_padding: f32,
    v_padding: f32,
    min_width: f32,
}

impl KbdIntrinsic {
    pub fn from_kbd(k: &Kbd) -> Self {
        let fs = k.style.font_size_px_or(k.font_size);
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

/// Intrinsic measurer for [`Counter`] — reserves space for the largest absolute
/// value the counter will display so layout never reflows during animation.
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
        // Counter is atomic: it never wraps.
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

/// Intrinsic measurer for [`Badge`] — measures the label text plus icon, gap,
/// and the size-derived horizontal/vertical padding.
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
        let font_size = b.style.font_size_px_or(default_fs);
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
    // (font_size, h_padding, v_padding, icon_size) — matches badge.rs::params
    match s {
        BadgeSize::Sm => (12.0, 8.0, 4.0, 14.0),
        BadgeSize::Md => (14.0, 12.0, 6.0, 18.0),
        BadgeSize::Lg => (18.0, 16.0, 8.0, 22.0),
    }
}

/// Build a CssStyle for text measurement carrying just the typography fields
/// from `src`, with a forced `font-size` and `font-family` fallback.
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

// Compatibility shim: keep an unused fn so old callers that referenced
// `LineHeight::Number` style helpers compile cleanly.
#[allow(dead_code)]
fn _line_height_unused(_: Option<&LineHeight>) {}

// ─────────────────────────────────────────────────────────────────────────────
// Terminal intrinsic measurer
// ─────────────────────────────────────────────────────────────────────────────

use crate::terminal::{
    Terminal, CHROME_HEIGHT, FONT_SIZE as TERM_FONT_SIZE, LINE_HEIGHT as TERM_LINE_HEIGHT,
    PADDING as TERM_PADDING,
};

/// Intrinsic measurer for [`Terminal`].
///
/// Natural size formula (matches the painter exactly):
/// - `line_height = ceil(font_size × TERM_LINE_HEIGHT / TERM_FONT_SIZE)`
/// - `height = chrome_height + 2 × TERM_PADDING + n_lines × line_height`
/// - `width` = widest line text (prefix + content) + 2 × TERM_PADDING
///
/// If the Skia font fails to load, returns (0, 0) so layout falls back to
/// whatever container constraints supply.
pub struct TerminalIntrinsic {
    line_height: f32,
    n_lines: usize,
    chrome_height: f32,
    padding: f32,
    /// Maximum measured text width across all lines (including prefix).
    max_line_width: f32,
}

impl TerminalIntrinsic {
    pub fn from_terminal(t: &Terminal) -> Self {
        let font_size = t.style.font_size_px_or(TERM_FONT_SIZE);
        let line_height = (font_size * TERM_LINE_HEIGHT / TERM_FONT_SIZE).ceil();
        let chrome_height = if t.show_chrome { CHROME_HEIGHT } else { 0.0 };

        // Measure each line (prefix + text) with the same Skia font the painter uses.
        let max_line_width = Self::measure_max_width(t, font_size);

        Self {
            line_height,
            n_lines: t.lines.len(),
            chrome_height,
            padding: TERM_PADDING,
            max_line_width,
        }
    }

    fn measure_max_width(t: &Terminal, font_size: f32) -> f32 {
        let font_style = skia_safe::FontStyle::normal();
        let Ok(typeface) = typeface_with_fallback("SF Mono", font_style) else {
            // Font unavailable (CI without fonts); return 0 — the layout will
            // be width-unconstrained and the container drives the size.
            return 0.0;
        };
        let font = Font::from_typeface(typeface, font_size);
        let emoji_font = emoji_typeface().map(|tf| Font::from_typeface(tf, font_size));

        t.lines
            .iter()
            .map(|line| {
                let prefix = match line.line_type {
                    crate::terminal::TerminalLineType::Prompt => "$ ",
                    _ => "",
                };
                let full = format!("{}{}", prefix, line.text);
                measure_text_with_fallback(&full, &font, &emoji_font, 0.0)
            })
            .fold(0.0f32, f32::max)
    }
}

impl IntrinsicMeasure for TerminalIntrinsic {
    fn measure(
        &self,
        known: (Option<f32>, Option<f32>),
        _available: (AvailableSpace, AvailableSpace),
    ) -> (f32, f32) {
        let w = known.0.unwrap_or(self.max_line_width + self.padding * 2.0);
        let h = known.1.unwrap_or(
            self.chrome_height + self.padding * 2.0 + self.n_lines as f32 * self.line_height,
        );
        (w, h)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Table intrinsic measurer
// ─────────────────────────────────────────────────────────────────────────────

use crate::table::{
    Table, DEFAULT_CELL_PADDING, DEFAULT_FONT_SIZE as TABLE_FONT_SIZE, DEFAULT_ROW_HEIGHT_RATIO,
};

/// Intrinsic measurer for [`Table`].
///
/// Natural size formula (matches the painter exactly):
/// - `row_height = font_size × DEFAULT_ROW_HEIGHT_RATIO`
/// - `height = (1 + row_count) × row_height`  (header + data rows)
/// - `width`: if `column_widths` are provided, their sum; otherwise each
///   column gets `max(header_text_width + 2 × cell_padding, min_col_width)`.
pub struct TableIntrinsic {
    row_height: f32,
    row_count: usize, // data rows only; header adds 1
    total_width: f32,
}

impl TableIntrinsic {
    pub fn from_table(t: &Table) -> Self {
        let font_size = t.style.font_size_px_or(TABLE_FONT_SIZE);
        let row_height = font_size * DEFAULT_ROW_HEIGHT_RATIO;

        let total_width = Self::compute_width(t, font_size);

        Self {
            row_height,
            row_count: t.rows.len(),
            total_width,
        }
    }

    fn compute_width(t: &Table, font_size: f32) -> f32 {
        // Explicit column widths provided → sum them.
        if let Some(widths) = &t.column_widths {
            if !widths.is_empty() {
                return widths.iter().sum();
            }
        }

        // Measure each header with the bold font; add 2× cell_padding per column.
        let font_style = skia_safe::FontStyle::bold();
        let family = t.style.font_family.as_deref().unwrap_or("Inter");
        let Ok(typeface) = typeface_with_fallback(family, font_style) else {
            // Font unavailable: fall back to col_count × a reasonable minimum.
            let col_count = t.headers.len().max(1) as f32;
            return col_count * (TABLE_FONT_SIZE * 8.0 + DEFAULT_CELL_PADDING * 2.0);
        };
        let font = Font::from_typeface(typeface, font_size);
        let emoji_font = emoji_typeface().map(|tf| Font::from_typeface(tf, font_size));
        let cell_padding = t.cell_padding;

        // Also consider data cell widths to size columns appropriately.
        let col_count = t.headers.len().max(1);
        let mut col_widths: Vec<f32> = vec![0.0; col_count];

        for (i, header) in t.headers.iter().enumerate() {
            let w = measure_text_with_fallback(header, &font, &emoji_font, 0.0);
            col_widths[i] = col_widths[i].max(w + cell_padding * 2.0);
        }
        for row in &t.rows {
            for (i, cell) in row.iter().enumerate() {
                if i >= col_count {
                    break;
                }
                let w = measure_text_with_fallback(cell, &font, &emoji_font, 0.0);
                col_widths[i] = col_widths[i].max(w + cell_padding * 2.0);
            }
        }

        col_widths.iter().sum()
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

// ─────────────────────────────────────────────────────────────────────────────
// Codeblock intrinsic measurer
// ─────────────────────────────────────────────────────────────────────────────

use crate::codeblock::dimensions::compute_code_dimensions;
use crate::codeblock::highlight::resolve_monospace_font;
use crate::codeblock::Codeblock;
use rustmotion_core::css::style::{FontWeight as CssFontWeight2, FontWeightKw as CssFontWeightKw2};
use rustmotion_core::schema::FontWeight;

/// Intrinsic measurer for [`Codeblock`].
///
/// Reuses `compute_code_dimensions` (same function as the painter) to derive:
/// - `width  = max_line_width + gutter_width + pad_left + pad_right`
/// - `height = line_count × line_height + pad_top + pad_bottom + chrome_height`
///
/// Computed once at construction from the initial `code` string. If a state
/// transition widens the content at paint time, `auto_scroll` handles vertical
/// overflow without needing the intrinsic to re-run.
pub struct CodeblockIntrinsic {
    natural_width: f32,
    natural_height: f32,
}

impl CodeblockIntrinsic {
    pub fn from_codeblock(c: &Codeblock) -> Self {
        let font_family = c.style.font_family_or("JetBrains Mono");
        let font_size = c.style.font_size_px_or(14.0);
        let font_weight = match &c.style.font_weight {
            Some(CssFontWeight2::Keyword(CssFontWeightKw2::Bold | CssFontWeightKw2::Bolder)) => {
                FontWeight::Bold
            }
            Some(CssFontWeight2::Number(n)) if *n >= 600 => FontWeight::Bold,
            Some(CssFontWeight2::Number(n)) => FontWeight::Weight(*n),
            _ => FontWeight::Normal,
        };

        let Some(font) = resolve_monospace_font(font_family, font_size, font_weight) else {
            return Self {
                natural_width: 0.0,
                natural_height: 0.0,
            };
        };

        let padding = {
            let (t, r, b, l) = c.style.padding_px();
            if t == 0.0 && r == 0.0 && b == 0.0 && l == 0.0 {
                (16.0, 16.0, 16.0, 16.0)
            } else {
                (t, r, b, l)
            }
        };

        let chrome_height = if c.chrome.as_ref().is_some_and(|ch| ch.enabled) {
            36.0
        } else {
            0.0
        };

        let dims = compute_code_dimensions(&c.code, &font, padding, chrome_height, c);

        Self {
            natural_width: dims.total_width,
            natural_height: dims.total_height,
        }
    }
}

impl IntrinsicMeasure for CodeblockIntrinsic {
    fn measure(
        &self,
        known: (Option<f32>, Option<f32>),
        _available: (AvailableSpace, AvailableSpace),
    ) -> (f32, f32) {
        let w = known.0.unwrap_or(self.natural_width);
        let h = known.1.unwrap_or(self.natural_height);
        (w, h)
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
        };
        let m = TextIntrinsic::from_text(&text);
        let (w, h) = m.measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        );
        assert_eq!(w, 0.0);
        assert!(h > 0.0);
    }
}
