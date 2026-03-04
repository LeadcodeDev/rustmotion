use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Font, FontStyle, Rect, TextBlob};

use crate::engine::renderer::{font_mgr, paint_from_hex};
use crate::layout::{Constraints, LayoutNode};
use crate::schema::{CaptionStyle, CaptionWord};
use crate::traits::{AnimationConfig, RenderContext, StyleConfig, Widget};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Caption {
    pub words: Vec<CaptionWord>,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default)]
    pub font_family: Option<String>,
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default = "default_active_color")]
    pub active_color: String,
    #[serde(default)]
    pub background: Option<String>,
    #[serde(default)]
    pub style: CaptionStyle,
    #[serde(default)]
    pub max_width: Option<f32>,
    // Caption supports animation (presets) but not timed visibility
    #[serde(flatten)]
    pub animation: AnimationConfig,
    #[serde(flatten)]
    pub style_config: StyleConfig,
}

crate::impl_traits!(Caption {
    Animatable => animation,
    Styled => style_config,
});

impl Widget for Caption {
    fn render(&self, canvas: &Canvas, layout: &LayoutNode, ctx: &RenderContext) -> Result<()> {
        let fm = font_mgr();
        let font_family = self.font_family.as_deref().unwrap_or("Inter");
        let typeface = fm
            .match_family_style(font_family, FontStyle::bold())
            .or_else(|| fm.match_family_style("Helvetica", FontStyle::bold()))
            .or_else(|| fm.match_family_style("Arial", FontStyle::bold()))
            .unwrap_or_else(|| fm.match_family_style("sans-serif", FontStyle::bold()).unwrap());

        let font = Font::from_typeface(typeface, self.font_size);

        match self.style {
            CaptionStyle::WordByWord => {
                for word in &self.words {
                    if ctx.time >= word.start && ctx.time < word.end {
                        let paint = paint_from_hex(&self.active_color);
                        let (text_width, _) = font.measure_str(&word.text, None);

                        // Center within layout width
                        let cx = layout.width / 2.0;

                        if let Some(ref bg_color) = self.background {
                            let padding = self.font_size * 0.3;
                            let bg_rect = Rect::from_xywh(
                                cx - text_width / 2.0 - padding,
                                -self.font_size - padding / 2.0,
                                text_width + padding * 2.0,
                                self.font_size * 1.4 + padding,
                            );
                            let bg_paint = paint_from_hex(bg_color);
                            let rrect = skia_safe::RRect::new_rect_xy(bg_rect, padding, padding);
                            canvas.draw_rrect(rrect, &bg_paint);
                        }

                        if let Some(blob) = TextBlob::new(&word.text, &font) {
                            let x = cx - text_width / 2.0;
                            canvas.draw_text_blob(&blob, (x, 0.0), &paint);
                        }
                        break;
                    }
                }
            }
            CaptionStyle::Highlight | CaptionStyle::Karaoke => {
                let max_width = self.max_width.unwrap_or(f32::MAX);
                let space_width = font.measure_str(" ", None).0;

                // Build lines with word wrapping
                let mut lines: Vec<Vec<(usize, f32)>> = vec![vec![]];
                let mut current_x = 0.0f32;

                for (i, word) in self.words.iter().enumerate() {
                    let (word_width, _) = font.measure_str(&word.text, None);
                    if current_x + word_width > max_width && !lines.last().unwrap().is_empty() {
                        lines.push(vec![]);
                        current_x = 0.0;
                    }
                    lines.last_mut().unwrap().push((i, word_width));
                    current_x += word_width + space_width;
                }

                let line_height = self.font_size * 1.4;
                let cx = layout.width / 2.0;

                // Draw background pill if configured
                if let Some(ref bg_color) = self.background {
                    let padding = self.font_size * 0.3;
                    let total_height = lines.len() as f32 * line_height;
                    let max_line_width = lines.iter().map(|line| {
                        line.iter().map(|(_, w)| w).sum::<f32>() + (line.len().saturating_sub(1)) as f32 * space_width
                    }).fold(0.0f32, f32::max);
                    let bg_rect = Rect::from_xywh(
                        cx - max_line_width / 2.0 - padding,
                        -self.font_size - padding / 2.0,
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
                        let is_active = ctx.time >= word.start && ctx.time < word.end;
                        let color = if is_active { &self.active_color } else { &self.color };
                        let paint = paint_from_hex(color);

                        if let Some(blob) = TextBlob::new(&word.text, &font) {
                            canvas.draw_text_blob(&blob, (x, y), &paint);
                        }
                        x += word_width + space_width;
                    }
                }
            }
        }

        Ok(())
    }

    fn measure(&self, _constraints: &Constraints) -> (f32, f32) {
        let w = self.max_width.unwrap_or(400.0);
        let h = self.font_size * 1.3;
        (w, h)
    }
}

fn default_font_size() -> f32 { 48.0 }
fn default_color() -> String { "#FFFFFF".to_string() }
fn default_active_color() -> String { "#FFFF00".to_string() }
