use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Font, FontStyle, Rect};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, font_mgr, measure_text_with_fallback, paint_from_hex,
};
use rustmotion_core::schema::{CaptionStyle, CaptionWord, TimelineStep};
use rustmotion_core::traits::{PaintCtx, Painter};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Caption {
    pub words: Vec<CaptionWord>,
    #[serde(default = "default_active_color")]
    pub active_color: String,
    #[serde(default)]
    pub mode: CaptionStyle,
    #[serde(default)]
    pub max_width: Option<f32>,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(Caption {
    Animatable => animation,
    Styled => style,
});

impl Caption {
    fn paint(&self, canvas: &Canvas, layout_width: f32, time: f64) {
        let font_size = self.style.font_size_px_or(48.0);
        let color = self.style.color_str_or("#FFFFFF");
        let font_family = self.style.font_family_or("Inter");

        let fm = font_mgr();
        let typeface = fm
            .match_family_style(font_family, FontStyle::bold())
            .or_else(|| fm.match_family_style("Helvetica", FontStyle::bold()))
            .or_else(|| fm.match_family_style("Arial", FontStyle::bold()))
            .or_else(|| fm.match_family_style("sans-serif", FontStyle::bold()))
            .unwrap_or_else(|| {
                fm.legacy_make_typeface(None, FontStyle::bold())
                    .expect("No fallback font")
            });

        let font = Font::from_typeface(typeface, font_size);
        let emoji_font = emoji_typeface().map(|tf| Font::from_typeface(tf, font_size));

        match self.mode {
            CaptionStyle::WordByWord => {
                for word in &self.words {
                    if time >= word.start && time < word.end {
                        let paint = paint_from_hex(&self.active_color);
                        let text_width =
                            measure_text_with_fallback(&word.text, &font, &emoji_font, 0.0);

                        let cx = layout_width / 2.0;

                        if let Some(bg_color) = self.style.background_color_str() {
                            let padding = font_size * 0.3;
                            let bg_rect = Rect::from_xywh(
                                cx - text_width / 2.0 - padding,
                                -font_size - padding / 2.0,
                                text_width + padding * 2.0,
                                font_size * 1.4 + padding,
                            );
                            let bg_paint = paint_from_hex(bg_color);
                            let rrect = skia_safe::RRect::new_rect_xy(bg_rect, padding, padding);
                            canvas.draw_rrect(rrect, &bg_paint);
                        }

                        let x = cx - text_width / 2.0;
                        draw_text_with_fallback(
                            canvas,
                            &word.text,
                            &font,
                            &emoji_font,
                            0.0,
                            x,
                            0.0,
                            &paint,
                        );
                        break;
                    }
                }
            }
            CaptionStyle::Highlight | CaptionStyle::Karaoke => {
                let max_width = self.max_width.unwrap_or(f32::MAX);
                let space_width = measure_text_with_fallback(" ", &font, &emoji_font, 0.0);

                let mut lines: Vec<Vec<(usize, f32)>> = vec![vec![]];
                let mut current_x = 0.0f32;

                for (i, word) in self.words.iter().enumerate() {
                    let word_width =
                        measure_text_with_fallback(&word.text, &font, &emoji_font, 0.0);
                    if current_x + word_width > max_width && !lines.last().unwrap().is_empty() {
                        lines.push(vec![]);
                        current_x = 0.0;
                    }
                    lines.last_mut().unwrap().push((i, word_width));
                    current_x += word_width + space_width;
                }

                let line_height = font_size * 1.4;
                let cx = layout_width / 2.0;

                if let Some(bg_color) = self.style.background_color_str() {
                    let padding = font_size * 0.3;
                    let total_height = lines.len() as f32 * line_height;
                    let max_line_width = lines
                        .iter()
                        .map(|line| {
                            line.iter().map(|(_, w)| w).sum::<f32>()
                                + (line.len().saturating_sub(1)) as f32 * space_width
                        })
                        .fold(0.0f32, f32::max);
                    let bg_rect = Rect::from_xywh(
                        cx - max_line_width / 2.0 - padding,
                        -font_size - padding / 2.0,
                        max_line_width + padding * 2.0,
                        total_height + padding,
                    );
                    let bg_paint = paint_from_hex(bg_color);
                    let rrect = skia_safe::RRect::new_rect_xy(bg_rect, padding, padding);
                    canvas.draw_rrect(rrect, &bg_paint);
                }

                for (line_idx, line) in lines.iter().enumerate() {
                    let line_width: f32 = line.iter().map(|(_, w)| w).sum::<f32>()
                        + (line.len().saturating_sub(1)) as f32 * space_width;
                    let mut x = cx - line_width / 2.0;
                    let y = line_idx as f32 * line_height;

                    for (word_idx, word_width) in line {
                        let word = &self.words[*word_idx];
                        let is_active = time >= word.start && time < word.end;
                        let word_color = if is_active { &self.active_color } else { color };
                        let paint = paint_from_hex(word_color);

                        draw_text_with_fallback(
                            canvas,
                            &word.text,
                            &font,
                            &emoji_font,
                            0.0,
                            x,
                            y,
                            &paint,
                        );
                        x += word_width + space_width;
                    }
                }
            }
        }
    }
}

impl Painter for Caption {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        self.paint(canvas, layout.width, ctx.time);
    }
}

fn default_active_color() -> String {
    "#FFFF00".to_string()
}
