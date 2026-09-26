use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Font, FontStyle};

use rustmotion_core::css::style::{
    FontStyle as CssFontStyle, FontWeight as CssFontWeight, FontWeightKw, TextAlign as CssTextAlign,
};
use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, measure_text_with_fallback, paint_from_hex,
    typeface_with_fallback,
};
use rustmotion_core::schema::{FontStyleType, FontWeight, TextAlign, TimelineStep};
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

/// A single styled span within a rich_text component.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RichTextSpan {
    pub text: String,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default, rename = "font-size")]
    pub font_size: Option<f32>,
    #[serde(default, rename = "font-weight")]
    pub font_weight: Option<FontWeight>,
    #[serde(default, rename = "font-family")]
    pub font_family: Option<String>,
    #[serde(default, rename = "font-style")]
    pub font_style: Option<FontStyleType>,
    #[serde(default, rename = "letter-spacing")]
    pub letter_spacing: Option<f32>,
}

/// Rich text component: renders multiple styled spans on the same line(s).
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct RichText {
    pub spans: Vec<RichTextSpan>,
    #[serde(default)]
    pub max_width: Option<f32>,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(RichText {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

fn make_font(
    family: &str,
    weight: &FontWeight,
    font_style_type: &FontStyleType,
    size: f32,
) -> Option<Font> {
    let slant = match font_style_type {
        FontStyleType::Normal => skia_safe::font_style::Slant::Upright,
        FontStyleType::Italic => skia_safe::font_style::Slant::Italic,
        FontStyleType::Oblique => skia_safe::font_style::Slant::Oblique,
    };
    let weight_val = match weight {
        FontWeight::Bold => skia_safe::font_style::Weight::BOLD,
        FontWeight::Normal => skia_safe::font_style::Weight::NORMAL,
        FontWeight::Weight(w) => skia_safe::font_style::Weight::from(*w as i32),
    };
    let skia_style = FontStyle::new(weight_val, skia_safe::font_style::Width::NORMAL, slant);
    let typeface = typeface_with_fallback(family, skia_style).ok()?;
    Some(Font::from_typeface(typeface, size))
}

struct SpanFontInfo {
    font: Font,
    color: String,
    letter_spacing: f32,
}

fn resolve_span_fonts(
    spans: &[RichTextSpan],
    style: &CssStyle,
    default_size: f32,
) -> Vec<Option<SpanFontInfo>> {
    let default_color = style.color_str_or("#FFFFFF");
    let default_family = style.font_family_or("Inter");
    let default_weight = match &style.font_weight {
        Some(CssFontWeight::Keyword(FontWeightKw::Bold | FontWeightKw::Bolder)) => FontWeight::Bold,
        Some(CssFontWeight::Number(n)) if *n >= 600 => FontWeight::Bold,
        Some(CssFontWeight::Number(n)) => FontWeight::Weight(*n),
        _ => FontWeight::Normal,
    };
    let default_font_style = match style.font_style {
        Some(CssFontStyle::Italic) => FontStyleType::Italic,
        Some(CssFontStyle::Oblique) => FontStyleType::Oblique,
        _ => FontStyleType::Normal,
    };

    spans
        .iter()
        .map(|span| {
            let size = span.font_size.unwrap_or(default_size);
            let family = span.font_family.as_deref().unwrap_or(default_family);
            let weight = span.font_weight.as_ref().unwrap_or(&default_weight);
            let fstyle = span.font_style.as_ref().unwrap_or(&default_font_style);
            let color = span.color.as_deref().unwrap_or(default_color).to_string();
            let letter_spacing = span.letter_spacing.unwrap_or(0.0);
            make_font(family, weight, fstyle, size).map(|font| SpanFontInfo {
                font,
                color,
                letter_spacing,
            })
        })
        .collect()
}

pub struct RichTextToken {
    pub span_idx: usize,
    pub text: String,
    pub x: f32,
    pub width: f32,
}

pub struct RichTextLine {
    pub tokens: Vec<RichTextToken>,
    pub width: f32,
}

pub struct RichTextLayout {
    pub lines: Vec<RichTextLine>,
    pub max_width: f32,
    pub line_height: f32,
    pub max_ascent: f32,
}

impl RichText {
    pub fn compute_layout(
        spans: &[RichTextSpan],
        style: &CssStyle,
        viewport_width: f32,
        viewport_height: f32,
        wrap_width: Option<f32>,
        visible_chars_progress: f32,
    ) -> RichTextLayout {
        let base_ctx = crate::intrinsic::font_size_ctx(
            viewport_width,
            viewport_height,
            wrap_width.unwrap_or(0.0),
        );
        let (default_size, _letter_spacing_unused, line_height_val) =
            style.typography_px_ctx(&base_ctx, 48.0);
        let span_fonts = resolve_span_fonts(spans, style, default_size);
        let emoji_tf = emoji_typeface();

        let texts: Vec<String> = if visible_chars_progress >= 0.0 {
            let total_chars: usize = spans.iter().map(|s| s.text.chars().count()).sum();
            let visible =
                ((visible_chars_progress * total_chars as f32).round() as usize).min(total_chars);
            let mut remaining = visible;
            spans
                .iter()
                .map(|s| {
                    let char_count = s.text.chars().count();
                    if remaining >= char_count {
                        remaining -= char_count;
                        s.text.clone()
                    } else if remaining == 0 {
                        String::new()
                    } else {
                        let truncated: String = s.text.chars().take(remaining).collect();
                        remaining = 0;
                        truncated
                    }
                })
                .collect()
        } else {
            spans.iter().map(|s| s.text.clone()).collect()
        };

        struct Tok {
            span_idx: usize,
            text: String,
            space_before: bool,
        }
        let mut tokens: Vec<Tok> = Vec::new();
        let mut prev_trailing_ws = true;
        for (span_idx, text) in texts.iter().enumerate() {
            if span_fonts.get(span_idx).and_then(|f| f.as_ref()).is_none() || text.is_empty() {
                continue;
            }
            let starts_ws = text.chars().next().is_some_and(char::is_whitespace);
            for (wi, w) in text.split_whitespace().enumerate() {
                let space_before = if tokens.is_empty() {
                    false
                } else if wi > 0 {
                    true
                } else {
                    prev_trailing_ws || starts_ws
                };
                tokens.push(Tok {
                    span_idx,
                    text: w.to_string(),
                    space_before,
                });
            }
            prev_trailing_ws = text.chars().last().is_none_or(char::is_whitespace);
        }

        let effective_wrap = wrap_width.unwrap_or(f32::INFINITY);
        let mut lines: Vec<RichTextLine> = vec![RichTextLine {
            tokens: Vec::new(),
            width: 0.0,
        }];

        for tok in &tokens {
            let sf = span_fonts[tok.span_idx]
                .as_ref()
                .expect("font presence checked during tokenization");
            let emoji_font = emoji_tf
                .as_ref()
                .map(|tf| Font::from_typeface(tf.clone(), sf.font.size()));
            let tok_width =
                measure_text_with_fallback(&tok.text, &sf.font, &emoji_font, sf.letter_spacing);
            let space_width = if tok.space_before {
                measure_text_with_fallback(" ", &sf.font, &emoji_font, 0.0)
            } else {
                0.0
            };

            let current = lines.last_mut().unwrap();
            let has_content = !current.tokens.is_empty();
            let extra = if has_content { space_width } else { 0.0 };
            let projected = current.width + extra + tok_width;

            if projected > effective_wrap && has_content {
                lines.push(RichTextLine {
                    tokens: vec![RichTextToken {
                        span_idx: tok.span_idx,
                        text: tok.text.clone(),
                        x: 0.0,
                        width: tok_width,
                    }],
                    width: tok_width,
                });
            } else {
                let x = current.width + extra;
                current.tokens.push(RichTextToken {
                    span_idx: tok.span_idx,
                    text: tok.text.clone(),
                    x,
                    width: tok_width,
                });
                current.width = x + tok_width;
            }
        }

        let max_width = lines.iter().map(|l| l.width).fold(0.0f32, f32::max);
        let max_ascent = span_fonts
            .iter()
            .flatten()
            .map(|sf| {
                let (_, m) = sf.font.metrics();
                -m.ascent
            })
            .fold(0.0f32, f32::max);

        RichTextLayout {
            lines,
            max_width,
            line_height: line_height_val,
            max_ascent,
        }
    }

    fn paint(
        &self,
        canvas: &Canvas,
        layout_width: f32,
        props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        let align = match self.style.text_align {
            Some(CssTextAlign::Center) => TextAlign::Center,
            Some(CssTextAlign::Right | CssTextAlign::End) => TextAlign::Right,
            _ => TextAlign::Left,
        };

        let wrap_width = if layout_width.is_finite() && layout_width > 0.0 {
            match self.max_width {
                Some(mw) => Some(mw.min(layout_width)),
                None => Some(layout_width),
            }
        } else {
            self.max_width
        };

        let layout = RichText::compute_layout(
            &self.spans,
            &self.style,
            ctx.video_width as f32,
            ctx.video_height as f32,
            wrap_width,
            props.visible_chars_progress,
        );
        if layout.lines.iter().all(|l| l.tokens.is_empty()) {
            return;
        }

        let base_ctx = crate::intrinsic::font_size_ctx(
            ctx.video_width as f32,
            ctx.video_height as f32,
            wrap_width.unwrap_or(0.0),
        );
        let default_size = self.style.font_size_px_ctx(&base_ctx, 48.0);
        let span_fonts = resolve_span_fonts(&self.spans, &self.style, default_size);
        let emoji_tf = emoji_typeface();

        let align_width = if layout_width.is_finite() && layout_width > 0.0 {
            layout_width
        } else {
            layout.max_width
        };

        let baseline_offset = (layout.line_height + layout.max_ascent) / 2.0;

        for (line_idx, line) in layout.lines.iter().enumerate() {
            let line_x_offset = match align {
                TextAlign::Left => 0.0,
                TextAlign::Center => (align_width - line.width) / 2.0,
                TextAlign::Right => align_width - line.width,
            };
            let y = line_idx as f32 * layout.line_height + baseline_offset;

            for tok in &line.tokens {
                let sf = span_fonts[tok.span_idx]
                    .as_ref()
                    .expect("font presence matches compute_layout's tokenization");
                let paint = paint_from_hex(&sf.color);
                let emoji_font = emoji_tf
                    .as_ref()
                    .map(|tf| Font::from_typeface(tf.clone(), sf.font.size()));

                draw_text_with_fallback(
                    canvas,
                    &tok.text,
                    &sf.font,
                    &emoji_font,
                    sf.letter_spacing,
                    line_x_offset + tok.x,
                    y,
                    &paint,
                );
            }
        }
    }
}

impl Painter for RichText {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        self.paint(canvas, layout.width, props, ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmotion_core::css::CssStyle;

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

    fn style(font_px: f32) -> CssStyle {
        CssStyle {
            font_size: Some(rustmotion_core::css::Length::Px(font_px)),
            ..Default::default()
        }
    }

    #[test]
    fn single_long_span_wraps_into_multiple_lines_at_constrained_width() {
        let spans = vec![span(
            "the quick brown fox jumps over the lazy dog and keeps going",
        )];
        let s = style(24.0);
        let unconstrained = RichText::compute_layout(&spans, &s, 1920.0, 1080.0, None, -1.0);
        assert_eq!(
            unconstrained.lines.len(),
            1,
            "unconstrained width must fit on one line"
        );

        let constrained = RichText::compute_layout(&spans, &s, 1920.0, 1080.0, Some(150.0), -1.0);
        assert!(
            constrained.lines.len() > 1,
            "a single long span must wrap into multiple lines at 150px, got {} line(s)",
            constrained.lines.len()
        );
        for line in &constrained.lines {
            assert!(
                line.width <= 150.0 + 0.5,
                "each wrapped line must fit the constraint, got {}",
                line.width
            );
        }
    }

    #[test]
    fn spans_glue_without_extra_space_when_source_has_none() {
        let glued = vec![span("Total:"), span("42"), span(" items")];
        let s = style(20.0);
        let layout = RichText::compute_layout(&glued, &s, 1920.0, 1080.0, None, -1.0);
        assert_eq!(layout.lines.len(), 1);
        let tokens = &layout.lines[0].tokens;
        assert_eq!(
            tokens.iter().map(|t| t.text.as_str()).collect::<Vec<_>>(),
            vec!["Total:", "42", "items"]
        );
        assert_eq!(tokens[1].x, tokens[0].width, "no space between glued spans");
        assert!(
            tokens[2].x > tokens[1].x + tokens[1].width,
            "a space must separate '42' and 'items' (source had a leading space)"
        );
    }

    #[test]
    fn typewriter_truncation_hides_tail_tokens() {
        let spans = vec![span("Hello "), span("world")];
        let s = style(20.0);
        let full = RichText::compute_layout(&spans, &s, 1920.0, 1080.0, None, -1.0);
        let half = RichText::compute_layout(&spans, &s, 1920.0, 1080.0, None, 0.5);
        let none = RichText::compute_layout(&spans, &s, 1920.0, 1080.0, None, 0.0);

        let full_tokens: usize = full.lines.iter().map(|l| l.tokens.len()).sum();
        let half_tokens: usize = half.lines.iter().map(|l| l.tokens.len()).sum();
        let none_tokens: usize = none.lines.iter().map(|l| l.tokens.len()).sum();

        assert!(full_tokens >= half_tokens);
        assert_eq!(none_tokens, 0, "progress 0.0 must show nothing");
        assert!(
            half_tokens >= 1,
            "progress 0.5 must show at least one token"
        );
    }

    #[test]
    fn empty_spans_produce_one_empty_line_not_a_panic() {
        let spans: Vec<RichTextSpan> = vec![];
        let s = style(20.0);
        let layout = RichText::compute_layout(&spans, &s, 1920.0, 1080.0, None, -1.0);
        assert_eq!(layout.lines.len(), 1);
        assert_eq!(layout.max_width, 0.0);
    }
}
