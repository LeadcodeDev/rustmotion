use skia_safe::{Canvas, Font, Paint, Rect, TextBlob};

use super::highlight::HighlightedLine;
use super::Codeblock;
use rustmotion_core::engine::animator::ease;
use rustmotion_core::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, measure_text_with_fallback, paint_from_hex,
};
use rustmotion_core::schema::{CodeblockHighlight, RevealMode};

// ─── Reveal ──────────────────────────────────────────────────────────────────

pub(super) fn compute_reveal(
    layer: &Codeblock,
    time: f64,
    highlighted: &[HighlightedLine],
) -> (usize, Option<usize>, f32) {
    let total_lines = highlighted.len();
    if total_lines == 0 {
        return (0, None, 1.0);
    }

    match &layer.reveal {
        None => (total_lines, None, 1.0),
        Some(reveal) => {
            if time < reveal.start {
                return (0, None, 1.0);
            }
            let raw_progress = ((time - reveal.start) / reveal.duration).clamp(0.0, 1.0);
            let progress = ease(raw_progress, &reveal.easing);

            match reveal.mode {
                RevealMode::Typewriter => {
                    let total_chars: usize = highlighted
                        .iter()
                        .map(|l| l.spans.iter().map(|s| s.text.len()).sum::<usize>())
                        .sum();
                    let visible_chars = (total_chars as f64 * progress).round() as usize;
                    let mut chars_remaining = visible_chars;
                    let mut visible_lines = 0;
                    let mut last_line_chars = None;
                    for line in highlighted {
                        let line_chars: usize = line.spans.iter().map(|s| s.text.len()).sum();
                        if chars_remaining >= line_chars {
                            chars_remaining -= line_chars;
                            visible_lines += 1;
                        } else {
                            visible_lines += 1;
                            last_line_chars = Some(chars_remaining);
                            break;
                        }
                    }
                    (visible_lines, last_line_chars, 1.0)
                }
                RevealMode::LineByLine => {
                    let visible_f = total_lines as f64 * progress;
                    let full_lines = visible_f.floor() as usize;
                    let fractional = (visible_f - full_lines as f64) as f32;
                    if full_lines >= total_lines {
                        (total_lines, None, 1.0)
                    } else {
                        (full_lines + 1, None, fractional.max(0.01))
                    }
                }
            }
        }
    }
}

// ─── Line numbers ────────────────────────────────────────────────────────────

pub(super) fn draw_line_numbers(
    canvas: &Canvas,
    font: &Font,
    x: f32,
    y: f32,
    line_height: f32,
    visible_lines: usize,
) {
    let mut paint = paint_from_hex("#65737E");
    paint.set_anti_alias(true);
    let (_sw, metrics) = font.metrics();
    let ascent = -metrics.ascent;

    for i in 0..visible_lines {
        let num_str = format!("{}", i + 1);
        let num_y = y + i as f32 * line_height + ascent;
        if let Some(blob) = TextBlob::new(&num_str, font) {
            canvas.draw_text_blob(&blob, (x + 12.0, num_y), &paint);
        }
    }
}

/// Draw a single line number at an arbitrary Y with given opacity
pub(super) fn draw_line_number_at(
    canvas: &Canvas,
    font: &Font,
    x: f32,
    y: f32,
    num: usize,
    opacity: f32,
) {
    let num_str = format!("{}", num);
    let mut paint = paint_from_hex("#65737E");
    paint.set_anti_alias(true);
    paint.set_alpha_f(opacity);
    if let Some(blob) = TextBlob::new(&num_str, font) {
        canvas.draw_text_blob(&blob, (x + 12.0, y), &paint);
    }
}

// ─── Highlights ──────────────────────────────────────────────────────────────

pub(super) fn draw_highlights(
    canvas: &Canvas,
    highlights: &[CodeblockHighlight],
    time: f64,
    x: f32,
    y: f32,
    line_height: f32,
    width: f32,
) {
    for hl in highlights {
        if let Some(start) = hl.start {
            if time < start {
                continue;
            }
        }
        if let Some(end) = hl.end {
            if time > end {
                continue;
            }
        }
        let mut hl_paint = paint_from_hex(&hl.color);
        hl_paint.set_anti_alias(false);

        // Sort line numbers and merge consecutive runs into single rects
        // to avoid sub-pixel seams between adjacent highlight lines.
        let mut sorted_lines: Vec<u32> = hl.lines.iter().copied().filter(|&n| n > 0).collect();
        sorted_lines.sort_unstable();
        sorted_lines.dedup();

        let mut i = 0;
        while i < sorted_lines.len() {
            let run_start = sorted_lines[i] - 1; // 0-based
            let mut run_end = run_start;
            while i + 1 < sorted_lines.len() && sorted_lines[i + 1] == sorted_lines[i] + 1 {
                i += 1;
                run_end = sorted_lines[i] - 1;
            }
            let ry = (y + run_start as f32 * line_height).floor();
            let ry_end = (y + (run_end + 1) as f32 * line_height).ceil();
            let hl_rect = Rect::from_ltrb(x.floor(), ry, (x + width).ceil(), ry_end);
            canvas.draw_rect(hl_rect, &hl_paint);
            i += 1;
        }
    }
}

// ─── Draw highlighted lines ──────────────────────────────────────────────────

pub(super) fn draw_highlighted_lines(
    canvas: &Canvas,
    highlighted: &[HighlightedLine],
    font: &Font,
    x: f32,
    y: f32,
    line_height: f32,
    visible_lines: usize,
    visible_chars_last_line: Option<usize>,
    last_line_opacity: f32,
) {
    let (_sw, metrics) = font.metrics();
    let ascent = -metrics.ascent;

    for (i, line) in highlighted.iter().enumerate() {
        if i >= visible_lines {
            break;
        }
        let is_last_visible = i == visible_lines - 1;
        let line_y = y + i as f32 * line_height + ascent;
        let char_limit = if is_last_visible {
            visible_chars_last_line
        } else {
            None
        };
        let opacity = if is_last_visible && last_line_opacity < 1.0 {
            last_line_opacity
        } else {
            1.0
        };
        draw_single_highlighted_line_partial(canvas, line, font, x, line_y, opacity, char_limit);
    }
}

pub(super) fn draw_single_highlighted_line_partial(
    canvas: &Canvas,
    line: &HighlightedLine,
    font: &Font,
    x: f32,
    y: f32,
    opacity: f32,
    char_limit: Option<usize>,
) {
    let mut cursor_x = x;
    let mut chars_drawn = 0usize;

    for span in &line.spans {
        let text_to_draw = if let Some(limit) = char_limit {
            let remaining = limit.saturating_sub(chars_drawn);
            if remaining == 0 {
                break;
            }
            let chars: Vec<char> = span.text.chars().collect();
            let take = remaining.min(chars.len());
            chars[..take].iter().collect::<String>()
        } else {
            span.text.clone()
        };

        if text_to_draw.is_empty() {
            chars_drawn += span.text.len();
            continue;
        }

        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        paint.set_color4f(
            skia_safe::Color4f::new(
                span.r as f32 / 255.0,
                span.g as f32 / 255.0,
                span.b as f32 / 255.0,
                (span.a as f32 / 255.0) * opacity,
            ),
            None,
        );

        let emoji_f = emoji_typeface().map(|tf| Font::from_typeface(tf, font.size()));
        draw_text_with_fallback(
            canvas,
            &text_to_draw,
            font,
            &emoji_f,
            0.0,
            cursor_x,
            y,
            &paint,
        );
        let w = measure_text_with_fallback(&text_to_draw, font, &emoji_f, 0.0);
        cursor_x += w;
        chars_drawn += text_to_draw.len();

        if let Some(limit) = char_limit {
            if chars_drawn >= limit {
                break;
            }
        }
    }
}

pub(super) fn draw_single_highlighted_line(
    canvas: &Canvas,
    line: &HighlightedLine,
    font: &Font,
    x: f32,
    y: f32,
    opacity: f32,
) {
    let mut cursor_x = x;
    for span in &line.spans {
        if span.text.is_empty() {
            continue;
        }
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        paint.set_color4f(
            skia_safe::Color4f::new(
                span.r as f32 / 255.0,
                span.g as f32 / 255.0,
                span.b as f32 / 255.0,
                (span.a as f32 / 255.0) * opacity,
            ),
            None,
        );
        let emoji_f = emoji_typeface().map(|tf| Font::from_typeface(tf, font.size()));
        draw_text_with_fallback(canvas, &span.text, font, &emoji_f, 0.0, cursor_x, y, &paint);
        let w = measure_text_with_fallback(&span.text, font, &emoji_f, 0.0);
        cursor_x += w;
    }
}
