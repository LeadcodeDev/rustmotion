use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, FontStyle, Rect};

use rustmotion_core::css::style::{FontStyle as CssFontStyle, WhiteSpace as CssWhiteSpace};
use rustmotion_core::css::units::LengthContext;
use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    css_font_weight, draw_text_with_fallback, emoji_typeface, measure_text_with_fallback,
    paint_from_hex, subpixel_font, typeface_with_fallback,
};
use rustmotion_core::schema::{CaptionStyle, CaptionWord, TimelineStep};
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[deprecated(
    since = "0.7.1",
    note = "`caption` is a frozen composition. Compose a `for-each` over the words with \
            `start_at`/`end_at` per word and an expression picking the active one instead \
            — see crates/rustmotion/skills/rules/composition-recipes.md. Kept for \
            compatibility; scheduled for removal in a future major version via \
            `rustmotion migrate` (#335)."
)]
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
    fn paint(&self, canvas: &Canvas, layout_width: f32, layout_height: f32, ctx: &PaintCtx) {
        let time = ctx.time;
        let base_ctx = crate::intrinsic::font_size_ctx(
            ctx.video_width as f32,
            ctx.video_height as f32,
            layout_width.max(0.0),
        );
        let font_size = self.style.font_size_px_ctx(&base_ctx, 48.0);
        let color = self.style.color_str_or("#FFFFFF");
        let font_family = self.style.font_family_or("Inter");

        let type_ctx = LengthContext {
            font_size,
            ..base_ctx
        };

        let font_style = Self::resolve_font_style(&self.style);

        let Ok(typeface) = typeface_with_fallback(font_family, font_style) else {
            return;
        };

        let font = subpixel_font(typeface, font_size);
        let emoji_font = emoji_typeface().map(|tf| subpixel_font(tf, font_size));

        let top_offset = font_size * 1.2;
        canvas.save();
        canvas.translate((0.0, top_offset));
        if layout_height > 0.0 {
            const HALF_PLANE: f32 = 1_000_000.0;
            canvas.clip_rect(
                Rect::from_xywh(-HALF_PLANE, -top_offset, HALF_PLANE * 2.0, layout_height),
                skia_safe::ClipOp::Intersect,
                true,
            );
        }

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

                        let t = (((time - word.start) / POP_DURATION).clamp(0.0, 1.0)) as f32;
                        let scale = ease_out_back(t).max(0.01);

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
                let nowrap = matches!(
                    self.style.white_space,
                    Some(CssWhiteSpace::Nowrap | CssWhiteSpace::Pre)
                );
                let max_width = if nowrap {
                    f32::MAX
                } else if layout_width.is_finite() && layout_width > 0.0 {
                    self.max_width
                        .map_or(layout_width, |mw| mw.min(layout_width))
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

                let line_height = self.style.line_height_for_ctx(font_size, &type_ctx);
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
        canvas.restore();
    }
}

impl Caption {
    fn draw_pill(&self, canvas: &Canvas, rect: Rect) {
        let radius = rect.height() / 2.0;
        let paint = paint_from_hex(self.pill_color.as_deref().unwrap_or(DEFAULT_PILL_COLOR));
        canvas.draw_rrect(skia_safe::RRect::new_rect_xy(rect, radius, radius), &paint);
    }

    fn resolve_font_style(style: &CssStyle) -> FontStyle {
        let weight = css_font_weight(style.font_weight.as_ref());
        let slant = match style.font_style {
            Some(CssFontStyle::Italic) => skia_safe::font_style::Slant::Italic,
            Some(CssFontStyle::Oblique) => skia_safe::font_style::Slant::Oblique,
            _ => skia_safe::font_style::Slant::Upright,
        };
        FontStyle::new(weight, skia_safe::font_style::Width::NORMAL, slant)
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
        self.paint(canvas, layout.width, layout.height, ctx);
    }
}

fn default_active_color() -> String {
    "#FFFF00".to_string()
}

const DEFAULT_PILL_COLOR: &str = "#000000B3";

const POP_DURATION: f64 = 0.18;

const KARAOKE_POP_SCALE: f32 = 1.15;

fn ease_out_back(t: f32) -> f32 {
    const C1: f32 = 1.70158;
    const C3: f32 = C1 + 1.0;
    let p = t - 1.0;
    1.0 + C3 * p * p * p + C1 * p * p
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmotion_core::css::style::{CssStyle, FontWeight as CssFontWeight, FontWeightKw};
    use rustmotion_core::css::Length;
    use rustmotion_core::schema::CaptionWord;

    fn test_ctx(time: f64) -> PaintCtx {
        PaintCtx {
            time,
            scenario_time: time,
            scene_duration: 2.0,
            frame_index: (time * 30.0) as u32,
            fps: 30,
            video_width: 1920,
            video_height: 1080,
            stagger_offset: 0.0,
        }
    }

    fn make_caption(text: &str, white_space: Option<CssWhiteSpace>) -> Caption {
        make_caption_with_max_width(text, white_space, Some(80.0))
    }

    fn make_caption_with_max_width(
        text: &str,
        white_space: Option<CssWhiteSpace>,
        max_width: Option<f32>,
    ) -> Caption {
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
            max_width,
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
    fn ink_never_starts_above_the_box_top() {
        let caption = make_caption("Hello world", None);
        const W: i32 = 400;
        const H: i32 = 200;
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        {
            let canvas = surface.canvas();
            caption.paint(canvas, W as f32, H as f32, &test_ctx(0.5));
        }
        let (_minx, _maxx, miny, _maxy) =
            ink_bounds(&mut surface, W, H).expect("caption must paint something");
        assert!(miny >= 0, "ink starts above the box top at y={miny}");
    }

    #[test]
    fn word_pop_pill_never_starts_above_the_box_top() {
        let mut caption = make_caption("Hello", None);
        caption.mode = CaptionStyle::WordPop;
        caption.words[0].start = 0.0;
        caption.words[0].end = 10.0;
        const W: i32 = 400;
        const H: i32 = 200;
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        {
            let canvas = surface.canvas();
            caption.paint(canvas, W as f32, H as f32, &test_ctx(0.1));
        }
        let (_minx, _maxx, miny, _maxy) =
            ink_bounds(&mut surface, W, H).expect("word_pop caption must paint something");
        assert!(miny >= 0, "pill starts above the box top at y={miny}");
    }

    #[test]
    fn nowrap_paints_one_wide_line_instead_of_wrapping_at_max_width() {
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
            caption.paint(canvas, 80.0, H as f32, &test_ctx(0.5));
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
            caption.paint(canvas, 80.0, H as f32, &test_ctx(0.5));
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

    #[test]
    fn wraps_at_layout_width_when_max_width_is_unset() {
        let caption = make_caption_with_max_width(
            "the quick brown fox jumps over the lazy dog again",
            None,
            None,
        );
        const W: i32 = 1600;
        const H: i32 = 400;
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        {
            let canvas = surface.canvas();
            canvas.translate((800.0, 200.0));
            caption.paint(canvas, 300.0, H as f32, &test_ctx(0.5));
        }
        let (minx, maxx, miny, maxy) =
            ink_bounds(&mut surface, W, H).expect("caption must paint something");

        assert!(
            maxx - minx < 320,
            "must wrap within ~layout_width (300px), got ink width {}",
            maxx - minx
        );
        assert!(
            maxy - miny > 50,
            "must spread across multiple lines when max_width is unset, got ink height {}",
            maxy - miny
        );
    }

    #[test]
    fn nowrap_still_ignores_layout_width_when_max_width_is_unset() {
        let caption = make_caption_with_max_width(
            "the quick brown fox jumps over the lazy dog",
            Some(CssWhiteSpace::Nowrap),
            None,
        );
        const W: i32 = 1600;
        const H: i32 = 400;
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        {
            let canvas = surface.canvas();
            canvas.translate((800.0, 200.0));
            caption.paint(canvas, 300.0, H as f32, &test_ctx(0.5));
        }
        let (minx, maxx, miny, maxy) =
            ink_bounds(&mut surface, W, H).expect("caption must paint something");

        assert!(
            maxx - minx > 400,
            "nowrap must still bleed past layout_width, got ink width {}",
            maxx - minx
        );
        assert!(
            maxy - miny < 50,
            "nowrap must stay on one line, got ink height {}",
            maxy - miny
        );
    }

    #[test]
    fn honours_style_line_height_instead_of_hardcoded_1_4() {
        let mut tight = make_caption_with_max_width(
            "one two three four five six seven eight",
            None,
            Some(80.0),
        );
        tight.style.line_height = Some(rustmotion_core::css::style::LineHeight::Number(0.9));
        let mut loose = make_caption_with_max_width(
            "one two three four five six seven eight",
            None,
            Some(80.0),
        );
        loose.style.line_height = Some(rustmotion_core::css::style::LineHeight::Number(2.0));

        const W: i32 = 1600;
        const H: i32 = 800;

        let mut surf_tight =
            skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        {
            let canvas = surf_tight.canvas();
            canvas.translate((800.0, 50.0));
            tight.paint(canvas, 80.0, H as f32, &test_ctx(0.5));
        }
        let (_, _, _, tight_maxy) =
            ink_bounds(&mut surf_tight, W, H).expect("tight caption must paint something");

        let mut surf_loose =
            skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        {
            let canvas = surf_loose.canvas();
            canvas.translate((800.0, 50.0));
            loose.paint(canvas, 80.0, H as f32, &test_ctx(0.5));
        }
        let (_, _, _, loose_maxy) =
            ink_bounds(&mut surf_loose, W, H).expect("loose caption must paint something");

        assert!(
            loose_maxy > tight_maxy + 50,
            "line-height: 2.0 must spread lines much further than 0.9 \
             (tight bottom={tight_maxy}, loose bottom={loose_maxy})"
        );
    }

    #[test]
    fn rem_font_size_paints_visible_ink() {
        let mut caption = make_caption_with_max_width("hello world", None, Some(300.0));
        caption.style.font_size = Some(Length::String("2rem".into()));
        const W: i32 = 400;
        const H: i32 = 200;
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        {
            let canvas = surface.canvas();
            caption.paint(canvas, 300.0, H as f32, &test_ctx(0.5));
        }
        let bounds = ink_bounds(&mut surface, W, H);
        assert!(
            bounds.is_some(),
            "caption at font-size: 2rem must paint visible ink"
        );
    }

    #[test]
    fn resolve_font_style_defaults_to_normal_matching_the_intrinsic_measurement() {
        let style = CssStyle::default();
        let resolved = Caption::resolve_font_style(&style);
        assert_eq!(
            *resolved.weight(),
            400,
            "unset font-weight must resolve to normal (400), not a hardcoded bold"
        );
    }

    #[test]
    fn resolve_font_style_honours_explicit_bold_and_numeric_weight() {
        let bold = CssStyle {
            font_weight: Some(CssFontWeight::Keyword(FontWeightKw::Bold)),
            ..Default::default()
        };
        assert_eq!(*Caption::resolve_font_style(&bold).weight(), 700);

        let numeric = CssStyle {
            font_weight: Some(CssFontWeight::Number(350)),
            ..Default::default()
        };
        assert_eq!(*Caption::resolve_font_style(&numeric).weight(), 350);
    }

    #[test]
    fn resolve_font_style_does_not_collapse_the_heavy_weights_to_bold() {
        for declared in [600u16, 700, 800, 900] {
            let style = CssStyle {
                font_weight: Some(CssFontWeight::Number(declared)),
                ..Default::default()
            };
            assert_eq!(
                *Caption::resolve_font_style(&style).weight(),
                i32::from(declared),
                "a numeric font-weight must be passed through, not rounded to 700"
            );
        }
    }

    #[test]
    fn resolve_font_style_reads_bolder_and_lighter_as_the_measurer_does() {
        let bolder = CssStyle {
            font_weight: Some(CssFontWeight::Keyword(FontWeightKw::Bolder)),
            ..Default::default()
        };
        assert_eq!(*Caption::resolve_font_style(&bolder).weight(), 800);

        let lighter = CssStyle {
            font_weight: Some(CssFontWeight::Keyword(FontWeightKw::Lighter)),
            ..Default::default()
        };
        assert_eq!(*Caption::resolve_font_style(&lighter).weight(), 300);
    }

    #[test]
    fn resolve_font_style_honours_italic() {
        let italic = CssStyle {
            font_style: Some(CssFontStyle::Italic),
            ..Default::default()
        };
        assert_eq!(
            Caption::resolve_font_style(&italic).slant(),
            skia_safe::font_style::Slant::Italic
        );
    }
}
