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
use rustmotion_core::engine::animator::{AnimatedProperties, ResolvedCharAnimation};
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, measure_text_with_fallback, paint_from_hex,
    typeface_with_fallback, wrap_text_with_tracking,
};
use rustmotion_core::schema::{
    CaretConfig, CaretShape, CharAnimPreset, FontStyleType, FontWeight, Stroke, TextAlign,
    TextAnimGranularity, TextBackground, TextShadow, TextState, TextSwapConfig, TimelineStep,
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
}

rustmotion_core::impl_traits!(Text {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

/// Eased progress (0..1) of unit `idx` at `time`, honouring the config's
/// deterministic jitter.
fn unit_progress(cfg: &ResolvedCharAnimation, idx: usize, time: f64) -> f32 {
    let unit_start = cfg.unit_start(idx);
    let unit_end = unit_start + cfg.duration as f64;
    let raw_t = if time <= unit_start {
        0.0
    } else if time >= unit_end {
        1.0
    } else {
        (time - unit_start) / (unit_end - unit_start)
    };
    ease(raw_t, &cfg.easing) as f32
}

/// The paint a unit draws with at progress `t`: the base paint, tinted from
/// `ink_from` towards the text's own colour when the config asks for it.
///
/// `None` means "use the base paint unchanged" — worth keeping distinct from
/// a clone, since the caller may already be mutating its own copy.
fn ink_paint(cfg: &ResolvedCharAnimation, paint: &Paint, t: f32) -> Option<Paint> {
    let from = cfg.ink_from.as_deref()?;
    let start = paint_from_hex(from).color();
    let end = paint.color();
    let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t.clamp(0.0, 1.0)) as u8;
    let mut p = paint.clone();
    p.set_color(skia_safe::Color::from_argb(
        end.a(),
        lerp(start.r(), end.r()),
        lerp(start.g(), end.g()),
        lerp(start.b(), end.b()),
    ));
    Some(p)
}

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
    // The tracking the *cursor* was advanced with. Drawing at 0 while the
    // advance carries a negative value makes the glyphs overrun their slot and
    // swallow the inter-word space — visible only once the overrun approaches
    // a space's width, i.e. at small sizes or long words.
    letter_spacing: f32,
    cfg: &ResolvedCharAnimation,
    t: f32,
    time: f64,
    unit_idx: usize,
    font_size: f32,
) {
    let preset = &cfg.preset;
    let overshoot = cfg.overshoot;
    let blur_radius = cfg.blur;
    let center_x = cursor_x + unit_width / 2.0;
    let center_y = line_y;

    // `ink_from` and `scale_from` are cross-cutting: they compose with
    // whatever the preset itself does rather than replacing it, so they are
    // resolved once here instead of inside each arm.
    let inked = ink_paint(cfg, paint, t);
    let paint = inked.as_ref().unwrap_or(paint);
    if let Some(from) = cfg.scale_from {
        // The scale-driven presets own their scale curve outright; stacking a
        // second one on top would fight it rather than compose with it.
        if !matches!(preset, CharAnimPreset::ScaleIn | CharAnimPreset::Bounce) {
            let s = from + (1.0 - from) * t.clamp(0.0, 1.0);
            canvas.translate((center_x, center_y));
            canvas.scale((s, s));
            canvas.translate((-center_x, -center_y));
        }
    }

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
            draw_text_with_fallback(
                canvas,
                text,
                font,
                emoji_font,
                letter_spacing,
                cursor_x,
                line_y,
                paint,
            );
        }
        CharAnimPreset::FadeIn => {
            let mut p = paint.clone();
            p.set_alpha_f(t * paint.alpha_f());
            draw_text_with_fallback(
                canvas,
                text,
                font,
                emoji_font,
                letter_spacing,
                cursor_x,
                line_y,
                &p,
            );
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
                letter_spacing,
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
            draw_text_with_fallback(
                canvas,
                text,
                font,
                emoji_font,
                letter_spacing,
                cursor_x,
                line_y,
                paint,
            );
        }
        CharAnimPreset::RotateIn => {
            let angle = (1.0 - t) * -90.0;
            let mut p = paint.clone();
            p.set_alpha_f(t * paint.alpha_f());
            canvas.translate((center_x, center_y));
            canvas.rotate(angle, None);
            canvas.translate((-center_x, -center_y));
            draw_text_with_fallback(
                canvas,
                text,
                font,
                emoji_font,
                letter_spacing,
                cursor_x,
                line_y,
                &p,
            );
        }
        CharAnimPreset::SlideUp => {
            // Despite the name, the travel axis is `direction`'s to choose —
            // `up` (the default) is what the preset has always done.
            let travel = (1.0 - t) * font_size * 0.8 * cfg.distance;
            let (dx, dy) = cfg.direction.offset(travel);
            let mut p = paint.clone();
            p.set_alpha_f(t * paint.alpha_f());
            draw_text_with_fallback(
                canvas,
                text,
                font,
                emoji_font,
                letter_spacing,
                cursor_x + dx,
                line_y + dy,
                &p,
            );
        }
        CharAnimPreset::BlurIn => {
            // One continuous progress value `t` drives all three
            // components at once (blur settle, upward drift, opacity
            // ramp) rather than sequencing them as separate effects.
            let tt = t.clamp(0.0, 1.0);
            let travel = (1.0 - tt) * font_size * 0.12 * cfg.distance;
            let (dx, dy) = cfg.direction.offset(travel);
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
                letter_spacing,
                cursor_x + dx,
                line_y + dy,
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
    char_anim: &ResolvedCharAnimation,
    time: f64,
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
                let t = unit_progress(char_anim, global_unit_idx, time);

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
                    letter_spacing,
                    char_anim,
                    t,
                    time,
                    global_unit_idx,
                    font.size(),
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

                let t = unit_progress(char_anim, global_unit_idx, time);

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
                    // Single characters carry no internal tracking, so this is
                    // 0 by construction — passed explicitly rather than left
                    // to a default, since the word path above needs the real
                    // value and the two must not drift apart.
                    0.0,
                    char_anim,
                    t,
                    time,
                    global_unit_idx,
                    font.size(),
                );
                canvas.restore();

                cursor_x += ch_width;
                global_unit_idx += 1;
            }
        }
    }
}

impl Text {
    /// Every label this text can display, in order — `content` followed by
    /// each state's.
    ///
    /// Used for measurement: a box sized for the first label alone would be
    /// overrun the moment the text swapped to a longer one, and the geometry
    /// validator would have signed off on it.
    pub fn all_labels(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.content.as_str()).chain(self.states.iter().map(|s| s.content.as_str()))
    }

    /// The label showing at `time`: the last state whose `at` has passed, or
    /// `content` before any of them.
    fn label_at(&self, time: f64) -> &str {
        self.states
            .iter()
            .rfind(|s| s.at <= time)
            .map(|s| s.content.as_str())
            .unwrap_or(&self.content)
    }

    /// The swap in progress at `time`, if any.
    ///
    /// `None` when the text declares no `swap` config, even if it declares
    /// `states`: the labels then simply cut over at each `at`, which is a
    /// legitimate (if abrupt) choice and the one the field's absence asks
    /// for.
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

    fn paint(
        &self,
        canvas: &Canvas,
        layout_width: f32,
        content_height: Option<f32>,
        time: f64,
        props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) -> Result<()> {
        // `font-size` itself, plus `letter-spacing`/`line-height`'s `em`/`%`
        // (relative to this element's *own*, just-resolved font-size) are
        // now all resolved together against a real `LengthContext` (real
        // viewport dims from `ctx`) via `typography_px_ctx`, which re-derives
        // the right base between the two steps (lot B, wave S — this used to
        // stop at the context-free `font_size_px_or`, so `rem`/`vw`/`vh`
        // font-size silently fell back to 0px with only a loud warning).
        //
        // `em`/`%` *on `font-size` itself* are the one case this still
        // doesn't get right: per CSS they're relative to the *parent's*
        // actual computed font-size, but `cascade.rs` inherits `font-size`
        // down the tree as a raw, unresolved `Length`, not a resolved px
        // value (see the module note on `CssStyle::font_size_px_ctx`) — no
        // caller here can supply the real cascaded value, so `base_ctx`
        // below uses the CSS root default (16px) as the best available
        // stand-in. `rem` (always relative to a fixed root, not a per-
        // ancestor chain) and `vw`/`vh` (relative to the real viewport,
        // available here via `ctx`) do not have this problem.
        let base_ctx = crate::intrinsic::font_size_ctx(
            ctx.video_width as f32,
            ctx.video_height as f32,
            layout_width.max(0.0),
        );
        let (mut font_size, mut letter_spacing, mut line_height_val) =
            self.style.typography_px_ctx(&base_ctx, 48.0);
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

        // The box's own resolved width — computed here (ahead of the
        // `white-space: nowrap` wrap decision below) because `text-autofit`
        // needs it as its width-fit target *regardless* of nowrap: a nowrap
        // line still shrinks to fit this box once `text-autofit` is on (see
        // `CssStyle::text_autofit`'s doc comment), it just never breaks
        // across lines while doing it.
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

        // `text-autofit`: resolve the *actual* font-size/letter-spacing/
        // line-height used for the rest of this function — the identical
        // computation `TextIntrinsic::measure` runs for this same node (see
        // `resolve_text_autofit`'s doc comment for the parity argument).
        // Must happen before `type_ctx`/the final `Font` are built below so
        // both reflect the resolved (possibly shrunk) size, not the
        // requested one.
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

        // This element's *own* resolved font-size as the `em`/`%` base —
        // needed below for `text-shadow` (its blur/offset are relative to
        // the shadow owner's own font-size, same rule as letter-spacing/
        // line-height, not the parent-proxy `base_ctx` above). Built from
        // the post-autofit `font_size` so a shrunk headline's shadow shrinks
        // with it instead of using the pre-shrink em/% base.
        let type_ctx = rustmotion_core::css::units::LengthContext {
            font_size,
            ..base_ctx
        };

        let font = Font::from_typeface(typeface, font_size);
        let emoji_font = emoji_typeface().map(|tf| Font::from_typeface(tf, font_size));
        let mut paint = paint_from_hex(color);
        paint.set_alpha_f(1.0);

        // Use the box width as the wrapping constraint (computed above, as
        // `box_width`, ahead of the autofit step). M1: `white-space:
        // nowrap|pre` disables wrapping entirely — the line may then exceed
        // `layout_width`. That's the point: it makes the property mean
        // something, and it's exactly the condition the geometry
        // validator's `unwrappable_text_overflow` check (which re-measures
        // via `TextIntrinsic::from_text`, now wrap-aware too) assumes the
        // renderer can produce.
        let wrap_width = if nowrap { None } else { box_width };

        // Apply typewriter effect: limit visible characters based on animation progress
        let label = self.label_at(time);
        let content = if props.visible_chars_progress >= 0.0 {
            let chars: Vec<char> = label.chars().collect();
            let visible = (props.visible_chars_progress * chars.len() as f32).round() as usize;
            let visible = visible.min(chars.len());
            if visible == 0 && self.caret.is_none() {
                return Ok(());
            }
            // With a caret, an empty reveal still has something to paint: the
            // caret itself, sitting where the first character is about to
            // appear. Bailing out here would make it pop into existence
            // alongside that character instead of waiting for it.
            chars[..visible].iter().collect::<String>()
        } else {
            label.to_string()
        };

        // Tracking-aware wrap (issue #125 §1): the fit test now measures
        // with this element's real `letter_spacing`, matching the
        // measurements below (`align_width`, per-line `advance_width`) that
        // already used it — the box this wraps for and the pixels painted
        // into it now agree.
        let lines =
            wrap_text_with_tracking(&content, &font, &emoji_font, wrap_width, letter_spacing);
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

        // Per-character animation mode (via style.animation char_* presets).
        // All seven presets — `char_blur_in` included since it was routed
        // through `extract_effects` like its siblings — arrive here already
        // resolved, with container-level stagger folded into `delay`.
        if let Some(ref resolved) = props.char_animation {
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
                resolved,
                time,
            );
            return Ok(());
        }

        // A state swap has two labels on screen at once, each with its own
        // travel, blur and opacity. Everything above — font, alignment,
        // metrics, decorations — is shared between them; only the label text
        // and the motion differ.
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

        // The caret rides the reveal head: the end of the last line that has
        // been revealed so far, which is where the next character will land.
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

/// Draw already-wrapped `lines` with their background, shadows, stroke and
/// fill, shifted down by `offset_y`.
///
/// Shared by the plain draw and by each half of a state swap, so a swapping
/// label keeps the decorations (`text-background`, `text-shadow`, `stroke`)
/// the same text has when it is not swapping.
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

        // Draw background highlight behind text
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

        // Draw shadows — reverse order so the first CSS shadow ends on top.
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

        // Draw stroke (outline)
        if let Some(sp) = stroke_paint {
            draw_text_with_fallback(canvas, line, font, emoji_font, letter_spacing, x, y, sp);
        }

        // Draw fill
        draw_text_with_fallback(canvas, line, font, emoji_font, letter_spacing, x, y, paint);
    }
}

/// A state swap in progress: the label leaving, the label arriving, and how
/// far through the crossing we are.
struct ActiveSwap {
    from: String,
    to: String,
    progress: f32,
    distance: f32,
    blur: f32,
}

impl ActiveSwap {
    /// `(label, offset_y, blur_sigma, alpha)` for each of the two labels.
    ///
    /// The outgoing label leaves upwards and the incoming one arrives from
    /// below, so the pair reads as one value moving up a slot rather than as
    /// two labels passing each other.
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

/// Paint a caret whose left edge sits at `x`, aligned to the text `baseline`.
fn draw_caret(
    canvas: &Canvas,
    cfg: &CaretConfig,
    x: f32,
    baseline: f32,
    font: &Font,
    text_paint: &Paint,
    time: f64,
) {
    // A blink is a square wave over one full period, so `blink: 1.0` reads as
    // "on for half a second, off for half a second" rather than as a rate
    // nobody can predict from the number.
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
        // Proportional to the type size: a 3px rule that reads as a caret at
        // 24px is a hairline at 120px.
        CaretShape::Line => ((size * 0.07).max(1.5), size * 0.05),
        // Roughly one character cell, the terminal look.
        CaretShape::Block => (size * 0.55, size * 0.04),
    };

    let mut paint = match &cfg.color {
        Some(hex) => paint_from_hex(hex),
        None => text_paint.clone(),
    };
    paint.set_style(PaintStyle::Fill);
    paint.set_anti_alias(true);
    // A caret is a solid mark, not a ghost: it must not inherit a stroke or
    // image filter the text set up for itself.
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
        // `text-autofit`'s height-fit target: the box's own content-box
        // height, exactly as taffy resolved it for this frame's layout —
        // `None` when it isn't a positive, finite number (an intrinsically-
        // sized box that grew to fit its content, i.e. nothing to shrink
        // for on this axis; see `CssStyle::text_autofit`'s doc comment).
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
        AnimationEffect, CharAnimationTiming, EasingType, TextAnimDirection,
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
        text.paint(canvas, 80.0, None, 0.0, &props, &ctx)
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

    // ─── Lot B, wave S: relative `font-size` units ─────────────────────────

    #[test]
    fn rem_font_size_paints_visible_ink() {
        // Reproduction: `font-size: "2rem"` used to resolve to 0px (the
        // context-free `font_size_px_or` cannot resolve `rem`), so
        // `TextIntrinsic` measured a 0-height box and `paint_pass`'s
        // `height <= 0.0` guard skipped painting this node entirely —
        // `validate` reported success with only a warning.
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

        // 2rem against the 16px CSS root default = 32px — comfortably tall
        // enough to show up in the first 60 rows.
        assert!(
            has_ink_in(&grid, W, 0, W, 0, 60),
            "font-size: 2rem must paint visible ink (32px glyphs), got none"
        );
    }

    #[test]
    fn vh_font_size_paints_visible_ink_scaled_to_the_real_viewport() {
        // `vh` needs the real per-frame viewport (`ctx.video_height`), not
        // just a fixed root size — a different resolution path from `rem`.
        // `test_ctx()` sets `video_height: 200`, so `20vh` = 40px.
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

    /// Resolve `style.animation` the way the engine does before it paints, so
    /// a char-animation test exercises the real wiring
    /// (`extract_effects` → `props.char_animation`) instead of a
    /// painter-private lookup.
    fn props_for(text: &Text) -> AnimatedProperties {
        AnimatedProperties {
            char_animation: rustmotion_core::engine::animator::extract_effects(
                &text.style.animation,
            )
            .char_animation,
            ..Default::default()
        }
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
            blur: Some(16.0),
            ..Default::default()
        })];

        const W: i32 = 900;
        const H: i32 = 180;
        let ctx = test_ctx();
        let props = props_for(&text);
        let font = inter_font(FONT_PX);
        let word1_end = measure_text_with_fallback("ONE", &font, &None, 0.0) as i32;

        // Before `delay`, nothing should paint at all.
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

        // t=0.65s: word 1's local progress is (0.65-0.5)/0.3 = 0.5 (mid
        // reveal), word 2 only starts at delay+stagger=1.1s.
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
        // Leave enough clearance past word 1's sharp-edge measurement for
        // its *own* Gaussian halo (a real, expected effect of blurring
        // that word — see the analogous note in the whitespace-gap test
        // above), so this only catches an actual word-2 leak.
        assert!(
            !has_ink_in(&mid_grid, W, word1_end + 70, W, 0, H),
            "word 2 (starts at delay+stagger=1.1s) must still be fully invisible at t=0.65"
        );
    }

    // ─── text-autofit ───────────────────────────────────────────────────

    fn autofit_text(content: &str, font_size: f32, white_space: Option<CssWhiteSpace>) -> Text {
        let mut t = make_text(content, white_space);
        t.style.font_size = Some(Length::Px(font_size));
        t.style.text_autofit = Some(true);
        t
    }

    /// Rightmost painted column across the whole surface — the horizontal
    /// extent of whatever ink was actually drawn.
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

    /// Bottommost painted row across the whole surface — the vertical
    /// extent of whatever ink was actually drawn.
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
        // Trap #1 — the one the brief calls the only one that can ruin this
        // work: `TextIntrinsic::measure` and `Text::paint` must resolve to
        // the *same* font size for the same node, or the box the layout
        // engine reserves stops matching what actually gets painted. Both
        // delegate to `resolve_text_autofit` with identical inputs (see its
        // doc comment); this proves that agreement operationally, on the
        // real render path, not by re-deriving the expected size by hand
        // (which would only test this test's own arithmetic).
        let text = autofit_text(
            "the quick brown fox jumps over the lazy dog",
            90.0,
            Some(CssWhiteSpace::Nowrap),
        );
        // 300px comfortably clears this sentence's floor-fit width (~252px
        // — this string never reads shorter than the calibrated legibility
        // floor allows), so the box is reachable by shrinking alone,
        // distinct from the separate floor-behaviour tests in
        // `intrinsic.rs`.
        const BOX_W: f32 = 300.0;
        const BOX_H: f32 = 60.0;

        let (measured_w, _measured_h) = TextIntrinsic::from_text(&text).measure(
            (None, None),
            (
                AvailableSpace::Definite(BOX_W),
                AvailableSpace::Definite(BOX_H),
            ),
        );
        // Sanity: at 90px this line would never fit a 300px box unshrunk —
        // proves the shrink path is actually exercised here.
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
        // Same agreement proof as above, on the height axis with wrapping
        // on: a paragraph whose box has an explicit height too short for
        // its natural (unshrunk) line count.
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
        // Trap #2: nothing in the resolution may depend on `ctx.time` for
        // fixed content — rendering it at two different times, same box,
        // must be byte-identical.
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
        // Trap #2's named example: a typewriter reveal
        // (`visible_chars_progress`) must not make the resolved font size
        // drift as more characters become visible — `resolve_text_autofit`
        // is always fed the full, untruncated content, never the
        // reveal-in-progress view (see its doc comment). Proof: the line's
        // vertical footprint (driven by line-height, hence font size) must
        // be identical at 30% and 100% reveal, even though the horizontal
        // extent legitimately differs (fewer glyphs are visible yet).
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

        // And the visible width at 30% must be meaningfully narrower than
        // the full line — otherwise this test would not actually be
        // exercising a partial reveal at all.
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
        // Backward compatibility: a scenario that does not declare
        // `text-autofit` must render exactly as it did before this feature
        // existed, even now that `content_height` is threaded through —
        // the render-level twin of
        // `intrinsic::tests::text_intrinsic_ignores_autofit_target_when_the_flag_is_off`.
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

    // ─── Text state swap ──────────────────────────────────────────────────────

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
        // `states` on its own is a hard cut: abrupt, but it is exactly what
        // omitting `swap` asks for, and it must not silently animate.
        let text = swapping_text(None);
        let before = render_plain(&text, 0.5);
        let after = render_plain(&text, 1.5);

        let width_of = |g: &[u8]| max_ink_x(g, CARET_W, CARET_H).unwrap_or(0);
        assert!(
            width_of(&before) > width_of(&after) + 20,
            "\"Saving draft\" should be visibly wider than \"Saved\" — the label must actually \
             have changed at t=1.0"
        );
        // A cut has exactly one label on screen at each instant, so the frame
        // right after the boundary equals the settled one.
        assert_eq!(
            render_plain(&text, 1.01),
            render_plain(&text, 1.5),
            "without a `swap`, the new label must be fully in place immediately"
        );
    }

    #[test]
    fn a_swap_puts_both_labels_on_screen_at_once() {
        let text = swapping_text(Some(TextSwapConfig::default()));

        // Just before, only the outgoing label; mid-window, both; well after,
        // only the incoming one. "Both" shows up as ink covering more rows
        // than either label alone occupies, since they are offset vertically.
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
        // Past `at + duration` the animated version must be indistinguishable
        // from a plain cut — no residual offset, blur or ghost of the old
        // label parked behind the new one.
        assert_eq!(
            render_plain(&swapped, 2.0),
            render_plain(&cut, 2.0),
            "once the swap window has passed, the frame must match a plain cut exactly"
        );
    }

    #[test]
    fn the_box_is_measured_for_the_widest_label_not_the_first() {
        // A box sized for "Saved" would be overrun the instant the text
        // swapped back to "Saving draft" — and the geometry validator, which
        // measures through this same intrinsic, would have signed off on it.
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

    // ─── Typewriter caret ─────────────────────────────────────────────────────

    const CARET_W: i32 = 900;
    const CARET_H: i32 = 160;

    fn typewriter_text(caret: Option<CaretConfig>) -> Text {
        let mut text = make_text("HELLO WORLD", Some(CssWhiteSpace::Nowrap));
        text.style.font_size = Some(Length::Px(64.0));
        text.caret = caret;
        text
    }

    /// Render a `visible_chars_progress` reveal at `progress`, at `time`.
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
        // The whole reason this is a field on `text` rather than a separate
        // `cursor` component placed next to it: a hand-placed caret stays
        // put while the text grows out from under it.
        let text = typewriter_text(Some(CaretConfig {
            blink: 0.0,
            ..Default::default()
        }));
        let plain = typewriter_text(None);

        // Rightmost ink, with and without the caret: the difference is the
        // caret's own position.
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

        // First half of the period: caret visible, so ink extends past the
        // text. Second half: it must be gone, i.e. back to the text's own
        // right edge.
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
        // At 0% reveal there is no text yet, but a typewriter that starts
        // with a blank frame and pops both caret and first letter together
        // reads as a glitch rather than as typing.
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

    // ─── Char-animation tuning (direction / distance / scale_from / ink_from) ──

    const TUNING_W: i32 = 520;
    const TUNING_H: i32 = 360;

    /// A single 90px word carrying `timing` as a `char_slide_up` effect.
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

    /// Topmost inked row, i.e. how high on the canvas the glyphs sit.
    fn min_ink_y(grid: &[u8], surface_width: i32, height: i32) -> Option<i32> {
        (0..height)
            .find(|&y| (0..surface_width).any(|x| grid[(y * surface_width + x) as usize] > 0))
    }

    #[test]
    fn direction_down_starts_the_unit_above_its_line_instead_of_below() {
        // `char_slide_up` used to hardcode a downward starting offset. With
        // `direction: "down"` the same preset has to start *above* the line
        // and fall — the "letters cascading from the top" look. Sampled
        // mid-travel, where the two are furthest apart.
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

        // Same instant, same preset: the only difference is how far each has
        // left to travel, which shows up as how far below its line it sits.
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
        // Whatever route it took, a unit's resting position is its laid-out
        // one — otherwise the tuning knobs would silently move the finished
        // frame, which is the frame that has to match the layout.
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
            // Isolate the scale: no travel to move the ink around.
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
        // `char_scale_in` is the one preset that leaves alpha alone, so the
        // measurement reads the colour ramp instead of a fade.
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
            // Pure red start → the green channel is the whole measurement.
            ink_from: Some("#FF0000".into()),
            ..Default::default()
        })];

        // Mean green over inked pixels: 0 at pure red, 255 once white.
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
            // Weight by alpha rather than requiring `alpha == 255`: the fill
            // colour is uniform across a unit regardless of AA coverage, so
            // this reads the same value it would with an opaque-only filter
            // — but it stays correct once `char_scale_in` has shrunk the
            // glyph enough (early in its ramp) that no single pixel is fully
            // covered.
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
}
