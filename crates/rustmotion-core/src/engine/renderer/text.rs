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

/// Wrap text respecting emoji font fallback for accurate measurement,
/// honouring `letter_spacing` in the fit test itself (issue #125).
///
/// This is the correct entry point for any caller whose paint step applies
/// non-zero `letter-spacing` (i.e. it also calls
/// [`measure_text_with_fallback`] / [`draw_text_with_fallback`] with a
/// non-zero `letter_spacing`): the word-fits-on-this-line test below now
/// measures with the *same* tracking that will actually be painted, so the
/// line count this function returns is the line count the paint step will
/// agree with.
///
/// Before this existed, every caller went through
/// [`wrap_text_with_fallback`], whose fit test always measured at zero
/// tracking regardless of the real value. That is harmless when
/// `letter_spacing == 0.0`, but wrong otherwise in a specific and dangerous
/// way for *negative* tracking (the register this engine targets: tight
/// negative tracking on 200–400px display type): negative tracking makes
/// the real painted line narrower than the zero-tracking fit test believes,
/// so the fit test can decide a line needs to wrap when the real, tighter
/// text would still have fit on one line — and because the box that holds
/// the text is sized from one width sample while the paint step re-derives
/// the same (still zero-tracking) wrap decision against a *different*
/// available width later in layout, the two passes can disagree on the line
/// count, leaving painted lines centered inside a box sized for a different
/// number of lines.
pub fn wrap_text_with_tracking(
    text: &str,
    font: &Font,
    emoji_font: &Option<Font>,
    max_width: Option<f32>,
    letter_spacing: f32,
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

            let width = measure_text_with_fallback(&test, font, emoji_font, letter_spacing);
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

/// Wrap text respecting emoji font fallback for accurate measurement.
///
/// **Known gap (issue #125):** the fit test below always measures at zero
/// letter-spacing, regardless of what the caller will actually paint with.
/// Kept at its original signature/behaviour for source compatibility with
/// existing call sites (`rustmotion-components/src/intrinsic.rs`,
/// `text.rs`, `shape.rs`, `gradient_text.rs` — outside this workstream's
/// file scope) that measure and paint with real `letter-spacing` but still
/// wrap through this function. **New call sites, and any existing call site
/// whose text can carry non-zero `letter-spacing`, should call
/// [`wrap_text_with_tracking`] instead** — it is a straight drop-in (same
/// signature plus one trailing `letter_spacing: f32` argument) that fixes
/// the measure/paint disagreement described there.
pub fn wrap_text_with_fallback(
    text: &str,
    font: &Font,
    emoji_font: &Option<Font>,
    max_width: Option<f32>,
) -> Vec<String> {
    wrap_text_with_tracking(text, font, emoji_font, max_width, 0.0)
}

// ─── Tests: issue #125 — letter-spacing-aware line breaking ───────────────

#[cfg(test)]
mod tracking_tests {
    use super::super::typeface_with_fallback;
    use super::*;
    use skia_safe::{surfaces, AlphaType, Color, ColorType, FontStyle as SkFontStyle, ImageInfo};

    /// A real bold typeface at `size`, via the same fallback chain
    /// (`typeface_with_fallback`) the renderer uses. Doesn't depend on any
    /// particular family being installed (falls through to Helvetica/Arial/
    /// the OS default), so this is safe in any CI environment.
    fn bold_font(size: f32) -> Font {
        let typeface = typeface_with_fallback("Helvetica", SkFontStyle::bold())
            .expect("host must have a fallback typeface");
        Font::from_typeface(typeface, size)
    }

    // ---- 1. The wrap decision must use real tracking, not 0.0 ----

    /// Direct reproduction of issue #125 §1's headline defect ("gains it a
    /// line break it did not have at zero"): negative tracking narrows the
    /// real painted width below the zero-tracking fit-test's estimate, so
    /// there is a width window — real width <= max_w < zero-tracking width —
    /// where the buggy zero-tracking test wraps to a second line that the
    /// real, tighter text did not need.
    #[test]
    fn negative_tracking_gains_an_unneeded_break_in_old_wrap_but_not_new() {
        let text = "THAT MOVES";
        for font_size in [240.0f32, 290.0, 300.0] {
            let font = bold_font(font_size);
            let letter_spacing = -9.0f32;

            let real_width = measure_text_with_fallback(text, &font, &None, letter_spacing);
            let zero_width = measure_text_with_fallback(text, &font, &None, 0.0);
            assert!(
                zero_width > real_width,
                "negative tracking must make the real width narrower than the \
                 zero-tracking estimate at {font_size}px (real={real_width}, zero={zero_width})"
            );

            // A width inside the window the bug lives in: the real (tracked)
            // string fits, but the zero-tracking fit test disagrees.
            let max_w = (real_width + zero_width) / 2.0;

            let old_lines = wrap_text_with_fallback(text, &font, &None, Some(max_w));
            let new_lines =
                wrap_text_with_tracking(text, &font, &None, Some(max_w), letter_spacing);

            assert_eq!(
                old_lines.len(),
                2,
                "old (zero-tracking) wrap should wrongly split at {font_size}px, got {old_lines:?}"
            );
            assert_eq!(
                new_lines.len(),
                1,
                "new (tracking-aware) wrap should correctly keep one line at {font_size}px, \
                 matching what {max_w}px >= real width {real_width}px allows, got {new_lines:?}"
            );
        }
    }

    /// The fixed wrap decision must be *stable*: re-evaluated at any width
    /// sample that is consistent with the real (tracked) content width, it
    /// always agrees on line count. This is the structural property whose
    /// absence causes issue #125 §1's "the box is sized for one line, paint
    /// decides two are needed" — two different width samples taken during
    /// layout (the intrinsic-measure pass vs. the final paint pass) must not
    /// be able to make the same content wrap differently.
    #[test]
    fn tracking_aware_wrap_agrees_across_width_samples_old_wrap_does_not() {
        let text = "THAT MOVES";
        let font = bold_font(290.0);
        let letter_spacing = -9.0f32;
        let real_width = measure_text_with_fallback(text, &font, &None, letter_spacing);
        let zero_width = measure_text_with_fallback(text, &font, &None, 0.0);

        // Two plausible width samples that both satisfy "the real content
        // fits": comfortably above the zero-tracking width, and just above
        // the real tracked width (inside the bug's window).
        let sample_a = zero_width + 24.0;
        let sample_b = real_width + 1.0;
        assert!(
            sample_b < zero_width,
            "test setup: sample_b must be inside the bug window"
        );

        let old_a = wrap_text_with_fallback(text, &font, &None, Some(sample_a)).len();
        let old_b = wrap_text_with_fallback(text, &font, &None, Some(sample_b)).len();
        let new_a =
            wrap_text_with_tracking(text, &font, &None, Some(sample_a), letter_spacing).len();
        let new_b =
            wrap_text_with_tracking(text, &font, &None, Some(sample_b), letter_spacing).len();

        assert_ne!(
            old_a, old_b,
            "reproduction: old wrap must disagree across the two width samples \
             (sample_a={sample_a}, sample_b={sample_b})"
        );
        assert_eq!(
            new_a, new_b,
            "fix: new wrap must agree across the two width samples \
             (sample_a={sample_a}, sample_b={sample_b})"
        );
        assert_eq!(
            new_a, 1,
            "both samples satisfy the real tracked width, so 1 line is correct"
        );
    }

    /// `wrap_text_with_fallback` (the pre-existing, still-used-by-default
    /// name) must keep its exact old behaviour — 0.0 tracking — so every
    /// existing call site's output is unchanged until it opts in to
    /// `wrap_text_with_tracking`.
    #[test]
    fn wrap_text_with_fallback_is_unchanged_zero_tracking_behaviour() {
        let text = "THAT MOVES";
        let font = bold_font(290.0);
        let max_w = 700.0;
        assert_eq!(
            wrap_text_with_fallback(text, &font, &None, Some(max_w)),
            wrap_text_with_tracking(text, &font, &None, Some(max_w), 0.0)
        );
    }

    // ---- 2. Real Skia render + measured ink extent ----
    //
    // Reproduces the audit's methodology from issue #125 §1 (render, then
    // measure the ink bounding box) directly against this module's public
    // functions — the primitive this workstream owns — rather than through
    // the full JSON-scenario pipeline. `rustmotion-components/src/
    // intrinsic.rs` and `text.rs` (the actual `text` component's box-sizing
    // and paint code) are outside this workstream's file scope and still
    // call `wrap_text_with_fallback` (0.0 tracking) as of this fix, so this
    // test proves the corrected primitive is centred and self-consistent
    // when used correctly end-to-end — i.e. what the render will look like
    // once those call sites switch to `wrap_text_with_tracking` — not what
    // today's shipped renderer currently outputs for a `text` component.

    /// Draw `lines`, each centred within a `box_width`-wide box at
    /// `box_left`, onto a `w`×`h` black raster, and return the horizontal
    /// ink centre (mean of the leftmost and rightmost non-black pixel
    /// columns across every line) — the same "ink extent" measurement
    /// issue #125's audit table reports.
    fn render_and_measure_ink_centre_x(
        lines: &[String],
        font: &Font,
        letter_spacing: f32,
        box_left: f32,
        box_width: f32,
        line_height: f32,
        top_y: f32,
        w: u32,
        h: u32,
    ) -> Option<f32> {
        let mut surface = surfaces::raster_n32_premul((w as i32, h as i32)).unwrap();
        let canvas = surface.canvas();
        canvas.clear(Color::BLACK);
        let mut paint = Paint::default();
        paint.set_color(Color::WHITE);
        paint.set_anti_alias(true);

        let (_, metrics) = font.metrics();
        let ascent = -metrics.ascent;

        for (i, line) in lines.iter().enumerate() {
            if line.is_empty() {
                continue;
            }
            let advance = measure_text_with_fallback(line, font, &None, letter_spacing);
            let x = box_left + (box_width - advance) / 2.0;
            let y = top_y + i as f32 * line_height + ascent;
            draw_text_with_fallback(canvas, line, font, &None, letter_spacing, x, y, &paint);
        }

        let info = ImageInfo::new(
            (w as i32, h as i32),
            ColorType::RGBA8888,
            AlphaType::Unpremul,
            None,
        );
        let mut buf = vec![0u8; (w * h * 4) as usize];
        surface.read_pixels(&info, &mut buf, (w * 4) as usize, (0, 0));

        let mut min_x: Option<u32> = None;
        let mut max_x: Option<u32> = None;
        for y in 0..h {
            for x in 0..w {
                let idx = ((y * w + x) * 4) as usize;
                if buf[idx] > 40 {
                    min_x = Some(min_x.map_or(x, |m| m.min(x)));
                    max_x = Some(max_x.map_or(x, |m| m.max(x)));
                }
            }
        }
        match (min_x, max_x) {
            (Some(a), Some(b)) => Some((a as f32 + b as f32) / 2.0),
            _ => None,
        }
    }

    /// The sweep from issue #125 §1's audit table — "THAT MOVES", 1920×1080,
    /// centred, `line-height: 0.85` — rendered and measured end-to-end with
    /// the fixed primitive. The wrap decision and the paint centring both
    /// derive from the *same* tracking-aware measurement, so the box the
    /// text is centred in and the lines painted into it never disagree: ink
    /// centre must land on the frame centre (960) at every size, whether or
    /// not tracking causes a wrap.
    #[test]
    fn sweep_that_moves_fixed_primitive_stays_centred() {
        const W: u32 = 1920;
        const H: u32 = 1080;
        const FRAME_CENTRE: f32 = 960.0;
        let text = "THAT MOVES";

        // A max-width tight enough that -9 tracking genuinely changes the
        // wrap outcome at some of these sizes (mirrors the audit's
        // font-size sweep; the exact width is a stand-in for "however wide
        // this headline's card/frame is" — not taken from the audit, which
        // doesn't publish its scenario JSON).
        let box_max_width = 1400.0f32;

        let mut results = Vec::new();
        for &(font_size, letter_spacing) in &[
            (240.0f32, 0.0f32),
            (240.0, -9.0),
            (290.0, -9.0),
            (300.0, -9.0),
        ] {
            let font = bold_font(font_size);
            let line_height = font_size * 0.85;

            let lines =
                wrap_text_with_tracking(text, &font, &None, Some(box_max_width), letter_spacing);
            let box_width = lines
                .iter()
                .map(|l| measure_text_with_fallback(l, &font, &None, letter_spacing))
                .fold(0.0f32, f32::max);
            let box_left = FRAME_CENTRE - box_width / 2.0;
            let top_y = (H as f32 - lines.len() as f32 * line_height) / 2.0;

            let ink_centre = render_and_measure_ink_centre_x(
                &lines,
                &font,
                letter_spacing,
                box_left,
                box_width,
                line_height,
                top_y,
                W,
                H,
            )
            .unwrap_or_else(|| panic!("expected ink at {font_size}px/{letter_spacing}"));

            results.push((font_size, letter_spacing, lines.len(), ink_centre));

            // Tolerance: advance-width centring (what text-align:center
            // actually does, here and in the real painter) is not the same
            // thing as ink-bbox centring — individual glyphs have left/right
            // side-bearings that don't visually balance out, especially
            // comparing e.g. "THAT" vs "MOVES" as separate lines. A few px
            // of optical vs. advance discrepancy is expected and is not the
            // #125 defect (that defect was a 408px miscentre from a wrap/
            // paint disagreement, not glyph-metric noise).
            assert!(
                (ink_centre - FRAME_CENTRE).abs() < 15.0,
                "fixed pipeline ink centre {ink_centre:.1} should be within 15px of \
                 {FRAME_CENTRE} at {font_size}px/{letter_spacing} tracking (lines={})",
                lines.len()
            );
        }

        eprintln!("sweep_that_moves_fixed_primitive_stays_centred (after fix):");
        for (font_size, letter_spacing, line_count, ink_centre) in &results {
            eprintln!(
                "  {font_size}px / {letter_spacing} tracking -> lines={line_count}, ink_centre={ink_centre:.1}"
            );
        }
    }
}
