//! Intrinsic measurers for components whose box size depends on content.
//!
//! Uses the same Skia metrics that the painter uses, so the box reserved by
//! taffy matches the pixels actually drawn — measure-vs-paint mismatches
//! would otherwise cause text to wrap onto an extra line at paint time and
//! overflow into the next sibling.

use skia_safe::{Font, FontStyle as SkFontStyle};

use rustmotion_core::css::style::{
    CssStyle, FontStyle as CssFontStyle, FontWeight as CssFontWeight, FontWeightKw, LineHeight,
    WhiteSpace,
};
use rustmotion_core::engine::box_tree::{AvailableSpace, IntrinsicMeasure};
use rustmotion_core::engine::renderer::{
    emoji_typeface, format_counter_value, measure_text_with_fallback, typeface_with_fallback,
    wrap_text_with_tracking,
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
    /// M1: `white-space: nowrap|pre` disables wrapping — the geometry
    /// validator's `unwrappable_text_overflow`/`ContentOverflowsBox` checks
    /// (crates/rustmotion-cli/src/commands/geometry.rs) already branch on
    /// exactly this pair of variants and re-measure via this same
    /// `TextIntrinsic`, so the wrap decision here must match theirs exactly
    /// or the validator's assumption about what the renderer produces is
    /// false.
    pub fn from_text(text: &Text) -> Self {
        let wrap = !matches!(
            text.style.white_space,
            Some(WhiteSpace::Nowrap | WhiteSpace::Pre)
        );
        Self::from_parts_with_wrap(&text.content, &text.style, text.max_width, wrap)
    }

    /// Generic constructor shared by [`GradientText`]/[`Caption`] intrinsics,
    /// whose painters don't (yet) implement `white-space: nowrap` — kept
    /// wrap:true unconditionally so their measured size still matches what
    /// those painters actually draw.
    pub fn from_parts(content: &str, style: &CssStyle, max_width: Option<f32>) -> Self {
        // No `LengthContext` is reachable here without changing this
        // constructor's signature — its only callers are `box_builder.rs`
        // and `rustmotion-cli/src/commands/geometry.rs`, both outside this
        // workstream's scope (box_builder.rs is a sibling's live file this
        // wave; the geometry validator re-measures via this exact type and
        // must keep agreeing with it byte-for-byte, so changing what it
        // needs to pass in is not a call to make unilaterally here). So
        // `font_size`/`line_height` stay on the context-free accessors
        // (issue #125 §2's `vw`/`vh`/`rem`/`%` gap is not closed for this
        // constructor) — only `letter_spacing` below, which is used
        // exclusively by the wrap fix in `measure()`, no signature change
        // needed for it.
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
        // Tracking-aware wrap (issue #125 §1): matches the real
        // `letter_spacing` used to measure each line's width just below, so
        // the box this measurer reserves and what `Text::paint` (also fixed,
        // same tracking) actually paints agree on line count.
        let lines = wrap_text_with_tracking(
            &self.content,
            &font,
            &emoji_font,
            wrap_at,
            self.letter_spacing,
        );

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
        // M1 follow-up (issue #109 review): gradient_text now word-wraps
        // like `text` (see `gradient_text.rs::paint`) — mirror the same
        // white-space: nowrap|pre rule here so measure and paint agree.
        let wrap = !matches!(
            t.style.white_space,
            Some(WhiteSpace::Nowrap | WhiteSpace::Pre)
        );
        Self(TextIntrinsic::from_parts_with_wrap(
            &t.content, &t.style, max_width, wrap,
        ))
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
        // M1 follow-up: `Highlight`/`Karaoke`/`KaraokePop` word-wrap all
        // words (see `caption.rs::paint`); `white-space: nowrap|pre` now
        // forces them onto one line there too — mirror it here. (`WordByWord`
        // /`WordPop` show one word at a time; wrapping is moot for those,
        // same as the existing kbd/counter/badge "atomic, never wraps"
        // components, so this doesn't need a mode-specific branch.)
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

// ─────────────────────────────────────────────────────────────────────────────
// RichText intrinsic measurer
// ─────────────────────────────────────────────────────────────────────────────

use crate::rich_text::{RichText, RichTextSpan};

/// Intrinsic measurer for [`RichText`].
///
/// M2: previously absent from `component_intrinsic` entirely, so a
/// `rich_text` with no explicit `width`/`height` laid out 0×0 and rendered
/// nothing. Reuses `RichText::compute_layout` — the exact same word-wrapped
/// line-breaking algorithm the painter uses — so the box taffy reserves
/// always matches what gets painted (same measure/paint-parity rationale as
/// [`TextIntrinsic`]).
///
/// Always measures the full (untruncated) content — a `visible_chars`
/// typewriter animation must not reflow layout as it plays.
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

        let layout = RichText::compute_layout(&self.spans, &self.style, max_width, -1.0);
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

    // ─── M1: white-space: nowrap/pre ────────────────────────────────────────

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
        // Regression guard: making `nowrap`/`pre` real must not touch the
        // default (`normal`/unset) wrapping path.
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

    // ─── M2: rich_text intrinsic ─────────────────────────────────────────────

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
        // M2's core defect: rich_text had no `component_intrinsic` entry at
        // all, so it measured 0×0 unless the author guessed a width/height.
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
        // M2's second ask: a long single span must wrap like any other text,
        // not just break at span boundaries (there is only one span here).
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
        // Measure/paint parity: the intrinsic must reuse the exact same
        // layout algorithm the painter does, or taffy could reserve a box
        // that doesn't match what gets drawn.
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
        let layout = RichText::compute_layout(&spans, &style, None, -1.0);
        assert_eq!(w, layout.max_width);
        assert_eq!(h, layout.lines.len().max(1) as f32 * layout.line_height);
    }

    // ─── M1 follow-up: gradient_text / caption honor white-space too ───────

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
        // Single line: height should be one line, not several.
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
}
