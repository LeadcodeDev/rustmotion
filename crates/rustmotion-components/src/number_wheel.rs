use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, ClipOp, Font, FontStyle, Rect};

use rustmotion_core::css::style::{
    FontStyle as CssFontStyle, FontWeight as CssFontWeight, FontWeightKw,
};
use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::{ease, AnimatedProperties};
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    draw_text_with_fallback, measure_text_with_fallback, paint_from_hex, typeface_with_fallback,
};
use rustmotion_core::schema::{EasingType, FontStyleType, FontWeight, TimelineStep};
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

/// How many full 0-9 revolutions a reel makes before landing.
///
/// The reel covers the same *time* whichever this is, so a higher setting is
/// a faster spin, not a longer one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WheelSpin {
    #[default]
    Single,
    Double,
    Triple,
}

impl WheelSpin {
    fn revolutions(self) -> f32 {
        match self {
            Self::Single => 1.0,
            Self::Double => 2.0,
            Self::Triple => 3.0,
        }
    }
}

fn default_wheel_duration() -> f64 {
    1.2
}

fn default_wheel_stagger() -> f64 {
    0.08
}

fn default_wheel_easing() -> EasingType {
    EasingType::EaseOutCubic
}

/// An odometer-style number where each digit rolls into place.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[deprecated(
    since = "0.7.1",
    note = "`number_wheel` is in the frozen-composition set (issue #333), but \
            crates/rustmotion/skills/rules/composition-recipes.md flags it (with `gauge`) \
            as a reasonable exception to keep using directly: reproducing genuine \
            per-digit scroll physics from card/text/shape primitives is mechanically \
            harder than the odometer effect itself. Kept for compatibility; scheduled for \
            removal in a future major version via `rustmotion migrate` (#335)."
)]
pub struct NumberWheel {
    /// The figure to land on, as written — `"30,222"`, `"5.7"`, `"98%"`.
    /// Digits roll; every other character (separators, signs, units) is
    /// painted where it stands.
    pub value: String,
    /// How far each reel travels before landing.
    #[serde(default)]
    pub spin: WheelSpin,
    /// How long one reel takes to land (seconds).
    #[serde(default = "default_wheel_duration")]
    pub duration: f64,
    /// Delay before the first reel starts (seconds).
    #[serde(default)]
    pub delay: f64,
    /// Extra delay per digit column, left to right (seconds). `0` lands
    /// every reel at once, which reads as a single flip rather than as a
    /// counter settling.
    #[serde(default = "default_wheel_stagger")]
    pub stagger_per_column: f64,
    /// Easing of a reel's travel. The default decelerates into the landing,
    /// which is what makes it read as mechanical rather than as a fade.
    #[serde(default = "default_wheel_easing")]
    pub easing: EasingType,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(NumberWheel {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

pub(crate) enum Cell {
    Digit(u32),
    Fixed(char),
}

impl NumberWheel {
    pub(crate) fn cells(value: &str) -> Vec<Cell> {
        value
            .chars()
            .map(|c| match c.to_digit(10) {
                Some(d) => Cell::Digit(d),
                None => Cell::Fixed(c),
            })
            .collect()
    }

    pub(crate) fn reel_position(&self, column: usize, target: u32, time: f64) -> f32 {
        let start = self.delay + column as f64 * self.stagger_per_column;
        let raw = if self.duration <= 0.0 {
            1.0
        } else {
            ((time - start) / self.duration).clamp(0.0, 1.0)
        };
        let p = ease(raw, &self.easing) as f32;
        let travel = self.spin.revolutions() * 10.0 + target as f32;
        travel * p
    }

    fn digit_advance(font: &Font, emoji: &Option<Font>, letter_spacing: f32) -> f32 {
        (0..10)
            .map(|d| measure_text_with_fallback(&d.to_string(), font, emoji, letter_spacing))
            .fold(0.0f32, f32::max)
    }

    pub(crate) fn build_font(&self, font_size: f32) -> Option<Font> {
        let font_family = self.style.font_family_or("Inter");
        let weight = match &self.style.font_weight {
            Some(CssFontWeight::Keyword(FontWeightKw::Bold | FontWeightKw::Bolder)) => {
                FontWeight::Bold
            }
            Some(CssFontWeight::Number(n)) if *n >= 600 => FontWeight::Bold,
            Some(CssFontWeight::Number(n)) => FontWeight::Weight(*n),
            _ => FontWeight::Normal,
        };
        let slant = match self.style.font_style {
            Some(CssFontStyle::Italic) => skia_safe::font_style::Slant::Italic,
            Some(CssFontStyle::Oblique) => skia_safe::font_style::Slant::Oblique,
            _ => skia_safe::font_style::Slant::Upright,
        };
        let weight = match weight {
            FontWeight::Bold => skia_safe::font_style::Weight::BOLD,
            FontWeight::Normal => skia_safe::font_style::Weight::NORMAL,
            FontWeight::Weight(w) => skia_safe::font_style::Weight::from(w as i32),
        };
        let _ = FontStyleType::Normal;
        let typeface = typeface_with_fallback(
            font_family,
            FontStyle::new(weight, skia_safe::font_style::Width::NORMAL, slant),
        )
        .ok()?;
        Some(Font::from_typeface(typeface, font_size))
    }
}

impl Painter for NumberWheel {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        let base_ctx = crate::intrinsic::font_size_ctx(
            ctx.video_width as f32,
            ctx.video_height as f32,
            layout.width.max(0.0),
        );
        let font_size = self.style.font_size_px_ctx(&base_ctx, 72.0);
        let Some(font) = self.build_font(font_size) else {
            return;
        };
        let emoji_font = rustmotion_core::engine::renderer::emoji_typeface()
            .map(|tf| Font::from_typeface(tf, font_size));
        let own_ctx = rustmotion_core::css::units::LengthContext {
            font_size,
            ..base_ctx
        };
        let letter_spacing = self.style.letter_spacing_px_ctx(&own_ctx);

        let color = props
            .color
            .as_deref()
            .unwrap_or_else(|| self.style.color_str_or("#FFFFFF"));
        let paint = paint_from_hex(color);

        let (_, metrics) = font.metrics();
        let ascent = -metrics.ascent;
        let descent = metrics.descent;
        let cell_h = ascent + descent;
        let baseline = ascent;

        let digit_w = Self::digit_advance(&font, &emoji_font, letter_spacing);
        let cells = Self::cells(&self.value);

        let mut x = 0.0f32;
        let mut column = 0usize;
        for cell in &cells {
            match cell {
                Cell::Fixed(c) => {
                    let s = c.to_string();
                    let w = measure_text_with_fallback(&s, &font, &emoji_font, letter_spacing);
                    draw_text_with_fallback(
                        canvas,
                        &s,
                        &font,
                        &emoji_font,
                        letter_spacing,
                        x,
                        baseline,
                        &paint,
                    );
                    x += w;
                }
                Cell::Digit(target) => {
                    let pos = self.reel_position(column, *target, ctx.time);
                    let whole = pos.floor();
                    let frac = pos - whole;

                    canvas.save();
                    canvas.clip_rect(
                        Rect::from_xywh(x, 0.0, digit_w, cell_h),
                        ClipOp::Intersect,
                        false,
                    );

                    for (step, offset) in [(0.0f32, -frac * cell_h), (1.0, (1.0 - frac) * cell_h)] {
                        let digit = ((whole + step) as i64).rem_euclid(10);
                        let s = digit.to_string();
                        let w = measure_text_with_fallback(&s, &font, &emoji_font, letter_spacing);
                        draw_text_with_fallback(
                            canvas,
                            &s,
                            &font,
                            &emoji_font,
                            letter_spacing,
                            x + (digit_w - w) / 2.0,
                            baseline + offset,
                            &paint,
                        );
                    }
                    canvas.restore();

                    x += digit_w;
                    column += 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wheel(json: serde_json::Value) -> NumberWheel {
        serde_json::from_value(json).expect("number_wheel fixture")
    }

    #[test]
    fn only_digits_become_reels() {
        let cells = NumberWheel::cells("1,204.5%");
        let digits = cells.iter().filter(|c| matches!(c, Cell::Digit(_))).count();
        assert_eq!(
            digits, 5,
            "1 2 0 4 5 roll; the comma, dot and percent do not"
        );
    }

    #[test]
    fn a_reel_lands_exactly_on_its_target_digit() {
        let w =
            wheel(serde_json::json!({ "value": "7", "duration": 1.0, "stagger_per_column": 0.0 }));
        let landed = w.reel_position(0, 7, 5.0);
        assert!(
            (landed % 10.0 - 7.0).abs() < 1e-4,
            "the reel should rest on 7, got cell {landed}"
        );
    }

    #[test]
    fn a_reel_starts_on_zero_before_it_moves() {
        let w = wheel(serde_json::json!({ "value": "42", "delay": 0.5 }));
        assert_eq!(
            w.reel_position(0, 4, 0.0),
            0.0,
            "before its delay a reel shows 0, it does not preview the answer"
        );
    }

    #[test]
    fn spin_changes_the_distance_not_the_landing() {
        let single = wheel(serde_json::json!({ "value": "3", "spin": "single" }));
        let triple = wheel(serde_json::json!({ "value": "3", "spin": "triple" }));

        assert!(
            triple.reel_position(0, 3, 0.4) > single.reel_position(0, 3, 0.4),
            "a triple spin covers more ground in the same time"
        );
        assert!(
            (single.reel_position(0, 3, 9.0) % 10.0 - 3.0).abs() < 1e-4
                && (triple.reel_position(0, 3, 9.0) % 10.0 - 3.0).abs() < 1e-4,
            "both must land on 3"
        );
    }

    #[test]
    fn columns_land_left_to_right() {
        let w = wheel(serde_json::json!({
            "value": "99", "duration": 0.5, "stagger_per_column": 0.25
        }));
        let first = w.reel_position(0, 9, 0.5);
        let second = w.reel_position(1, 9, 0.5);
        assert!(
            (first % 10.0 - 9.0).abs() < 1e-4,
            "the leftmost reel should have landed by t=0.5"
        );
        assert!(
            second < first,
            "the next reel should still be travelling (first={first}, second={second})"
        );
    }
}
