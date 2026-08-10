use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Font, FontStyle, Rect};

use rustmotion_core::css::style::{
    FontStyle as CssFontStyle, FontWeight as CssFontWeight, FontWeightKw,
    WhiteSpace as CssWhiteSpace,
};
use rustmotion_core::css::units::LengthContext;
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
    fn paint(&self, canvas: &Canvas, layout_width: f32, layout_height: f32, ctx: &PaintCtx) {
        let time = ctx.time;
        // #9 / lot B (wave S): `font-size` itself now resolves through the
        // same context-aware machinery as `letter-spacing`/`line-height`
        // below — it used to stay on the context-free `font_size_px_or`,
        // silently dropping `rem`/`vw`/`vh` font-size to 0px. `em`/`%` on
        // `font-size` itself remain approximate (see
        // `crate::intrinsic::font_size_ctx`'s doc comment) — cascade.rs
        // doesn't track the real parent font-size.
        let base_ctx = crate::intrinsic::font_size_ctx(
            ctx.video_width as f32,
            ctx.video_height as f32,
            layout_width.max(0.0),
        );
        let font_size = self.style.font_size_px_ctx(&base_ctx, 48.0);
        let color = self.style.color_str_or("#FFFFFF");
        let font_family = self.style.font_family_or("Inter");

        // #9: `letter-spacing`/`line-height` `em`/`%` resolve against this
        // element's own font-size (just above); `vw`/`vh` resolve against
        // the real viewport, available here via `ctx` (mirrors
        // `text.rs::paint`'s `type_ctx`).
        let type_ctx = LengthContext {
            font_size,
            ..base_ctx
        };

        // #9: derive weight/slant from `style.font-weight`/`font-style`
        // instead of always painting bold. `CaptionIntrinsic` (via
        // `TextIntrinsic`) measures at whatever weight the style declares
        // (400/normal when unset) — painting an unconditional bold made the
        // glyphs wider than the box that was centred/measured for them.
        let font_style = Self::resolve_font_style(&self.style);

        let Ok(typeface) = typeface_with_fallback(font_family, font_style) else {
            return;
        };

        let font = Font::from_typeface(typeface, font_size);
        let emoji_font = emoji_typeface().map(|tf| Font::from_typeface(tf, font_size));

        // Every branch below draws text with its *baseline* at local y=0 and
        // (for the pill-background presets) a highlight box extending up to
        // `font_size + padding/2` above that baseline. Treated as the box's
        // own coordinate space (y=0 = box top, as every other painter
        // assumes), that put glyphs — and pills further still — above the
        // assigned box: `CaptionIntrinsic` sizes the box for one line's
        // ascent+descent starting at y=0, not for a baseline sitting at 0
        // with ascenders going negative. Shifting the whole paint down by a
        // margin that covers the tallest pill (WordPop's `font_size*0.35`
        // padding, ~1.175×font_size) puts the topmost ink at/after y=0
        // without touching any of the per-preset layout math below.
        let top_offset = font_size * 1.2;
        canvas.save();
        canvas.translate((0.0, top_offset));
        // Safety net: even with the offset above, an unusually large pill
        // padding combined with a short assigned box could still spill past
        // the bottom edge. Clip vertically only (not horizontally) — a
        // caption in `white-space: nowrap` mode is *meant* to bleed past
        // its own width when `max_width` doesn't fit the line (see
        // `box_builder.rs`'s nowrap comment and the geometry validator's
        // `unwrappable_text_overflow`), so clipping width here would hide a
        // condition the validator is supposed to catch instead.
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
                // #1: when `max_width` is unset, wrap at the box `layout`
                // actually gave this caption (matches `text.rs:442-451`)
                // instead of never wrapping — `CaptionIntrinsic` measures
                // (and taffy reserves a box) against that same width, so
                // painting at `f32::MAX` here painted a single line far
                // wider than the reserved box, bleeding past it and past
                // the viewport with `validate` never seeing the mismatch
                // (it re-measures via the same intrinsic, not this paint
                // path).
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

                // #9: honour `style.line-height` like `CaptionIntrinsic`
                // does (via `TextIntrinsic::from_parts` ->
                // `line_height_for_ctx`) instead of a hardcoded 1.4 — the
                // box taffy reserves is sized from the former, so painting
                // with the latter drifted the line spacing away from what
                // was measured (7.7% at the unset default, arbitrarily more
                // with an explicit `line-height`), and the caption's own
                // vertical clip (below in the outer `paint`) silently crops
                // whatever spills past the mismatch.
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
        canvas.restore();
    }
}

impl Caption {
    /// Draws the rounded pill background used by the pop presets.
    fn draw_pill(&self, canvas: &Canvas, rect: Rect) {
        let radius = rect.height() / 2.0;
        let paint = paint_from_hex(self.pill_color.as_deref().unwrap_or(DEFAULT_PILL_COLOR));
        canvas.draw_rrect(skia_safe::RRect::new_rect_xy(rect, radius, radius), &paint);
    }

    /// #9: the Skia `FontStyle` to paint with, derived from `style.font-
    /// weight`/`font-style` — mirrors `text.rs`'s weight/slant mapping and
    /// `intrinsic.rs`'s `weight_to_u16` (used to measure the box), so the
    /// weight the box was measured at and the weight painted into it always
    /// agree. Pulled out as its own function so it's directly unit-testable
    /// without needing to render anything.
    fn resolve_font_style(style: &CssStyle) -> FontStyle {
        let weight = match &style.font_weight {
            Some(CssFontWeight::Keyword(FontWeightKw::Bold | FontWeightKw::Bolder)) => {
                skia_safe::font_style::Weight::BOLD
            }
            Some(CssFontWeight::Number(n)) if *n >= 600 => skia_safe::font_style::Weight::BOLD,
            Some(CssFontWeight::Number(n)) => skia_safe::font_style::Weight::from(*n as i32),
            _ => skia_safe::font_style::Weight::NORMAL,
        };
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

    /// A `PaintCtx` for tests that don't care about frame/fps bookkeeping —
    /// only `time` and, since #9, the viewport dims threaded into the
    /// `LengthContext` used to resolve `vw`/`vh` typography units.
    fn test_ctx(time: f64) -> PaintCtx {
        PaintCtx {
            time,
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
    fn ink_never_starts_above_the_box_top() {
        // #127: every branch drew its baseline at local y=0 (the box's own
        // top edge) — ascenders, and pill backgrounds further still,
        // painted *above* y=0, bleeding out of whatever box the layout
        // gave this caption (measured: assigned box top at y=124 in a
        // card starting at y=100, but ink started at y≈85 — above the
        // card itself, not just the box).
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
        // The pill-background presets pad further above the baseline than
        // plain text (up to `font_size * 0.35` extra) — the worst case
        // among the four rendering modes.
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

    // ─── #1: wrap at the box's layout_width when max_width is unset ───────

    #[test]
    fn wraps_at_layout_width_when_max_width_is_unset() {
        // Reproduction: no `max_width` on the caption (the common case — a
        // caption's box comes from wherever it's placed, e.g. a card), but
        // the layout pass still hands `paint` a real, finite `layout_width`
        // (mirrors `CaptionIntrinsic`, which measures against exactly this
        // width). Before the fix, `max_width.unwrap_or(f32::MAX)` ignored
        // `layout_width` entirely and painted one line stretching far past
        // the box — and past the viewport in the audit's repro.
        let caption = make_caption_with_max_width(
            "the quick brown fox jumps over the lazy dog again",
            None,
            None, // no explicit max_width
        );
        const W: i32 = 1600;
        const H: i32 = 400;
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        {
            let canvas = surface.canvas();
            canvas.translate((800.0, 200.0));
            // The box the layout pass assigned: 300px wide, well short of
            // this sentence's unwrapped width at font-size 28.
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
        // Regression guard: the #1 fix must not touch `white-space:
        // nowrap`'s existing "always ignore any width constraint" contract.
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

    // ─── #9: line-height / font-weight measure-vs-paint parity ────────────

    #[test]
    fn honours_style_line_height_instead_of_hardcoded_1_4() {
        // Reproduction: `style.line-height: 0.9` must change the vertical
        // gap between wrapped lines. Before the fix, the painter always
        // used `font_size * 1.4` regardless of `style.line-height`, while
        // `CaptionIntrinsic` (the box taffy reserves) honoured it — a
        // caption author following rules/typography-readability.md's
        // guidance to set `line-height` got a box sized for their value but
        // glyphs painted at a fixed 1.4.
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

    // ─── Lot B, wave S: relative `font-size` units ─────────────────────────

    #[test]
    fn rem_font_size_paints_visible_ink() {
        // Reproduction: `font-size: "2rem"` used to resolve to 0px on the
        // context-free `font_size_px_or` path — `CaptionIntrinsic` (via
        // `TextIntrinsic`) measured a 0-height box and nothing painted.
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

    // `Caption::resolve_font_style` is the exact weight/slant computation
    // `paint` uses; testing it directly is deterministic regardless of
    // whether the system's resolved "bold" and "normal" typefaces happen to
    // have visually/metrically distinct advance widths on this particular
    // host (on this machine, Helvetica's bold and normal share identical
    // glyph metrics — a pixel-width comparison would pass whether or not
    // `paint` used the right weight, which isn't a real check of the fix).

    #[test]
    fn resolve_font_style_defaults_to_normal_matching_the_intrinsic_measurement() {
        // #9 (weight half): `CaptionIntrinsic` (via `TextIntrinsic`'s
        // `weight_to_u16`) measures at weight 400 when `style.font-weight`
        // is unset. Before the fix, `paint` ignored `style.font-weight`
        // entirely and always painted `FontStyle::bold()` (weight 700) — a
        // silent measure-vs-paint weight mismatch on every caption that
        // doesn't set an explicit font-weight (the common case).
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

        // Below the >=600 "treat as bold" threshold (same threshold
        // `text.rs`'s equivalent mapping uses), so the exact numeric value
        // passes through unchanged.
        let numeric = CssStyle {
            font_weight: Some(CssFontWeight::Number(350)),
            ..Default::default()
        };
        assert_eq!(*Caption::resolve_font_style(&numeric).weight(), 350);
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
