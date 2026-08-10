use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, RRect, Rect};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::{ease, AnimatedProperties};
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, paint_from_hex, resolve_custom_typeface,
    typeface_with_fallback,
};
use rustmotion_core::schema::{CodeblockReveal, RevealMode, TimelineStep};
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum TerminalLineType {
    Prompt,
    Command,
    #[default]
    Output,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TerminalLine {
    pub text: String,
    #[serde(default)]
    pub line_type: TerminalLineType,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum TerminalTheme {
    #[default]
    Dark,
    Light,
}

impl TerminalTheme {
    fn bg(&self) -> &str {
        match self {
            TerminalTheme::Dark => "#1E1E1E",
            TerminalTheme::Light => "#F5F5F5",
        }
    }

    fn chrome_bg(&self) -> &str {
        match self {
            TerminalTheme::Dark => "#2D2D2D",
            TerminalTheme::Light => "#E5E5E5",
        }
    }

    fn prompt_color(&self) -> &str {
        match self {
            TerminalTheme::Dark => "#22C55E",
            TerminalTheme::Light => "#16A34A",
        }
    }

    fn command_color(&self) -> &str {
        match self {
            TerminalTheme::Dark => "#FFFFFF",
            TerminalTheme::Light => "#000000",
        }
    }

    fn output_color(&self) -> &str {
        match self {
            TerminalTheme::Dark => "#A0A0A0",
            TerminalTheme::Light => "#555555",
        }
    }

    fn title_color(&self) -> &str {
        match self {
            TerminalTheme::Dark => "#808080",
            TerminalTheme::Light => "#666666",
        }
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Terminal {
    pub lines: Vec<TerminalLine>,
    #[serde(default)]
    pub theme: TerminalTheme,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default = "default_show_chrome")]
    pub show_chrome: bool,
    #[serde(default)]
    pub reveal: Option<CodeblockReveal>,
    /// When rendered content overflows the box vertically, scroll up so the
    /// last revealed line stays visible. Default: `true`. Set to `false` to
    /// require all lines fit — the geometry validator will fail otherwise.
    /// Font size is never reduced.
    #[serde(default = "default_auto_scroll")]
    pub auto_scroll: bool,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

fn default_show_chrome() -> bool {
    true
}

fn default_auto_scroll() -> bool {
    true
}

rustmotion_core::impl_traits!(Terminal {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

pub const CORNER_RADIUS: f32 = 10.0;
pub(crate) const CHROME_HEIGHT: f32 = 36.0;
pub(crate) const FONT_SIZE: f32 = 14.0;
pub(crate) const LINE_HEIGHT: f32 = 22.0;
pub(crate) const PADDING: f32 = 16.0;

/// The typeface both the painter and the intrinsic measurement resolve.
///
/// These must never diverge. The measurement reserves the box the painter then
/// fills, so a different face on either side produces text that overflows a box
/// the geometry validator has already declared safe — the exact failure mode the
/// audit found across the text components. Both sides used to hardcode
/// `"SF Mono"` independently, which agreed only by coincidence and ignored an
/// explicit `font-family` outright, custom or not.
pub(crate) fn resolve_typeface(style: &CssStyle) -> Option<skia_safe::Typeface> {
    let font_style = skia_safe::FontStyle::normal();
    let family = style.font_family_or("SF Mono");
    resolve_custom_typeface(family, font_style)
        .or_else(|| typeface_with_fallback(family, font_style).ok())
}

impl Terminal {
    /// `font_size` is resolved once by the caller (`paint`, against a real
    /// `LengthContext`) and threaded through here and [`Self::line_height`]
    /// instead of each independently re-deriving it via the context-free
    /// `font_size_px_or` — four separate call sites used to do exactly that,
    /// which is how a relative unit could silently diverge between them
    /// (lot B, wave S).
    fn make_font(&self, font_size: f32) -> Option<skia_safe::Font> {
        let typeface = resolve_typeface(&self.style)?;
        Some(skia_safe::Font::from_typeface(typeface, font_size))
    }

    fn line_height(&self, font_size: f32) -> f32 {
        (font_size * LINE_HEIGHT / FONT_SIZE).ceil()
    }

    /// Get the prefix string for a line type.
    fn line_prefix(line_type: &TerminalLineType) -> &'static str {
        match line_type {
            TerminalLineType::Prompt => "$ ",
            TerminalLineType::Command | TerminalLineType::Output => "",
        }
    }

    /// Compute reveal visibility: (visible_lines, partial_chars_on_last_line, last_line_opacity)
    fn compute_reveal(&self, time: f64) -> (usize, Option<usize>, f32) {
        let total_lines = self.lines.len();
        if total_lines == 0 {
            return (0, None, 1.0);
        }

        let reveal = match &self.reveal {
            None => return (total_lines, None, 1.0),
            Some(r) => r,
        };

        if time < reveal.start {
            return (0, None, 1.0);
        }

        let raw_progress = ((time - reveal.start) / reveal.duration).clamp(0.0, 1.0);
        let progress = ease(raw_progress, &reveal.easing);

        match reveal.mode {
            RevealMode::Typewriter => {
                // Count total characters including prefixes
                let total_chars: usize = self
                    .lines
                    .iter()
                    .map(|l| Self::line_prefix(&l.line_type).len() + l.text.len())
                    .sum();

                let visible_chars = (total_chars as f64 * progress).round() as usize;
                let mut chars_remaining = visible_chars;
                let mut visible_lines = 0;
                let mut partial_chars = None;

                for line in &self.lines {
                    let line_chars = Self::line_prefix(&line.line_type).len() + line.text.len();
                    if chars_remaining >= line_chars {
                        chars_remaining -= line_chars;
                        visible_lines += 1;
                    } else {
                        visible_lines += 1;
                        partial_chars = Some(chars_remaining);
                        break;
                    }
                }

                (visible_lines, partial_chars, 1.0)
            }
            RevealMode::LineByLine => {
                let visible_f = total_lines as f64 * progress;
                let full_lines = visible_f.floor() as usize;
                let fractional = (visible_f - full_lines as f64) as f32;

                if full_lines >= total_lines {
                    (total_lines, None, 1.0)
                } else {
                    (full_lines + 1, None, fractional.max(0.01))
                }
            }
        }
    }
}

impl Terminal {
    fn paint(&self, canvas: &Canvas, layout_w: f32, layout_h: f32, ctx: &PaintCtx) {
        let time = ctx.time;
        let w = layout_w;
        let h = layout_h;

        // Resolved once, against the real per-frame viewport (`rem`/`vw`/
        // `vh` on `font-size` now resolve instead of silently dropping to
        // 0px — lot B, wave S) and threaded through every call below that
        // used to independently re-derive it.
        let font_size = self.style.font_size_px_ctx(
            &crate::intrinsic::font_size_ctx(ctx.video_width as f32, ctx.video_height as f32, 0.0),
            FONT_SIZE,
        );

        // Background
        let bg_rect = Rect::from_xywh(0.0, 0.0, w, h);
        let bg_rrect = RRect::new_rect_xy(bg_rect, CORNER_RADIUS, CORNER_RADIUS);
        let mut bg_paint = paint_from_hex(self.theme.bg());
        bg_paint.set_style(PaintStyle::Fill);
        bg_paint.set_anti_alias(true);
        canvas.draw_rrect(bg_rrect, &bg_paint);

        // Resolve the terminal font up front — bail before any canvas.save()
        // so save/restore stays balanced if no font is available.
        let Some(font) = self.make_font(font_size) else {
            return;
        };

        // Always clip content to the rounded box so scrolled lines fade
        // cleanly at the edges and never escape the device viewport.
        canvas.save();
        canvas.clip_rrect(bg_rrect, skia_safe::ClipOp::Intersect, true);

        let mut y_offset = 0.0;

        // Chrome (title bar)
        if self.show_chrome {
            // Chrome background
            let chrome_rect = Rect::from_xywh(0.0, 0.0, w, CHROME_HEIGHT);
            canvas.save();
            canvas.clip_rrect(bg_rrect, skia_safe::ClipOp::Intersect, true);
            let mut chrome_paint = paint_from_hex(self.theme.chrome_bg());
            chrome_paint.set_style(PaintStyle::Fill);
            canvas.draw_rect(chrome_rect, &chrome_paint);
            canvas.restore();

            // Traffic light dots
            let dot_colors = ["#FF5F57", "#FEBC2E", "#28C840"];
            let dot_y = CHROME_HEIGHT / 2.0;
            for (i, color) in dot_colors.iter().enumerate() {
                let dot_x = 14.0 + i as f32 * 20.0;
                let mut dot_paint = paint_from_hex(color);
                dot_paint.set_style(PaintStyle::Fill);
                dot_paint.set_anti_alias(true);
                canvas.draw_circle((dot_x, dot_y), 6.0, &dot_paint);
            }

            // Title
            if let Some(title) = &self.title {
                let emoji_font =
                    emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, font_size));
                let mut title_paint = paint_from_hex(self.theme.title_color());
                title_paint.set_anti_alias(true);
                let title_w = rustmotion_core::engine::renderer::measure_text_with_fallback(
                    title,
                    &font,
                    &emoji_font,
                    0.0,
                );
                let x = (w - title_w) / 2.0;
                let (_, metrics) = font.metrics();
                let y = CHROME_HEIGHT / 2.0 + (-metrics.ascent) / 2.0;
                draw_text_with_fallback(canvas, title, &font, &emoji_font, 0.0, x, y, &title_paint);
            }

            y_offset = CHROME_HEIGHT;
        }

        // Compute reveal visibility
        let (visible_lines, partial_chars, last_line_opacity) = self.compute_reveal(time);

        // Lines
        let emoji_font = emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, font_size));
        let (_, metrics) = font.metrics();
        let ascent = -metrics.ascent;

        y_offset += PADDING;

        // auto_scroll: when the rendered content is taller than the box,
        // translate the lines upward so the latest revealed line stays in
        // view. We open a nested clip below the chrome so scrolled lines
        // never bleed onto the title bar.
        let chrome_h = if self.show_chrome { CHROME_HEIGHT } else { 0.0 };
        canvas.save();
        canvas.clip_rect(
            Rect::from_xywh(0.0, chrome_h, w, h - chrome_h),
            skia_safe::ClipOp::Intersect,
            true,
        );
        if self.auto_scroll {
            let line_h = self.line_height(font_size);
            let content_h = visible_lines as f32 * line_h + PADDING * 2.0 + chrome_h;
            let overflow = content_h - h;
            if overflow > 0.0 {
                canvas.translate((0.0, -overflow));
            }
        }

        for (i, line) in self.lines.iter().enumerate() {
            if i >= visible_lines {
                break;
            }

            let is_last_visible = i == visible_lines - 1;
            let opacity = if is_last_visible {
                last_line_opacity
            } else {
                1.0
            };

            let prefix = Self::line_prefix(&line.line_type);
            let (prefix_color, text_color) = match line.line_type {
                TerminalLineType::Prompt => (self.theme.prompt_color(), self.theme.prompt_color()),
                TerminalLineType::Command => ("", self.theme.command_color()),
                TerminalLineType::Output => ("", self.theme.output_color()),
            };

            let color = line.color.as_deref().unwrap_or(text_color);
            let y = y_offset + ascent;
            let mut x = PADDING;

            // Determine what to draw based on partial_chars for typewriter mode
            let (draw_prefix, draw_text) = if is_last_visible {
                if let Some(char_limit) = partial_chars {
                    // Truncate: prefix first, then text
                    let prefix_len = prefix.len();
                    if char_limit <= prefix_len {
                        // Only partial prefix visible
                        let partial: String = prefix.chars().take(char_limit).collect();
                        (partial, String::new())
                    } else {
                        // Full prefix + partial text
                        let text_chars = char_limit - prefix_len;
                        let partial: String = line.text.chars().take(text_chars).collect();
                        (prefix.to_string(), partial)
                    }
                } else {
                    (prefix.to_string(), line.text.clone())
                }
            } else {
                (prefix.to_string(), line.text.clone())
            };

            // Draw prompt prefix
            if !draw_prefix.is_empty() {
                let mut prefix_paint = paint_from_hex(prefix_color);
                prefix_paint.set_anti_alias(true);
                prefix_paint.set_alpha_f(opacity);
                let prefix_w = rustmotion_core::engine::renderer::measure_text_with_fallback(
                    &draw_prefix,
                    &font,
                    &emoji_font,
                    0.0,
                );
                draw_text_with_fallback(
                    canvas,
                    &draw_prefix,
                    &font,
                    &emoji_font,
                    0.0,
                    x,
                    y,
                    &prefix_paint,
                );
                x += prefix_w + 2.0;
            }

            // Draw text
            if !draw_text.is_empty() {
                let mut text_paint = paint_from_hex(color);
                text_paint.set_anti_alias(true);
                text_paint.set_alpha_f(opacity);
                let text_w = rustmotion_core::engine::renderer::measure_text_with_fallback(
                    &draw_text,
                    &font,
                    &emoji_font,
                    0.0,
                );
                draw_text_with_fallback(
                    canvas,
                    &draw_text,
                    &font,
                    &emoji_font,
                    0.0,
                    x,
                    y,
                    &text_paint,
                );
                x += text_w;
            }

            // Blinking cursor on the last visible line during typewriter reveal
            if is_last_visible && self.reveal.is_some() && partial_chars.is_some() {
                let blink = ((time * 2.0) as i32) % 2 == 0;
                if blink {
                    let cursor_w = font_size * 0.55;
                    let cursor_h = font_size * 1.2;
                    let cursor_y = y - font_size;
                    let cursor_rect = Rect::from_xywh(x + 1.0, cursor_y, cursor_w, cursor_h);
                    let mut cursor_paint = paint_from_hex(self.theme.command_color());
                    cursor_paint.set_style(PaintStyle::Fill);
                    cursor_paint.set_anti_alias(true);
                    canvas.draw_rect(cursor_rect, &cursor_paint);
                }
            }

            y_offset += self.line_height(font_size);
        }

        canvas.restore(); // close inner content clip
        canvas.restore(); // close outer rrect clip
    }
}

impl Painter for Terminal {
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

#[cfg(test)]
mod tests {
    use super::*;

    // ─── Lot B, wave S: relative `font-size` units ─────────────────────────

    #[test]
    fn rem_font_size_paints_visible_ink() {
        // Reproduction: `font-size: "2rem"` used to resolve to 0px via the
        // context-free `font_size_px_or`, at all four call sites that used
        // to independently re-derive it in `paint`.
        let terminal = Terminal {
            lines: vec![TerminalLine {
                text: "hello world".to_string(),
                line_type: TerminalLineType::Output,
                color: None,
            }],
            theme: TerminalTheme::default(),
            title: None,
            show_chrome: false,
            reveal: None,
            auto_scroll: true,
            timing: Default::default(),
            style: CssStyle {
                font_size: Some(rustmotion_core::css::Length::String("2rem".into())),
                ..Default::default()
            },
            timeline: Vec::new(),
            stagger: None,
        };
        const W: i32 = 400;
        const H: i32 = 200;
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        let ctx = PaintCtx {
            time: 0.0,
            scene_duration: 1.0,
            frame_index: 0,
            fps: 30,
            video_width: 400,
            video_height: 200,
            stagger_offset: 0.0,
        };
        {
            let canvas = surface.canvas();
            terminal.paint(canvas, W as f32, H as f32, &ctx);
        }
        let snapshot = surface.image_snapshot();
        let info = skia_safe::ImageInfo::new(
            (W, H),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut buf = vec![0u8; (W * H * 4) as usize];
        let ok = snapshot.read_pixels(
            &info,
            &mut buf,
            (W * 4) as usize,
            skia_safe::IPoint::new(0, 0),
            skia_safe::image::CachingHint::Disallow,
        );
        assert!(ok, "pixel read should succeed");
        // Background always paints (opaque bg_rrect), so probe for text ink
        // specifically: pixels that are not the dark theme background color
        // (#1E1E1E) and not fully transparent.
        let text_ink = buf
            .chunks_exact(4)
            .filter(|p| p[3] > 0 && !(p[0] < 40 && p[1] < 40 && p[2] < 40))
            .count();
        assert!(
            text_ink > 20,
            "terminal at font-size: 2rem must paint visible text ink, got {text_ink} pixels"
        );
    }
}
