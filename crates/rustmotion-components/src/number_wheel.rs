use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, ClipOp, Font, FontStyle, Rect};

use rustmotion_core::css::style::{
    FontStyle as CssFontStyle, FontWeight as CssFontWeight, FontWeightKw, TextAlign as CssTextAlign,
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

    pub(crate) fn advance(
        cells: &[Cell],
        digit_w: f32,
        font: &Font,
        emoji: &Option<Font>,
        letter_spacing: f32,
    ) -> f32 {
        cells
            .iter()
            .map(|cell| match cell {
                Cell::Digit(_) => digit_w,
                Cell::Fixed(c) => {
                    measure_text_with_fallback(&c.to_string(), font, emoji, letter_spacing)
                }
            })
            .sum()
    }

    pub(crate) fn align_offset(align: CssTextAlign, box_width: f32, advance: f32) -> f32 {
        if !box_width.is_finite() || box_width <= 0.0 {
            return 0.0;
        }
        match align {
            CssTextAlign::Center => (box_width - advance) / 2.0,
            CssTextAlign::Right | CssTextAlign::End => box_width - advance,
            _ => 0.0,
        }
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
        let advance = Self::advance(&cells, digit_w, &font, &emoji_font, letter_spacing);

        let mut x = Self::align_offset(
            self.style.text_align.unwrap_or(CssTextAlign::Left),
            layout.width,
            advance,
        );
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

    fn test_ctx() -> PaintCtx {
        PaintCtx {
            time: 5.0,
            scenario_time: 5.0,
            scene_duration: 6.0,
            frame_index: 150,
            fps: 30,
            video_width: 1920,
            video_height: 1080,
            stagger_offset: 0.0,
        }
    }

    fn box_layout(width: f32) -> BoxLayout {
        BoxLayout {
            x: 0.0,
            y: 0.0,
            width,
            height: 200.0,
            border: Default::default(),
            padding: Default::default(),
        }
    }

    fn ink_x_span(w: &NumberWheel, box_width: f32, surface_width: i32) -> (i32, i32) {
        let height = 200;
        let mut surface = skia_safe::surfaces::raster_n32_premul((surface_width, height))
            .expect("raster surface");
        w.paint_content(
            surface.canvas(),
            &box_layout(box_width),
            &AnimatedProperties::default(),
            &test_ctx(),
        );
        let image = surface.image_snapshot();
        let info = skia_safe::ImageInfo::new(
            (surface_width, height),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Unpremul,
            None,
        );
        let mut buf = vec![0u8; (surface_width * height * 4) as usize];
        image.read_pixels(
            &info,
            &mut buf,
            (surface_width * 4) as usize,
            (0, 0),
            skia_safe::image::CachingHint::Allow,
        );
        let mut lo = i32::MAX;
        let mut hi = i32::MIN;
        for y in 0..height {
            for x in 0..surface_width {
                if buf[((y * surface_width + x) * 4 + 3) as usize] > 0 {
                    lo = lo.min(x);
                    hi = hi.max(x);
                }
            }
        }
        assert!(lo <= hi, "the wheel painted nothing at all");
        (lo, hi)
    }

    fn landed_wheel(align: Option<CssTextAlign>) -> NumberWheel {
        let mut w = wheel(serde_json::json!({
            "value": "27",
            "duration": 1.0,
            "stagger_per_column": 0.0,
            "style": { "font-size": 120 }
        }));
        w.style.text_align = align;
        w
    }

    #[test]
    fn text_align_center_centres_the_reels_in_the_box() {
        const BOX_W: f32 = 1000.0;
        let (lo, hi) = ink_x_span(&landed_wheel(Some(CssTextAlign::Center)), BOX_W, 1100);
        let centre = (lo + hi) as f32 / 2.0;
        assert!(
            (centre - BOX_W / 2.0).abs() <= 6.0,
            "a centred number_wheel must sit on the box centre {}, painted [{lo}, {hi}] \
             with centre {centre}",
            BOX_W / 2.0
        );
        assert!(
            lo > 100,
            "a centred number_wheel must leave a left margin, first ink at {lo}"
        );
    }

    #[test]
    fn text_align_right_ends_the_reels_on_the_box_edge() {
        const BOX_W: f32 = 1000.0;
        let (lo, hi) = ink_x_span(&landed_wheel(Some(CssTextAlign::Right)), BOX_W, 1100);
        assert!(
            (BOX_W - hi as f32) <= 8.0,
            "a right-aligned number_wheel must end on the box edge {BOX_W}, last ink at {hi}"
        );
        assert!(
            lo > 700,
            "a right-aligned number_wheel starts far from the left edge, first ink at {lo}"
        );
    }

    #[test]
    fn no_text_align_still_starts_at_the_box_left_edge() {
        let (lo, _) = ink_x_span(&landed_wheel(None), 1000.0, 1100);
        assert!(
            lo < 12,
            "without text_align the wheel keeps starting at the box's left edge, first ink at {lo}"
        );
    }

    #[test]
    fn a_box_with_no_usable_width_falls_back_to_the_left_edge() {
        for width in [0.0_f32, -5.0, f32::INFINITY, f32::NAN] {
            let advance = 240.0;
            assert_eq!(
                NumberWheel::align_offset(CssTextAlign::Center, width, advance),
                0.0,
                "a box width of {width} cannot centre anything — paint from the left \
                 rather than off-screen"
            );
        }
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
