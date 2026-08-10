use skia_safe::Font;

use super::Codeblock;

/// Computed dimensions for a code block
#[allow(dead_code)]
pub(crate) struct CodeDimensions {
    pub(crate) line_count: usize,
    pub(crate) max_line_width: f32,
    pub(crate) gutter_width: f32,
    pub(crate) total_width: f32,
    pub(crate) total_height: f32,
}

pub(crate) fn compute_code_dimensions(
    code: &str,
    font: &Font,
    font_size: f32,
    padding: (f32, f32, f32, f32),
    chrome_height: f32,
    layer: &Codeblock,
) -> CodeDimensions {
    // `font_size` is now a caller-supplied parameter instead of being
    // re-derived here from `layer.style` — the caller (`render.rs`,
    // `intrinsic.rs`) already resolved it once (with the real `LengthContext`
    // where one is available) to build `font`; re-deriving it a second time
    // with the context-free accessor was exactly the kind of duplicate
    // computation that let this and the caller's value silently diverge for
    // relative units (lot B, wave S).
    let actual_line_height = layer.style.line_height_for(font_size);
    let lines: Vec<&str> = code.lines().collect();
    let line_count = lines.len().max(1);

    let gutter_width = if layer.show_line_numbers {
        let digits = format!("{}", line_count).len();
        let digit_width = font.measure_str("0", None).0;
        (digits as f32 * digit_width) + 24.0
    } else {
        0.0
    };

    let max_line_width = lines
        .iter()
        .map(|l| font.measure_str(l, None).0)
        .fold(0.0f32, f32::max);

    let (pad_top, pad_right, pad_bottom, pad_left) = padding;
    let content_width = max_line_width + gutter_width + pad_left + pad_right;
    let content_height = line_count as f32 * actual_line_height + pad_top + pad_bottom;

    CodeDimensions {
        line_count,
        max_line_width,
        gutter_width,
        total_width: content_width,
        total_height: content_height + chrome_height,
    }
}

pub(super) fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}
