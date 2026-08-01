use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Font, FontStyle, Rect};

use rustmotion_core::css::style::WhiteSpace as CssWhiteSpace;
use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, measure_text_with_fallback, paint_from_hex,
    typeface_with_fallback,
};
use rustmotion_core::schema::{CaptionStyle, CaptionWord, TimelineStep};
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Caption {
    pub words: Vec<CaptionWord>,
    #[serde(default = "default_active_color")]
    pub active_color: String,
    #[serde(default)]
    pub mode: CaptionStyle,
    #[serde(default)]
    pub max_width: Option<f32>,
    /// Pill background color behind the active word (`word_pop` /
    /// `karaoke_pop` modes). Defaults to black at 70% opacity.
    #[serde(default)]
    pub pill_color: Option<String>,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(Caption {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Caption {
    fn paint(&self, canvas: &Canvas, layout_width: f32, time: f64) {
        let font_size = self.style.font_size_px_or(48.0);
        let color = self.style.color_str_or("#FFFFFF");
        let font_family = self.style.font_family_or("Inter");

        let Ok(typeface) = typeface_with_fallback(font_family, FontStyle::bold()) else {
            return;
        };

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
            CaptionStyle::WordPop => {
                for word in &self.words {
                    if time >= word.start && time < word.end {
                        let text_width =
                            measure_text_with_fallback(&word.text, &font, &emoji_font, 0.0);
                        let cx = layout_width / 2.0;

                        // Spring-like pop: ease-out-back over the first 180ms
                        // of the word window (overshoots ~1.1 then settles).
                        let t = (((time - word.start) / POP_DURATION).clamp(0.0, 1.0)) as f32;
                        let scale = ease_out_back(t).max(0.01);

                        // Scale around the visual center of the word (the
                        // baseline sits at y=0, glyphs extend upward).
                        let cy = -font_size * 0.35;
                        canvas.save();
                        canvas.translate((cx, cy));
                        canvas.scale((scale, scale));
                        canvas.translate((-cx, -cy));

                        let padding = font_size * 0.35;
                        self.draw_pill(
                            canvas,
                            Rect::from_xywh(
                                cx - text_width / 2.0 - padding,
                                -font_size - padding / 2.0,
                                text_width + padding * 2.0,
                                font_size * 1.4 + padding,
                            ),
                        );

                        let paint = paint_from_hex(&self.active_color);
                        draw_text_with_fallback(
                            canvas,
                            &word.text,
                            &font,
                            &emoji_font,
                            0.0,
                            cx - text_width / 2.0,
                            0.0,
                            &paint,
                        );
                        canvas.restore();
                        break;
                    }
                }
            }
            CaptionStyle::Highlight | CaptionStyle::Karaoke | CaptionStyle::KaraokePop => {
                // M1: `white-space: nowrap|pre` keeps every word on one
                // line — ignore `max_width` entirely so the line can bleed
                // past it, same rule `text.rs` uses. (`WordByWord`/`WordPop`
                // above show a single word at a time; wrapping is moot
                // there, same as the existing kbd/counter/badge "atomic"
                // components, so they don't need this branch.)
                let nowrap = matches!(
                    self.style.white_space,
                    Some(CssWhiteSpace::Nowrap | CssWhiteSpace::Pre)
                );
                let max_width = if nowrap {
                    f32::MAX
                } else {
                    self.max_width.unwrap_or(f32::MAX)
                };
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
                        let pop = is_active && matches!(self.mode, CaptionStyle::KaraokePop);
                        let word_color = if is_active { &self.active_color } else { color };
                        let paint = paint_from_hex(word_color);

                        if pop {
                            // Active word scales up ~1.15x around its visual
                            // center, on top of a pill background.
                            let wcx = x + word_width / 2.0;
                            let wcy = y - font_size * 0.35;
                            canvas.save();
                            canvas.translate((wcx, wcy));
                            canvas.scale((KARAOKE_POP_SCALE, KARAOKE_POP_SCALE));
                            canvas.translate((-wcx, -wcy));

                            let padding = font_size * 0.18;
                            self.draw_pill(
                                canvas,
                                Rect::from_xywh(
                                    x - padding,
                                    y - font_size - padding / 2.0,
                                    word_width + padding * 2.0,
                                    font_size * 1.4 + padding,
                                ),
                            );
                        }

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
                        if pop {
                            canvas.restore();
                        }
                        x += word_width + space_width;
                    }
                }
            }
        }
    }
}

impl Caption {
    /// Draws the rounded pill background used by the pop presets.
    fn draw_pill(&self, canvas: &Canvas, rect: Rect) {
        let radius = rect.height() / 2.0;
        let paint = paint_from_hex(self.pill_color.as_deref().unwrap_or(DEFAULT_PILL_COLOR));
        canvas.draw_rrect(skia_safe::RRect::new_rect_xy(rect, radius, radius), &paint);
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

/// Black at 70% opacity — default pill background for the pop presets.
const DEFAULT_PILL_COLOR: &str = "#000000B3";

/// Duration (seconds) of the word_pop scale-in.
const POP_DURATION: f64 = 0.18;

/// Scale factor applied to the active word in karaoke_pop.
const KARAOKE_POP_SCALE: f32 = 1.15;

/// Ease-out-back easing: starts at 0, overshoots ~1.1, settles at 1.
fn ease_out_back(t: f32) -> f32 {
    const C1: f32 = 1.70158;
    const C3: f32 = C1 + 1.0;
    let p = t - 1.0;
    1.0 + C3 * p * p * p + C1 * p * p
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmotion_core::css::style::CssStyle;
    use rustmotion_core::css::Length;
    use rustmotion_core::schema::CaptionWord;

    fn make_caption(text: &str, white_space: Option<CssWhiteSpace>) -> Caption {
        let words = text
            .split_whitespace()
            .map(|w| CaptionWord {
                text: w.to_string(),
                start: 0.0,
                end: 1000.0,
            })
            .collect();
        Caption {
            words,
            active_color: default_active_color(),
            mode: CaptionStyle::Highlight,
            max_width: Some(80.0),
            pill_color: None,
            style: CssStyle {
                font_size: Some(Length::Px(28.0)),
                white_space,
                ..Default::default()
            },
            timing: Default::default(),
            timeline: Vec::new(),
            stagger: None,
        }
    }

    /// Bounding box (min_x, max_x, min_y, max_y) of every non-transparent
    /// pixel on the surface, or `None` if nothing was painted.
    fn ink_bounds(
        surface: &mut skia_safe::Surface,
        w: i32,
        h: i32,
    ) -> Option<(i32, i32, i32, i32)> {
        let snapshot = surface.image_snapshot();
        let info = skia_safe::ImageInfo::new(
            (w, h),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let ok = snapshot.read_pixels(
            &info,
            &mut buf,
            (w * 4) as usize,
            skia_safe::IPoint::new(0, 0),
            skia_safe::image::CachingHint::Disallow,
        );
        assert!(ok, "pixel read should succeed");
        let (mut minx, mut maxx, mut miny, mut maxy) = (i32::MAX, i32::MIN, i32::MAX, i32::MIN);
        for y in 0..h {
            for x in 0..w {
                if buf[((y * w + x) * 4 + 3) as usize] > 0 {
                    minx = minx.min(x);
                    maxx = maxx.max(x);
                    miny = miny.min(y);
                    maxy = maxy.max(y);
                }
            }
        }
        (minx <= maxx).then_some((minx, maxx, miny, maxy))
    }

    #[test]
    fn nowrap_paints_one_wide_line_instead_of_wrapping_at_max_width() {
        // M1 render-level proof, caption (Highlight mode): `white-space:
        // nowrap` keeps every word on one line — much wider than
        // `max_width: 80`, and only one line tall — instead of wrapping.
        let caption = make_caption(
            "the quick brown fox jumps over the lazy dog",
            Some(CssWhiteSpace::Nowrap),
        );
        const W: i32 = 1600;
        const H: i32 = 400;
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        {
            let canvas = surface.canvas();
            canvas.translate((800.0, 250.0));
            caption.paint(canvas, 80.0, 0.5);
        }
        let (minx, maxx, miny, maxy) =
            ink_bounds(&mut surface, W, H).expect("nowrap caption must paint something");

        assert!(
            maxx - minx > 240,
            "nowrap caption must bleed far past its 80px max_width, got ink width {}",
            maxx - minx
        );
        assert!(
            maxy - miny < 50,
            "nowrap caption must stay on one line, got ink height {}",
            maxy - miny
        );
    }

    #[test]
    fn normal_white_space_wraps_at_max_width() {
        let caption = make_caption("the quick brown fox jumps over the lazy dog", None);
        const W: i32 = 1600;
        const H: i32 = 400;
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        {
            let canvas = surface.canvas();
            canvas.translate((800.0, 250.0));
            caption.paint(canvas, 80.0, 0.5);
        }
        let (minx, maxx, miny, maxy) =
            ink_bounds(&mut surface, W, H).expect("wrapped caption must paint something");

        assert!(
            maxx - minx < 200,
            "wrapped caption must pack close to its 80px max_width, got ink width {}",
            maxx - minx
        );
        assert!(
            maxy - miny > 50,
            "wrapped caption must spread across multiple lines, got ink height {}",
            maxy - miny
        );
    }
}
