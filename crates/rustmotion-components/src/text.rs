use rustmotion_core::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Font, FontStyle, Paint, PaintStyle, Rect};

use rustmotion_core::css::style::{
    FontStyle as CssFontStyle, FontWeight as CssFontWeight, FontWeightKw,
    TextAlign as CssTextAlign, WhiteSpace as CssWhiteSpace,
};
use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, measure_text_with_fallback, paint_from_hex,
    typeface_with_fallback, wrap_text_with_tracking,
};
use rustmotion_core::schema::{
    CaretConfig, CaretShape, FontStyleType, FontWeight, Stroke, TextAlign, TextBackground,
    TextShadow, TextState, TextSwapConfig, TimelineStep,
};
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Text {
    pub content: String,
    #[serde(default)]
    pub max_width: Option<f32>,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
    #[serde(default, rename = "text-shadow")]
    pub text_shadow: Option<TextShadow>,
    #[serde(default)]
    pub stroke: Option<Stroke>,
    #[serde(default, rename = "text-background")]
    pub text_background: Option<TextBackground>,
    /// A caret pinned to the reveal head of a `typewriter` animation.
    /// See [`CaretConfig`].
    #[serde(default)]
    pub caret: Option<CaretConfig>,
    /// Later labels this text swaps to. See [`TextState`].
    #[serde(default)]
    pub states: Vec<TextState>,
    /// How the crossing between `states` is animated. See [`TextSwapConfig`].
    #[serde(default)]
    pub swap: Option<TextSwapConfig>,
    /// A letter-by-letter transition between two `states`, instead of the
    /// whole-label rise-and-blur `swap` performs. See [`TextMorphConfig`].
    #[serde(default)]
    pub morph: Option<TextMorphConfig>,
}

rustmotion_core::impl_traits!(Text {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

/// How unmatched glyphs behave during a [`TextMorphConfig`] transition —
/// characters present in the incoming label with no identical counterpart in
/// the outgoing one.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, JsonSchema, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TextMorphUnmatched {
    /// The unmatched glyph simply fades in (or out) at its resting position.
    #[default]
    Fade,
    /// The unmatched glyph cycles through deterministic random characters
    /// before settling on the real one, instead of just fading in blank.
    Scramble,
}

/// A letter-by-letter morph between two `states` labels: identical
/// characters are paired left to right and slide from their old position to
/// their new one; characters with no pair fade (or, with `unmatched:
/// "scramble"`, cycle through placeholder glyphs before settling).
///
/// This is a different crossing than [`TextSwapConfig`]: `swap` treats each
/// label as one rigid block (rise + blur); `morph` treats it as a bag of
/// glyphs that rearranges itself. Setting both is not meaningful — `morph`
/// takes over the transition window whenever it applies, `swap` is only
/// consulted outside it.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TextMorphConfig {
    /// How long the morph takes (seconds).
    #[serde(default = "default_morph_duration")]
    pub duration: f64,
    /// Behaviour of glyphs that have no identical counterpart in the other label.
    #[serde(default)]
    pub unmatched: TextMorphUnmatched,
    /// Seed for the deterministic scramble sequence. Same seed, same scramble.
    #[serde(default)]
    pub seed: u32,
}

impl Default for TextMorphConfig {
    fn default() -> Self {
        Self {
            duration: default_morph_duration(),
            unmatched: TextMorphUnmatched::default(),
            seed: 0,
        }
    }
}

fn default_morph_duration() -> f64 {
    0.6
}

impl Text {
    pub fn all_labels(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.content.as_str()).chain(self.states.iter().map(|s| s.content.as_str()))
    }

    fn label_at(&self, time: f64) -> &str {
        self.states
            .iter()
            .rfind(|s| s.at <= time)
            .map(|s| s.content.as_str())
            .unwrap_or(&self.content)
    }

    fn active_swap(&self, time: f64) -> Option<ActiveSwap> {
        let cfg = self.swap.as_ref()?;
        if cfg.duration <= 0.0 {
            return None;
        }
        let (idx, state) = self
            .states
            .iter()
            .enumerate()
            .find(|(_, s)| time >= s.at && time < s.at + cfg.duration)?;
        let from = if idx == 0 {
            self.content.clone()
        } else {
            self.states[idx - 1].content.clone()
        };
        Some(ActiveSwap {
            from,
            to: state.content.clone(),
            progress: ((time - state.at) / cfg.duration) as f32,
            distance: cfg.distance,
            blur: cfg.blur,
        })
    }

    fn active_morph(&self, time: f64) -> Option<ActiveMorph> {
        let cfg = self.morph.as_ref()?;
        if cfg.duration <= 0.0 {
            return None;
        }
        let (idx, state) = self
            .states
            .iter()
            .enumerate()
            .find(|(_, s)| time >= s.at && time < s.at + cfg.duration)?;
        let from = if idx == 0 {
            self.content.clone()
        } else {
            self.states[idx - 1].content.clone()
        };
        Some(ActiveMorph {
            from,
            to: state.content.clone(),
            progress: ((time - state.at) / cfg.duration) as f32,
            unmatched: cfg.unmatched,
            seed: cfg.seed,
        })
    }

    fn paint(
        &self,
        canvas: &Canvas,
        layout_width: f32,
        content_height: Option<f32>,
        time: f64,
        props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) -> Result<()> {
        let base_ctx = crate::intrinsic::font_size_ctx(
            ctx.video_width as f32,
            ctx.video_height as f32,
            layout_width.max(0.0),
        );
        let (mut font_size, mut letter_spacing, mut line_height_val) =
            self.style.typography_px_ctx(&base_ctx, 48.0);
        let color = props
            .color
            .as_deref()
            .unwrap_or_else(|| self.style.color_str_or("#FFFFFF"));
        let font_family = self.style.font_family_or("Inter");
        let font_weight = match &self.style.font_weight {
            Some(CssFontWeight::Keyword(FontWeightKw::Bold | FontWeightKw::Bolder)) => {
                FontWeight::Bold
            }
            Some(CssFontWeight::Number(n)) if *n >= 600 => FontWeight::Bold,
            Some(CssFontWeight::Number(n)) => FontWeight::Weight(*n),
            _ => FontWeight::Normal,
        };
        let font_style_type = match self.style.font_style {
            Some(CssFontStyle::Italic) => FontStyleType::Italic,
            Some(CssFontStyle::Oblique) => FontStyleType::Oblique,
            _ => FontStyleType::Normal,
        };
        let align = match self.style.text_align {
            Some(CssTextAlign::Center) => TextAlign::Center,
            Some(CssTextAlign::Right | CssTextAlign::End) => TextAlign::Right,
            _ => TextAlign::Left,
        };

        let slant = match font_style_type {
            FontStyleType::Normal => skia_safe::font_style::Slant::Upright,
            FontStyleType::Italic => skia_safe::font_style::Slant::Italic,
            FontStyleType::Oblique => skia_safe::font_style::Slant::Oblique,
        };
        let weight = match font_weight {
            FontWeight::Bold => skia_safe::font_style::Weight::BOLD,
            FontWeight::Normal => skia_safe::font_style::Weight::NORMAL,
            FontWeight::Weight(w) => skia_safe::font_style::Weight::from(w as i32),
        };
        let skia_font_style = FontStyle::new(weight, skia_safe::font_style::Width::NORMAL, slant);

        let typeface = typeface_with_fallback(font_family, skia_font_style)?;

        let nowrap = matches!(
            self.style.white_space,
            Some(CssWhiteSpace::Nowrap | CssWhiteSpace::Pre)
        );
        let box_width = if layout_width.is_finite() && layout_width > 0.0 {
            Some(match self.max_width {
                Some(mw) => mw.min(layout_width),
                None => layout_width,
            })
        } else {
            self.max_width
        };

        if matches!(self.style.text_autofit, Some(true)) {
            let declared_height = content_height.filter(|h| *h > 0.0 && h.is_finite());
            let (fs, ls, lh) = crate::intrinsic::resolve_text_autofit(
                &self.content,
                &typeface,
                font_size,
                letter_spacing,
                line_height_val,
                !nowrap,
                box_width,
                declared_height,
            );
            font_size = fs;
            letter_spacing = ls;
            line_height_val = lh;
        }

        let type_ctx = rustmotion_core::css::units::LengthContext {
            font_size,
            ..base_ctx
        };

        let font = Font::from_typeface(typeface, font_size);
        let emoji_font = emoji_typeface().map(|tf| Font::from_typeface(tf, font_size));
        let paint = paint_from_hex(color);

        let wrap_width = if nowrap { None } else { box_width };

        let label = self.label_at(time);
        let content = if props.visible_chars_progress >= 0.0 {
            let chars: Vec<char> = label.chars().collect();
            let visible = (props.visible_chars_progress * chars.len() as f32).round() as usize;
            let visible = visible.min(chars.len());
            if visible == 0 && self.caret.is_none() {
                return Ok(());
            }
            chars[..visible].iter().collect::<String>()
        } else {
            label.to_string()
        };

        let lines =
            wrap_text_with_tracking(&content, &font, &emoji_font, wrap_width, letter_spacing);
        let (_, metrics) = font.metrics();
        let ascent = -metrics.ascent;
        let descent = metrics.descent;
        let baseline_offset = (line_height_val + ascent - descent) / 2.0;

        let shadows: Vec<rustmotion_core::schema::TextShadow> = if let Some(s) = &self.text_shadow {
            vec![s.clone()]
        } else if let Some(list) = &self.style.text_shadow {
            list.iter().map(|s| s.to_schema(&type_ctx)).collect()
        } else {
            Vec::new()
        };
        let shadow_paints: Vec<(skia_safe::Paint, f32, f32)> = shadows
            .iter()
            .map(|shadow| {
                let mut p = paint_from_hex(&shadow.color);
                if shadow.blur > 0.01 {
                    if let Some(filter) = skia_safe::image_filters::blur(
                        (shadow.blur, shadow.blur),
                        skia_safe::TileMode::Clamp,
                        None,
                        None,
                    ) {
                        p.set_image_filter(filter);
                    }
                }
                (p, shadow.offset_x, shadow.offset_y)
            })
            .collect();

        let stroke_paint = self.stroke.as_ref().map(|stroke| {
            let mut p = paint_from_hex(&stroke.color);
            p.set_style(PaintStyle::Stroke);
            p.set_stroke_width(stroke.width);
            p
        });

        let align_width = if layout_width.is_finite() && layout_width > 0.0 {
            layout_width
        } else {
            let mut max_w = 0.0f32;
            for line in &lines {
                let w = measure_text_with_fallback(line, &font, &emoji_font, letter_spacing);
                max_w = max_w.max(w);
            }
            max_w
        };

        if let Some(ref resolved) = props.char_animation {
            crate::intrinsic::render_char_animation(
                canvas,
                &font,
                &emoji_font,
                &paint,
                letter_spacing,
                align,
                align_width,
                line_height_val,
                baseline_offset,
                &lines,
                resolved,
                time,
            );
            return Ok(());
        }

        if let Some(morph) = self.active_morph(time) {
            paint_morph(
                canvas,
                &morph,
                &font,
                &emoji_font,
                &paint,
                letter_spacing,
                wrap_width,
                align,
                align_width,
                line_height_val,
                baseline_offset,
                time,
            );
            return Ok(());
        }

        if let Some(swap) = self.active_swap(time) {
            for (label, offset_y, blur, alpha) in swap.labels() {
                let label_lines =
                    wrap_text_with_tracking(&label, &font, &emoji_font, wrap_width, letter_spacing);
                let mut p = paint.clone();
                p.set_alpha_f(alpha * paint.alpha_f());
                if blur > 0.05 {
                    if let Some(filter) = skia_safe::image_filters::blur(
                        (blur, blur),
                        skia_safe::TileMode::Clamp,
                        None,
                        None,
                    ) {
                        p.set_image_filter(filter);
                    }
                }
                draw_text_lines(
                    canvas,
                    &label_lines,
                    &font,
                    &emoji_font,
                    &p,
                    &shadow_paints,
                    stroke_paint.as_ref(),
                    self.text_background.as_ref(),
                    letter_spacing,
                    &align,
                    align_width,
                    line_height_val,
                    baseline_offset,
                    offset_y,
                );
            }
            return Ok(());
        }

        draw_text_lines(
            canvas,
            &lines,
            &font,
            &emoji_font,
            &paint,
            &shadow_paints,
            stroke_paint.as_ref(),
            self.text_background.as_ref(),
            letter_spacing,
            &align,
            align_width,
            line_height_val,
            baseline_offset,
            0.0,
        );

        if let Some(caret) = &self.caret {
            let done = props.visible_chars_progress < 0.0 || props.visible_chars_progress >= 1.0;
            if !(done && caret.hide_when_done) {
                let last = lines.len().saturating_sub(1);
                let line = lines.last().map(String::as_str).unwrap_or("");
                let advance_width =
                    measure_text_with_fallback(line, &font, &emoji_font, letter_spacing);
                let x = match align {
                    TextAlign::Left => 0.0,
                    TextAlign::Center => (align_width - advance_width) / 2.0,
                    TextAlign::Right => align_width - advance_width,
                };
                let baseline = last as f32 * line_height_val + baseline_offset;
                draw_caret(
                    canvas,
                    caret,
                    x + advance_width,
                    baseline,
                    &font,
                    &paint,
                    time,
                );
            }
        }

        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_text_lines(
    canvas: &Canvas,
    lines: &[String],
    font: &Font,
    emoji_font: &Option<Font>,
    paint: &Paint,
    shadow_paints: &[(Paint, f32, f32)],
    stroke_paint: Option<&Paint>,
    text_background: Option<&TextBackground>,
    letter_spacing: f32,
    align: &TextAlign,
    align_width: f32,
    line_height_val: f32,
    baseline_offset: f32,
    offset_y: f32,
) {
    for (i, line) in lines.iter().enumerate() {
        if line.is_empty() {
            continue;
        }

        let advance_width = measure_text_with_fallback(line, font, emoji_font, letter_spacing);

        let x = match align {
            TextAlign::Left => 0.0,
            TextAlign::Center => (align_width - advance_width) / 2.0,
            TextAlign::Right => align_width - advance_width,
        };
        let y = i as f32 * line_height_val + baseline_offset + offset_y;

        if let Some(bg) = text_background {
            let bg_paint = paint_from_hex(&bg.color);
            let (_, font_rect) = font.measure_str(line, None);
            let bg_rect = Rect::from_xywh(
                x - bg.padding + font_rect.left,
                y + font_rect.top - bg.padding / 2.0,
                advance_width + bg.padding * 2.0,
                -font_rect.top + font_rect.bottom + bg.padding,
            );
            if bg.corner_radius > 0.0 {
                let rrect =
                    skia_safe::RRect::new_rect_xy(bg_rect, bg.corner_radius, bg.corner_radius);
                canvas.draw_rrect(rrect, &bg_paint);
            } else {
                canvas.draw_rect(bg_rect, &bg_paint);
            }
        }

        for (sp, ox, oy) in shadow_paints.iter().rev() {
            draw_text_with_fallback(
                canvas,
                line,
                font,
                emoji_font,
                letter_spacing,
                x + ox,
                y + oy,
                sp,
            );
        }

        if let Some(sp) = stroke_paint {
            draw_text_with_fallback(canvas, line, font, emoji_font, letter_spacing, x, y, sp);
        }
        draw_text_with_fallback(canvas, line, font, emoji_font, letter_spacing, x, y, paint);
    }
}

struct ActiveSwap {
    from: String,
    to: String,
    progress: f32,
    distance: f32,
    blur: f32,
}

impl ActiveSwap {
    fn labels(&self) -> [(String, f32, f32, f32); 2] {
        let p = self.progress.clamp(0.0, 1.0);
        [
            (
                self.from.clone(),
                -self.distance * p,
                self.blur * p,
                1.0 - p,
            ),
            (
                self.to.clone(),
                self.distance * (1.0 - p),
                self.blur * (1.0 - p),
                p,
            ),
        ]
    }
}

struct ActiveMorph {
    from: String,
    to: String,
    progress: f32,
    unmatched: TextMorphUnmatched,
    seed: u32,
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn char_positions(
    lines: &[String],
    font: &Font,
    emoji_font: &Option<Font>,
    letter_spacing: f32,
    align: &TextAlign,
    align_width: f32,
    line_height_val: f32,
    baseline_offset: f32,
) -> Vec<(f32, f32)> {
    let mut out = Vec::new();
    for (line_idx, line) in lines.iter().enumerate() {
        let advance_width = measure_text_with_fallback(line, font, emoji_font, letter_spacing);
        let line_x = match align {
            TextAlign::Left => 0.0,
            TextAlign::Center => (align_width - advance_width) / 2.0,
            TextAlign::Right => align_width - advance_width,
        };
        let y = line_idx as f32 * line_height_val + baseline_offset;
        let mut cursor_x = line_x;
        for ch in line.chars() {
            out.push((cursor_x, y));
            let ch_str = ch.to_string();
            let ch_width = measure_text_with_fallback(&ch_str, font, emoji_font, letter_spacing);
            cursor_x += ch_width;
        }
    }
    out
}

fn lcs_pairs(from: &[char], to: &[char]) -> Vec<(usize, usize)> {
    let n = from.len();
    let m = to.len();
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if from[i] == to[j] {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut pairs = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if from[i] == to[j] {
            pairs.push((i, j));
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    pairs
}

const SCRAMBLE_POOL: &[char] = &[
    'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', 'P', 'Q', 'R', 'S',
    'T', 'U', 'V', 'W', 'X', 'Y', 'Z', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', '#', '%',
    '&', '*', '?',
];

fn scrambled_char(seed: u32, idx: usize, time: f64) -> char {
    let bucket = (time * 20.0) as u64;
    let mut h = (idx as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (seed as u64);
    h ^= bucket.wrapping_mul(0xD1B5_4A32_D192_ED03);
    h ^= h >> 33;
    h = h.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    h ^= h >> 33;
    let pool_idx = (h % SCRAMBLE_POOL.len() as u64) as usize;
    SCRAMBLE_POOL[pool_idx]
}

#[allow(clippy::too_many_arguments)]
fn paint_morph(
    canvas: &Canvas,
    morph: &ActiveMorph,
    font: &Font,
    emoji_font: &Option<Font>,
    paint: &Paint,
    letter_spacing: f32,
    wrap_width: Option<f32>,
    align: TextAlign,
    align_width: f32,
    line_height_val: f32,
    baseline_offset: f32,
    time: f64,
) {
    let from_lines =
        wrap_text_with_tracking(&morph.from, font, emoji_font, wrap_width, letter_spacing);
    let to_lines = wrap_text_with_tracking(&morph.to, font, emoji_font, wrap_width, letter_spacing);

    let from_positions = char_positions(
        &from_lines,
        font,
        emoji_font,
        letter_spacing,
        &align,
        align_width,
        line_height_val,
        baseline_offset,
    );
    let to_positions = char_positions(
        &to_lines,
        font,
        emoji_font,
        letter_spacing,
        &align,
        align_width,
        line_height_val,
        baseline_offset,
    );

    let from_chars: Vec<char> = from_lines.iter().flat_map(|l| l.chars()).collect();
    let to_chars: Vec<char> = to_lines.iter().flat_map(|l| l.chars()).collect();

    let pairs = lcs_pairs(&from_chars, &to_chars);
    let mut matched_from = vec![false; from_chars.len()];
    let mut matched_to = vec![false; to_chars.len()];
    for &(i, j) in &pairs {
        matched_from[i] = true;
        matched_to[j] = true;
    }

    let p = smoothstep(morph.progress);

    for &(i, j) in &pairs {
        let (fx, fy) = from_positions[i];
        let (tx, ty) = to_positions[j];
        let x = fx + (tx - fx) * p;
        let y = fy + (ty - fy) * p;
        draw_text_with_fallback(
            canvas,
            &to_chars[j].to_string(),
            font,
            emoji_font,
            0.0,
            x,
            y,
            paint,
        );
    }

    for (i, &(x, y)) in from_positions.iter().enumerate() {
        if matched_from[i] {
            continue;
        }
        let alpha = 1.0 - p;
        if alpha <= 0.001 {
            continue;
        }
        let mut ap = paint.clone();
        ap.set_alpha_f(alpha * paint.alpha_f());
        draw_text_with_fallback(
            canvas,
            &from_chars[i].to_string(),
            font,
            emoji_font,
            0.0,
            x,
            y,
            &ap,
        );
    }

    for (j, &(x, y)) in to_positions.iter().enumerate() {
        if matched_to[j] {
            continue;
        }
        let alpha = p;
        if alpha <= 0.001 {
            continue;
        }
        let mut ap = paint.clone();
        ap.set_alpha_f(alpha * paint.alpha_f());
        let glyph = match morph.unmatched {
            TextMorphUnmatched::Fade => to_chars[j],
            TextMorphUnmatched::Scramble => {
                if p < 0.7 {
                    scrambled_char(morph.seed, j, time)
                } else {
                    to_chars[j]
                }
            }
        };
        draw_text_with_fallback(canvas, &glyph.to_string(), font, emoji_font, 0.0, x, y, &ap);
    }
}

fn draw_caret(
    canvas: &Canvas,
    cfg: &CaretConfig,
    x: f32,
    baseline: f32,
    font: &Font,
    text_paint: &Paint,
    time: f64,
) {
    if cfg.blink > 0.0 {
        let phase = (time / cfg.blink as f64).rem_euclid(1.0);
        if phase >= 0.5 {
            return;
        }
    }

    let (_, metrics) = font.metrics();
    let ascent = -metrics.ascent;
    let descent = metrics.descent;
    let size = font.size();

    let (width, gap) = match cfg.shape {
        CaretShape::Line => ((size * 0.07).max(1.5), size * 0.05),
        CaretShape::Block => (size * 0.55, size * 0.04),
    };

    let mut paint = match &cfg.color {
        Some(hex) => paint_from_hex(hex),
        None => text_paint.clone(),
    };
    paint.set_style(PaintStyle::Fill);
    paint.set_anti_alias(true);
    paint.set_image_filter(None);

    canvas.draw_rect(
        Rect::from_xywh(x + gap, baseline - ascent, width, ascent + descent),
        &paint,
    );
}

impl Painter for Text {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        let (_, _, _, content_height) = layout.content_box();
        let content_height =
            (content_height > 0.0 && content_height.is_finite()).then_some(content_height);
        let _ = self.paint(canvas, layout.width, content_height, ctx.time, props, ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intrinsic::TextIntrinsic;
    use rustmotion_core::css::style::CssStyle;
    use rustmotion_core::css::Length;
    use rustmotion_core::engine::box_tree::{AvailableSpace, IntrinsicMeasure};
    use rustmotion_core::schema::{
        AnimationEffect, CharAnimationTiming, EasingType, TextAnimDirection, TextAnimGranularity,
    };

    fn make_text(content: &str, white_space: Option<CssWhiteSpace>) -> Text {
        Text {
            content: content.into(),
            max_width: None,
            timing: Default::default(),
            style: CssStyle {
                font_size: Some(Length::Px(28.0)),
                color: Some(rustmotion_core::css::style::Color::String("#FFFFFF".into())),
                white_space,
                ..Default::default()
            },
            timeline: Vec::new(),
            stagger: None,
            text_shadow: None,
            stroke: None,
            text_background: None,
            caret: None,
            states: Vec::new(),
            swap: None,
            morph: None,
        }
    }

    fn test_ctx() -> PaintCtx {
        PaintCtx {
            time: 0.0,
            scenario_time: 0.0,
            scene_duration: 1.0,
            frame_index: 0,
            fps: 30,
            video_width: 600,
            video_height: 200,
            stagger_offset: 0.0,
        }
    }

    fn alpha_grid(surface: &mut skia_safe::Surface, width: i32, height: i32) -> Vec<u8> {
        let snapshot = surface.image_snapshot();
        let info = skia_safe::ImageInfo::new(
            (width, height),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut buf = vec![0u8; (width * height * 4) as usize];
        let ok = snapshot.read_pixels(
            &info,
            &mut buf,
            (width * 4) as usize,
            skia_safe::IPoint::new(0, 0),
            skia_safe::image::CachingHint::Disallow,
        );
        assert!(ok, "pixel read should succeed");
        (0..(width * height) as usize)
            .map(|i| buf[i * 4 + 3])
            .collect()
    }

    fn has_ink_in(grid: &[u8], surface_width: i32, x0: i32, x1: i32, y0: i32, y1: i32) -> bool {
        for y in y0..y1 {
            for x in x0..x1 {
                if grid[(y * surface_width + x) as usize] > 0 {
                    return true;
                }
            }
        }
        false
    }

    #[test]
    fn nowrap_paints_a_single_line_past_the_layout_width() {
        let text = make_text(
            "the quick brown fox jumps over the lazy dog",
            Some(CssWhiteSpace::Nowrap),
        );
        const W: i32 = 600;
        const H: i32 = 200;
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        let canvas = surface.canvas();
        let ctx = test_ctx();
        let props = AnimatedProperties::default();
        text.paint(canvas, 80.0, None, 0.0, &props, &ctx)
            .expect("paint succeeds");
        let grid = alpha_grid(&mut surface, W, H);

        assert!(
            has_ink_in(&grid, W, 300, W, 0, 45),
            "nowrap text must paint past its 80px box on line 1 (scanned x∈[300,600), y∈[0,45))"
        );

        assert!(
            !has_ink_in(&grid, W, 0, W, 55, H),
            "nowrap text must stay on a single line; found ink on what would be line 2"
        );
    }

    #[test]
    fn normal_white_space_wraps_within_the_layout_width() {
        let text = make_text("the quick brown fox jumps over the lazy dog", None);
        const W: i32 = 600;
        const H: i32 = 200;
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        let canvas = surface.canvas();
        let ctx = test_ctx();
        let props = AnimatedProperties::default();
        text.paint(canvas, 80.0, None, 0.0, &props, &ctx)
            .expect("paint succeeds");
        let grid = alpha_grid(&mut surface, W, H);

        assert!(
            !has_ink_in(&grid, W, 300, W, 0, 45),
            "wrapped text must not reach x∈[300,600) on line 1 within an 80px box"
        );
        assert!(
            has_ink_in(&grid, W, 0, W, 55, H),
            "wrapped text must spill onto a second line within the box width"
        );
    }

    #[test]
    fn rem_font_size_paints_visible_ink() {
        let text = Text {
            content: "HELLO".into(),
            max_width: None,
            timing: Default::default(),
            style: CssStyle {
                font_size: Some(Length::String("2rem".into())),
                color: Some(rustmotion_core::css::style::Color::String("#FFFFFF".into())),
                ..Default::default()
            },
            timeline: Vec::new(),
            stagger: None,
            text_shadow: None,
            stroke: None,
            text_background: None,
            caret: None,
            states: Vec::new(),
            swap: None,
            morph: None,
        };
        const W: i32 = 400;
        const H: i32 = 200;
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        let canvas = surface.canvas();
        let ctx = test_ctx();
        let props = AnimatedProperties::default();
        text.paint(canvas, 300.0, None, 0.0, &props, &ctx)
            .expect("paint succeeds");
        let grid = alpha_grid(&mut surface, W, H);

        assert!(
            has_ink_in(&grid, W, 0, W, 0, 60),
            "font-size: 2rem must paint visible ink (32px glyphs), got none"
        );
    }

    #[test]
    fn vh_font_size_paints_visible_ink_scaled_to_the_real_viewport() {
        let text = Text {
            content: "HI".into(),
            max_width: None,
            timing: Default::default(),
            style: CssStyle {
                font_size: Some(Length::String("20vh".into())),
                color: Some(rustmotion_core::css::style::Color::String("#FFFFFF".into())),
                ..Default::default()
            },
            timeline: Vec::new(),
            stagger: None,
            text_shadow: None,
            stroke: None,
            text_background: None,
            caret: None,
            states: Vec::new(),
            swap: None,
            morph: None,
        };
        const W: i32 = 400;
        const H: i32 = 200;
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        let canvas = surface.canvas();
        let ctx = test_ctx();
        let props = AnimatedProperties::default();
        text.paint(canvas, 300.0, None, 0.0, &props, &ctx)
            .expect("paint succeeds");
        let grid = alpha_grid(&mut surface, W, H);

        assert!(
            has_ink_in(&grid, W, 0, W, 0, 70),
            "font-size: 20vh (40px against a 200px-tall test viewport) must paint visible ink"
        );
    }

    #[test]
    fn a_translucent_color_alpha_channel_survives_to_the_painted_pixels() {
        let mut text = make_text("A", Some(CssWhiteSpace::Nowrap));
        text.style.font_size = Some(Length::Px(120.0));
        text.style.color = Some(rustmotion_core::css::style::Color::String(
            "#FFFFFF12".into(),
        ));

        const W: i32 = 300;
        const H: i32 = 200;
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        let canvas = surface.canvas();
        let ctx = test_ctx();
        let props = AnimatedProperties::default();
        text.paint(canvas, W as f32, None, 0.0, &props, &ctx)
            .expect("paint succeeds");
        let grid = alpha_grid(&mut surface, W, H);

        let max_alpha = grid.iter().copied().max().unwrap_or(0);
        assert!(max_alpha > 0, "the glyph must paint some ink");
        assert!(
            max_alpha <= 40,
            "style.color's alpha channel (0x12 = 18/255, about 7%) must survive to the painted \
             pixels instead of being forced fully opaque; got max alpha {max_alpha}"
        );
    }

    fn soft_pixel_fraction(
        grid: &[u8],
        surface_width: i32,
        x0: i32,
        x1: i32,
        y0: i32,
        y1: i32,
    ) -> f32 {
        let mut inked = 0u32;
        let mut soft = 0u32;
        for y in y0..y1 {
            for x in x0..x1 {
                let a = grid[(y * surface_width + x) as usize];
                if a > 0 {
                    inked += 1;
                    if a < 250 {
                        soft += 1;
                    }
                }
            }
        }
        if inked == 0 {
            return 0.0;
        }
        soft as f32 / inked as f32
    }

    fn props_for(text: &Text) -> AnimatedProperties {
        AnimatedProperties {
            char_animation: rustmotion_core::engine::animator::extract_effects(
                &text.style.animation,
            )
            .char_animation,
            ..Default::default()
        }
    }

    fn inter_font(px: f32) -> Font {
        let style = FontStyle::new(
            skia_safe::font_style::Weight::NORMAL,
            skia_safe::font_style::Width::NORMAL,
            skia_safe::font_style::Slant::Upright,
        );
        let typeface = typeface_with_fallback("Inter", style).expect("typeface resolves");
        Font::from_typeface(typeface, px)
    }

    #[test]
    fn char_blur_in_word_is_blurred_mid_reveal_and_sharp_when_settled() {
        let mut text = make_text("BLUR", None);
        text.style.font_size = Some(Length::Px(100.0));
        text.style.white_space = Some(CssWhiteSpace::Nowrap);
        text.style.animation = vec![AnimationEffect::CharBlurIn(CharAnimationTiming {
            delay: 0.0,
            duration: 0.5,
            stagger: 0.03,
            granularity: TextAnimGranularity::Word,
            easing: EasingType::Linear,
            ..Default::default()
        })];

        const W: i32 = 700;
        const H: i32 = 220;
        let ctx = test_ctx();
        let props = props_for(&text);

        let render_at = |t: f64| -> Vec<u8> {
            let mut surface =
                skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
            {
                let canvas = surface.canvas();
                text.paint(canvas, W as f32, None, t, &props, &ctx)
                    .expect("paint succeeds");
            }
            alpha_grid(&mut surface, W, H)
        };

        let early = render_at(0.15);
        let settled = render_at(1.0);

        assert!(
            has_ink_in(&early, W, 0, W, 0, H),
            "the word must have started painting by t=0.15"
        );

        let early_soft = soft_pixel_fraction(&early, W, 0, W, 0, H);
        let settled_soft = soft_pixel_fraction(&settled, W, 0, W, 0, H);

        assert!(
            early_soft > settled_soft + 0.15,
            "mid-reveal soft-pixel fraction ({early_soft:.3}) must be clearly higher than the \
             settled fraction ({settled_soft:.3}) — the word should read as blurred while \
             animating and sharp at rest"
        );
        assert!(
            settled_soft < 0.25,
            "settled frame should read as sharp text, not blur (soft fraction {settled_soft:.3})"
        );
    }

    #[test]
    fn char_blur_in_whitespace_gap_stays_empty_while_words_animate() {
        const FONT_PX: f32 = 90.0;
        let mut text = make_text("FIRST               SECOND", None);
        text.style.font_size = Some(Length::Px(FONT_PX));
        text.style.white_space = Some(CssWhiteSpace::Nowrap);
        text.style.animation = vec![AnimationEffect::CharBlurIn(CharAnimationTiming {
            delay: 0.0,
            duration: 0.4,
            stagger: 0.2,
            granularity: TextAnimGranularity::Word,
            easing: EasingType::Linear,
            blur: Some(18.0),
            ..Default::default()
        })];

        const W: i32 = 1400;
        const H: i32 = 180;
        let ctx = test_ctx();
        let props = props_for(&text);

        let font = inter_font(FONT_PX);
        let first_w = measure_text_with_fallback("FIRST", &font, &None, 0.0);
        let gap_w = measure_text_with_fallback("               ", &font, &None, 0.0);
        let margin = (gap_w / 3.0).max(40.0);
        let gap_x0 = (first_w + margin) as i32;
        let gap_x1 = (first_w + gap_w - margin) as i32;
        assert!(
            gap_x1 > gap_x0,
            "test setup: the space run must measure to a real gap (got [{gap_x0},{gap_x1}))"
        );

        for &t in &[0.05_f64, 0.35, 1.0] {
            let mut surface =
                skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
            {
                let canvas = surface.canvas();
                text.paint(canvas, W as f32, None, t, &props, &ctx)
                    .expect("paint succeeds");
            }
            let grid = alpha_grid(&mut surface, W, H);
            assert!(
                !has_ink_in(&grid, W, gap_x0, gap_x1, 0, H),
                "inter-word gap [{gap_x0},{gap_x1}) must stay empty at t={t}"
            );
        }
    }

    #[test]
    fn char_blur_in_honors_word_delay_and_stagger() {
        const FONT_PX: f32 = 90.0;
        let mut text = make_text("ONE TWO", None);
        text.style.font_size = Some(Length::Px(FONT_PX));
        text.style.white_space = Some(CssWhiteSpace::Nowrap);
        text.style.animation = vec![AnimationEffect::CharBlurIn(CharAnimationTiming {
            delay: 0.5,
            duration: 0.3,
            stagger: 0.6,
            granularity: TextAnimGranularity::Word,
            easing: EasingType::Linear,
            blur: Some(16.0),
            ..Default::default()
        })];

        const W: i32 = 900;
        const H: i32 = 180;
        let ctx = test_ctx();
        let props = props_for(&text);
        let font = inter_font(FONT_PX);
        let word1_end = measure_text_with_fallback("ONE", &font, &None, 0.0) as i32;

        let mut before = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        {
            let canvas = before.canvas();
            text.paint(canvas, W as f32, None, 0.1, &props, &ctx)
                .expect("paint succeeds");
        }
        let before_grid = alpha_grid(&mut before, W, H);
        assert!(
            !has_ink_in(&before_grid, W, 0, W, 0, H),
            "nothing should paint before `delay` has elapsed"
        );

        let mut mid = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        {
            let canvas = mid.canvas();
            text.paint(canvas, W as f32, None, 0.65, &props, &ctx)
                .expect("paint succeeds");
        }
        let mid_grid = alpha_grid(&mut mid, W, H);
        assert!(
            has_ink_in(&mid_grid, W, 0, word1_end, 0, H),
            "word 1 should show ink by t=0.65 (mid-reveal)"
        );
        assert!(
            !has_ink_in(&mid_grid, W, word1_end + 70, W, 0, H),
            "word 2 (starts at delay+stagger=1.1s) must still be fully invisible at t=0.65"
        );
    }

    fn autofit_text(content: &str, font_size: f32, white_space: Option<CssWhiteSpace>) -> Text {
        let mut t = make_text(content, white_space);
        t.style.font_size = Some(Length::Px(font_size));
        t.style.text_autofit = Some(true);
        t
    }

    fn max_ink_x(grid: &[u8], surface_width: i32, height: i32) -> Option<i32> {
        let mut max_x: Option<i32> = None;
        for y in 0..height {
            for x in (0..surface_width).rev() {
                if grid[(y * surface_width + x) as usize] > 0 {
                    max_x = Some(max_x.map_or(x, |m| m.max(x)));
                    break;
                }
            }
        }
        max_x
    }

    fn max_ink_y(grid: &[u8], surface_width: i32, height: i32) -> Option<i32> {
        for y in (0..height).rev() {
            for x in 0..surface_width {
                if grid[(y * surface_width + x) as usize] > 0 {
                    return Some(y);
                }
            }
        }
        None
    }

    #[test]
    fn measure_and_paint_agree_on_a_shrunk_nowrap_line() {
        let text = autofit_text(
            "the quick brown fox jumps over the lazy dog",
            90.0,
            Some(CssWhiteSpace::Nowrap),
        );
        const BOX_W: f32 = 300.0;
        const BOX_H: f32 = 60.0;

        let (measured_w, _measured_h) = TextIntrinsic::from_text(&text).measure(
            (None, None),
            (
                AvailableSpace::Definite(BOX_W),
                AvailableSpace::Definite(BOX_H),
            ),
        );
        assert!(
            measured_w <= BOX_W + 0.5,
            "TextIntrinsic itself must report a fit once autofit is on, got {measured_w}"
        );

        const W: i32 = 900;
        const H: i32 = 300;
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        {
            let canvas = surface.canvas();
            let ctx = test_ctx();
            let props = AnimatedProperties::default();
            text.paint(canvas, BOX_W, Some(BOX_H), 0.0, &props, &ctx)
                .expect("paint succeeds");
        }
        let grid = alpha_grid(&mut surface, W, H);
        let ink_right = max_ink_x(&grid, W, H).expect("text must paint some ink");

        assert!(
            (ink_right as f32) <= measured_w + 3.0,
            "painted ink (right edge {ink_right}) must not exceed the box TextIntrinsic reserved \
             ({measured_w}) — a wider paint than measure is exactly the class of bug this \
             workstream exists to close"
        );
        assert!(
            (ink_right as f32) >= measured_w - 15.0,
            "painted ink (right edge {ink_right}) should land close to what TextIntrinsic \
             measured ({measured_w}); a big gap would mean the two disagree on the resolved \
             font size in the other direction (paint drawing much smaller than reserved)"
        );
    }

    #[test]
    fn measure_and_paint_agree_on_a_shrunk_wrapped_paragraph_height() {
        let text = autofit_text(
            "the quick brown fox jumps over the lazy dog and then keeps going for quite a while longer",
            60.0,
            None,
        );
        const BOX_W: f32 = 300.0;
        const BOX_H: f32 = 90.0;

        let (_measured_w, measured_h) = TextIntrinsic::from_text(&text).measure(
            (None, None),
            (
                AvailableSpace::Definite(BOX_W),
                AvailableSpace::Definite(BOX_H),
            ),
        );
        assert!(
            measured_h <= BOX_H + 0.5,
            "TextIntrinsic itself must report a fit once autofit is on, got {measured_h}"
        );

        const W: i32 = 500;
        const H: i32 = 400;
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        {
            let canvas = surface.canvas();
            let ctx = test_ctx();
            let props = AnimatedProperties::default();
            text.paint(canvas, BOX_W, Some(BOX_H), 0.0, &props, &ctx)
                .expect("paint succeeds");
        }
        let grid = alpha_grid(&mut surface, W, H);
        let ink_bottom = max_ink_y(&grid, W, H).expect("text must paint some ink");

        assert!(
            (ink_bottom as f32) <= measured_h + 6.0,
            "painted ink (bottom edge {ink_bottom}) must not exceed the box TextIntrinsic \
             reserved ({measured_h})"
        );
    }

    #[test]
    fn autofit_size_is_stable_across_frames_for_fixed_content() {
        let text = autofit_text(
            "the quick brown fox jumps over the lazy dog",
            90.0,
            Some(CssWhiteSpace::Nowrap),
        );
        const W: i32 = 900;
        const H: i32 = 300;
        let ctx = test_ctx();
        let props = AnimatedProperties::default();

        let render_at = |t: f64| -> Vec<u8> {
            let mut surface =
                skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
            {
                let canvas = surface.canvas();
                text.paint(canvas, 250.0, Some(60.0), t, &props, &ctx)
                    .expect("paint succeeds");
            }
            alpha_grid(&mut surface, W, H)
        };

        let frame_a = render_at(0.0);
        let frame_b = render_at(0.9);
        assert_eq!(
            frame_a, frame_b,
            "fixed content in a fixed box must render byte-identically regardless of ctx.time — \
             a per-frame drift here is exactly what the temporal-stability requirement forbids"
        );
    }

    #[test]
    fn autofit_size_does_not_drift_during_a_typewriter_reveal() {
        let text = autofit_text(
            "the quick brown fox jumps over the lazy dog",
            90.0,
            Some(CssWhiteSpace::Nowrap),
        );
        const W: i32 = 900;
        const H: i32 = 300;
        let ctx = test_ctx();

        let render_at = |progress: f32| -> Vec<u8> {
            let mut surface =
                skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
            let props = AnimatedProperties {
                visible_chars_progress: progress,
                ..Default::default()
            };
            {
                let canvas = surface.canvas();
                text.paint(canvas, 250.0, Some(60.0), 0.0, &props, &ctx)
                    .expect("paint succeeds");
            }
            alpha_grid(&mut surface, W, H)
        };

        let early = render_at(0.3);
        let full = render_at(1.0);

        let early_bottom = max_ink_y(&early, W, H).expect("some ink must paint at 30% reveal");
        let full_bottom = max_ink_y(&full, W, H).expect("some ink must paint at full reveal");
        assert_eq!(
            early_bottom, full_bottom,
            "the resolved font size (line height, hence vertical ink footprint) must not change \
             as the typewriter reveal progresses: early={early_bottom}, full={full_bottom}"
        );

        let early_right = max_ink_x(&early, W, H).expect("some ink at 30% reveal");
        let full_right = max_ink_x(&full, W, H).expect("some ink at full reveal");
        assert!(
            early_right < full_right,
            "test setup: 30% reveal should show measurably less horizontal ink than the full \
             line (early={early_right}, full={full_right})"
        );
    }

    #[test]
    fn without_text_autofit_nowrap_still_bleeds_past_the_box_exactly_as_before() {
        let text = make_text(
            "the quick brown fox jumps over the lazy dog",
            Some(CssWhiteSpace::Nowrap),
        );
        const W: i32 = 900;
        const H: i32 = 300;
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        let canvas = surface.canvas();
        let ctx = test_ctx();
        let props = AnimatedProperties::default();
        text.paint(canvas, 250.0, Some(60.0), 0.0, &props, &ctx)
            .expect("paint succeeds");
        let grid = alpha_grid(&mut surface, W, H);
        assert!(
            has_ink_in(&grid, W, 260, W, 0, 45),
            "without text-autofit, nowrap must still bleed past its box exactly as before"
        );
    }

    fn swapping_text(swap: Option<TextSwapConfig>) -> Text {
        let mut text = make_text("Saving draft", Some(CssWhiteSpace::Nowrap));
        text.style.font_size = Some(Length::Px(48.0));
        text.states = vec![TextState {
            at: 1.0,
            content: "Saved".into(),
        }];
        text.swap = swap;
        text
    }

    fn render_plain(text: &Text, time: f64) -> Vec<u8> {
        let mut surface =
            skia_safe::surfaces::raster_n32_premul((CARET_W, CARET_H)).expect("raster surface");
        {
            let canvas = surface.canvas();
            text.paint(
                canvas,
                CARET_W as f32,
                None,
                time,
                &AnimatedProperties::default(),
                &test_ctx(),
            )
            .expect("paint succeeds");
        }
        alpha_grid(&mut surface, CARET_W, CARET_H)
    }

    #[test]
    fn states_cut_over_at_their_own_time_without_a_swap_config() {
        let text = swapping_text(None);
        let before = render_plain(&text, 0.5);
        let after = render_plain(&text, 1.5);

        let width_of = |g: &[u8]| max_ink_x(g, CARET_W, CARET_H).unwrap_or(0);
        assert!(
            width_of(&before) > width_of(&after) + 20,
            "\"Saving draft\" should be visibly wider than \"Saved\" — the label must actually \
             have changed at t=1.0"
        );
        assert_eq!(
            render_plain(&text, 1.01),
            render_plain(&text, 1.5),
            "without a `swap`, the new label must be fully in place immediately"
        );
    }

    #[test]
    fn a_swap_puts_both_labels_on_screen_at_once() {
        let text = swapping_text(Some(TextSwapConfig::default()));

        let rows_with_ink = |time: f64| -> usize {
            let grid = render_plain(&text, time);
            (0..CARET_H)
                .filter(|&y| (0..CARET_W).any(|x| grid[(y * CARET_W + x) as usize] > 0))
                .count()
        };

        let settled = rows_with_ink(0.5);
        let mid = rows_with_ink(1.0 + 0.45 / 2.0);
        assert!(
            mid > settled,
            "mid-swap, the two offset labels should span more rows than one settled label \
             (settled={settled}, mid={mid})"
        );
    }

    #[test]
    fn a_finished_swap_settles_on_the_incoming_label_alone() {
        let swapped = swapping_text(Some(TextSwapConfig::default()));
        let cut = swapping_text(None);
        assert_eq!(
            render_plain(&swapped, 2.0),
            render_plain(&cut, 2.0),
            "once the swap window has passed, the frame must match a plain cut exactly"
        );
    }

    #[test]
    fn the_box_is_measured_for_the_widest_label_not_the_first() {
        let mut short_first = make_text("Saved", Some(CssWhiteSpace::Nowrap));
        short_first.states = vec![TextState {
            at: 1.0,
            content: "Saving draft".into(),
        }];
        let only_short = make_text("Saved", Some(CssWhiteSpace::Nowrap));

        let measure = |t: &Text| {
            TextIntrinsic::from_text(t)
                .measure(
                    (None, None),
                    (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
                )
                .0
        };

        assert!(
            measure(&short_first) > measure(&only_short) + 10.0,
            "the reserved width must cover the longest label the text can show \
             (with states={}, without={})",
            measure(&short_first),
            measure(&only_short)
        );
    }

    const CARET_W: i32 = 900;
    const CARET_H: i32 = 160;

    fn typewriter_text(caret: Option<CaretConfig>) -> Text {
        let mut text = make_text("HELLO WORLD", Some(CssWhiteSpace::Nowrap));
        text.style.font_size = Some(Length::Px(64.0));
        text.caret = caret;
        text
    }

    fn render_reveal(text: &Text, progress: f32, time: f64) -> Vec<u8> {
        let props = AnimatedProperties {
            visible_chars_progress: progress,
            ..AnimatedProperties::default()
        };
        let mut surface =
            skia_safe::surfaces::raster_n32_premul((CARET_W, CARET_H)).expect("raster surface");
        {
            let canvas = surface.canvas();
            text.paint(canvas, CARET_W as f32, None, time, &props, &test_ctx())
                .expect("paint succeeds");
        }
        alpha_grid(&mut surface, CARET_W, CARET_H)
    }

    #[test]
    fn the_caret_follows_the_reveal_head_instead_of_standing_still() {
        let text = typewriter_text(Some(CaretConfig {
            blink: 0.0,
            ..Default::default()
        }));
        let plain = typewriter_text(None);

        let caret_x = |progress: f32| -> i32 {
            let with = render_reveal(&text, progress, 0.0);
            let without = render_reveal(&plain, progress, 0.0);
            let with_right = max_ink_x(&with, CARET_W, CARET_H).expect("caret paints");
            let without_right = max_ink_x(&without, CARET_W, CARET_H).unwrap_or(0);
            assert!(
                with_right > without_right,
                "the caret should extend past the last revealed glyph \
                 (with={with_right}, without={without_right}) at progress {progress}"
            );
            with_right
        };

        let early = caret_x(0.25);
        let late = caret_x(0.75);
        assert!(
            late > early + 40,
            "the caret should have travelled with the reveal head (early={early}, late={late})"
        );
    }

    #[test]
    fn the_caret_blinks_off_for_half_of_each_period() {
        let text = typewriter_text(Some(CaretConfig {
            blink: 1.0,
            ..Default::default()
        }));
        let plain = typewriter_text(None);

        let right_edge = |grid: &[u8]| max_ink_x(grid, CARET_W, CARET_H).unwrap_or(0);
        let baseline = right_edge(&render_reveal(&plain, 0.5, 0.0));

        let on = right_edge(&render_reveal(&text, 0.5, 0.1));
        let off = right_edge(&render_reveal(&text, 0.5, 0.6));
        assert!(on > baseline, "caret should be visible at phase 0.1");
        assert_eq!(
            off, baseline,
            "caret should be blinked out at phase 0.6, leaving only the text's own ink"
        );
    }

    #[test]
    fn hide_when_done_removes_the_caret_once_the_reveal_finishes() {
        let hiding = typewriter_text(Some(CaretConfig {
            blink: 0.0,
            hide_when_done: true,
            ..Default::default()
        }));
        let staying = typewriter_text(Some(CaretConfig {
            blink: 0.0,
            ..Default::default()
        }));
        let plain = typewriter_text(None);

        let right_edge = |t: &Text, progress: f32| {
            max_ink_x(&render_reveal(t, progress, 0.0), CARET_W, CARET_H).unwrap_or(0)
        };
        let text_edge = right_edge(&plain, 1.0);

        assert_eq!(
            right_edge(&hiding, 1.0),
            text_edge,
            "with hide_when_done, a finished reveal must leave no caret behind"
        );
        assert!(
            right_edge(&staying, 1.0) > text_edge,
            "without hide_when_done, the caret parks at the end of the text"
        );
    }

    #[test]
    fn the_caret_is_there_before_the_first_character_is() {
        let text = typewriter_text(Some(CaretConfig {
            blink: 0.0,
            ..Default::default()
        }));
        let grid = render_reveal(&text, 0.0, 0.0);
        assert!(
            has_ink_in(&grid, CARET_W, 0, CARET_W, 0, CARET_H),
            "the caret must be painting at 0% reveal, before any glyph"
        );
    }

    const TUNING_W: i32 = 520;
    const TUNING_H: i32 = 360;

    fn tuned_slide_up(timing: CharAnimationTiming) -> Text {
        let mut text = make_text("GO", None);
        text.style.font_size = Some(Length::Px(90.0));
        text.style.white_space = Some(CssWhiteSpace::Nowrap);
        text.style.animation = vec![AnimationEffect::CharSlideUp(timing)];
        text
    }

    fn render_alpha(text: &Text, time: f64) -> Vec<u8> {
        let mut surface =
            skia_safe::surfaces::raster_n32_premul((TUNING_W, TUNING_H)).expect("raster surface");
        {
            let canvas = surface.canvas();
            text.paint(
                canvas,
                TUNING_W as f32,
                None,
                time,
                &props_for(text),
                &test_ctx(),
            )
            .expect("paint succeeds");
        }
        alpha_grid(&mut surface, TUNING_W, TUNING_H)
    }

    fn min_ink_y(grid: &[u8], surface_width: i32, height: i32) -> Option<i32> {
        (0..height)
            .find(|&y| (0..surface_width).any(|x| grid[(y * surface_width + x) as usize] > 0))
    }

    #[test]
    fn direction_down_starts_the_unit_above_its_line_instead_of_below() {
        let base = || CharAnimationTiming {
            duration: 1.0,
            stagger: 0.0,
            granularity: TextAnimGranularity::Word,
            easing: EasingType::Linear,
            distance: Some(0.5),
            ..Default::default()
        };
        let up = tuned_slide_up(CharAnimationTiming {
            direction: TextAnimDirection::Up,
            ..base()
        });
        let down = tuned_slide_up(CharAnimationTiming {
            direction: TextAnimDirection::Down,
            ..base()
        });

        let up_grid = render_alpha(&up, 0.5);
        let down_grid = render_alpha(&down, 0.5);

        let up_top = min_ink_y(&up_grid, TUNING_W, TUNING_H).expect("up-travelling word paints");
        let down_top =
            min_ink_y(&down_grid, TUNING_W, TUNING_H).expect("down-travelling word paints");

        assert!(
            down_top < up_top - 10,
            "at the same instant, a `down` unit should sit clearly higher on the canvas than an \
             `up` one (down_top={down_top}, up_top={up_top})"
        );
    }

    #[test]
    fn distance_scales_how_far_the_unit_travels() {
        let base = || CharAnimationTiming {
            duration: 1.0,
            stagger: 0.0,
            granularity: TextAnimGranularity::Word,
            easing: EasingType::Linear,
            ..Default::default()
        };
        let close = tuned_slide_up(CharAnimationTiming {
            distance: Some(0.25),
            ..base()
        });
        let far = tuned_slide_up(CharAnimationTiming {
            distance: Some(1.0),
            ..base()
        });

        let close_top =
            min_ink_y(&render_alpha(&close, 0.5), TUNING_W, TUNING_H).expect("close word paints");
        let far_top =
            min_ink_y(&render_alpha(&far, 0.5), TUNING_W, TUNING_H).expect("far word paints");

        assert!(
            far_top > close_top + 10,
            "a `distance: 1.0` unit should still be further below its line than a `0.25` one at \
             the same instant (far_top={far_top}, close_top={close_top})"
        );
    }

    #[test]
    fn settled_units_land_in_the_same_place_whatever_the_direction_and_distance() {
        let settled = |timing: CharAnimationTiming| {
            let grid = render_alpha(&tuned_slide_up(timing), 5.0);
            min_ink_y(&grid, TUNING_W, TUNING_H).expect("settled word paints")
        };
        let base = || CharAnimationTiming {
            duration: 1.0,
            stagger: 0.0,
            granularity: TextAnimGranularity::Word,
            easing: EasingType::Linear,
            ..Default::default()
        };

        let plain = settled(base());
        let downward = settled(CharAnimationTiming {
            direction: TextAnimDirection::Down,
            distance: Some(1.85),
            ..base()
        });
        let sideways = settled(CharAnimationTiming {
            direction: TextAnimDirection::Right,
            distance: Some(1.85),
            ..base()
        });

        assert_eq!(
            plain, downward,
            "a settled `down` unit must land on its line"
        );
        assert_eq!(
            plain, sideways,
            "a settled `right` unit must land on its line"
        );
    }

    #[test]
    fn scale_from_shrinks_the_unit_at_the_start_and_releases_it_by_the_end() {
        let timing = CharAnimationTiming {
            duration: 1.0,
            stagger: 0.0,
            granularity: TextAnimGranularity::Word,
            easing: EasingType::Linear,
            distance: Some(0.0),
            scale_from: Some(0.5),
            ..Default::default()
        };
        let text = tuned_slide_up(timing);

        let ink_width = |time: f64| -> i32 {
            let grid = render_alpha(&text, time);
            let left = (0..TUNING_W)
                .find(|&x| (0..TUNING_H).any(|y| grid[(y * TUNING_W + x) as usize] > 0));
            let right = (0..TUNING_W)
                .rev()
                .find(|&x| (0..TUNING_H).any(|y| grid[(y * TUNING_W + x) as usize] > 0));
            match (left, right) {
                (Some(l), Some(r)) => r - l,
                _ => 0,
            }
        };

        let early = ink_width(0.35);
        let settled = ink_width(5.0);
        assert!(early > 0, "the word must be painting by t=0.35");
        assert!(
            (early as f32) < settled as f32 * 0.9,
            "a `scale_from: 0.5` unit should still be visibly narrower than its settled self \
             early on (early={early}px, settled={settled}px)"
        );
    }

    #[test]
    fn ink_from_starts_at_the_given_colour_and_settles_to_the_texts_own() {
        let mut text = make_text("INK", None);
        text.style.font_size = Some(Length::Px(90.0));
        text.style.white_space = Some(CssWhiteSpace::Nowrap);
        text.style.color = Some(rustmotion_core::css::style::Color::String("#FFFFFF".into()));
        text.style.animation = vec![AnimationEffect::CharScaleIn(CharAnimationTiming {
            duration: 1.0,
            stagger: 0.0,
            granularity: TextAnimGranularity::Word,
            easing: EasingType::Linear,
            overshoot: Some(0.0),
            ink_from: Some("#FF0000".into()),
            ..Default::default()
        })];

        let mean_green = |time: f64| -> f32 {
            let mut surface = skia_safe::surfaces::raster_n32_premul((TUNING_W, TUNING_H))
                .expect("raster surface");
            {
                let canvas = surface.canvas();
                text.paint(
                    canvas,
                    TUNING_W as f32,
                    None,
                    time,
                    &props_for(&text),
                    &test_ctx(),
                )
                .expect("paint succeeds");
            }
            let snapshot = surface.image_snapshot();
            let info = skia_safe::ImageInfo::new(
                (TUNING_W, TUNING_H),
                skia_safe::ColorType::RGBA8888,
                skia_safe::AlphaType::Unpremul,
                None,
            );
            let mut buf = vec![0u8; (TUNING_W * TUNING_H * 4) as usize];
            assert!(snapshot.read_pixels(
                &info,
                &mut buf,
                (TUNING_W * 4) as usize,
                skia_safe::IPoint::new(0, 0),
                skia_safe::image::CachingHint::Disallow,
            ));
            let (weighted, alpha_sum) =
                (0..(TUNING_W * TUNING_H) as usize).fold((0u64, 0u64), |(s, a), i| {
                    let alpha = buf[i * 4 + 3] as u64;
                    (s + buf[i * 4 + 1] as u64 * alpha, a + alpha)
                });
            assert!(alpha_sum > 0, "some inked pixels must exist at t={time}");
            weighted as f32 / alpha_sum as f32
        };

        let early = mean_green(0.1);
        let mid = mean_green(0.5);
        let settled = mean_green(5.0);

        assert!(
            early < 60.0,
            "at 10% the word should read nearly pure red (mean green {early:.1})"
        );
        assert!(
            mid > early + 40.0 && mid < settled - 40.0,
            "at 50% the word should be halfway between its start colour and the text colour \
             (early={early:.1}, mid={mid:.1}, settled={settled:.1})"
        );
        assert!(
            settled > 250.0,
            "once settled the word must be the text's own white, not a tint of it \
             (mean green {settled:.1})"
        );
    }

    fn morphing_text(from: &str, to: &str, morph: Option<TextMorphConfig>) -> Text {
        let mut text = make_text(from, Some(CssWhiteSpace::Nowrap));
        text.style.font_size = Some(Length::Px(90.0));
        text.states = vec![TextState {
            at: 1.0,
            content: to.into(),
        }];
        text.morph = morph;
        text
    }

    #[test]
    fn lcs_pairs_matches_identical_characters_left_to_right() {
        let from: Vec<char> = "AX".chars().collect();
        let to: Vec<char> = "YA".chars().collect();
        assert_eq!(
            lcs_pairs(&from, &to),
            vec![(0, 1)],
            "the shared 'A' must be paired even though it moves from index 0 to index 1"
        );
    }

    #[test]
    fn scrambled_char_is_deterministic_and_varies_across_time() {
        let a = scrambled_char(4, 2, 0.31);
        let b = scrambled_char(4, 2, 0.31);
        assert_eq!(
            a, b,
            "the same seed/index/time must always scramble to the same glyph"
        );

        let distinct: std::collections::HashSet<char> = (0..30)
            .map(|i| scrambled_char(4, 2, i as f64 * 0.05))
            .collect();
        assert!(
            distinct.len() > 1,
            "the scramble must vary as time advances, not freeze on one glyph"
        );
    }

    #[test]
    fn morph_looks_different_from_a_hard_cut_mid_transition() {
        let morphed = morphing_text(
            "AX",
            "YA",
            Some(TextMorphConfig {
                duration: 1.0,
                unmatched: TextMorphUnmatched::Fade,
                seed: 0,
            }),
        );
        let cut = morphing_text("AX", "YA", None);

        assert_ne!(
            render_plain(&morphed, 1.5),
            render_plain(&cut, 1.5),
            "50% through the morph, the frame must differ from an immediate hard cut to the \
             final label — the matched letter should still be travelling and the unmatched \
             letters should still be fading, not already fully settled"
        );
    }

    #[test]
    fn morph_keeps_progressing_between_two_instants_in_the_window() {
        let morphed = morphing_text(
            "AX",
            "YA",
            Some(TextMorphConfig {
                duration: 1.0,
                unmatched: TextMorphUnmatched::Fade,
                seed: 0,
            }),
        );

        assert_ne!(
            render_plain(&morphed, 1.1),
            render_plain(&morphed, 1.9),
            "the morph must keep changing across its window, not snap to one position and hold"
        );
    }

    #[test]
    fn scramble_mode_paints_a_glyph_other_than_the_target_before_settling() {
        let seed = 3u32;
        let idx = 0usize;
        let visibly_faded_in_but_before_the_settle_threshold = 20..60;
        let sample_time = visibly_faded_in_but_before_the_settle_threshold
            .map(|i| 1.0 + i as f64 * 0.01)
            .find(|&t| scrambled_char(seed, idx, t) != 'B')
            .expect("at least one sampled instant must scramble to something other than 'B'");

        let fade = morphing_text(
            "A",
            "B",
            Some(TextMorphConfig {
                duration: 1.0,
                unmatched: TextMorphUnmatched::Fade,
                seed,
            }),
        );
        let scramble = morphing_text(
            "A",
            "B",
            Some(TextMorphConfig {
                duration: 1.0,
                unmatched: TextMorphUnmatched::Scramble,
                seed,
            }),
        );

        const W: i32 = 400;
        const H: i32 = 200;
        let ctx = test_ctx();
        let props = AnimatedProperties::default();

        let render = |text: &Text| -> Vec<u8> {
            let mut surface =
                skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
            text.paint(surface.canvas(), W as f32, None, sample_time, &props, &ctx)
                .expect("paint succeeds");
            alpha_grid(&mut surface, W, H)
        };

        let fade_grid = render(&fade);
        let scramble_grid = render(&scramble);

        assert_ne!(
            fade_grid, scramble_grid,
            "at t={sample_time}, `unmatched: scramble` must paint a different glyph than \
             `unmatched: fade` — scramble is supposed to cycle through placeholder characters \
             instead of just fading the real one in"
        );
    }

    #[test]
    fn morph_settles_on_a_plain_cut_once_the_window_has_passed() {
        let morphed = morphing_text(
            "AX",
            "YA",
            Some(TextMorphConfig {
                duration: 1.0,
                unmatched: TextMorphUnmatched::Fade,
                seed: 0,
            }),
        );
        let cut = morphing_text("AX", "YA", None);

        assert_eq!(
            render_plain(&morphed, 2.0),
            render_plain(&cut, 2.0),
            "once the morph window has passed, the frame must match a plain cut to the final label"
        );
    }
}
