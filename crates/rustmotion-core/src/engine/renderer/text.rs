use skia_safe::{Canvas, Font, Paint, Point, TextBlob, Typeface};

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

/// Code points that render as emoji **by default**, in every context,
/// regardless of any following variation selector — genuine pictograph
/// blocks (Miscellaneous Symbols and Pictographs, Emoticons, Transport,
/// Supplemental Symbols/Pictographs, flags, keycaps, ZWJ sequences...).
/// Nothing in these ranges has a meaningful plain-text rendering, so there
/// is no narrowing to do here — contrast with
/// [`is_text_presentation_by_default`], which needs one (audit #8).
fn is_emoji_presentation_default(c: char) -> bool {
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
        // Misc technical, unconditionally emoji (⏩..⏳ ⏸..⏺)
        0x23E9..=0x23F3 |
        0x23F8..=0x23FA |
        // Arrows and geometric symbols, unconditionally emoji
        0x2B1B..=0x2B1C |
        0x2B50 | 0x2B55
    )
}

/// Code points that are TEXT-presentation **by default** — a normal glyph
/// in the primary font, honouring `style.color` — but that Unicode still
/// marks `Emoji=Yes`: they render as color emoji only when the author
/// explicitly opts in with a following U+FE0F variation selector (audit
/// #8). Routing these unconditionally to the emoji font (the previous
/// behaviour, lumped in with [`is_emoji_presentation_default`]) either
/// painted a color bitmap that ignores `style.color` (✔, ©, ®, ™ — all
/// covered by Apple Color Emoji) or a `.notdef` tofu square for code points
/// the emoji font itself doesn't cover even though the primary font does
/// (✓ U+2713 — absent from Apple Color Emoji even though U+2714 sits one
/// code point over and is present).
fn is_text_presentation_by_default(c: char) -> bool {
    let cp = c as u32;
    matches!(cp,
        // Copyright, registered, trademark
        0x00A9 | 0x00AE | 0x2122 |
        // Misc technical (⌚ ⌛)
        0x231A..=0x231B |
        // Miscellaneous Symbols (☀️..⛿)
        0x2600..=0x26FF |
        // Dingbats (✂️..➰ and arrows/symbols), includes ✓/✔ U+2713/2714
        0x2702..=0x27B0 |
        // Arrows and geometric symbols used as emoji only with VS16
        0x2934..=0x2935 |
        0x25AA..=0x25AB |
        0x25B6 | 0x25C0 |
        0x25FB..=0x25FE |
        0x2B05..=0x2B07 |
        // CJK symbols
        0x3030 | 0x303D |
        0x3297 | 0x3299
    )
}

/// Variation Selector-16 — forces the *preceding* text-presentation-default
/// code point (see [`is_text_presentation_by_default`]) into emoji
/// presentation. Narrower than the old catch-all `0xFE00..=0xFE0F` range:
/// VS-15 (U+FE0E, forces *text* presentation) and the rest of that block
/// are not emoji-forcing.
const VARIATION_SELECTOR_EMOJI: char = '\u{FE0F}';

/// True if `c` should be painted/measured with the emoji font, given the
/// character immediately following it (`next`). Needed because
/// [`is_text_presentation_by_default`] code points only opt into emoji
/// presentation when explicitly followed by U+FE0F — a bare lookup of `c`
/// alone can't tell "©" (text) from "©️" (explicit emoji presentation)
/// apart.
fn char_wants_emoji_font(c: char, next: Option<char>) -> bool {
    if c == VARIATION_SELECTOR_EMOJI {
        return true; // always grouped with whatever code point selected it
    }
    if is_emoji_presentation_default(c) {
        return true;
    }
    if is_text_presentation_by_default(c) {
        return next == Some(VARIATION_SELECTOR_EMOJI);
    }
    false
}

/// Which font a run of text should be painted/measured with.
enum RunKind {
    Primary,
    Emoji,
    /// #3: a system fallback typeface resolved for a code point the
    /// primary font doesn't cover (e.g. CJK/Arabic/Devanagari when only a
    /// Latin `font-family` was requested).
    Fallback(Typeface),
}

/// True if `a` and `b` are the "same" run kind for the purpose of merging
/// adjacent characters into one run. Two `Fallback` runs merge only if they
/// resolved to the *same* typeface (compared by Skia's unique id) — two
/// characters that both need fallback but belong to different scripts
/// (e.g. mixed CJK + Arabic) must not merge into a single run painted with
/// only one of the two fonts.
fn same_run_kind(a: &RunKind, b: &RunKind) -> bool {
    match (a, b) {
        (RunKind::Primary, RunKind::Primary) => true,
        (RunKind::Emoji, RunKind::Emoji) => true,
        (RunKind::Fallback(ta), RunKind::Fallback(tb)) => ta.unique_id() == tb.unique_id(),
        _ => false,
    }
}

/// True if `font` has an actual glyph (not `.notdef`, glyph id 0) for every
/// character in `text`. Used both to gate whether a code point needs a
/// fallback lookup at all (#3), and as a coverage guard before actually
/// using the emoji font for a run classified as emoji (#8): some code
/// points Unicode marks emoji-capable are, on a given platform, absent
/// from the color emoji font even though the primary font has a perfectly
/// good text glyph for them (U+2713 on Apple Color Emoji).
fn font_covers(font: &Font, text: &str) -> bool {
    let glyphs = font.str_to_glyphs_vec(text);
    !glyphs.is_empty() && glyphs.iter().all(|&g| g != 0)
}

/// Classify a single character's font choice: emoji presentation first
/// (#8), then primary-font glyph coverage, then a system fallback typeface
/// resolved via [`super::fallback_typeface_for_char`] for the primary
/// font's own `(family, style)` (#3). Whitespace/control code points are
/// always `Primary` (assumed universally present, or invisible) so the
/// fallback lookup isn't triggered by the spaces between same-script words.
fn classify_char(c: char, primary: &Font, next: Option<char>) -> RunKind {
    if char_wants_emoji_font(c, next) {
        return RunKind::Emoji;
    }
    if c.is_whitespace() || (c as u32) < 0x20 {
        return RunKind::Primary;
    }
    if primary.unichar_to_glyph(c as i32) != 0 {
        return RunKind::Primary;
    }
    let primary_typeface = primary.typeface();
    let style = primary_typeface.font_style();
    let family = primary_typeface.family_name();
    match super::fallback_typeface_for_char(&family, style, c) {
        Some(tf) => RunKind::Fallback(tf),
        // No installed font covers `c` either — same `.notdef` tofu as
        // before this fix, not worse.
        None => RunKind::Primary,
    }
}

/// Segment text into runs, each tagged with which font should paint/measure
/// it (see [`classify_char`]).
fn segment_text_runs(text: &str, primary: &Font) -> Vec<TextRun> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut runs = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let (start_byte, c) = chars[i];
        let next = chars.get(i + 1).map(|&(_, ch)| ch);
        let kind = classify_char(c, primary, next);
        let mut end_byte = start_byte + c.len_utf8();
        i += 1;

        while let Some(&(nb, nc)) = chars.get(i) {
            let nnext = chars.get(i + 1).map(|&(_, ch)| ch);
            let nkind = classify_char(nc, primary, nnext);
            if !same_run_kind(&kind, &nkind) {
                break;
            }
            end_byte = nb + nc.len_utf8();
            i += 1;
        }

        runs.push(TextRun {
            start: start_byte,
            end: end_byte,
            kind,
        });
    }
    runs
}

/// A segment of text tagged with which font it should be painted/measured
/// with (see [`RunKind`]).
struct TextRun {
    start: usize, // byte offset
    end: usize,   // byte offset
    kind: RunKind,
}

/// True if `text` contains any character that wants emoji presentation
/// (#8-aware: honours the VS16 opt-in for text-presentation-default code
/// points, so plain "©"/"✓" no longer count as emoji here).
pub fn has_emoji(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    chars
        .iter()
        .enumerate()
        .any(|(i, &c)| char_wants_emoji_font(c, chars.get(i + 1).copied()))
}

/// True if `text` needs the full run-segmentation machinery in
/// [`draw_text_with_fallback`]/[`measure_text_with_fallback`]: it contains
/// emoji-presentation content (and an emoji font is actually available), or
/// at least one non-whitespace/control code point the primary `font`
/// doesn't cover (#3). When neither is true, every caller's existing
/// single-font fast path is exactly correct and segmentation would be
/// wasted work.
fn needs_segmentation(text: &str, primary: &Font, emoji_font: &Option<Font>) -> bool {
    if emoji_font.is_some() && has_emoji(text) {
        return true;
    }
    text.chars().any(|c| {
        // ASCII short-circuits before the `unichar_to_glyph` FFI call: every
        // font this engine resolves covers printable ASCII, and callers
        // that draw a lot of short spans per frame (codeblock's per-token
        // syntax highlighting, in particular) call this once per span —
        // skipping the Skia round-trip for the overwhelmingly common
        // all-ASCII case keeps #3's fix from adding per-glyph FFI overhead
        // to code that was never affected by the tofu/coverage bug it
        // fixes.
        !(c.is_ascii() || c.is_whitespace() || (c as u32) < 0x20)
            && primary.unichar_to_glyph(c as i32) == 0
    })
}

/// Resolve which `Font` a run should actually be painted/measured with,
/// applying the emoji coverage guard (#8) and building a same-size `Font`
/// from a resolved fallback [`Typeface`] (#3). Returns a borrow of one of
/// the two inputs, or an owned font stored in `owned` to keep the borrow
/// alive at the call site.
fn resolve_run_font<'a>(
    kind: &RunKind,
    segment: &str,
    primary: &'a Font,
    emoji_font: &'a Option<Font>,
    owned: &'a mut Option<Font>,
) -> &'a Font {
    match kind {
        RunKind::Primary => primary,
        RunKind::Emoji => match emoji_font {
            Some(ef) if font_covers(ef, segment.trim_end_matches(VARIATION_SELECTOR_EMOJI)) => ef,
            // Coverage guard (#8): the emoji font doesn't actually have
            // this glyph (or isn't available at all) — fall back to the
            // primary font rather than paint `.notdef`.
            _ => primary,
        },
        RunKind::Fallback(tf) => {
            *owned = Some(Font::from_typeface(tf.clone(), primary.size()));
            owned.as_ref().unwrap()
        }
    }
}

/// Draw a text line with emoji-font and glyph-coverage fallback (#3, #8).
/// If `emoji_font` is None, falls back to drawing everything with the
/// primary font (plus any `#3` system fallback the primary font's coverage
/// gap needs).
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
    // Fast path: nothing here needs emoji presentation or a glyph-coverage
    // fallback — every earlier caller's exact previous behaviour.
    if !needs_segmentation(text, font, emoji_font) {
        if letter_spacing.abs() > 0.01 {
            if let Some(blob) = make_text_blob_with_spacing(text, font, letter_spacing) {
                canvas.draw_text_blob(&blob, (x, y), paint);
            }
        } else if let Some(blob) = TextBlob::new(text, font) {
            canvas.draw_text_blob(&blob, (x, y), paint);
        }
        return;
    }

    let runs = segment_text_runs(text, font);
    let mut cursor_x = x;

    for run in &runs {
        let segment = &text[run.start..run.end];
        let mut owned = None;
        let f = resolve_run_font(&run.kind, segment, font, emoji_font, &mut owned);

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

/// Measure the width of a text line with emoji-font and glyph-coverage
/// fallback (#3, #8) — mirrors [`draw_text_with_fallback`] run-for-run so
/// the width this returns always matches what actually gets painted.
pub fn measure_text_with_fallback(
    text: &str,
    font: &Font,
    emoji_font: &Option<Font>,
    letter_spacing: f32,
) -> f32 {
    // Fast path
    if !needs_segmentation(text, font, emoji_font) {
        let (w, _) = font.measure_str(text, None);
        let extra = if letter_spacing.abs() > 0.01 {
            letter_spacing * (text.chars().count() as f32 - 1.0).max(0.0)
        } else {
            0.0
        };
        return w + extra;
    }

    let runs = segment_text_runs(text, font);
    let mut total_w = 0.0f32;

    for run in &runs {
        let segment = &text[run.start..run.end];
        let mut owned = None;
        let f = resolve_run_font(&run.kind, segment, font, emoji_font, &mut owned);
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

// ─── Tests: #8 — emoji presentation must default to text, not tofu/color ──

#[cfg(test)]
mod emoji_presentation_tests {
    use super::super::{emoji_typeface, typeface_with_fallback};
    use super::*;

    // These assertions are pure code-point classification — no font, no
    // machine dependency — so they hold on every host.

    #[test]
    fn text_presentation_default_symbols_are_not_emoji_without_vs16() {
        // #8's headline repro: ✓ (U+2713) and ✔ (U+2714) — both inside the
        // old unconditional 0x2702..=0x27B0 dingbats range — must NOT be
        // classified as emoji when they appear bare, since Unicode marks
        // them text-presentation by default.
        assert!(
            !char_wants_emoji_font('\u{2713}', None),
            "✓ bare must be text"
        );
        assert!(
            !char_wants_emoji_font('\u{2714}', None),
            "✔ bare must be text"
        );
        // Copyright/registered/trademark: also text-presentation by default.
        assert!(
            !char_wants_emoji_font('\u{00A9}', None),
            "© bare must be text"
        );
        assert!(
            !char_wants_emoji_font('\u{00AE}', None),
            "® bare must be text"
        );
        assert!(
            !char_wants_emoji_font('\u{2122}', None),
            "™ bare must be text"
        );
    }

    #[test]
    fn text_presentation_default_symbols_opt_into_emoji_with_vs16() {
        // Explicit author intent (U+FE0F immediately after) must still be
        // honoured — this is what "presentation *by default*" means.
        assert!(char_wants_emoji_font('\u{2713}', Some('\u{FE0F}')));
        assert!(char_wants_emoji_font('\u{00A9}', Some('\u{FE0F}')));
    }

    #[test]
    fn genuine_pictographs_are_always_emoji_regardless_of_vs16() {
        // Regression guard: the narrowing must not touch the actual emoji
        // blocks (grinning face, etc.) — these have no meaningful text
        // rendering at all.
        assert!(
            char_wants_emoji_font('\u{1F600}', None),
            "😀 must stay emoji"
        );
        assert!(char_wants_emoji_font('\u{1F600}', Some('\u{FE0F}')));
    }

    #[test]
    fn variation_selector_16_itself_is_always_emoji() {
        // So it merges into whatever run selected it, rather than becoming
        // its own (invisible, harmless either way) primary-font run.
        assert!(char_wants_emoji_font('\u{FE0F}', None));
    }

    #[test]
    fn variation_selector_15_does_not_force_emoji() {
        // VS-15 (U+FE0E) forces *text* presentation — must not be conflated
        // with VS-16 the way the old catch-all 0xFE00..=0xFE0F range did.
        assert!(!char_wants_emoji_font('\u{2713}', Some('\u{FE0E}')));
    }

    #[test]
    fn has_emoji_reflects_the_narrowed_classification() {
        assert!(!has_emoji("2713:\u{2713} copyright:\u{00A9}"));
        assert!(has_emoji("checked \u{2713}\u{FE0F}"));
        assert!(has_emoji("grinning \u{1F600}"));
    }

    // ---- render-level reproduction: color and coverage, no emoji font needed ----

    fn helvetica_font(size: f32) -> Font {
        let typeface = typeface_with_fallback("Helvetica", skia_safe::FontStyle::normal())
            .expect("host must have a fallback typeface");
        Font::from_typeface(typeface, size)
    }

    /// Renders `text` at `size` in white and returns `(ink_pixel_count,
    /// mean_r, mean_g, mean_b)` over every non-transparent pixel.
    fn render_and_sample(text: &str, size: f32) -> (usize, f64, f64, f64) {
        use skia_safe::{surfaces, AlphaType, Color, ColorType, ImageInfo};
        const W: i32 = 200;
        const H: i32 = 200;
        let font = helvetica_font(size);
        let emoji_font = emoji_typeface().map(|tf| Font::from_typeface(tf, size));
        let mut surface = surfaces::raster_n32_premul((W, H)).unwrap();
        let canvas = surface.canvas();
        canvas.clear(Color::BLACK);
        let mut paint = Paint::default();
        paint.set_color(Color::WHITE);
        paint.set_anti_alias(true);
        let (_, metrics) = font.metrics();
        let ascent = -metrics.ascent;
        draw_text_with_fallback(
            canvas,
            text,
            &font,
            &emoji_font,
            0.0,
            10.0,
            ascent + 10.0,
            &paint,
        );

        let info = ImageInfo::new((W, H), ColorType::RGBA8888, AlphaType::Unpremul, None);
        let mut buf = vec![0u8; (W * H * 4) as usize];
        surface.read_pixels(&info, &mut buf, (W * 4) as usize, (0, 0));

        let (mut n, mut sr, mut sg, mut sb) = (0usize, 0f64, 0f64, 0f64);
        for px in buf.chunks_exact(4) {
            if px[3] > 40 {
                n += 1;
                sr += px[0] as f64;
                sg += px[1] as f64;
                sb += px[2] as f64;
            }
        }
        if n == 0 {
            (0, 0.0, 0.0, 0.0)
        } else {
            (n, sr / n as f64, sg / n as f64, sb / n as f64)
        }
    }

    #[test]
    fn bare_check_mark_paints_requested_white_not_tofu_or_color_bitmap() {
        // #8 render-level reproduction, environment-independent: whether or
        // not an emoji font is installed on this host is irrelevant here —
        // with the fix, U+2713 alone is classified as *text*, so
        // `draw_text_with_fallback` never even looks at `emoji_font` for
        // it. Guard on the primary font actually covering the glyph so
        // this doesn't fail on some exotic host where even Helvetica lacks
        // it (the audit's own finding: Helvetica/Menlo DO have it, only
        // Apple Color Emoji doesn't).
        let font = helvetica_font(64.0);
        if !font_covers(&font, "\u{2713}") {
            eprintln!("skip: primary font doesn't cover U+2713 on this host");
            return;
        }
        let (ink, r, g, b) = render_and_sample("\u{2713}", 64.0);
        assert!(ink > 20, "expected visible ink for ✓, got {ink} pixels");
        // White request -> painted channels should be bright and roughly
        // neutral (not a dark/colored emoji-bitmap tint).
        assert!(
            r > 150.0 && g > 150.0 && b > 150.0,
            "✓ should paint near-white, got mean rgb=({r:.0},{g:.0},{b:.0})"
        );
    }
}

// ─── Tests: #3 — glyph fallback for scripts the primary font doesn't cover ─

#[cfg(test)]
mod glyph_fallback_tests {
    use super::super::{fallback_typeface_for_char, typeface_with_fallback};
    use super::*;

    fn helvetica_font(size: f32) -> Font {
        let typeface = typeface_with_fallback("Helvetica", skia_safe::FontStyle::normal())
            .expect("host must have a fallback typeface");
        Font::from_typeface(typeface, size)
    }

    #[test]
    fn font_covers_detects_missing_glyphs_deterministically() {
        // The detection mechanism itself, independent of whatever fallback
        // font may or may not be installed: Helvetica (guaranteed present
        // by `typeface_with_fallback`'s own contract) covers plain ASCII
        // and does not cover CJK.
        let font = helvetica_font(32.0);
        assert!(font_covers(&font, "ABC"), "Helvetica must cover ASCII");
        assert!(
            !font_covers(&font, "\u{4F60}\u{597D}"), // 你好
            "Helvetica must not cover CJK"
        );
    }

    #[test]
    fn classify_char_stays_primary_for_a_codepoint_no_font_covers() {
        // A Private Use Area code point is uncovered by the primary font
        // (nothing standard assigns glyphs there); it may or may not be
        // "covered" by *some* installed font's own PUA convention (icon
        // fonts, vendor glyphs) depending on the host, so skip gracefully
        // rather than assume every machine agrees — the point of this test
        // is that `classify_char` doesn't crash/loop when nothing covers a
        // code point, falling back to `Primary` (the pre-fix, tofu-
        // producing behaviour) rather than panicking.
        let font = helvetica_font(32.0);
        let pua = '\u{E000}';
        assert!(
            !font_covers(&font, &pua.to_string()),
            "test setup: PUA code point must be uncovered by the primary font"
        );
        let style = font.typeface().font_style();
        let family = font.typeface().family_name();
        if fallback_typeface_for_char(&family, style, pua).is_some() {
            eprintln!(
                "skip: host has some font claiming to cover U+E000 (PUA) — can't exercise the \
                 'nothing covers it anywhere' branch on this host"
            );
            return;
        }
        let kind = classify_char(pua, &font, None);
        assert!(
            matches!(kind, RunKind::Primary),
            "an uncoverable-anywhere code point must resolve to Primary, not panic"
        );
    }

    #[test]
    fn classify_char_resolves_a_fallback_when_the_host_has_one() {
        // Environment-dependent by nature (issue: CJK coverage depends on
        // installed fonts) — skip gracefully, per this workstream's brief,
        // rather than asserting a specific glyph renders. When a fallback
        // *is* available (true on stock macOS/Windows/most Linux desktops
        // via Noto/PingFang/MS-Gothic-class fonts), `classify_char` itself
        // — not just the underlying `fallback_typeface_for_char` primitive
        // — must actually route to it, and the resolved typeface must
        // cover the character that triggered the lookup.
        let font = helvetica_font(32.0);
        let style = font.typeface().font_style();
        let family = font.typeface().family_name();
        if fallback_typeface_for_char(&family, style, '\u{4F60}').is_none() {
            eprintln!("skip: no CJK-capable font installed on this host");
            return;
        }
        let kind = classify_char('\u{4F60}', &font, None);
        let RunKind::Fallback(fallback) = kind else {
            panic!(
                "classify_char must resolve a Fallback run for an uncovered CJK code point when \
                 the host has a capable font"
            );
        };
        let fallback_font = Font::from_typeface(fallback, 32.0);
        assert!(
            font_covers(&fallback_font, "\u{4F60}"),
            "resolved fallback typeface must actually cover the code point that triggered it"
        );
    }

    #[test]
    fn measure_and_paint_agree_on_cjk_width_when_fallback_is_available() {
        // Measure-vs-paint parity (this workstream's core mandate):
        // `measure_text_with_fallback` must report the width that actually
        // gets painted. Skips gracefully (see above) when the host has no
        // CJK-capable font at all — on such a host both measure and paint
        // agree on the pre-fix degraded behaviour (0-width primary-font
        // tofu), which is a separate, already-covered case.
        let font = helvetica_font(48.0);
        let text = "\u{4F60}\u{597D}"; // 你好
        if !text.chars().any(|c| {
            fallback_typeface_for_char("Helvetica", font.typeface().font_style(), c).is_some()
        }) {
            eprintln!("skip: no CJK-capable font installed on this host");
            return;
        }

        // Directly prove segmentation actually routes through the fallback
        // mechanism (not just that `fallback_typeface_for_char` the
        // primitive would resolve *something* if called) — this is what
        // distinguishes "the fix is wired up" from "the fix exists but
        // nothing calls it".
        let runs = segment_text_runs(text, &font);
        assert!(
            runs.iter().any(|r| matches!(r.kind, RunKind::Fallback(_))),
            "expected at least one Fallback run when segmenting CJK text with a fallback font \
             available, got kinds: {:?}",
            runs.iter()
                .map(|r| match &r.kind {
                    RunKind::Primary => "Primary",
                    RunKind::Emoji => "Emoji",
                    RunKind::Fallback(_) => "Fallback",
                })
                .collect::<Vec<_>>()
        );

        let measured_w = measure_text_with_fallback(text, &font, &None, 0.0);
        // Not tofu-narrow: with a real CJK fallback, two full-width
        // ideographs at 48px measure well beyond a couple of `.notdef` box
        // glyphs' worth of width.
        assert!(
            measured_w > 30.0,
            "expected a real CJK measurement, got suspiciously narrow {measured_w}"
        );

        use skia_safe::{surfaces, AlphaType, Color, ColorType, ImageInfo};
        const W: i32 = 400;
        const H: i32 = 200;
        let mut surface = surfaces::raster_n32_premul((W, H)).unwrap();
        let canvas = surface.canvas();
        canvas.clear(Color::BLACK);
        let mut paint = Paint::default();
        paint.set_color(Color::WHITE);
        paint.set_anti_alias(true);
        let (_, metrics) = font.metrics();
        let ascent = -metrics.ascent;
        draw_text_with_fallback(canvas, text, &font, &None, 0.0, 10.0, ascent + 10.0, &paint);

        // Ink detection: the canvas is cleared to opaque black (alpha=255
        // everywhere), so — unlike a transparent-cleared surface — alpha
        // can't distinguish text from background here. White text against
        // black shows up as a bright *red* (or green/blue) channel instead
        // (same technique `tracking_tests::render_and_measure_ink_centre_x`
        // above uses).
        let info = ImageInfo::new((W, H), ColorType::RGBA8888, AlphaType::Unpremul, None);
        let mut buf = vec![0u8; (W * H * 4) as usize];
        surface.read_pixels(&info, &mut buf, (W * 4) as usize, (0, 0));
        let mut min_x: Option<i32> = None;
        let mut max_x: Option<i32> = None;
        for y in 0..H {
            for x in 0..W {
                let idx = ((y * W + x) * 4) as usize;
                if buf[idx] > 40 {
                    min_x = Some(min_x.map_or(x, |m| m.min(x)));
                    max_x = Some(max_x.map_or(x, |m| m.max(x)));
                }
            }
        }
        let (min_x, max_x) = (
            min_x.expect("must paint something"),
            max_x.expect("must paint something"),
        );
        let painted_width = (max_x - min_x) as f32;

        // Loose tolerance: ink-bbox width vs advance width can differ from
        // glyph side-bearings/overhang even for a well-behaved font — this
        // is not the #125-style "wrap/paint disagreement" defect (a wild,
        // multiple-hundred-px miscentre), just normal glyph-metric slack.
        assert!(
            (painted_width - measured_w).abs() < measured_w * 0.5 + 20.0,
            "measured width {measured_w} should roughly match the painted ink width \
             {painted_width} (min_x={min_x}, max_x={max_x})"
        );
    }
}
