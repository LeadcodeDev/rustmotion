use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, Rect, RRect};

use crate::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, font_mgr, measure_text_with_fallback, paint_from_hex,
    parse_hex_color,
};
use crate::layout::{Constraints, LayoutNode};
use crate::schema::{LayerStyle, Size};
use crate::traits::{RenderContext, TimingConfig, Widget};

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

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
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
    pub size: Option<Size>,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

crate::impl_traits!(Countdown {
    Animatable => style,
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
            canvas, &digit_str, font, emoji_font, 0.0, text_x, text_y, &text_paint,
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

        draw_text_with_fallback(
            canvas, sep, font, emoji_font, 0.0, sep_x, sep_y, &sep_paint,
        );

        self.gap
    }
}

impl Widget for Countdown {
    fn render(
        &self,
        canvas: &Canvas,
        _layout: &LayoutNode,
        ctx: &RenderContext,
        _props: &crate::engine::animator::AnimatedProperties,
    ) -> Result<()> {
        let remaining = (self.seconds - ctx.time).max(0.0);
        let total_secs = remaining as u64;
        let hours = total_secs / 3600;
        let minutes = (total_secs % 3600) / 60;
        let secs = total_secs % 60;

        let font_size = self.digit_size * 0.6;
        let fm = font_mgr();
        let font_style = skia_safe::FontStyle::bold();
        let typeface = fm
            .match_family_style("Inter", font_style)
            .or_else(|| fm.match_family_style("Helvetica", font_style))
            .unwrap_or_else(|| fm.legacy_make_typeface(None, font_style).unwrap());
        let font = skia_safe::Font::from_typeface(typeface, font_size);
        let emoji_font = emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, font_size));

        let (box_w, _box_h) = self.digit_box_size();

        // Calculate total width for centering
        let mut num_groups = 0;
        if self.show_hours {
            num_groups += 1;
        }
        if self.show_minutes {
            num_groups += 1;
        }
        if self.show_seconds {
            num_groups += 1;
        }

        let digit_pair_w = box_w * 2.0 + self.gap * 0.3; // two digit boxes + small inner gap
        let separators = if num_groups > 1 { num_groups - 1 } else { 0 };
        let total_w = digit_pair_w * num_groups as f32 + self.gap * separators as f32;

        let mut cursor_x = 0.0_f32;
        let cursor_y = 0.0_f32;

        // Center the whole thing if we have a layout
        let _ = total_w; // used for measure

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

        Ok(())
    }

    fn measure(&self, _constraints: &Constraints) -> (f32, f32) {
        if let Some(size) = &self.size {
            return (size.width, size.height);
        }

        let (box_w, box_h) = self.digit_box_size();
        let inner_gap = self.gap * 0.3;
        let digit_pair_w = box_w * 2.0 + inner_gap;

        let mut num_groups = 0;
        if self.show_hours {
            num_groups += 1;
        }
        if self.show_minutes {
            num_groups += 1;
        }
        if self.show_seconds {
            num_groups += 1;
        }

        let separators = if num_groups > 1 { num_groups - 1 } else { 0 };
        let total_w = digit_pair_w * num_groups as f32 + self.gap * separators as f32;

        (total_w, box_h)
    }
}
