use skia_safe::Font;

use crate::components::Codeblock;
use crate::schema::Spacing;

/// Computed dimensions for a code block
#[allow(dead_code)]
pub(super) struct CodeDimensions {
    pub(super) line_count: usize,
    pub(super) max_line_width: f32,
    pub(super) gutter_width: f32,
    pub(super) total_width: f32,
    pub(super) total_height: f32,
}

pub(super) fn compute_code_dimensions(
    code: &str,
    font: &Font,
    padding: &Spacing,
    chrome_height: f32,
    layer: &Codeblock,
) -> CodeDimensions {
    let font_size = layer.style.font_size.unwrap_or(14.0);
    let line_height = layer.style.line_height.unwrap_or(1.5);
    let actual_line_height = font_size * line_height;
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

    let (pad_top, pad_right, pad_bottom, pad_left) = padding.resolve();
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
