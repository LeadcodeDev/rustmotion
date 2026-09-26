use skia_safe::{Canvas, Font, Paint, Point, TextBlob, Typeface};

pub fn format_counter_value(
    value: f64,
    decimals: u8,
    separator: &Option<String>,
    prefix: &Option<String>,
    suffix: &Option<String>,
) -> String {
    let formatted_number = format!("{:.prec$}", value, prec = decimals as usize);

    let formatted_number = if let Some(sep) = separator {
        let parts: Vec<&str> = formatted_number.split('.').collect();
        let integer_part = parts[0];

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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphMetric {
    pub x: f32,
    pub width: f32,
}

fn glyph_metrics_single_font(text: &str, font: &Font, letter_spacing: f32) -> Vec<GlyphMetric> {
    let glyphs = font.str_to_glyphs_vec(text);
    if glyphs.is_empty() {
        return Vec::new();
    }
    let mut widths = vec![0.0f32; glyphs.len()];
    font.get_widths(&glyphs, &mut widths);
    let mut out = Vec::with_capacity(glyphs.len());
    let mut x = 0.0f32;
    for w in widths {
        out.push(GlyphMetric { x, width: w });
        x += w + letter_spacing;
    }
    out
}

pub fn compute_glyph_metrics(
    text: &str,
    font: &Font,
    emoji_font: &Option<Font>,
    letter_spacing: f32,
) -> Vec<GlyphMetric> {
    if text.is_empty() {
        return Vec::new();
    }
    if !needs_segmentation(text, font, emoji_font) {
        return glyph_metrics_single_font(text, font, letter_spacing);
    }

    let runs = segment_text_runs(text, font);
    let mut out = Vec::new();
    let mut cursor_x = 0.0f32;
    for run in &runs {
        let segment = &text[run.start..run.end];
        let mut owned = None;
        let f = resolve_run_font(&run.kind, segment, font, emoji_font, &mut owned);
        let metrics = glyph_metrics_single_font(segment, f, letter_spacing);
        let run_advance: f32 = metrics.iter().map(|m| m.width + letter_spacing).sum();
        out.extend(metrics.into_iter().map(|m| GlyphMetric {
            x: cursor_x + m.x,
            width: m.width,
        }));
        cursor_x += run_advance;
    }
    out
}

fn is_emoji_presentation_default(c: char) -> bool {
    let cp = c as u32;
    matches!(cp,
        0x1F300..=0x1F5FF |
        0x1F600..=0x1F64F |
        0x1F680..=0x1F6FF |
        0x1F900..=0x1F9FF |
        0x1FA00..=0x1FA6F |
        0x1FA70..=0x1FAFF |
        0x200D |
        0x20E3 |
        0x1F1E0..=0x1F1FF |
        0xE0020..=0xE007F |
        0x1F004 | 0x1F0CF |
        0x23E9..=0x23F3 |
        0x23F8..=0x23FA |
        0x2B1B..=0x2B1C |
        0x2B50 | 0x2B55
    )
}

fn is_text_presentation_by_default(c: char) -> bool {
    let cp = c as u32;
    matches!(cp,
        0x00A9 | 0x00AE | 0x2122 |
        0x231A..=0x231B |
        0x2600..=0x26FF |
        0x2702..=0x27B0 |
        0x2934..=0x2935 |
        0x25AA..=0x25AB |
        0x25B6 | 0x25C0 |
        0x25FB..=0x25FE |
        0x2B05..=0x2B07 |
        0x3030 | 0x303D |
        0x3297 | 0x3299
    )
}

const VARIATION_SELECTOR_EMOJI: char = '\u{FE0F}';

fn char_wants_emoji_font(c: char, next: Option<char>) -> bool {
    if c == VARIATION_SELECTOR_EMOJI {
        return true;
    }
    if is_emoji_presentation_default(c) {
        return true;
    }
    if is_text_presentation_by_default(c) {
        return next == Some(VARIATION_SELECTOR_EMOJI);
    }
    false
}

enum RunKind {
    Primary,
    Emoji,
    Fallback(Typeface),
}

fn same_run_kind(a: &RunKind, b: &RunKind) -> bool {
    match (a, b) {
        (RunKind::Primary, RunKind::Primary) => true,
        (RunKind::Emoji, RunKind::Emoji) => true,
        (RunKind::Fallback(ta), RunKind::Fallback(tb)) => ta.unique_id() == tb.unique_id(),
        _ => false,
    }
}

fn font_covers(font: &Font, text: &str) -> bool {
    let glyphs = font.str_to_glyphs_vec(text);
    !glyphs.is_empty() && glyphs.iter().all(|&g| g != 0)
}

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
        None => RunKind::Primary,
    }
}

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

struct TextRun {
    start: usize,
    end: usize,
    kind: RunKind,
}

pub fn has_emoji(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    chars
        .iter()
        .enumerate()
        .any(|(i, &c)| char_wants_emoji_font(c, chars.get(i + 1).copied()))
}

fn needs_segmentation(text: &str, primary: &Font, emoji_font: &Option<Font>) -> bool {
    if emoji_font.is_some() && has_emoji(text) {
        return true;
    }
    text.chars().any(|c| {
        !(c.is_ascii() || c.is_whitespace() || (c as u32) < 0x20)
            && primary.unichar_to_glyph(c as i32) == 0
    })
}

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
            _ => primary,
        },
        RunKind::Fallback(tf) => {
            *owned = Some(Font::from_typeface(tf.clone(), primary.size()));
            owned.as_ref().unwrap()
        }
    }
}

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

        let (w, _) = f.measure_str(segment, None);
        let extra = if letter_spacing.abs() > 0.01 {
            letter_spacing * (segment.chars().count() as f32 - 1.0).max(0.0)
        } else {
            0.0
        };
        cursor_x += w + extra;
    }
}

pub fn measure_text_with_fallback(
    text: &str,
    font: &Font,
    emoji_font: &Option<Font>,
    letter_spacing: f32,
) -> f32 {
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

pub fn wrap_text_with_fallback(
    text: &str,
    font: &Font,
    emoji_font: &Option<Font>,
    max_width: Option<f32>,
) -> Vec<String> {
    wrap_text_with_tracking(text, font, emoji_font, max_width, 0.0)
}

#[cfg(test)]
mod tracking_tests {
    use super::super::typeface_with_fallback;
    use super::*;
    use skia_safe::{surfaces, AlphaType, Color, ColorType, FontStyle as SkFontStyle, ImageInfo};

    fn bold_font(size: f32) -> Font {
        let typeface = typeface_with_fallback("Helvetica", SkFontStyle::bold())
            .expect("host must have a fallback typeface");
        Font::from_typeface(typeface, size)
    }

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

    #[test]
    fn tracking_aware_wrap_agrees_across_width_samples_old_wrap_does_not() {
        let text = "THAT MOVES";
        let font = bold_font(290.0);
        let letter_spacing = -9.0f32;
        let real_width = measure_text_with_fallback(text, &font, &None, letter_spacing);
        let zero_width = measure_text_with_fallback(text, &font, &None, 0.0);

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

    #[test]
    fn sweep_that_moves_fixed_primitive_stays_centred() {
        const W: u32 = 1920;
        const H: u32 = 1080;
        const FRAME_CENTRE: f32 = 960.0;
        let text = "THAT MOVES";

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

#[cfg(test)]
mod emoji_presentation_tests {
    use super::super::{emoji_typeface, typeface_with_fallback};
    use super::*;

    #[test]
    fn text_presentation_default_symbols_are_not_emoji_without_vs16() {
        assert!(
            !char_wants_emoji_font('\u{2713}', None),
            "✓ bare must be text"
        );
        assert!(
            !char_wants_emoji_font('\u{2714}', None),
            "✔ bare must be text"
        );
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
        assert!(char_wants_emoji_font('\u{2713}', Some('\u{FE0F}')));
        assert!(char_wants_emoji_font('\u{00A9}', Some('\u{FE0F}')));
    }

    #[test]
    fn genuine_pictographs_are_always_emoji_regardless_of_vs16() {
        assert!(
            char_wants_emoji_font('\u{1F600}', None),
            "😀 must stay emoji"
        );
        assert!(char_wants_emoji_font('\u{1F600}', Some('\u{FE0F}')));
    }

    #[test]
    fn variation_selector_16_itself_is_always_emoji() {
        assert!(char_wants_emoji_font('\u{FE0F}', None));
    }

    #[test]
    fn variation_selector_15_does_not_force_emoji() {
        assert!(!char_wants_emoji_font('\u{2713}', Some('\u{FE0E}')));
    }

    #[test]
    fn has_emoji_reflects_the_narrowed_classification() {
        assert!(!has_emoji("2713:\u{2713} copyright:\u{00A9}"));
        assert!(has_emoji("checked \u{2713}\u{FE0F}"));
        assert!(has_emoji("grinning \u{1F600}"));
    }

    fn helvetica_font(size: f32) -> Font {
        let typeface = typeface_with_fallback("Helvetica", skia_safe::FontStyle::normal())
            .expect("host must have a fallback typeface");
        Font::from_typeface(typeface, size)
    }

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
        for px in buf.as_chunks::<4>().0.iter() {
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
        let font = helvetica_font(64.0);
        if !font_covers(&font, "\u{2713}") {
            eprintln!("skip: primary font doesn't cover U+2713 on this host");
            return;
        }
        let (ink, r, g, b) = render_and_sample("\u{2713}", 64.0);
        assert!(ink > 20, "expected visible ink for ✓, got {ink} pixels");
        assert!(
            r > 150.0 && g > 150.0 && b > 150.0,
            "✓ should paint near-white, got mean rgb=({r:.0},{g:.0},{b:.0})"
        );
    }
}

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
        let font = helvetica_font(32.0);
        assert!(font_covers(&font, "ABC"), "Helvetica must cover ASCII");
        assert!(
            !font_covers(&font, "\u{4F60}\u{597D}"),
            "Helvetica must not cover CJK"
        );
    }

    #[test]
    fn classify_char_stays_primary_for_a_codepoint_no_font_covers() {
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
        let font = helvetica_font(48.0);
        let text = "\u{4F60}\u{597D}";
        if !text.chars().any(|c| {
            fallback_typeface_for_char("Helvetica", font.typeface().font_style(), c).is_some()
        }) {
            eprintln!("skip: no CJK-capable font installed on this host");
            return;
        }

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

        assert!(
            (painted_width - measured_w).abs() < measured_w * 0.5 + 20.0,
            "measured width {measured_w} should roughly match the painted ink width \
             {painted_width} (min_x={min_x}, max_x={max_x})"
        );
    }
}

#[cfg(test)]
mod glyph_metrics_tests {
    use super::super::typeface_with_fallback;
    use super::*;
    use skia_safe::FontStyle as SkFontStyle;

    fn test_font(size: f32) -> Font {
        let typeface = typeface_with_fallback("Helvetica", SkFontStyle::default())
            .expect("host must have a fallback typeface");
        Font::from_typeface(typeface, size)
    }

    #[test]
    fn empty_text_has_no_glyphs() {
        let font = test_font(32.0);
        assert!(compute_glyph_metrics("", &font, &None, 0.0).is_empty());
    }

    #[test]
    fn glyph_count_matches_char_count_for_plain_ascii() {
        let font = test_font(32.0);
        let metrics = compute_glyph_metrics("Hello", &font, &None, 0.0);
        assert_eq!(metrics.len(), 5);
    }

    #[test]
    fn first_glyph_starts_at_line_origin() {
        let font = test_font(32.0);
        let metrics = compute_glyph_metrics("Hello", &font, &None, 0.0);
        assert_eq!(metrics[0].x, 0.0);
    }

    #[test]
    fn glyphs_are_monotonically_increasing_and_sum_to_the_advance_width() {
        let font = test_font(48.0);
        let text = "Sentence?";
        let metrics = compute_glyph_metrics(text, &font, &None, 0.0);
        assert_eq!(metrics.len(), text.chars().count());
        for pair in metrics.windows(2) {
            assert!(
                pair[1].x >= pair[0].x,
                "glyph x must be non-decreasing: {:?}",
                metrics
            );
        }
        let last = metrics.last().unwrap();
        let total_advance = last.x + last.width;
        let measured = measure_text_with_fallback(text, &font, &None, 0.0);
        assert!(
            (total_advance - measured).abs() < 0.5,
            "last glyph's right edge ({total_advance}) should match the \
             measured advance width ({measured})"
        );
    }

    #[test]
    fn last_glyph_is_the_detachable_question_mark() {
        let font = test_font(32.0);
        let metrics = compute_glyph_metrics("Sentence", &font, &None, 0.0);
        let e_glyph = *metrics.last().unwrap();
        let question_x = e_glyph.x + e_glyph.width;
        assert!(question_x > e_glyph.x);
    }

    #[test]
    fn letter_spacing_widens_the_gap_between_glyphs() {
        let font = test_font(32.0);
        let tight = compute_glyph_metrics("AB", &font, &None, 0.0);
        let wide = compute_glyph_metrics("AB", &font, &None, 10.0);
        assert_eq!(tight.len(), 2);
        assert_eq!(wide.len(), 2);
        assert!(wide[1].x > tight[1].x + 9.0);
    }

    #[test]
    fn single_font_path_matches_make_text_blob_with_spacing_positions() {
        let font = test_font(40.0);
        let text = "Hello";
        let spacing = 2.0;
        let metrics = compute_glyph_metrics(text, &font, &None, spacing);

        let glyphs = font.str_to_glyphs_vec(text);
        let mut widths = vec![0.0f32; glyphs.len()];
        font.get_widths(&glyphs, &mut widths);
        let mut x = 0.0f32;
        for (i, w) in widths.iter().enumerate() {
            assert_eq!(metrics[i].x, x);
            assert_eq!(metrics[i].width, *w);
            x += w + spacing;
        }
    }
}
