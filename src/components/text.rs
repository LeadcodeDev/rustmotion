use crate::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Font, FontStyle, Paint, PaintStyle, Rect};

use crate::engine::animator::ease;
use crate::error::RustmotionError;

/// Resolve `line-height`: values <= 10 are treated as a multiplier (CSS-like),
/// values > 10 are absolute pixels.
fn resolve_line_height(line_height: Option<f32>, font_size: f32) -> f32 {
    match line_height {
        Some(v) if v <= 10.0 => font_size * v,
        Some(v) => v,
        None => font_size * 1.3,
    }
}

use crate::engine::renderer::{font_mgr, paint_from_hex, wrap_text_with_fallback, draw_text_with_fallback, measure_text_with_fallback, emoji_typeface};
use crate::layout::{Constraints, LayoutNode};
use crate::schema::{CharAnimPreset, CharAnimation, FontStyleType, FontWeight, LayerStyle, TextAlign, TextAnimGranularity};
use crate::traits::{RenderContext, TimingConfig, Widget};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Text {
    pub content: String,
    #[serde(default)]
    pub max_width: Option<f32>,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

crate::impl_traits!(Text {
    Animatable => style,
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
            let wave_offset = (time as f32 * 4.0 + unit_idx as f32 * 0.5).sin() * 8.0 * (1.0 - t * 0.5);
            let mut p = paint.clone();
            p.set_alpha_f(t.min(1.0) * paint.alpha_f());
            draw_text_with_fallback(canvas, text, font, emoji_font, 0.0, cursor_x, line_y + wave_offset, &p);
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
            draw_text_with_fallback(canvas, text, font, emoji_font, 0.0, cursor_x, line_y + offset_y, &p);
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
                    let space_w = measure_text_with_fallback(&spaces, font, emoji_font, letter_spacing);
                    // Draw spaces without animation
                    draw_text_with_fallback(canvas, &spaces, font, emoji_font, 0.0, cursor_x, line_y, paint);
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

                let word_width = measure_text_with_fallback(&word, font, emoji_font, letter_spacing);

                // Calculate animation progress for this word
                let unit_start = char_anim.delay as f64 + global_unit_idx as f64 * char_anim.stagger as f64;
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
                    canvas, &word, font, emoji_font, paint,
                    cursor_x, line_y, word_width,
                    &char_anim.preset, t, time, global_unit_idx, font.size(),
                    overshoot,
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

                let unit_start = char_anim.delay as f64 + global_unit_idx as f64 * char_anim.stagger as f64;
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
                    canvas, &ch_str, font, emoji_font, paint,
                    cursor_x, line_y, ch_width,
                    &char_anim.preset, t, time, global_unit_idx, font.size(),
                    overshoot,
                );
                canvas.restore();

                cursor_x += ch_width;
                global_unit_idx += 1;
            }
        }
    }
}

impl Widget for Text {
    fn render(&self, canvas: &Canvas, layout: &LayoutNode, ctx: &RenderContext, props: &crate::engine::animator::AnimatedProperties) -> Result<()> {
        let font_size = self.style.font_size_or(48.0);
        let color = self.style.color_or("#FFFFFF");
        let font_family = self.style.font_family_or("Inter");
        let font_weight = self.style.font_weight_or(FontWeight::Normal);
        let font_style_type = self.style.font_style_or(FontStyleType::Normal);
        let align = self.style.text_align_or(TextAlign::Left);
        let line_height_val = resolve_line_height(self.style.line_height, font_size);
        let letter_spacing = self.style.letter_spacing.unwrap_or(0.0);

        let fm = font_mgr();
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

        let typeface = fm
            .match_family_style(font_family, skia_font_style)
            .or_else(|| fm.match_family_style("Helvetica", skia_font_style))
            .or_else(|| fm.match_family_style("Arial", skia_font_style))
            .or_else(|| fm.match_family_style("sans-serif", skia_font_style))
            .or_else(|| {
                if fm.count_families() > 0 {
                    fm.match_family_style(&fm.family_name(0), skia_font_style)
                } else {
                    None
                }
            })
            .ok_or(RustmotionError::FontNotFound)?;

        let font = Font::from_typeface(typeface, font_size);
        let emoji_font = emoji_typeface().map(|tf| Font::from_typeface(tf, font_size));
        let mut paint = paint_from_hex(color);
        paint.set_alpha_f(1.0);

        // Use layout width as wrapping constraint, combined with max_width
        let wrap_width = if layout.width.is_finite() && layout.width > 0.0 {
            match self.max_width {
                Some(mw) => Some(mw.min(layout.width)),
                None => Some(layout.width),
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

        // Prepare optional shadow and stroke paints
        let shadow_paint = self.style.text_shadow.as_ref().map(|shadow| {
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
        });

        let stroke_paint = self.style.stroke.as_ref().map(|stroke| {
            let mut p = paint_from_hex(&stroke.color);
            p.set_style(PaintStyle::Stroke);
            p.set_stroke_width(stroke.width);
            p
        });

        // Compute alignment width
        let align_width = if layout.width.is_finite() && layout.width > 0.0 {
            layout.width
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
                canvas, &content, &font, &emoji_font, &paint,
                letter_spacing, align, align_width, line_height_val, baseline_offset,
                &lines, &char_anim, ctx.time, resolved.overshoot,
            );
            return Ok(());
        }

        for (i, line) in lines.iter().enumerate() {
            if line.is_empty() {
                continue;
            }

            let advance_width = measure_text_with_fallback(line, &font, &emoji_font, letter_spacing);

            let x = match align {
                TextAlign::Left => 0.0,
                TextAlign::Center => (align_width - advance_width) / 2.0,
                TextAlign::Right => align_width - advance_width,
            };
            let y = i as f32 * line_height_val + baseline_offset;

            // Draw background highlight behind text
            if let Some(ref bg) = self.style.text_background {
                let bg_paint = paint_from_hex(&bg.color);
                let (_, font_rect) = font.measure_str(line, None);
                let bg_rect = Rect::from_xywh(
                    x - bg.padding + font_rect.left,
                    y + font_rect.top - bg.padding / 2.0,
                    advance_width + bg.padding * 2.0,
                    -font_rect.top + font_rect.bottom + bg.padding,
                );
                if bg.corner_radius > 0.0 {
                    let rrect = skia_safe::RRect::new_rect_xy(bg_rect, bg.corner_radius, bg.corner_radius);
                    canvas.draw_rrect(rrect, &bg_paint);
                } else {
                    canvas.draw_rect(bg_rect, &bg_paint);
                }
            }

            // Draw shadow
            if let Some((ref sp, ox, oy)) = shadow_paint {
                draw_text_with_fallback(canvas, line, &font, &emoji_font, letter_spacing, x + ox, y + oy, sp);
            }

            // Draw stroke (outline)
            if let Some(ref sp) = stroke_paint {
                draw_text_with_fallback(canvas, line, &font, &emoji_font, letter_spacing, x, y, sp);
            }

            // Draw fill
            draw_text_with_fallback(canvas, line, &font, &emoji_font, letter_spacing, x, y, &paint);
        }

        Ok(())
    }

    fn measure(&self, _constraints: &Constraints) -> (f32, f32) {
        let font_size = self.style.font_size_or(48.0);
        let font_family = self.style.font_family_or("Inter");
        let font_weight = self.style.font_weight_or(FontWeight::Normal);
        let font_style_type = self.style.font_style_or(FontStyleType::Normal);

        let fm = font_mgr();
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
        let typeface = fm
            .match_family_style(font_family, skia_font_style)
            .or_else(|| fm.match_family_style("Helvetica", skia_font_style))
            .or_else(|| fm.match_family_style("Arial", skia_font_style))
            .or_else(|| fm.match_family_style("sans-serif", skia_font_style))
            .unwrap_or_else(|| fm.legacy_make_typeface(None, skia_font_style).expect("No fallback font"));
        let font = Font::from_typeface(typeface, font_size);
        let emoji_font = emoji_typeface().map(|tf| Font::from_typeface(tf, font_size));
        let lines = wrap_text_with_fallback(&self.content, &font, &emoji_font, self.max_width);
        let line_height_val = resolve_line_height(self.style.line_height, font_size);
        let mut max_w = lines.iter().map(|l| {
            measure_text_with_fallback(l, &font, &emoji_font, 0.0)
        }).fold(0.0f32, f32::max);
        let mut h = lines.len() as f32 * line_height_val;

        // Account for text-background padding in measurement
        if let Some(ref bg) = self.style.text_background {
            max_w += bg.padding * 2.0;
            h += bg.padding;
        }

        (max_w, h)
    }
}
