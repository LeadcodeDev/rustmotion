use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::font_style::Weight;
use skia_safe::{Canvas, Font, FontStyle, Point, RRect, Rect, Typeface};

use rustmotion_core::css::style::{
    FontStyle as CssFontStyle, TextAlign as CssTextAlign, WhiteSpace as CssWhiteSpace,
};
use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::{AnimatedProperties, ResolvedCharAnimation};
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    css_font_weight, draw_text_with_fallback, emoji_typeface, measure_text_with_fallback,
    paint_from_hex, subpixel_font, typeface_with_fallback,
};
use rustmotion_core::schema::{
    FontStyleType, FontWeight, TextAlign, TextAnimGranularity, TimelineStep,
};
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

/// Padding around a pill span's background box, in px on each side. `left`
/// and `right` grow the token's advance width, so following spans shift
/// over instead of overlapping the pill. `top` and `bottom` only grow the
/// box — they never change the line's height.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, JsonSchema)]
pub struct RichTextSpanPadding {
    /// Padding above the glyph run, in px.
    #[serde(default)]
    pub top: f32,
    /// Padding to the right of the glyph run, in px. Added to the line's advance.
    #[serde(default)]
    pub right: f32,
    /// Padding below the glyph run, in px.
    #[serde(default)]
    pub bottom: f32,
    /// Padding to the left of the glyph run, in px. Added to the line's advance.
    #[serde(default)]
    pub left: f32,
}

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
    /// Background colour (hex) painted behind this span's glyph run, as a
    /// pill. Absent means no box — the span paints glyphs only, and
    /// `padding`/`border-radius`/`rotation` are then inert.
    #[serde(default)]
    pub background: Option<String>,
    /// Padding around the pill box. Ignored when `background` is absent.
    #[serde(default)]
    pub padding: Option<RichTextSpanPadding>,
    /// Corner radius of the pill box, in px. Ignored when `background` is absent.
    #[serde(default, rename = "border-radius")]
    pub border_radius: Option<f32>,
    /// Rotation of the pill box and its glyphs together, in degrees, about
    /// the box's own centre. Does not affect layout. Ignored when
    /// `background` is absent.
    #[serde(default)]
    pub rotation: Option<f32>,
}

impl RichTextSpan {
    fn pill_padding(&self) -> RichTextSpanPadding {
        self.padding.unwrap_or_default()
    }
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
    weight: Weight,
    font_style_type: &FontStyleType,
    size: f32,
) -> Option<Font> {
    let slant = match font_style_type {
        FontStyleType::Normal => skia_safe::font_style::Slant::Upright,
        FontStyleType::Italic => skia_safe::font_style::Slant::Italic,
        FontStyleType::Oblique => skia_safe::font_style::Slant::Oblique,
    };
    let skia_style = FontStyle::new(weight, skia_safe::font_style::Width::NORMAL, slant);
    let typeface = typeface_with_fallback(family, skia_style).ok()?;
    Some(subpixel_font(typeface, size))
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
    default_letter_spacing: f32,
) -> Vec<Option<SpanFontInfo>> {
    let default_color = style.color_str_or("#FFFFFF");
    let default_family = style.font_family_or("Inter");
    let default_weight = css_font_weight(style.font_weight.as_ref());
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
            let weight = span
                .font_weight
                .as_ref()
                .map_or(default_weight, |w| Weight::from(w.to_skia_weight()));
            let fstyle = span.font_style.as_ref().unwrap_or(&default_font_style);
            let color = span.color.as_deref().unwrap_or(default_color).to_string();
            let letter_spacing = span.letter_spacing.unwrap_or(default_letter_spacing);
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
    pub max_descent: f32,
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
        let (default_size, default_letter_spacing, line_height_val) =
            style.typography_px_ctx(&base_ctx, 48.0);
        let span_fonts = resolve_span_fonts(spans, style, default_size, default_letter_spacing);
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
        let literal_whitespace = matches!(
            style.white_space,
            Some(CssWhiteSpace::Nowrap | CssWhiteSpace::Pre)
        );
        let mut tokens: Vec<Tok> = Vec::new();
        if literal_whitespace {
            for (span_idx, text) in texts.iter().enumerate() {
                if span_fonts.get(span_idx).and_then(|f| f.as_ref()).is_none() || text.is_empty() {
                    continue;
                }
                tokens.push(Tok {
                    span_idx,
                    text: text.clone(),
                    space_before: false,
                });
            }
        } else {
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
        }

        let effective_wrap = if literal_whitespace {
            f32::INFINITY
        } else {
            wrap_width.unwrap_or(f32::INFINITY)
        };
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
                .map(|tf| subpixel_font(tf.clone(), sf.font.size()));
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

        for line in &mut lines {
            let mut extra = 0.0f32;
            let n = line.tokens.len();
            for i in 0..n {
                let span_idx = line.tokens[i].span_idx;
                let is_pill = spans.get(span_idx).is_some_and(|s| s.background.is_some());
                if is_pill && (i == 0 || line.tokens[i - 1].span_idx != span_idx) {
                    extra += spans[span_idx].pill_padding().left;
                }
                line.tokens[i].x += extra;
                if is_pill && (i + 1 == n || line.tokens[i + 1].span_idx != span_idx) {
                    extra += spans[span_idx].pill_padding().right;
                }
            }
            line.width += extra;
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
        let max_descent = span_fonts
            .iter()
            .flatten()
            .map(|sf| {
                let (_, m) = sf.font.metrics();
                m.descent
            })
            .fold(0.0f32, f32::max);

        RichTextLayout {
            lines,
            max_width,
            line_height: line_height_val,
            max_ascent,
            max_descent,
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
        let (default_size, default_letter_spacing, _) =
            self.style.typography_px_ctx(&base_ctx, 48.0);
        let span_fonts = resolve_span_fonts(
            &self.spans,
            &self.style,
            default_size,
            default_letter_spacing,
        );
        let emoji_tf = emoji_typeface();

        let align_width = if layout_width.is_finite() && layout_width > 0.0 {
            layout_width
        } else {
            layout.max_width
        };

        let baseline_offset = (layout.line_height + layout.max_ascent - layout.max_descent) / 2.0;

        if let Some(ref resolved) = props.char_animation {
            render_rich_text_char_animation(
                canvas,
                &layout,
                &self.spans,
                &span_fonts,
                &emoji_tf,
                align,
                align_width,
                baseline_offset,
                resolved,
                ctx.time,
            );
            return;
        }

        for (line_idx, line) in layout.lines.iter().enumerate() {
            let line_x_offset = match align {
                TextAlign::Left => 0.0,
                TextAlign::Center => (align_width - line.width) / 2.0,
                TextAlign::Right => align_width - line.width,
            };
            let y = line_idx as f32 * layout.line_height + baseline_offset;

            let mut i = 0;
            while i < line.tokens.len() {
                let span_idx = line.tokens[i].span_idx;
                let mut j = i + 1;
                while j < line.tokens.len() && line.tokens[j].span_idx == span_idx {
                    j += 1;
                }
                let run = &line.tokens[i..j];

                if self.spans[span_idx].background.is_some() {
                    self.paint_pill_run(
                        canvas,
                        &self.spans[span_idx],
                        run,
                        &span_fonts,
                        &emoji_tf,
                        line_x_offset,
                        y,
                    );
                } else {
                    for tok in run {
                        Self::paint_token(canvas, tok, &span_fonts, &emoji_tf, line_x_offset, y);
                    }
                }
                i = j;
            }
        }
    }

    fn paint_token(
        canvas: &Canvas,
        tok: &RichTextToken,
        span_fonts: &[Option<SpanFontInfo>],
        emoji_tf: &Option<Typeface>,
        line_x_offset: f32,
        y: f32,
    ) {
        let sf = span_fonts[tok.span_idx]
            .as_ref()
            .expect("font presence matches compute_layout's tokenization");
        let paint = paint_from_hex(&sf.color);
        let emoji_font = emoji_tf
            .as_ref()
            .map(|tf| subpixel_font(tf.clone(), sf.font.size()));

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

    #[allow(clippy::too_many_arguments)]
    fn paint_pill_run(
        &self,
        canvas: &Canvas,
        span: &RichTextSpan,
        run: &[RichTextToken],
        span_fonts: &[Option<SpanFontInfo>],
        emoji_tf: &Option<Typeface>,
        line_x_offset: f32,
        y: f32,
    ) {
        let (Some(first), Some(last)) = (run.first(), run.last()) else {
            return;
        };
        let sf = span_fonts[first.span_idx]
            .as_ref()
            .expect("font presence matches compute_layout's tokenization");
        let (_, metrics) = sf.font.metrics();
        let ascent = -metrics.ascent;
        let descent = metrics.descent;
        let padding = span.pill_padding();

        let x0 = line_x_offset + first.x - padding.left;
        let x1 = line_x_offset + last.x + last.width + padding.right;
        let y0 = y - ascent - padding.top;
        let y1 = y + descent + padding.bottom;
        let rect = Rect::from_ltrb(x0, y0, x1, y1);
        let radius = span.border_radius.unwrap_or(0.0).max(0.0);
        let rrect = RRect::new_rect_xy(rect, radius, radius);
        let center = Point::new((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        let rotation = span.rotation.unwrap_or(0.0);

        canvas.save();
        if rotation != 0.0 {
            canvas.rotate(rotation, Some(center));
        }

        let background = span
            .background
            .as_deref()
            .expect("paint_pill_run is only called for spans with a background");
        let bg_paint = paint_from_hex(background);
        canvas.draw_rrect(rrect, &bg_paint);

        for tok in run {
            Self::paint_token(canvas, tok, span_fonts, emoji_tf, line_x_offset, y);
        }

        canvas.restore();
    }
}

fn paint_pill_background(
    canvas: &Canvas,
    span: &RichTextSpan,
    run: &[RichTextToken],
    span_fonts: &[Option<SpanFontInfo>],
    line_x_offset: f32,
    y: f32,
) {
    let (Some(first), Some(last)) = (run.first(), run.last()) else {
        return;
    };
    let sf = span_fonts[first.span_idx]
        .as_ref()
        .expect("font presence matches compute_layout's tokenization");
    let (_, metrics) = sf.font.metrics();
    let ascent = -metrics.ascent;
    let descent = metrics.descent;
    let padding = span.pill_padding();

    let x0 = line_x_offset + first.x - padding.left;
    let x1 = line_x_offset + last.x + last.width + padding.right;
    let y0 = y - ascent - padding.top;
    let y1 = y + descent + padding.bottom;
    let rect = Rect::from_ltrb(x0, y0, x1, y1);
    let radius = span.border_radius.unwrap_or(0.0).max(0.0);
    let rrect = RRect::new_rect_xy(rect, radius, radius);
    let center = Point::new((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let rotation = span.rotation.unwrap_or(0.0);

    let background = span
        .background
        .as_deref()
        .expect("paint_pill_background is only called for spans with a background");
    let bg_paint = paint_from_hex(background);

    canvas.save();
    if rotation != 0.0 {
        canvas.rotate(rotation, Some(center));
    }
    canvas.draw_rrect(rrect, &bg_paint);
    canvas.restore();
}

#[allow(clippy::too_many_arguments)]
fn render_rich_text_char_animation(
    canvas: &Canvas,
    layout: &RichTextLayout,
    spans: &[RichTextSpan],
    span_fonts: &[Option<SpanFontInfo>],
    emoji_tf: &Option<Typeface>,
    align: TextAlign,
    align_width: f32,
    baseline_offset: f32,
    char_anim: &ResolvedCharAnimation,
    time: f64,
) {
    let is_word_mode = matches!(char_anim.granularity, TextAnimGranularity::Word);
    let mut global_unit_idx = 0usize;

    for (line_idx, line) in layout.lines.iter().enumerate() {
        if line.tokens.is_empty() {
            continue;
        }
        let line_x_offset = match align {
            TextAlign::Left => 0.0,
            TextAlign::Center => (align_width - line.width) / 2.0,
            TextAlign::Right => align_width - line.width,
        };
        let y = line_idx as f32 * layout.line_height + baseline_offset;

        let mut i = 0;
        while i < line.tokens.len() {
            let span_idx = line.tokens[i].span_idx;
            let mut j = i + 1;
            while j < line.tokens.len() && line.tokens[j].span_idx == span_idx {
                j += 1;
            }
            let run = &line.tokens[i..j];
            if spans[span_idx].background.is_some() {
                paint_pill_background(canvas, &spans[span_idx], run, span_fonts, line_x_offset, y);
            }
            i = j;
        }

        for tok in &line.tokens {
            let sf = span_fonts[tok.span_idx]
                .as_ref()
                .expect("font presence matches compute_layout's tokenization");
            let paint = paint_from_hex(&sf.color);
            let emoji_font = emoji_tf
                .as_ref()
                .map(|tf| subpixel_font(tf.clone(), sf.font.size()));

            if is_word_mode {
                let t = crate::intrinsic::unit_progress(char_anim, global_unit_idx, time);
                canvas.save();
                crate::intrinsic::apply_text_anim_preset(
                    canvas,
                    &tok.text,
                    &sf.font,
                    &emoji_font,
                    &paint,
                    line_x_offset + tok.x,
                    y,
                    tok.width,
                    sf.letter_spacing,
                    char_anim,
                    t,
                    time,
                    global_unit_idx,
                    sf.font.size(),
                );
                canvas.restore();
                global_unit_idx += 1;
            } else {
                let mut cursor_x = line_x_offset + tok.x;
                for ch in tok.text.chars() {
                    let ch_str = ch.to_string();
                    let (ch_width, _) = sf.font.measure_str(&ch_str, None);
                    let ch_width = ch_width + sf.letter_spacing;

                    let t = crate::intrinsic::unit_progress(char_anim, global_unit_idx, time);
                    canvas.save();
                    crate::intrinsic::apply_text_anim_preset(
                        canvas,
                        &ch_str,
                        &sf.font,
                        &emoji_font,
                        &paint,
                        cursor_x,
                        y,
                        ch_width,
                        0.0,
                        char_anim,
                        t,
                        time,
                        global_unit_idx,
                        sf.font.size(),
                    );
                    canvas.restore();

                    cursor_x += ch_width;
                    global_unit_idx += 1;
                }
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
    use rustmotion_core::schema::{
        AnimationEffect, CharAnimationTiming, EasingType, TextAnimGranularity,
    };

    fn span(text: &str) -> RichTextSpan {
        RichTextSpan {
            text: text.into(),
            color: None,
            font_size: None,
            font_weight: None,
            font_family: None,
            font_style: None,
            letter_spacing: None,
            background: None,
            padding: None,
            border_radius: None,
            rotation: None,
        }
    }

    fn pill_span(text: &str, background: &str, padding: RichTextSpanPadding) -> RichTextSpan {
        RichTextSpan {
            background: Some(background.into()),
            padding: Some(padding),
            ..span(text)
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

    #[test]
    fn white_space_pre_keeps_leading_spaces_as_a_single_literal_token() {
        let spans = vec![span("    AB")];
        let mut s = style(20.0);
        s.white_space = Some(CssWhiteSpace::Pre);
        let layout = RichText::compute_layout(&spans, &s, 1920.0, 1080.0, Some(80.0), -1.0);

        assert_eq!(layout.lines.len(), 1, "pre must not wrap onto extra lines");
        let tokens = &layout.lines[0].tokens;
        assert_eq!(
            tokens.len(),
            1,
            "pre must not split the span into words, losing the run of spaces"
        );
        assert_eq!(
            tokens[0].text, "    AB",
            "white-space: pre must keep the leading spaces verbatim"
        );
    }

    fn alpha_grid(surface: &mut skia_safe::Surface, width: i32, height: i32) -> Vec<u8> {
        let snapshot = surface.image_snapshot();
        let info = skia_safe::ImageInfo::new(
            (width, height),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut buf = vec![0u8; (width * height * 4) as usize];
        let ok = snapshot.read_pixels(
            &info,
            &mut buf,
            (width * 4) as usize,
            skia_safe::IPoint::new(0, 0),
            skia_safe::image::CachingHint::Disallow,
        );
        assert!(ok, "pixel read should succeed");
        (0..(width * height) as usize)
            .map(|i| buf[i * 4 + 3])
            .collect()
    }

    fn min_ink_x(grid: &[u8], surface_width: i32, height: i32) -> Option<i32> {
        for x in 0..surface_width {
            for y in 0..height {
                if grid[(y * surface_width + x) as usize] > 0 {
                    return Some(x);
                }
            }
        }
        None
    }

    fn min_ink_y(grid: &[u8], surface_width: i32, height: i32) -> Option<i32> {
        for y in 0..height {
            for x in 0..surface_width {
                if grid[(y * surface_width + x) as usize] > 0 {
                    return Some(y);
                }
            }
        }
        None
    }

    fn test_ctx() -> PaintCtx {
        PaintCtx {
            time: 0.0,
            scenario_time: 0.0,
            scene_duration: 1.0,
            frame_index: 0,
            fps: 30,
            video_width: 900,
            video_height: 400,
            stagger_offset: 0.0,
        }
    }

    #[test]
    fn white_space_pre_preserves_leading_spaces_at_the_pixel_level() {
        let plain = RichText {
            spans: vec![span("AB")],
            max_width: None,
            timing: Default::default(),
            style: style(60.0),
            timeline: Vec::new(),
            stagger: None,
        };
        let padded = RichText {
            spans: vec![span("    AB")],
            max_width: None,
            timing: Default::default(),
            style: CssStyle {
                white_space: Some(CssWhiteSpace::Pre),
                ..style(60.0)
            },
            timeline: Vec::new(),
            stagger: None,
        };

        const W: i32 = 400;
        const H: i32 = 150;
        let ctx = test_ctx();
        let props = AnimatedProperties::default();

        let plain_x = {
            let mut surface =
                skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
            plain.paint(surface.canvas(), W as f32, &props, &ctx);
            let grid = alpha_grid(&mut surface, W, H);
            min_ink_x(&grid, W, H).expect("plain text paints")
        };
        let padded_x = {
            let mut surface =
                skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
            padded.paint(surface.canvas(), W as f32, &props, &ctx);
            let grid = alpha_grid(&mut surface, W, H);
            min_ink_x(&grid, W, H).expect("padded text paints")
        };

        assert!(
            padded_x > plain_x + 20,
            "white-space: pre must preserve the 4 leading spaces, shifting first ink right \
             (plain first ink at {plain_x}, padded first ink at {padded_x})"
        );
    }

    #[test]
    fn baseline_offset_matches_the_text_components_formula() {
        let content = "Hamburgefonts";
        let font_px = 72.0;
        let s = style(font_px);
        let rt = RichText {
            spans: vec![span(content)],
            max_width: None,
            timing: Default::default(),
            style: s.clone(),
            timeline: Vec::new(),
            stagger: None,
        };

        const W: i32 = 900;
        const H: i32 = 200;
        let ctx = test_ctx();
        let props = AnimatedProperties::default();

        let rich_text_top = {
            let mut surface =
                skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
            rt.paint(surface.canvas(), W as f32, &props, &ctx);
            let grid = alpha_grid(&mut surface, W, H);
            min_ink_y(&grid, W, H).expect("rich_text must paint ink")
        };

        let typeface =
            typeface_with_fallback("Inter", FontStyle::default()).expect("typeface resolves");
        let font = subpixel_font(typeface, font_px);
        let (_, m) = font.metrics();
        let ascent = -m.ascent;
        let descent = m.descent;
        let base_ctx = crate::intrinsic::font_size_ctx(1920.0, 1080.0, 0.0);
        let (_, _, line_height) = s.typography_px_ctx(&base_ctx, 48.0);
        let expected_baseline = (line_height + ascent - descent) / 2.0;

        let reference_top = {
            let mut surface =
                skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
            let paint = paint_from_hex("#FFFFFF");
            draw_text_with_fallback(
                surface.canvas(),
                content,
                &font,
                &None,
                0.0,
                0.0,
                expected_baseline,
                &paint,
            );
            let grid = alpha_grid(&mut surface, W, H);
            min_ink_y(&grid, W, H).expect("reference draw must paint ink")
        };

        assert!(
            (rich_text_top - reference_top).abs() <= 1,
            "rich_text's baseline must be computed as (line_height + ascent - descent) / 2, the \
             same formula text.rs uses — got top {rich_text_top}, expected {reference_top} \
             (formula gave baseline {expected_baseline})"
        );
    }

    fn pixel_rgba(
        surface: &mut skia_safe::Surface,
        w: i32,
        h: i32,
        x: f32,
        y: f32,
    ) -> (u8, u8, u8, u8) {
        let snapshot = surface.image_snapshot();
        let info = skia_safe::ImageInfo::new(
            (w, h),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Unpremul,
            None,
        );
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let ok = snapshot.read_pixels(
            &info,
            &mut buf,
            (w * 4) as usize,
            skia_safe::IPoint::new(0, 0),
            skia_safe::image::CachingHint::Disallow,
        );
        assert!(ok, "pixel read should succeed");
        let ix = x.round() as i32;
        let iy = y.round() as i32;
        let idx = ((iy * w + ix) * 4) as usize;
        (buf[idx], buf[idx + 1], buf[idx + 2], buf[idx + 3])
    }

    fn any_pixel_near_white(surface: &mut skia_safe::Surface, w: i32, h: i32) -> bool {
        let snapshot = surface.image_snapshot();
        let info = skia_safe::ImageInfo::new(
            (w, h),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Unpremul,
            None,
        );
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let ok = snapshot.read_pixels(
            &info,
            &mut buf,
            (w * 4) as usize,
            skia_safe::IPoint::new(0, 0),
            skia_safe::image::CachingHint::Disallow,
        );
        assert!(ok, "pixel read should succeed");
        buf.as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[0] > 200 && p[1] > 200 && p[2] > 200 && p[3] > 200)
    }

    #[test]
    fn pill_padding_grows_the_line_advance_and_shifts_the_following_span() {
        let s = style(40.0);
        let padding = RichTextSpanPadding {
            top: 4.0,
            right: 16.0,
            bottom: 6.0,
            left: 16.0,
        };

        let plain = vec![span("tag"), span(" rest")];
        let with_pill = vec![pill_span("tag", "#1F6FEB", padding), span(" rest")];

        let plain_layout = RichText::compute_layout(&plain, &s, 1920.0, 1080.0, None, -1.0);
        let pill_layout = RichText::compute_layout(&with_pill, &s, 1920.0, 1080.0, None, -1.0);

        let plain_rest_x = plain_layout.lines[0].tokens[1].x;
        let pill_rest_x = pill_layout.lines[0].tokens[1].x;
        let expected_shift = padding.left + padding.right;

        assert!(
            (pill_rest_x - plain_rest_x - expected_shift).abs() < 0.5,
            "a pill's horizontal padding must be added to the line's advance, moving the \
             following span over by left+right padding: plain x={plain_rest_x}, pill \
             x={pill_rest_x}, expected shift {expected_shift}"
        );
    }

    #[test]
    fn a_pill_that_wraps_gets_its_own_padding_per_line_fragment() {
        let s = style(40.0);
        let padding = RichTextSpanPadding {
            top: 4.0,
            right: 16.0,
            bottom: 4.0,
            left: 16.0,
        };
        const WRAP: f32 = 100.0;

        let plain = vec![span("aaaa bbbb")];
        let plain_layout = RichText::compute_layout(&plain, &s, 1920.0, 1080.0, Some(WRAP), -1.0);
        assert_eq!(
            plain_layout.lines.len(),
            2,
            "the reference text must wrap into two lines for this test to be meaningful"
        );

        let pill = vec![pill_span("aaaa bbbb", "#1F6FEB", padding)];
        let pill_layout = RichText::compute_layout(&pill, &s, 1920.0, 1080.0, Some(WRAP), -1.0);
        assert_eq!(
            pill_layout.lines.len(),
            2,
            "padding must not change where the raw text wraps"
        );

        assert!(
            (pill_layout.lines[0].tokens[0].x - padding.left).abs() < 0.5,
            "the first line's fragment must get its own left padding, got x={}",
            pill_layout.lines[0].tokens[0].x
        );
        assert!(
            (pill_layout.lines[1].tokens[0].x - padding.left).abs() < 0.5,
            "the second line's fragment must ALSO get its own left padding — \
             box-decoration-break: clone — got x={}",
            pill_layout.lines[1].tokens[0].x
        );

        let expected_extra = padding.left + padding.right;
        assert!(
            (pill_layout.lines[0].width - plain_layout.lines[0].width - expected_extra).abs() < 0.5,
            "line 1's fragment must grow by its own left+right padding"
        );
        assert!(
            (pill_layout.lines[1].width - plain_layout.lines[1].width - expected_extra).abs() < 0.5,
            "line 2's fragment must ALSO grow by its own left+right padding"
        );
    }

    #[test]
    fn pill_background_paints_behind_the_glyphs_not_in_front() {
        let padding = RichTextSpanPadding {
            top: 4.0,
            right: 16.0,
            bottom: 4.0,
            left: 16.0,
        };
        let s = CssStyle {
            color: Some(rustmotion_core::css::style::Color::String("#FFFFFF".into())),
            ..style(60.0)
        };
        let spans = vec![pill_span("Tag", "#000000", padding)];
        let rt = RichText {
            spans: spans.clone(),
            max_width: None,
            timing: Default::default(),
            style: s.clone(),
            timeline: Vec::new(),
            stagger: None,
        };

        const W: i32 = 400;
        const H: i32 = 200;
        let ctx = test_ctx();
        let props = AnimatedProperties::default();

        let layout = RichText::compute_layout(
            &spans,
            &s,
            ctx.video_width as f32,
            ctx.video_height as f32,
            Some(W as f32),
            -1.0,
        );
        let baseline_offset = (layout.line_height + layout.max_ascent - layout.max_descent) / 2.0;
        let token = &layout.lines[0].tokens[0];
        let box_x0 = token.x - padding.left;
        let box_y0 = baseline_offset - layout.max_ascent - padding.top;

        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        {
            let canvas = surface.canvas();
            canvas.clear(skia_safe::Color::from_argb(255, 0, 255, 0));
            rt.paint(canvas, W as f32, &props, &ctx);
        }

        let inside_padding = pixel_rgba(&mut surface, W, H, box_x0 + 4.0, box_y0 + 4.0);
        assert_eq!(
            inside_padding,
            (0, 0, 0, 255),
            "just inside the pill's padding, clear of any glyph, must show the pill's own \
             background colour, got {inside_padding:?}"
        );

        assert!(
            any_pixel_near_white(&mut surface, W, H),
            "the glyphs must still paint their own (white) colour on top of the pill's black \
             background — a background painted in front of the text would leave no white pixel"
        );
    }

    #[test]
    fn pill_rotation_turns_the_box_and_text_without_touching_layout() {
        let s = style(50.0);
        let padding = RichTextSpanPadding {
            top: 6.0,
            right: 10.0,
            bottom: 6.0,
            left: 10.0,
        };
        let flat = vec![pill_span("Hi", "#1F6FEB", padding)];
        let mut rotated_spans = flat.clone();
        rotated_spans[0].rotation = Some(45.0);

        let flat_layout = RichText::compute_layout(&flat, &s, 1920.0, 1080.0, None, -1.0);
        let rotated_layout =
            RichText::compute_layout(&rotated_spans, &s, 1920.0, 1080.0, None, -1.0);
        assert_eq!(
            flat_layout.lines[0].tokens[0].x, rotated_layout.lines[0].tokens[0].x,
            "rotation must not move the token — it is a paint-only transform"
        );
        assert_eq!(
            flat_layout.lines[0].width, rotated_layout.lines[0].width,
            "rotation must not change the line's advance"
        );

        const W: i32 = 300;
        const H: i32 = 200;
        let ctx = test_ctx();
        let props = AnimatedProperties::default();
        let baseline_offset =
            (flat_layout.line_height + flat_layout.max_ascent - flat_layout.max_descent) / 2.0;
        let token = &flat_layout.lines[0].tokens[0];
        let box_x1 = token.x + token.width + padding.right;
        let box_y1 = baseline_offset + flat_layout.max_descent + padding.bottom;

        let render_at = |rotation: Option<f32>| {
            let mut spans = flat.clone();
            spans[0].rotation = rotation;
            let rt = RichText {
                spans,
                max_width: None,
                timing: Default::default(),
                style: s.clone(),
                timeline: Vec::new(),
                stagger: None,
            };
            let mut surface =
                skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
            {
                let canvas = surface.canvas();
                canvas.clear(skia_safe::Color::from_argb(255, 0, 255, 0));
                rt.paint(canvas, W as f32, &props, &ctx);
            }
            surface
        };

        let mut unrotated = render_at(None);
        let corner_flat = pixel_rgba(&mut unrotated, W, H, box_x1 - 2.0, box_y1 - 2.0);
        assert_eq!(
            corner_flat,
            (0x1F, 0x6F, 0xEB, 255),
            "unrotated: the bottom-right corner of the pill box must be filled, got {corner_flat:?}"
        );

        let mut rotated = render_at(Some(45.0));
        let corner_rotated = pixel_rgba(&mut rotated, W, H, box_x1 - 2.0, box_y1 - 2.0);
        assert_ne!(
            corner_rotated, corner_flat,
            "rotating the pill 45° about its own centre must move its corner away from the \
             unrotated position, got the same pixel {corner_rotated:?} at both"
        );
    }

    fn richtext_with_char_anim(
        spans: Vec<RichTextSpan>,
        effect: AnimationEffect,
        font_px: f32,
    ) -> RichText {
        RichText {
            spans,
            max_width: None,
            timing: Default::default(),
            style: CssStyle {
                font_size: Some(rustmotion_core::css::Length::Px(font_px)),
                white_space: Some(CssWhiteSpace::Nowrap),
                animation: vec![effect],
                ..Default::default()
            },
            timeline: Vec::new(),
            stagger: None,
        }
    }

    fn props_for(rt: &RichText) -> AnimatedProperties {
        AnimatedProperties {
            char_animation: rustmotion_core::engine::animator::extract_effects(&rt.style.animation)
                .char_animation,
            ..Default::default()
        }
    }

    fn ctx_at(time: f64, video_width: u32, video_height: u32) -> PaintCtx {
        PaintCtx {
            time,
            scenario_time: time,
            scene_duration: 2.0,
            frame_index: 0,
            fps: 30,
            video_width,
            video_height,
            stagger_offset: 0.0,
        }
    }

    fn has_ink_in(grid: &[u8], surface_width: i32, x0: i32, x1: i32, y0: i32, y1: i32) -> bool {
        for y in y0..y1 {
            for x in x0..x1 {
                if grid[(y * surface_width + x) as usize] > 0 {
                    return true;
                }
            }
        }
        false
    }

    fn soft_pixel_fraction(
        grid: &[u8],
        surface_width: i32,
        x0: i32,
        x1: i32,
        y0: i32,
        y1: i32,
    ) -> f32 {
        let mut inked = 0u32;
        let mut soft = 0u32;
        for y in y0..y1 {
            for x in x0..x1 {
                let a = grid[(y * surface_width + x) as usize];
                if a > 0 {
                    inked += 1;
                    if a < 250 {
                        soft += 1;
                    }
                }
            }
        }
        if inked == 0 {
            return 0.0;
        }
        soft as f32 / inked as f32
    }

    fn inter_font_px(px: f32) -> Font {
        let typeface = typeface_with_fallback("Inter", FontStyle::default()).expect("resolves");
        subpixel_font(typeface, px)
    }

    #[test]
    fn char_blur_in_animates_a_rich_text_word_instead_of_painting_it_sharp_immediately() {
        let font_px = 100.0;
        let rt = richtext_with_char_anim(
            vec![span("BLUR")],
            AnimationEffect::CharBlurIn(CharAnimationTiming {
                delay: 0.0,
                duration: 0.5,
                stagger: 0.03,
                granularity: TextAnimGranularity::Word,
                easing: EasingType::Linear,
                ..Default::default()
            }),
            font_px,
        );

        const W: i32 = 700;
        const H: i32 = 220;
        let props = props_for(&rt);

        let render_at = |t: f64| -> Vec<u8> {
            let mut surface =
                skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
            {
                let canvas = surface.canvas();
                rt.paint(canvas, W as f32, &props, &ctx_at(t, W as u32, H as u32));
            }
            alpha_grid(&mut surface, W, H)
        };

        let early = render_at(0.15);
        let settled = render_at(1.0);

        assert!(
            has_ink_in(&early, W, 0, W, 0, H),
            "the word must have started painting by t=0.15 — a char_* preset on rich_text used \
             to be silently ignored and paint the word fully sharp from frame 0"
        );

        let early_soft = soft_pixel_fraction(&early, W, 0, W, 0, H);
        let settled_soft = soft_pixel_fraction(&settled, W, 0, W, 0, H);

        assert!(
            early_soft > settled_soft + 0.15,
            "mid-reveal soft-pixel fraction ({early_soft:.3}) must be clearly higher than the \
             settled fraction ({settled_soft:.3}) — rich_text must actually blur while animating, \
             not just render the sharp glyph unconditionally"
        );
        assert!(
            settled_soft < 0.25,
            "settled frame should read as sharp text, not blur (soft fraction {settled_soft:.3})"
        );
    }

    #[test]
    fn char_blur_in_word_stagger_carries_across_a_span_boundary() {
        let font_px = 90.0;
        let rt = richtext_with_char_anim(
            vec![span("ONE "), span("TWO")],
            AnimationEffect::CharBlurIn(CharAnimationTiming {
                delay: 0.5,
                duration: 0.3,
                stagger: 0.6,
                granularity: TextAnimGranularity::Word,
                easing: EasingType::Linear,
                blur: Some(16.0),
                ..Default::default()
            }),
            font_px,
        );

        const W: i32 = 900;
        const H: i32 = 180;
        let props = props_for(&rt);
        let font = inter_font_px(font_px);
        let word1_end = measure_text_with_fallback("ONE", &font, &None, 0.0) as i32;

        let mut before = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        {
            let canvas = before.canvas();
            rt.paint(canvas, W as f32, &props, &ctx_at(0.1, W as u32, H as u32));
        }
        let before_grid = alpha_grid(&mut before, W, H);
        assert!(
            !has_ink_in(&before_grid, W, 0, W, 0, H),
            "nothing should paint before `delay` has elapsed"
        );

        let mut mid = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        {
            let canvas = mid.canvas();
            rt.paint(canvas, W as f32, &props, &ctx_at(0.65, W as u32, H as u32));
        }
        let mid_grid = alpha_grid(&mut mid, W, H);
        assert!(
            has_ink_in(&mid_grid, W, 0, word1_end, 0, H),
            "the first span's word should show ink by t=0.65 (mid-reveal)"
        );
        assert!(
            !has_ink_in(&mid_grid, W, word1_end + 70, W, 0, H),
            "the SECOND span's word (a different RichTextSpan, starting at delay+stagger=1.1s) \
             must still be fully invisible at t=0.65 — the stagger index must run across the span \
             boundary rather than resetting per span"
        );
    }

    #[test]
    fn ink_from_converges_to_each_spans_own_colour_not_a_shared_default() {
        let font_px = 90.0;
        let spans = vec![
            RichTextSpan {
                color: Some("#00FF00".into()),
                ..span("AAAA")
            },
            RichTextSpan {
                color: Some("#0000FF".into()),
                ..span(" BBBB")
            },
        ];
        let rt = richtext_with_char_anim(
            spans.clone(),
            AnimationEffect::CharFadeIn(CharAnimationTiming {
                delay: 0.0,
                duration: 1.0,
                stagger: 0.0,
                granularity: TextAnimGranularity::Word,
                easing: EasingType::Linear,
                ink_from: Some("#FF0000".into()),
                ..Default::default()
            }),
            font_px,
        );

        const W: i32 = 900;
        const H: i32 = 200;
        let props = props_for(&rt);

        let layout = RichText::compute_layout(&spans, &rt.style, W as f32, H as f32, None, -1.0);
        let win = |idx: usize| {
            let tok = &layout.lines[0].tokens[idx];
            (tok.x as i32, (tok.x + tok.width) as i32)
        };
        let (w1_x0, w1_x1) = win(0);
        let (w2_x0, w2_x1) = win(1);

        let mean_rgb = |time: f64, x0: i32, x1: i32| -> (f32, f32, f32) {
            let mut surface =
                skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
            {
                let canvas = surface.canvas();
                rt.paint(canvas, W as f32, &props, &ctx_at(time, W as u32, H as u32));
            }
            let snapshot = surface.image_snapshot();
            let info = skia_safe::ImageInfo::new(
                (W, H),
                skia_safe::ColorType::RGBA8888,
                skia_safe::AlphaType::Unpremul,
                None,
            );
            let mut buf = vec![0u8; (W * H * 4) as usize];
            assert!(snapshot.read_pixels(
                &info,
                &mut buf,
                (W * 4) as usize,
                skia_safe::IPoint::new(0, 0),
                skia_safe::image::CachingHint::Disallow,
            ));
            let mut sum = (0u64, 0u64, 0u64);
            let mut alpha_sum = 0u64;
            for y in 0..H {
                for x in x0.max(0)..x1.min(W) {
                    let i = ((y * W + x) * 4) as usize;
                    let a = buf[i + 3] as u64;
                    sum.0 += buf[i] as u64 * a;
                    sum.1 += buf[i + 1] as u64 * a;
                    sum.2 += buf[i + 2] as u64 * a;
                    alpha_sum += a;
                }
            }
            assert!(
                alpha_sum > 0,
                "some inked pixels must exist in this window at t={time}"
            );
            (
                sum.0 as f32 / alpha_sum as f32,
                sum.1 as f32 / alpha_sum as f32,
                sum.2 as f32 / alpha_sum as f32,
            )
        };

        let (r1_early, g1_early, b1_early) = mean_rgb(0.05, w1_x0, w1_x1);
        let (r2_early, g2_early, b2_early) = mean_rgb(0.05, w2_x0, w2_x1);
        assert!(
            r1_early > g1_early && r1_early > b1_early,
            "span 1 should read close to ink_from (red) early on, got rgb=({r1_early},{g1_early},{b1_early})"
        );
        assert!(
            r2_early > g2_early && r2_early > b2_early,
            "span 2 should read close to ink_from (red) early on, got rgb=({r2_early},{g2_early},{b2_early})"
        );

        let (r1, g1, b1) = mean_rgb(5.0, w1_x0, w1_x1);
        let (r2, g2, b2) = mean_rgb(5.0, w2_x0, w2_x1);
        assert!(
            g1 > r1 && g1 > b1,
            "settled span 1 must converge to ITS OWN colour (green), got rgb=({r1},{g1},{b1})"
        );
        assert!(
            b2 > r2 && b2 > g2,
            "settled span 2 must converge to ITS OWN colour (blue), not span 1's — a shared \
             default would fail this, got rgb=({r2},{g2},{b2})"
        );
    }
}
