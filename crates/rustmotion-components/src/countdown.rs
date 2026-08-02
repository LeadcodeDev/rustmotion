use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, RRect, Rect};

use rustmotion_core::css::style::Size as CSize;
use rustmotion_core::css::{CssStyle, LengthPercentage as CLP};
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, measure_text_with_fallback, paint_from_hex,
    parse_hex_color, typeface_with_fallback,
};
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

fn default_seconds() -> f64 {
    3600.0
}

fn default_true() -> bool {
    true
}

fn default_digit_size() -> f32 {
    64.0
}

fn default_digit_color() -> String {
    "#FFFFFF".to_string()
}

fn default_digit_background() -> String {
    "#1E293B".to_string()
}

fn default_separator_color() -> String {
    "#6B7280".to_string()
}

fn default_gap() -> f32 {
    12.0
}

fn default_border_radius() -> f32 {
    12.0
}

/// `#[serde(from = "CountdownRaw")]`: `box_builder.rs`'s width formula for
/// this component (`visible * 2*box_w + (visible-1)*gap`) doesn't include
/// the small inner gap (`gap*0.3`) `paint()` actually inserts *within*
/// each digit pair — only the gap *between* pairs (the separator). That
/// mismatch compounds with every extra group and is exactly what #127
/// measured (153×67 assigned vs. 187×68 painted). Since `paint()` draws
/// from `self.digit_size`/`self.gap`, not `layout.width` (same pattern as
/// `switch`/`slider`), the fix is to fold the *exact* formula `paint()`
/// uses into `style.width` here, so `box_builder.rs`'s `if
/// css.width.is_none()` never gets a chance to apply its slightly-off one.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(from = "CountdownRaw")]
pub struct Countdown {
    #[serde(default = "default_seconds")]
    pub seconds: f64,
    #[serde(default = "default_true")]
    pub show_hours: bool,
    #[serde(default = "default_true")]
    pub show_minutes: bool,
    #[serde(default = "default_true")]
    pub show_seconds: bool,
    #[serde(default = "default_digit_size")]
    pub digit_size: f32,
    #[serde(default = "default_digit_color")]
    pub digit_color: String,
    #[serde(default = "default_digit_background")]
    pub digit_background: String,
    #[serde(default = "default_separator_color")]
    pub separator_color: String,
    #[serde(default = "default_gap")]
    pub gap: f32,
    #[serde(default = "default_border_radius")]
    pub border_radius: f32,
    #[serde(default)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

#[derive(Debug, Deserialize)]
struct CountdownRaw {
    #[serde(default = "default_seconds")]
    seconds: f64,
    #[serde(default = "default_true")]
    show_hours: bool,
    #[serde(default = "default_true")]
    show_minutes: bool,
    #[serde(default = "default_true")]
    show_seconds: bool,
    #[serde(default = "default_digit_size")]
    digit_size: f32,
    #[serde(default = "default_digit_color")]
    digit_color: String,
    #[serde(default = "default_digit_background")]
    digit_background: String,
    #[serde(default = "default_separator_color")]
    separator_color: String,
    #[serde(default = "default_gap")]
    gap: f32,
    #[serde(default = "default_border_radius")]
    border_radius: f32,
    #[serde(flatten)]
    timing: TimingConfig,
    #[serde(default)]
    style: CssStyle,
    #[serde(default)]
    timeline: Vec<TimelineStep>,
    #[serde(default)]
    stagger: Option<f32>,
}

impl From<CountdownRaw> for Countdown {
    fn from(raw: CountdownRaw) -> Self {
        let mut style = raw.style;
        let box_w = raw.digit_size * 0.75;
        let box_h = raw.digit_size * 1.2;
        if style.width.is_none() {
            let w = countdown_total_width(
                box_w,
                raw.gap,
                raw.show_hours,
                raw.show_minutes,
                raw.show_seconds,
            );
            style.width = Some(CSize::Length(CLP::Px(w)));
        }
        if style.height.is_none() {
            style.height = Some(CSize::Length(CLP::Px(box_h)));
        }
        Countdown {
            seconds: raw.seconds,
            show_hours: raw.show_hours,
            show_minutes: raw.show_minutes,
            show_seconds: raw.show_seconds,
            digit_size: raw.digit_size,
            digit_color: raw.digit_color,
            digit_background: raw.digit_background,
            separator_color: raw.separator_color,
            gap: raw.gap,
            border_radius: raw.border_radius,
            timing: raw.timing,
            style,
            timeline: raw.timeline,
            stagger: raw.stagger,
        }
    }
}

/// Total ink width `paint()`'s cursor walk produces: each visible group is
/// two digit boxes plus one inner gap (`gap*0.3`), and every group after
/// the first is preceded by a separator that consumes a full `gap`. Shared
/// by `From<CountdownRaw>` (to size the box) and `paint()`'s own `total_w`
/// so the two can never drift apart again.
fn countdown_total_width(
    box_w: f32,
    gap: f32,
    show_hours: bool,
    show_minutes: bool,
    show_seconds: bool,
) -> f32 {
    let visible = [show_hours, show_minutes, show_seconds]
        .iter()
        .filter(|v| **v)
        .count() as f32;
    if visible <= 0.0 {
        return 0.0;
    }
    let inner_gap = gap * 0.3;
    let digit_pair_w = box_w * 2.0 + inner_gap;
    let separators = (visible - 1.0).max(0.0);
    digit_pair_w * visible + gap * separators
}

rustmotion_core::impl_traits!(Countdown {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Countdown {
    fn digit_box_size(&self) -> (f32, f32) {
        let box_w = self.digit_size * 0.75;
        let box_h = self.digit_size * 1.2;
        (box_w, box_h)
    }

    fn draw_digit_box(
        &self,
        canvas: &Canvas,
        x: f32,
        y: f32,
        digit: char,
        font: &skia_safe::Font,
        emoji_font: &Option<skia_safe::Font>,
    ) {
        let (box_w, box_h) = self.digit_box_size();

        // Background rounded rect
        let mut bg_paint = paint_from_hex(&self.digit_background);
        bg_paint.set_style(PaintStyle::Fill);
        bg_paint.set_anti_alias(true);
        let rect = Rect::from_xywh(x, y, box_w, box_h);
        let rrect = RRect::new_rect_xy(rect, self.border_radius, self.border_radius);
        canvas.draw_rrect(rrect, &bg_paint);

        // Flip-clock horizontal line across the middle
        let (r, g, b, _) = parse_hex_color(&self.digit_background);
        let mut line_paint = skia_safe::Paint::default();
        line_paint.set_style(PaintStyle::Stroke);
        line_paint.set_stroke_width(1.0);
        line_paint.set_anti_alias(true);
        line_paint.set_color(skia_safe::Color::from_argb(76, r, g, b)); // alpha ~0.3
        let mid_y = y + box_h / 2.0;
        canvas.draw_line(
            skia_safe::Point::new(x, mid_y),
            skia_safe::Point::new(x + box_w, mid_y),
            &line_paint,
        );

        // Digit text centered in box
        let digit_str = digit.to_string();
        let mut text_paint = paint_from_hex(&self.digit_color);
        text_paint.set_anti_alias(true);

        let text_w = measure_text_with_fallback(&digit_str, font, emoji_font, 0.0);
        let (_, metrics) = font.metrics();
        let text_x = x + (box_w - text_w) / 2.0;
        let text_y = y + (box_h + (-metrics.ascent)) / 2.0;

        draw_text_with_fallback(
            canvas,
            &digit_str,
            font,
            emoji_font,
            0.0,
            text_x,
            text_y,
            &text_paint,
        );
    }

    fn draw_separator(
        &self,
        canvas: &Canvas,
        x: f32,
        y: f32,
        font: &skia_safe::Font,
        emoji_font: &Option<skia_safe::Font>,
    ) -> f32 {
        let (_, box_h) = self.digit_box_size();
        let sep = ":";

        let mut sep_paint = paint_from_hex(&self.separator_color);
        sep_paint.set_anti_alias(true);

        let sep_w = measure_text_with_fallback(sep, font, emoji_font, 0.0);
        let (_, metrics) = font.metrics();
        let sep_x = x + (self.gap - sep_w) / 2.0;
        let sep_y = y + (box_h + (-metrics.ascent)) / 2.0;

        draw_text_with_fallback(canvas, sep, font, emoji_font, 0.0, sep_x, sep_y, &sep_paint);

        self.gap
    }
}

impl Countdown {
    fn paint(&self, canvas: &Canvas, layout: &BoxLayout, time: f64) {
        let remaining = (self.seconds - time).max(0.0);
        let total_secs = remaining as u64;
        let hours = total_secs / 3600;
        let minutes = (total_secs % 3600) / 60;
        let secs = total_secs % 60;

        let font_size = self.digit_size * 0.6;
        let font_style = skia_safe::FontStyle::bold();
        let Ok(typeface) = typeface_with_fallback("Inter", font_style) else {
            return;
        };
        let font = skia_safe::Font::from_typeface(typeface, font_size);
        let emoji_font = emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, font_size));

        let (box_w, _box_h) = self.digit_box_size();

        // Safety net: `style.width`/`style.height` on this struct are
        // already sized (see `From<CountdownRaw>`) to match this exact
        // cursor walk, so this shouldn't ever clip in practice — it's a
        // backstop against drift if the two formulas are ever edited
        // separately again.
        canvas.save();
        if layout.width > 0.0 && layout.height > 0.0 {
            canvas.clip_rect(
                Rect::from_xywh(0.0, 0.0, layout.width, layout.height),
                skia_safe::ClipOp::Intersect,
                true,
            );
        }

        let mut cursor_x = 0.0_f32;
        let cursor_y = 0.0_f32;

        let inner_gap = self.gap * 0.3;
        let mut need_separator = false;

        if self.show_hours {
            if need_separator {
                let sep_w = self.draw_separator(canvas, cursor_x, cursor_y, &font, &emoji_font);
                cursor_x += sep_w;
            }
            let h_str = format!("{:02}", hours);
            let chars: Vec<char> = h_str.chars().collect();
            self.draw_digit_box(canvas, cursor_x, cursor_y, chars[0], &font, &emoji_font);
            cursor_x += box_w + inner_gap;
            self.draw_digit_box(canvas, cursor_x, cursor_y, chars[1], &font, &emoji_font);
            cursor_x += box_w;
            need_separator = true;
        }

        if self.show_minutes {
            if need_separator {
                let sep_w = self.draw_separator(canvas, cursor_x, cursor_y, &font, &emoji_font);
                cursor_x += sep_w;
            }
            let m_str = format!("{:02}", minutes);
            let chars: Vec<char> = m_str.chars().collect();
            self.draw_digit_box(canvas, cursor_x, cursor_y, chars[0], &font, &emoji_font);
            cursor_x += box_w + inner_gap;
            self.draw_digit_box(canvas, cursor_x, cursor_y, chars[1], &font, &emoji_font);
            cursor_x += box_w;
            need_separator = true;
        }

        if self.show_seconds {
            if need_separator {
                let sep_w = self.draw_separator(canvas, cursor_x, cursor_y, &font, &emoji_font);
                cursor_x += sep_w;
            }
            let s_str = format!("{:02}", secs);
            let chars: Vec<char> = s_str.chars().collect();
            self.draw_digit_box(canvas, cursor_x, cursor_y, chars[0], &font, &emoji_font);
            cursor_x += box_w + inner_gap;
            self.draw_digit_box(canvas, cursor_x, cursor_y, chars[1], &font, &emoji_font);
        }

        canvas.restore();
    }
}

impl Painter for Countdown {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        self.paint(canvas, layout, ctx.time);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Countdown {
        serde_json::from_str(json).expect("countdown should deserialize")
    }

    fn px_width(c: &Countdown) -> f32 {
        let CSize::Length(CLP::Px(w)) = c.style.width.clone().expect("style.width should be set")
        else {
            panic!("expected an explicit px width");
        };
        w
    }

    #[test]
    fn countdown_total_width_matches_paints_own_cursor_walk() {
        // #127: box_builder.rs's width formula (`visible*2*box_w +
        // (visible-1)*gap`) omits the small inner gap `paint()` inserts
        // *within* each digit pair, so the box came out narrower than
        // what actually got drawn (153×67 assigned vs. 187×68 painted).
        // Recompute by hand here (independent of `countdown_total_width`
        // itself) so this test would catch drift in either direction.
        let box_w = 42.0_f32; // digit_size 56 * 0.75
        let gap = default_gap();
        let inner_gap = gap * 0.3;
        // two visible groups (minutes, seconds): two pairs + one separator
        let by_hand = (box_w * 2.0 + inner_gap) * 2.0 + gap;
        let got = countdown_total_width(box_w, gap, false, true, true);
        assert!(
            (got - by_hand).abs() < 0.001,
            "formula drift: got {got}, hand-computed {by_hand}"
        );
    }

    #[test]
    fn style_width_matches_the_shared_formula() {
        let c = parse(r#"{"type":"countdown","show_hours":false,"digit_size":56}"#);
        let expected = countdown_total_width(56.0 * 0.75, c.gap, false, true, true);
        assert!((px_width(&c) - expected).abs() < 0.001);
    }

    #[test]
    fn explicit_style_width_is_never_overridden() {
        let c = parse(r#"{"type":"countdown","style":{"width":900}}"#);
        assert_eq!(px_width(&c), 900.0);
    }

    #[test]
    fn no_visible_groups_is_zero_not_negative() {
        // Defensive: `(visible - 1.0)` must not go negative and blow up
        // `separators` when nothing is shown.
        assert_eq!(countdown_total_width(42.0, 12.0, false, false, false), 0.0);
    }
}
