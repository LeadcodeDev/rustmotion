use rustmotion_core::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Font, FontStyle, Paint, PaintStyle, Rect};

use rustmotion_core::engine::animator::ease;

use rustmotion_core::css::style::{
    FontStyle as CssFontStyle, FontWeight as CssFontWeight, FontWeightKw,
    TextAlign as CssTextAlign, WhiteSpace as CssWhiteSpace,
};
use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, measure_text_with_fallback, paint_from_hex,
    typeface_with_fallback, wrap_text_with_fallback,
};
use rustmotion_core::schema::{
    AnimationEffect, CharAnimPreset, CharAnimation, FontStyleType, FontWeight, Stroke, TextAlign,
    TextAnimGranularity, TextBackground, TextShadow, TimelineStep,
};
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

/// Default starting blur sigma (px) for `char_blur_in` when
/// `CharAnimationTiming.blur` is not set. Tuned against rendered output at
/// 120px display type (see issue #118's render proof): low enough that
/// individual letterforms stay ghost-legible at the start of a unit's
/// reveal (this is a *reveal*, not a smoke effect), high enough that the
/// blur is unmistakable next to the settled, sharp frame.
const DEFAULT_CHAR_BLUR_SIGMA: f32 = 14.0;

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
}

rustmotion_core::impl_traits!(Text {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

/// Apply a text animation preset to a single unit (char or word).
/// Returns the text draw position adjustments and paint modifications.
fn apply_text_anim_preset(
    canvas: &Canvas,
    text: &str,
    font: &Font,
    emoji_font: &Option<Font>,
    paint: &Paint,
    cursor_x: f32,
    line_y: f32,
    unit_width: f32,
    preset: &CharAnimPreset,
    t: f32,
    time: f64,
    unit_idx: usize,
    font_size: f32,
    overshoot: f32,
    blur_radius: f32,
) {
    let center_x = cursor_x + unit_width / 2.0;
    let center_y = line_y;

    match preset {
        CharAnimPreset::ScaleIn => {
            // 0→(1+overshoot) at 70%, then settle to 1.0
            let scale = if overshoot > 0.001 {
                if t < 0.7 {
                    let p = t / 0.7;
                    p * (1.0 + overshoot)
                } else {
                    let p = (t - 0.7) / 0.3;
                    (1.0 + overshoot) - overshoot * p
                }
            } else {
                t
            };
            if scale < 0.001 {
                return;
            }
            canvas.translate((center_x, center_y));
            canvas.scale((scale, scale));
            canvas.translate((-center_x, -center_y));
            draw_text_with_fallback(canvas, text, font, emoji_font, 0.0, cursor_x, line_y, paint);
        }
        CharAnimPreset::FadeIn => {
            let mut p = paint.clone();
            p.set_alpha_f(t * paint.alpha_f());
            draw_text_with_fallback(canvas, text, font, emoji_font, 0.0, cursor_x, line_y, &p);
        }
        CharAnimPreset::Wave => {
            let wave_offset =
                (time as f32 * 4.0 + unit_idx as f32 * 0.5).sin() * 8.0 * (1.0 - t * 0.5);
            let mut p = paint.clone();
            p.set_alpha_f(t.min(1.0) * paint.alpha_f());
            draw_text_with_fallback(
                canvas,
                text,
                font,
                emoji_font,
                0.0,
                cursor_x,
                line_y + wave_offset,
                &p,
            );
        }
        CharAnimPreset::Bounce => {
            let peak = 1.0 + overshoot.max(0.3); // bounce always overshoots, min 0.3
            let scale = if t < 0.5 {
                t * 2.0 * peak
            } else {
                peak - (peak - 1.0) * ((t - 0.5) * 2.0)
            };
            let scale = scale.max(0.001);
            canvas.translate((center_x, center_y));
            canvas.scale((scale, scale));
            canvas.translate((-center_x, -center_y));
            draw_text_with_fallback(canvas, text, font, emoji_font, 0.0, cursor_x, line_y, paint);
        }
        CharAnimPreset::RotateIn => {
            let angle = (1.0 - t) * -90.0;
            let mut p = paint.clone();
            p.set_alpha_f(t * paint.alpha_f());
            canvas.translate((center_x, center_y));
            canvas.rotate(angle, None);
            canvas.translate((-center_x, -center_y));
            draw_text_with_fallback(canvas, text, font, emoji_font, 0.0, cursor_x, line_y, &p);
        }
        CharAnimPreset::SlideUp => {
            let offset_y = (1.0 - t) * font_size * 0.8;
            let mut p = paint.clone();
            p.set_alpha_f(t * paint.alpha_f());
            draw_text_with_fallback(
                canvas,
                text,
                font,
                emoji_font,
                0.0,
                cursor_x,
                line_y + offset_y,
                &p,
            );
        }
        CharAnimPreset::BlurIn => {
            // One continuous progress value `t` drives all three
            // components at once (blur settle, upward drift, opacity
            // ramp) rather than sequencing them as separate effects.
            let tt = t.clamp(0.0, 1.0);
            let offset_y = (1.0 - tt) * font_size * 0.12;
            let sigma = ((1.0 - tt) * blur_radius).max(0.0);
            let mut p = paint.clone();
            p.set_alpha_f(tt * paint.alpha_f());
            if sigma > 0.05 {
                if let Some(filter) = skia_safe::image_filters::blur(
                    (sigma, sigma),
                    skia_safe::TileMode::Clamp,
                    None,
                    None,
                ) {
                    p.set_image_filter(filter);
                }
            }
            draw_text_with_fallback(
                canvas,
                text,
                font,
                emoji_font,
                0.0,
                cursor_x,
                line_y + offset_y,
                &p,
            );
        }
    }
}

/// Render text with per-character or per-word animation.
fn render_char_animation(
    canvas: &Canvas,
    _content: &str,
    font: &Font,
    emoji_font: &Option<Font>,
    paint: &Paint,
    letter_spacing: f32,
    align: TextAlign,
    align_width: f32,
    line_height_val: f32,
    baseline_offset: f32,
    lines: &[String],
    char_anim: &CharAnimation,
    time: f64,
    overshoot: f32,
    blur_radius: f32,
) {
    let is_word_mode = matches!(char_anim.granularity, TextAnimGranularity::Word);
    let mut global_unit_idx = 0usize;

    for (line_idx, line) in lines.iter().enumerate() {
        if line.is_empty() {
            continue;
        }

        let advance_width = measure_text_with_fallback(line, font, emoji_font, letter_spacing);
        let line_x = match align {
            TextAlign::Left => 0.0,
            TextAlign::Center => (align_width - advance_width) / 2.0,
            TextAlign::Right => align_width - advance_width,
        };
        let line_y = line_idx as f32 * line_height_val + baseline_offset;

        if is_word_mode {
            // Per-word animation: split line into words and spaces
            let mut cursor_x = line_x;
            let mut chars = line.chars().peekable();

            while chars.peek().is_some() {
                // Collect leading spaces
                let mut spaces = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_whitespace() {
                        spaces.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if !spaces.is_empty() {
                    let space_w =
                        measure_text_with_fallback(&spaces, font, emoji_font, letter_spacing);
                    // Draw spaces without animation
                    draw_text_with_fallback(
                        canvas, &spaces, font, emoji_font, 0.0, cursor_x, line_y, paint,
                    );
                    cursor_x += space_w;
                }

                // Collect the word
                let mut word = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_whitespace() {
                        break;
                    }
                    word.push(c);
                    chars.next();
                }
                if word.is_empty() {
                    continue;
                }

                let word_width =
                    measure_text_with_fallback(&word, font, emoji_font, letter_spacing);

                // Calculate animation progress for this word
                let unit_start =
                    char_anim.delay as f64 + global_unit_idx as f64 * char_anim.stagger as f64;
                let unit_end = unit_start + char_anim.duration as f64;
                let raw_t = if time <= unit_start {
                    0.0
                } else if time >= unit_end {
                    1.0
                } else {
                    (time - unit_start) / (unit_end - unit_start)
                };
                let t = ease(raw_t, &char_anim.easing) as f32;

                canvas.save();
                apply_text_anim_preset(
                    canvas,
                    &word,
                    font,
                    emoji_font,
                    paint,
                    cursor_x,
                    line_y,
                    word_width,
                    &char_anim.preset,
                    t,
                    time,
                    global_unit_idx,
                    font.size(),
                    overshoot,
                    blur_radius,
                );
                canvas.restore();

                cursor_x += word_width;
                global_unit_idx += 1;
            }
        } else {
            // Per-character animation (original behavior)
            let mut cursor_x = line_x;
            for ch in line.chars() {
                let ch_str = ch.to_string();
                let (ch_width, _) = font.measure_str(&ch_str, None);
                let ch_width = ch_width + letter_spacing;

                let unit_start =
                    char_anim.delay as f64 + global_unit_idx as f64 * char_anim.stagger as f64;
                let unit_end = unit_start + char_anim.duration as f64;
                let raw_t = if time <= unit_start {
                    0.0
                } else if time >= unit_end {
                    1.0
                } else {
                    (time - unit_start) / (unit_end - unit_start)
                };
                let t = ease(raw_t, &char_anim.easing) as f32;

                canvas.save();
                apply_text_anim_preset(
                    canvas,
                    &ch_str,
                    font,
                    emoji_font,
                    paint,
                    cursor_x,
                    line_y,
                    ch_width,
                    &char_anim.preset,
                    t,
                    time,
                    global_unit_idx,
                    font.size(),
                    overshoot,
                    blur_radius,
                );
                canvas.restore();

                cursor_x += ch_width;
                global_unit_idx += 1;
            }
        }
    }
}

impl Text {
    fn paint(
        &self,
        canvas: &Canvas,
        layout_width: f32,
        time: f64,
        props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) -> Result<()> {
        let font_size = self.style.font_size_px_or(48.0);
        // Animated color (timeline style-state transitions) overrides the
        // static style color.
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
        let line_height_val = self.style.line_height_for(font_size);
        let letter_spacing = self.style.letter_spacing_px();

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

        let font = Font::from_typeface(typeface, font_size);
        let emoji_font = emoji_typeface().map(|tf| Font::from_typeface(tf, font_size));
        let mut paint = paint_from_hex(color);
        paint.set_alpha_f(1.0);

        // Use layout width as wrapping constraint, combined with max_width.
        // M1: `white-space: nowrap|pre` disables wrapping entirely — the
        // line may then exceed `layout_width`. That's the point: it makes
        // the property mean something, and it's exactly the condition the
        // geometry validator's `unwrappable_text_overflow` check (which
        // re-measures via `TextIntrinsic::from_text`, now wrap-aware too)
        // assumes the renderer can produce.
        let nowrap = matches!(
            self.style.white_space,
            Some(CssWhiteSpace::Nowrap | CssWhiteSpace::Pre)
        );
        let wrap_width = if nowrap {
            None
        } else if layout_width.is_finite() && layout_width > 0.0 {
            match self.max_width {
                Some(mw) => Some(mw.min(layout_width)),
                None => Some(layout_width),
            }
        } else {
            self.max_width
        };

        // Apply typewriter effect: limit visible characters based on animation progress
        let content = if props.visible_chars_progress >= 0.0 {
            let chars: Vec<char> = self.content.chars().collect();
            let visible = (props.visible_chars_progress * chars.len() as f32).round() as usize;
            let visible = visible.min(chars.len());
            if visible == 0 {
                return Ok(());
            }
            chars[..visible].iter().collect::<String>()
        } else {
            self.content.clone()
        };

        let lines = wrap_text_with_fallback(&content, &font, &emoji_font, wrap_width);
        let (_, metrics) = font.metrics();
        let ascent = -metrics.ascent;
        let descent = metrics.descent;
        let baseline_offset = (line_height_val + ascent - descent) / 2.0;

        // Prepare optional shadow and stroke paints. The component-level
        // `text-shadow` field wins; otherwise the CSS `style.text-shadow`
        // list is bridged (it used to be parsed and silently dropped).
        let shadows: Vec<rustmotion_core::schema::TextShadow> = if let Some(s) = &self.text_shadow {
            vec![s.clone()]
        } else if let Some(list) = &self.style.text_shadow {
            let lctx = rustmotion_core::css::units::LengthContext {
                viewport_width: ctx.video_width as f32,
                viewport_height: ctx.video_height as f32,
                parent_size: layout_width.max(0.0),
                font_size,
                root_font_size: 16.0,
            };
            list.iter().map(|s| s.to_schema(&lctx)).collect()
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

        // Compute alignment width
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

        // Per-character animation mode (via style.animation char_* presets)
        if let Some(ref resolved) = props.char_animation {
            let char_anim = CharAnimation {
                preset: resolved.preset.clone(),
                granularity: resolved.granularity.clone(),
                stagger: resolved.stagger,
                duration: resolved.duration,
                easing: resolved.easing.clone(),
                delay: resolved.delay,
            };
            render_char_animation(
                canvas,
                &content,
                &font,
                &emoji_font,
                &paint,
                letter_spacing,
                align,
                align_width,
                line_height_val,
                baseline_offset,
                &lines,
                &char_anim,
                time,
                resolved.overshoot,
                0.0, // blur is only meaningful for char_blur_in, handled below
            );
            return Ok(());
        }

        // `char_blur_in` is read directly off `style.animation` rather than
        // through `engine::animator::extract_effects` → `props.char_animation`
        // like its five siblings above: that resolution path lives outside
        // this workstream's file scope (schema/video.rs + text.rs only, see
        // issue #118 / chantier #117 W1). This is a top-level
        // `style.animation` entry lookup, so — unlike the branch above — it
        // does not pick up container-level stagger delay shifting or
        // `timeline`-step-embedded copies.
        if let Some(timing) = self.style.animation.iter().find_map(|effect| match effect {
            AnimationEffect::CharBlurIn(t) => Some(t),
            _ => None,
        }) {
            let char_anim = CharAnimation {
                preset: CharAnimPreset::BlurIn,
                granularity: timing.granularity.clone(),
                stagger: timing.stagger as f32,
                duration: timing.duration as f32,
                easing: timing.easing.clone(),
                delay: timing.delay as f32,
            };
            render_char_animation(
                canvas,
                &content,
                &font,
                &emoji_font,
                &paint,
                letter_spacing,
                align,
                align_width,
                line_height_val,
                baseline_offset,
                &lines,
                &char_anim,
                time,
                timing.overshoot.unwrap_or(0.08) as f32,
                timing
                    .blur
                    .map(|b| b as f32)
                    .unwrap_or(DEFAULT_CHAR_BLUR_SIGMA),
            );
            return Ok(());
        }

        for (i, line) in lines.iter().enumerate() {
            if line.is_empty() {
                continue;
            }

            let advance_width =
                measure_text_with_fallback(line, &font, &emoji_font, letter_spacing);

            let x = match align {
                TextAlign::Left => 0.0,
                TextAlign::Center => (align_width - advance_width) / 2.0,
                TextAlign::Right => align_width - advance_width,
            };
            let y = i as f32 * line_height_val + baseline_offset;

            // Draw background highlight behind text
            if let Some(ref bg) = self.text_background {
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

            // Draw shadows — reverse order so the first CSS shadow ends on top.
            for (sp, ox, oy) in shadow_paints.iter().rev() {
                draw_text_with_fallback(
                    canvas,
                    line,
                    &font,
                    &emoji_font,
                    letter_spacing,
                    x + ox,
                    y + oy,
                    sp,
                );
            }

            // Draw stroke (outline)
            if let Some(ref sp) = stroke_paint {
                draw_text_with_fallback(canvas, line, &font, &emoji_font, letter_spacing, x, y, sp);
            }

            // Draw fill
            draw_text_with_fallback(
                canvas,
                line,
                &font,
                &emoji_font,
                letter_spacing,
                x,
                y,
                &paint,
            );
        }

        Ok(())
    }
}

impl Painter for Text {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        let _ = self.paint(canvas, layout.width, ctx.time, props, ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmotion_core::css::style::CssStyle;
    use rustmotion_core::css::Length;
    use rustmotion_core::schema::{CharAnimationTiming, EasingType};

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
        }
    }

    fn test_ctx() -> PaintCtx {
        PaintCtx {
            time: 0.0,
            scene_duration: 1.0,
            frame_index: 0,
            fps: 30,
            video_width: 600,
            video_height: 200,
            stagger_offset: 0.0,
        }
    }

    /// Reads back the full alpha channel of the surface as a `width × height`
    /// row-major byte grid.
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

    /// Does any pixel in `[x0, x1) × [y0, y1)` have non-zero alpha? Scanning
    /// a region rather than a single exact pixel avoids flaking on the gap
    /// between two glyphs or on a space character.
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
        // M1 render-level proof: a `white-space: nowrap` line stays on one
        // line and its glyphs visibly extend past `layout_width` — the box
        // it was allocated.
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
        text.paint(canvas, 80.0, 0.0, &props, &ctx)
            .expect("paint succeeds");
        let grid = alpha_grid(&mut surface, W, H);

        // Far past the 80px box, on the first line's height band: nowrap
        // must have painted ink there (the line didn't break at 80px).
        assert!(
            has_ink_in(&grid, W, 300, W, 0, 45),
            "nowrap text must paint past its 80px box on line 1 (scanned x∈[300,600), y∈[0,45))"
        );

        // Nothing should be on a *second* line — nowrap never word-wraps
        // (only literal newlines would start a new line, and there are
        // none here), so all ink stays within the first line's height band.
        assert!(
            !has_ink_in(&grid, W, 0, W, 55, H),
            "nowrap text must stay on a single line; found ink on what would be line 2"
        );
    }

    #[test]
    fn normal_white_space_wraps_within_the_layout_width() {
        // Contrast case: default wrapping keeps ink within the box on the
        // first line, and instead spills onto additional lines below.
        let text = make_text("the quick brown fox jumps over the lazy dog", None);
        const W: i32 = 600;
        const H: i32 = 200;
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        let canvas = surface.canvas();
        let ctx = test_ctx();
        let props = AnimatedProperties::default();
        text.paint(canvas, 80.0, 0.0, &props, &ctx)
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

    // ─── char_blur_in ───────────────────────────────────────────────────

    /// Fraction of "inked" pixels (alpha > 0) in the region that are
    /// partially transparent (0 < alpha < 250) rather than solid. A sharp
    /// glyph is mostly solid fill with a thin antialiased edge, so this
    /// fraction is low. A heavily blurred glyph is a soft gradient
    /// wherever it has any ink at all, so this fraction is high.
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

    /// Build the same `Font` the renderer would build for `family`/`px`, so
    /// tests can measure exact word boundaries instead of guessing pixel
    /// coordinates.
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
        // Render-level proof that char_blur_in actually blurs: a single
        // word must read as measurably softer mid-reveal than once
        // settled. Also exercises the DEFAULT_CHAR_BLUR_SIGMA fallback
        // (`blur: None`).
        let mut text = make_text("BLUR", None);
        text.style.font_size = Some(Length::Px(100.0));
        text.style.white_space = Some(CssWhiteSpace::Nowrap);
        text.style.animation = vec![AnimationEffect::CharBlurIn(CharAnimationTiming {
            delay: 0.0,
            duration: 0.5,
            stagger: 0.03,
            granularity: TextAnimGranularity::Word,
            easing: EasingType::Linear,
            overshoot: None,
            blur: None,
        })];

        const W: i32 = 700;
        const H: i32 = 220;
        let ctx = test_ctx();
        let props = AnimatedProperties::default();

        let render_at = |t: f64| -> Vec<u8> {
            let mut surface =
                skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
            {
                let canvas = surface.canvas();
                text.paint(canvas, W as f32, t, &props, &ctx)
                    .expect("paint succeeds");
            }
            alpha_grid(&mut surface, W, H)
        };

        let early = render_at(0.15); // raw progress 0.15/0.5 = 0.3 into the reveal
        let settled = render_at(1.0); // long past duration: sigma → 0, alpha → 1

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
        // The word-mode char path draws inter-word whitespace unanimated
        // at full opacity (see render_char_animation) — harmless for the
        // six existing opacity-only presets since a space glyph has no
        // ink. Pin down that this holds for char_blur_in too: each word's
        // blur filter is scoped to that word's own draw call, so it must
        // never smear ink into the gap between words.
        //
        // Note this is *not* the same as "a blurred word's own halo never
        // reaches near the gap" — a Gaussian blur legitimately spreads a
        // word's own ink a few sigma past its sharp glyph edge, which is
        // correct behaviour, not smearing into whitespace. So this checks
        // the gap's true center (rendering evidence: crates/.../issue-118
        // render proof measured the same distinction against real render
        // output — a word's halo fades to background well within a third
        // of a multi-space gap).
        const FONT_PX: f32 = 90.0;
        let mut text = make_text("FIRST               SECOND", None); // 15 spaces
        text.style.font_size = Some(Length::Px(FONT_PX));
        text.style.white_space = Some(CssWhiteSpace::Nowrap);
        text.style.animation = vec![AnimationEffect::CharBlurIn(CharAnimationTiming {
            delay: 0.0,
            duration: 0.4,
            stagger: 0.2,
            granularity: TextAnimGranularity::Word,
            easing: EasingType::Linear,
            overshoot: None,
            blur: Some(18.0),
        })];

        const W: i32 = 1400;
        const H: i32 = 180;
        let ctx = test_ctx();
        let props = AnimatedProperties::default();

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
                text.paint(canvas, W as f32, t, &props, &ctx)
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
        // With a stagger larger than the per-word duration, word 2 must
        // not have started at all while word 1 is mid-reveal, and nothing
        // should paint before `delay` has elapsed either.
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
            overshoot: None,
            blur: Some(16.0),
        })];

        const W: i32 = 900;
        const H: i32 = 180;
        let ctx = test_ctx();
        let props = AnimatedProperties::default();
        let font = inter_font(FONT_PX);
        let word1_end = measure_text_with_fallback("ONE", &font, &None, 0.0) as i32;

        // Before `delay`, nothing should paint at all.
        let mut before = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        {
            let canvas = before.canvas();
            text.paint(canvas, W as f32, 0.1, &props, &ctx)
                .expect("paint succeeds");
        }
        let before_grid = alpha_grid(&mut before, W, H);
        assert!(
            !has_ink_in(&before_grid, W, 0, W, 0, H),
            "nothing should paint before `delay` has elapsed"
        );

        // t=0.65s: word 1's local progress is (0.65-0.5)/0.3 = 0.5 (mid
        // reveal), word 2 only starts at delay+stagger=1.1s.
        let mut mid = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        {
            let canvas = mid.canvas();
            text.paint(canvas, W as f32, 0.65, &props, &ctx)
                .expect("paint succeeds");
        }
        let mid_grid = alpha_grid(&mut mid, W, H);
        assert!(
            has_ink_in(&mid_grid, W, 0, word1_end, 0, H),
            "word 1 should show ink by t=0.65 (mid-reveal)"
        );
        // Leave enough clearance past word 1's sharp-edge measurement for
        // its *own* Gaussian halo (a real, expected effect of blurring
        // that word — see the analogous note in the whitespace-gap test
        // above), so this only catches an actual word-2 leak.
        assert!(
            !has_ink_in(&mid_grid, W, word1_end + 70, W, 0, H),
            "word 2 (starts at delay+stagger=1.1s) must still be fully invisible at t=0.65"
        );
    }
}
