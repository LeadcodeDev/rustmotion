use skia_safe::{Canvas, Font, Paint, Point, TextBlob};

// ─── Counter formatting ─────────────────────────────────────────────────────

pub fn format_counter_value(
    value: f64,
    decimals: u8,
    separator: &Option<String>,
    prefix: &Option<String>,
    suffix: &Option<String>,
) -> String {
    // Format with decimals
    let formatted_number = format!("{:.prec$}", value, prec = decimals as usize);

    // Apply thousands separator if specified
    let formatted_number = if let Some(sep) = separator {
        let parts: Vec<&str> = formatted_number.split('.').collect();
        let integer_part = parts[0];

        // Handle negative sign
        let (sign, digits) = if let Some(stripped) = integer_part.strip_prefix('-') {
            ("-", stripped)
        } else {
            ("", integer_part)
        };

        let mut result = String::new();
        for (i, ch) in digits.chars().rev().enumerate() {
            if i > 0 && i % 3 == 0 {
                result.insert(0, sep.chars().next().unwrap_or(' '));
            }
            result.insert(0, ch);
        }

        if !sign.is_empty() {
            result.insert_str(0, sign);
        }

        if parts.len() > 1 {
            result.push('.');
            result.push_str(parts[1]);
        }

        result
    } else {
        formatted_number
    };

    // Build final string with prefix/suffix
    let mut result = String::new();
    if let Some(p) = prefix {
        result.push_str(p);
    }
    result.push_str(&formatted_number);
    if let Some(s) = suffix {
        result.push_str(s);
    }
    result
}

// ─── Text utilities ─────────────────────────────────────────────────────────

pub fn wrap_text(text: &str, font: &Font, max_width: Option<f32>) -> Vec<String> {
    let explicit_lines: Vec<&str> = text.split('\n').collect();

    let max_w = match max_width {
        Some(w) => w,
        None => return explicit_lines.iter().map(|s| s.to_string()).collect(),
    };

    let mut result = Vec::new();
    for line in explicit_lines {
        let words: Vec<&str> = line.split_whitespace().collect();
        if words.is_empty() {
            result.push(String::new());
            continue;
        }

        let mut current_line = String::new();
        for word in words {
            let test = if current_line.is_empty() {
                word.to_string()
            } else {
                format!("{} {}", current_line, word)
            };

            let (width, _) = font.measure_str(&test, None);
            if width > max_w && !current_line.is_empty() {
                result.push(current_line);
                current_line = word.to_string();
            } else {
                current_line = test;
            }
        }
        if !current_line.is_empty() {
            result.push(current_line);
        }
    }
    result
}

pub fn make_text_blob_with_spacing(text: &str, font: &Font, spacing: f32) -> Option<TextBlob> {
    let glyphs = font.str_to_glyphs_vec(text);
    if glyphs.is_empty() {
        return None;
    }

    let mut widths = vec![0.0f32; glyphs.len()];
    font.get_widths(&glyphs, &mut widths);

    let mut positions = Vec::with_capacity(glyphs.len());
    let mut x = 0.0f32;
    for (i, _glyph) in glyphs.iter().enumerate() {
        positions.push(Point::new(x, 0.0));
        x += widths[i] + spacing;
    }

    TextBlob::from_pos_text(text, &positions, font)
}

// ─── Emoji support ──────────────────────────────────────────────────────────

/// Check if a character is an emoji or emoji-related codepoint.
fn is_emoji(c: char) -> bool {
    let cp = c as u32;
    matches!(cp,
        // Miscellaneous Symbols and Pictographs (includes skin tone modifiers 1F3FB-1F3FF)
        0x1F300..=0x1F5FF |
        // Emoticons
        0x1F600..=0x1F64F |
        // Transport and Map Symbols
        0x1F680..=0x1F6FF |
        // Supplemental Symbols and Pictographs
        0x1F900..=0x1F9FF |
        // Symbols and Pictographs Extended-A
        0x1FA00..=0x1FA6F |
        // Symbols and Pictographs Extended-B
        0x1FA70..=0x1FAFF |
        // Dingbats (includes ✂️..➰ and arrows/symbols)
        0x2702..=0x27B0 |
        // Miscellaneous Symbols (includes ☀️..⛿)
        0x2600..=0x26FF |
        // Variation Selectors (keep with preceding emoji)
        0xFE00..=0xFE0F |
        // Zero-Width Joiner
        0x200D |
        // Combining Enclosing Keycap
        0x20E3 |
        // Regional Indicator Symbols (flags)
        0x1F1E0..=0x1F1FF |
        // Tags block (flag subdivisions)
        0xE0020..=0xE007F |
        // Playing cards, mahjong
        0x1F004 | 0x1F0CF |
        // Misc technical (⌚ ⌛ ⏩..⏳ ⏸..⏺)
        0x231A..=0x231B |
        0x23E9..=0x23F3 |
        0x23F8..=0x23FA |
        // Arrows and geometric symbols used as emoji
        0x2934..=0x2935 |
        0x25AA..=0x25AB |
        0x25B6 | 0x25C0 |
        0x25FB..=0x25FE |
        // Arrows
        0x2B05..=0x2B07 |
        0x2B1B..=0x2B1C |
        0x2B50 | 0x2B55 |
        // CJK symbols
        0x3030 | 0x303D |
        0x3297 | 0x3299 |
        // Copyright, registered, trademark
        0x00A9 | 0x00AE | 0x2122
    )
}

/// A segment of text that uses either the primary font or the emoji font.
struct TextRun {
    start: usize, // byte offset
    end: usize,   // byte offset
    is_emoji: bool,
}

/// Segment text into runs of emoji vs non-emoji characters.
fn segment_text_runs(text: &str) -> Vec<TextRun> {
    let mut runs = Vec::new();
    let mut chars = text.char_indices().peekable();

    while let Some(&(start_byte, c)) = chars.peek() {
        let emoji = is_emoji(c);
        let mut end_byte = start_byte + c.len_utf8();
        chars.next();

        while let Some(&(_, next_c)) = chars.peek() {
            if is_emoji(next_c) != emoji {
                break;
            }
            end_byte += next_c.len_utf8();
            chars.next();
        }

        runs.push(TextRun {
            start: start_byte,
            end: end_byte,
            is_emoji: emoji,
        });
    }
    runs
}

/// Check if text contains any emoji characters.
pub fn has_emoji(text: &str) -> bool {
    text.chars().any(is_emoji)
}

/// Draw a text line with emoji font fallback.
/// If `emoji_font` is None, falls back to drawing everything with the primary font.
pub fn draw_text_with_fallback(
    canvas: &Canvas,
    text: &str,
    font: &Font,
    emoji_font: &Option<Font>,
    letter_spacing: f32,
    x: f32,
    y: f32,
    paint: &Paint,
) {
    // Fast path: no emoji font or no emoji in text
    if emoji_font.is_none() || !has_emoji(text) {
        if letter_spacing.abs() > 0.01 {
            if let Some(blob) = make_text_blob_with_spacing(text, font, letter_spacing) {
                canvas.draw_text_blob(&blob, (x, y), paint);
            }
        } else if let Some(blob) = TextBlob::new(text, font) {
            canvas.draw_text_blob(&blob, (x, y), paint);
        }
        return;
    }

    let emoji_font = emoji_font.as_ref().unwrap();
    let runs = segment_text_runs(text);
    let mut cursor_x = x;

    for run in &runs {
        let segment = &text[run.start..run.end];
        let f = if run.is_emoji { emoji_font } else { font };

        if letter_spacing.abs() > 0.01 {
            if let Some(blob) = make_text_blob_with_spacing(segment, f, letter_spacing) {
                canvas.draw_text_blob(&blob, (cursor_x, y), paint);
            }
        } else if let Some(blob) = TextBlob::new(segment, f) {
            canvas.draw_text_blob(&blob, (cursor_x, y), paint);
        }

        // Advance cursor by the measured width of this run
        let (w, _) = f.measure_str(segment, None);
        let extra = if letter_spacing.abs() > 0.01 {
            letter_spacing * (segment.chars().count() as f32 - 1.0).max(0.0)
        } else {
            0.0
        };
        cursor_x += w + extra;
    }
}

/// Measure the width of a text line with emoji font fallback.
pub fn measure_text_with_fallback(
    text: &str,
    font: &Font,
    emoji_font: &Option<Font>,
    letter_spacing: f32,
) -> f32 {
    // Fast path
    if emoji_font.is_none() || !has_emoji(text) {
        let (w, _) = font.measure_str(text, None);
        let extra = if letter_spacing.abs() > 0.01 {
            letter_spacing * (text.chars().count() as f32 - 1.0).max(0.0)
        } else {
            0.0
        };
        return w + extra;
    }

    let emoji_font = emoji_font.as_ref().unwrap();
    let runs = segment_text_runs(text);
    let mut total_w = 0.0f32;

    for run in &runs {
        let segment = &text[run.start..run.end];
        let f = if run.is_emoji { emoji_font } else { font };
        let (w, _) = f.measure_str(segment, None);
        let extra = if letter_spacing.abs() > 0.01 {
            letter_spacing * (segment.chars().count() as f32 - 1.0).max(0.0)
        } else {
            0.0
        };
        total_w += w + extra;
    }
    total_w
}

/// Wrap text respecting emoji font fallback for accurate measurement.
pub fn wrap_text_with_fallback(
    text: &str,
    font: &Font,
    emoji_font: &Option<Font>,
    max_width: Option<f32>,
) -> Vec<String> {
    let explicit_lines: Vec<&str> = text.split('\n').collect();

    let max_w = match max_width {
        Some(w) => w,
        None => return explicit_lines.iter().map(|s| s.to_string()).collect(),
    };

    let mut result = Vec::new();
    for line in explicit_lines {
        let words: Vec<&str> = line.split_whitespace().collect();
        if words.is_empty() {
            result.push(String::new());
            continue;
        }

        let mut current_line = String::new();
        for word in words {
            let test = if current_line.is_empty() {
                word.to_string()
            } else {
                format!("{} {}", current_line, word)
            };

            let width = measure_text_with_fallback(&test, font, emoji_font, 0.0);
            if width > max_w && !current_line.is_empty() {
                result.push(current_line);
                current_line = word.to_string();
            } else {
                current_line = test;
            }
        }
        if !current_line.is_empty() {
            result.push(current_line);
        }
    }
    result
}
