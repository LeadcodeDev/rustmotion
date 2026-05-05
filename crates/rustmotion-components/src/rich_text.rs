use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Font, FontStyle};

use rustmotion_core::css::style::{FontStyle as CssFontStyle, FontWeight as CssFontWeight, FontWeightKw, TextAlign as CssTextAlign};
use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{font_mgr, paint_from_hex, draw_text_with_fallback, measure_text_with_fallback, emoji_typeface};
use rustmotion_core::schema::{AnimationEffect, FontStyleType, FontWeight, TextAlign, TimelineStep};
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
    #[serde(default, deserialize_with = "rustmotion_core::schema::deserialize_animation_effects")]
    pub animation: Vec<AnimationEffect>,
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

/// Resolve a font for a span, inheriting from parent style defaults.
fn make_font(
    family: &str,
    weight: &FontWeight,
    font_style_type: &FontStyleType,
    size: f32,
) -> Font {
    let fm = font_mgr();
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
    let typeface = fm
        .match_family_style(family, skia_style)
        .or_else(|| fm.match_family_style("Helvetica", skia_style))
        .or_else(|| fm.match_family_style("Arial", skia_style))
        .or_else(|| fm.match_family_style("sans-serif", skia_style))
        .unwrap_or_else(|| {
            fm.legacy_make_typeface(None, skia_style).expect("No fallback font")
        });
    Font::from_typeface(typeface, size)
}

/// A prepared span ready for rendering (with resolved font, paint, measurements).
struct PreparedSpan {
    text: String,
    font: Font,
    color: String,
    letter_spacing: f32,
    width: f32,
}

impl RichText {
    fn paint(&self, canvas: &Canvas, layout_width: f32, props: &AnimatedProperties) {
        let default_size = self.style.font_size_px_or(48.0);
        let default_color = self.style.color_str_or("#FFFFFF");
        let default_family = self.style.font_family_or("Inter");
        let default_weight = match &self.style.font_weight {
            Some(CssFontWeight::Keyword(FontWeightKw::Bold | FontWeightKw::Bolder)) => FontWeight::Bold,
            Some(CssFontWeight::Number(n)) if *n >= 600 => FontWeight::Bold,
            Some(CssFontWeight::Number(n)) => FontWeight::Weight(*n),
            _ => FontWeight::Normal,
        };
        let default_font_style = match self.style.font_style {
            Some(CssFontStyle::Italic) => FontStyleType::Italic,
            Some(CssFontStyle::Oblique) => FontStyleType::Oblique,
            _ => FontStyleType::Normal,
        };
        let align = match self.style.text_align {
            Some(CssTextAlign::Center) => TextAlign::Center,
            Some(CssTextAlign::Right | CssTextAlign::End) => TextAlign::Right,
            _ => TextAlign::Left,
        };
        let line_height_val = self.style.line_height_for(default_size);

        let emoji_tf = emoji_typeface();

        // Prepare all spans with their fonts and measurements
        let mut prepared: Vec<PreparedSpan> = self.spans.iter().map(|span| {
            let size = span.font_size.unwrap_or(default_size);
            let family = span.font_family.as_deref().unwrap_or(default_family);
            let weight = span.font_weight.as_ref().unwrap_or(&default_weight);
            let fstyle = span.font_style.as_ref().unwrap_or(&default_font_style);
            let color = span.color.as_deref().unwrap_or(default_color).to_string();
            let letter_spacing = span.letter_spacing.unwrap_or(0.0);
            let font = make_font(family, weight, fstyle, size);
            let emoji_font = emoji_tf.as_ref().map(|tf| Font::from_typeface(tf.clone(), size));
            let width = measure_text_with_fallback(&span.text, &font, &emoji_font, letter_spacing);

            PreparedSpan { text: span.text.clone(), font, color, letter_spacing, width }
        }).collect();

        // Typewriter animation: truncate spans based on visible_chars_progress
        if props.visible_chars_progress >= 0.0 {
            let total_chars: usize = prepared.iter().map(|ps| ps.text.chars().count()).sum();
            let visible = ((props.visible_chars_progress * total_chars as f32).round() as usize).min(total_chars);
            if visible == 0 {
                return;
            }
            if visible < total_chars {
                let mut remaining = visible;
                let mut truncated = Vec::new();
                for mut ps in prepared.into_iter() {
                    let char_count = ps.text.chars().count();
                    if remaining >= char_count {
                        remaining -= char_count;
                        truncated.push(ps);
                    } else {
                        let truncated_text: String = ps.text.chars().take(remaining).collect();
                        let emoji_font = emoji_tf.as_ref().map(|tf| Font::from_typeface(tf.clone(), ps.font.size()));
                        ps.width = measure_text_with_fallback(&truncated_text, &ps.font, &emoji_font, ps.letter_spacing);
                        ps.text = truncated_text;
                        truncated.push(ps);
                        break;
                    }
                }
                prepared = truncated;
            }
        }

        let wrap_width = if layout_width.is_finite() && layout_width > 0.0 {
            match self.max_width {
                Some(mw) => mw.min(layout_width),
                None => layout_width,
            }
        } else {
            self.max_width.unwrap_or(f32::INFINITY)
        };

        // Simple line-breaking: pack spans into lines
        // A span stays on the current line if it fits; otherwise start a new line
        struct LineSpan {
            span_idx: usize,
            x: f32,
        }
        struct Line {
            spans: Vec<LineSpan>,
            width: f32,
        }

        let mut lines: Vec<Line> = vec![Line { spans: vec![], width: 0.0 }];

        for (i, ps) in prepared.iter().enumerate() {
            let current = lines.last_mut().unwrap();
            if current.width + ps.width > wrap_width && !current.spans.is_empty() {
                // Start new line
                lines.push(Line {
                    spans: vec![LineSpan { span_idx: i, x: 0.0 }],
                    width: ps.width,
                });
            } else {
                let x = current.width;
                current.spans.push(LineSpan { span_idx: i, x });
                current.width += ps.width;
            }
        }

        let align_width = if layout_width.is_finite() && layout_width > 0.0 {
            layout_width
        } else {
            lines.iter().map(|l| l.width).fold(0.0f32, f32::max)
        };

        // Find max ascent for baseline alignment
        let max_ascent = prepared.iter().map(|ps| {
            let (_, m) = ps.font.metrics();
            -m.ascent
        }).fold(0.0f32, f32::max);

        let baseline_offset = (line_height_val + max_ascent) / 2.0;

        for (line_idx, line) in lines.iter().enumerate() {
            let line_x_offset = match align {
                TextAlign::Left => 0.0,
                TextAlign::Center => (align_width - line.width) / 2.0,
                TextAlign::Right => align_width - line.width,
            };

            let y = line_idx as f32 * line_height_val + baseline_offset;

            for ls in &line.spans {
                let ps = &prepared[ls.span_idx];
                let paint = paint_from_hex(&ps.color);
                let emoji_font = emoji_tf.as_ref().map(|tf| Font::from_typeface(tf.clone(), ps.font.size()));

                draw_text_with_fallback(
                    canvas,
                    &ps.text,
                    &ps.font,
                    &emoji_font,
                    ps.letter_spacing,
                    line_x_offset + ls.x,
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
        _ctx: &PaintCtx,
    ) {
        self.paint(canvas, layout.width, props);
    }
}
