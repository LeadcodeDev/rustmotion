//! Intrinsic measurers for components whose box size depends on content.
//!
//! Currently exposes [`TextIntrinsic`] — a thin wrapper around cosmic-text's
//! `measure_text` that satisfies [`IntrinsicMeasure`] so `box_builder` can
//! attach it to text-bearing leaves and let taffy size them in flex/grid
//! parents the same way a browser would.

use rustmotion_core::engine::box_tree::{AvailableSpace, IntrinsicMeasure};
use rustmotion_core::engine::text::cosmic::{measure_text, TextStyle};
use rustmotion_core::schema::{FontStyleType, FontWeight, LayerStyle};

use crate::text::Text;

/// Cosmic-text–backed intrinsic measurer for [`Text`].
pub struct TextIntrinsic {
    content: String,
    font_family: Option<String>,
    font_size: f32,
    line_height: Option<f32>,
    weight: u16,
    italic: bool,
    letter_spacing: f32,
    max_width: Option<f32>,
    wrap: bool,
}

impl TextIntrinsic {
    pub fn from_text(text: &Text) -> Self {
        Self::from_parts(&text.content, &text.style, text.max_width)
    }

    pub fn from_parts(content: &str, style: &LayerStyle, max_width: Option<f32>) -> Self {
        let font_size = style.font_size.unwrap_or(48.0);
        Self {
            content: content.to_string(),
            font_family: style.font_family.clone(),
            font_size,
            line_height: style.line_height,
            weight: weight_to_u16(style.font_weight.as_ref()),
            italic: matches!(style.font_style, Some(FontStyleType::Italic)),
            letter_spacing: style.letter_spacing.unwrap_or(0.0),
            max_width,
            wrap: style.wrap.unwrap_or(true),
        }
    }

    fn resolved_line_height(&self) -> f32 {
        match self.line_height {
            Some(v) if v <= 10.0 => self.font_size * v,
            Some(v) => v,
            None => self.font_size * 1.3,
        }
    }
}

impl IntrinsicMeasure for TextIntrinsic {
    fn measure(
        &self,
        known: (Option<f32>, Option<f32>),
        available: (AvailableSpace, AvailableSpace),
    ) -> (f32, f32) {
        // If width is already known, use it; else cap at min(self.max_width, available width).
        let max_width = if let Some(w) = known.0 {
            Some(w)
        } else {
            let avail_w = match available.0 {
                AvailableSpace::Definite(w) => Some(w),
                AvailableSpace::MaxContent => None,
                AvailableSpace::MinContent => Some(0.0),
            };
            match (self.max_width, avail_w) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            }
        };

        let style = TextStyle {
            font_family: self.font_family.as_deref(),
            font_size: self.font_size,
            line_height: self.resolved_line_height(),
            weight: self.weight,
            italic: self.italic,
            max_width: if self.wrap { max_width } else { None },
            wrap: self.wrap,
            letter_spacing: self.letter_spacing,
        };

        let m = measure_text(&self.content, &style);
        (m.width, m.height)
    }
}

fn weight_to_u16(w: Option<&FontWeight>) -> u16 {
    match w {
        Some(FontWeight::Normal) | None => 400,
        Some(FontWeight::Bold) => 700,
        Some(FontWeight::Weight(n)) => (*n).clamp(1, 1000) as u16,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmotion_core::engine::box_tree::AvailableSpace;

    #[test]
    fn measure_returns_positive_size_for_non_empty_text() {
        let text = Text {
            content: "Hello World".into(),
            max_width: None,
            timing: Default::default(),
            style: LayerStyle { font_size: Some(32.0), ..Default::default() },
        };
        let m = TextIntrinsic::from_text(&text);
        let (w, h) = m.measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        );
        assert!(w > 0.0, "width should be > 0, got {}", w);
        assert!(h > 30.0, "height should be roughly font_size * line_height, got {}", h);
    }

    #[test]
    fn wrapping_grows_height_when_max_width_constrained() {
        let text = Text {
            content: "the quick brown fox jumps over the lazy dog".into(),
            max_width: None,
            timing: Default::default(),
            style: LayerStyle { font_size: Some(20.0), ..Default::default() },
        };
        let m = TextIntrinsic::from_text(&text);
        let (_w_unwrapped, h_unwrapped) = m.measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        );
        let (_w_wrapped, h_wrapped) = m.measure(
            (None, None),
            (AvailableSpace::Definite(80.0), AvailableSpace::MaxContent),
        );
        assert!(
            h_wrapped > h_unwrapped,
            "wrapped height ({}) should exceed unwrapped ({})",
            h_wrapped,
            h_unwrapped,
        );
    }

    #[test]
    fn empty_text_has_zero_width_but_one_line_height() {
        let text = Text {
            content: "".into(),
            max_width: None,
            timing: Default::default(),
            style: LayerStyle { font_size: Some(24.0), ..Default::default() },
        };
        let m = TextIntrinsic::from_text(&text);
        let (w, h) = m.measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        );
        assert_eq!(w, 0.0);
        assert!(h > 0.0);
    }
}
