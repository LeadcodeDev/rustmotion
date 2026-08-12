use rustmotion_core::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Font, FontStyle, PaintStyle};

use rustmotion_core::css::style::{
    FontStyle as CssFontStyle, FontWeight as CssFontWeight, FontWeightKw, TextAlign as CssTextAlign,
};
use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, format_counter_value, measure_text_with_fallback,
    paint_from_hex, typeface_with_fallback,
};
use rustmotion_core::schema::{
    EasingType, FontStyleType, FontWeight, Stroke, TextAlign, TextShadow, TimelineStep,
};
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Counter {
    pub from: f64,
    pub to: f64,
    #[serde(default)]
    pub decimals: u8,
    #[serde(default)]
    pub separator: Option<String>,
    #[serde(default)]
    pub prefix: Option<String>,
    #[serde(default)]
    pub suffix: Option<String>,
    #[serde(default)]
    pub easing: EasingType,
    /// Seconds the count takes to run, measured from `start_at`.
    ///
    /// Left unset the count stretches over whatever remains of the scene, so it
    /// only reaches `to` on the very last frame and the viewer never gets to
    /// read the figure they were counting towards. Setting this shorter than
    /// the scene makes the count land early and hold.
    #[serde(default)]
    pub duration: Option<f64>,
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
}

rustmotion_core::impl_traits!(Counter {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Counter {
    /// Where the count sits on its 0..1 ramp at `time`, before easing.
    ///
    /// The ramp starts at `start_at` and runs for `duration`, falling back to
    /// the rest of the scene when no duration is given.
    fn ramp_progress(&self, time: f64, scene_duration: f64) -> f64 {
        let start = self.timing.start_at.unwrap_or(0.0);
        let elapsed = (time - start).max(0.0);
        let ramp = match self.duration {
            Some(d) if d > 0.0 => d,
            _ => scene_duration - start,
        };
        if ramp > 0.0 {
            (elapsed / ramp).clamp(0.0, 1.0)
        } else {
            1.0
        }
    }

    fn paint(
        &self,
        canvas: &Canvas,
        layout_width: f32,
        time: f64,
        scene_duration: f64,
        props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) -> Result<()> {
        use rustmotion_core::engine::animator::ease;

        // Lot B (wave S): `font-size` now resolves through the same
        // context-aware machinery `text-shadow` already used below (see
        // `lctx`) — it used to stay on the context-free `font_size_px_or`,
        // silently dropping `rem`/`vw`/`vh` font-size to 0px. `em`/`%` on
        // `font-size` itself remain approximate — see
        // `crate::intrinsic::font_size_ctx`'s doc comment.
        let base_ctx = crate::intrinsic::font_size_ctx(
            ctx.video_width as f32,
            ctx.video_height as f32,
            layout_width.max(0.0),
        );
        let font_size = self.style.font_size_px_ctx(&base_ctx, 48.0);
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

        let progress = ease(self.ramp_progress(time, scene_duration), &self.easing);
        let value = self.from + (self.to - self.from) * progress;
        let content = format_counter_value(
            value,
            self.decimals,
            &self.separator,
            &self.prefix,
            &self.suffix,
        );

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

        // This element's own resolved font-size as the `em`/`%` base for
        // `letter-spacing` and (below) `text-shadow` — same context reused
        // for both instead of each rebuilding an equivalent one.
        let own_ctx = rustmotion_core::css::units::LengthContext {
            font_size,
            ..base_ctx
        };
        let letter_spacing = self.style.letter_spacing_px_ctx(&own_ctx);

        let advance_width =
            measure_text_with_fallback(&content, &font, &emoji_font, letter_spacing);

        // For center/right alignment, anchor positioning on the same `absmax`
        // width that `measure()` reserved. This keeps the right edge (or
        // bounding box midpoint) of the counter stable across frames instead
        // of letting it shift sub-pixel as the digit count changes.
        let stable_width = if matches!(align, TextAlign::Center | TextAlign::Right) {
            let absmax = self.from.abs().max(self.to.abs());
            let signed = if self.from < 0.0 || self.to < 0.0 {
                -absmax
            } else {
                absmax
            };
            let display = format_counter_value(
                signed,
                self.decimals,
                &self.separator,
                &self.prefix,
                &self.suffix,
            );
            measure_text_with_fallback(&display, &font, &emoji_font, letter_spacing)
        } else {
            advance_width
        };

        let raw_x = match align {
            TextAlign::Left => 0.0,
            TextAlign::Center => {
                (layout_width - stable_width) / 2.0 + (stable_width - advance_width) / 2.0
            }
            TextAlign::Right => layout_width - advance_width,
        };
        // Snap to whole pixels to eliminate the sub-pixel jitter that the
        // glyph rasterizer would otherwise introduce on a moving counter.
        let x = raw_x.round();
        let (_, metrics) = font.metrics();
        let line_height = font_size * 1.3;
        let ascent = -metrics.ascent;
        let descent = metrics.descent;
        let y = (line_height + ascent - descent) / 2.0;

        // Draw shadows — component field wins, else the bridged CSS
        // `style.text-shadow` list (reverse order: first shadow on top).
        let shadows: Vec<rustmotion_core::schema::TextShadow> = if let Some(s) = &self.text_shadow {
            vec![s.clone()]
        } else if let Some(list) = &self.style.text_shadow {
            list.iter().map(|s| s.to_schema(&own_ctx)).collect()
        } else {
            Vec::new()
        };
        for shadow in shadows.iter().rev() {
            let mut sp = paint_from_hex(&shadow.color);
            if shadow.blur > 0.01 {
                if let Some(filter) = skia_safe::image_filters::blur(
                    (shadow.blur, shadow.blur),
                    skia_safe::TileMode::Clamp,
                    None,
                    None,
                ) {
                    sp.set_image_filter(filter);
                }
            }
            draw_text_with_fallback(
                canvas,
                &content,
                &font,
                &emoji_font,
                letter_spacing,
                x + shadow.offset_x,
                y + shadow.offset_y,
                &sp,
            );
        }

        // Draw stroke
        if let Some(ref stroke) = self.stroke {
            let mut sp = paint_from_hex(&stroke.color);
            sp.set_style(PaintStyle::Stroke);
            sp.set_stroke_width(stroke.width);
            draw_text_with_fallback(
                canvas,
                &content,
                &font,
                &emoji_font,
                letter_spacing,
                x,
                y,
                &sp,
            );
        }

        draw_text_with_fallback(
            canvas,
            &content,
            &font,
            &emoji_font,
            letter_spacing,
            x,
            y,
            &paint,
        );

        Ok(())
    }
}

impl Painter for Counter {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        let _ = self.paint(
            canvas,
            layout.width,
            ctx.time,
            ctx.scene_duration,
            props,
            ctx,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counter(duration: Option<f64>, start_at: Option<f64>) -> Counter {
        Counter {
            from: 0.0,
            to: 100.0,
            decimals: 0,
            separator: None,
            prefix: None,
            suffix: None,
            easing: EasingType::default(),
            duration,
            timing: TimingConfig {
                start_at,
                end_at: None,
            },
            style: CssStyle::default(),
            timeline: Vec::new(),
            stagger: None,
            text_shadow: None,
            stroke: None,
        }
    }

    #[test]
    fn without_duration_the_count_only_lands_on_the_last_frame() {
        // The behaviour that made counters unreadable: nothing settles early,
        // so the figure is still moving when the scene cuts away.
        let c = counter(None, None);
        assert!(c.ramp_progress(3.9, 4.0) < 1.0);
        assert_eq!(c.ramp_progress(4.0, 4.0), 1.0);
    }

    #[test]
    fn duration_makes_the_count_land_early_and_hold() {
        let c = counter(Some(1.5), None);
        assert_eq!(c.ramp_progress(1.5, 4.0), 1.0);
        // Held for the rest of the scene, which is the point.
        assert_eq!(c.ramp_progress(3.0, 4.0), 1.0);
        assert!((c.ramp_progress(0.75, 4.0) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn duration_is_measured_from_start_at() {
        let c = counter(Some(2.0), Some(1.0));
        assert_eq!(c.ramp_progress(1.0, 6.0), 0.0);
        assert!((c.ramp_progress(2.0, 6.0) - 0.5).abs() < 1e-9);
        assert_eq!(c.ramp_progress(3.0, 6.0), 1.0);
    }

    #[test]
    fn a_duration_outlasting_the_scene_is_honoured_not_clamped() {
        // Deliberate: the author asked for a slow count, and silently speeding
        // it up would be a surprise. It simply never reaches `to`.
        let c = counter(Some(10.0), None);
        assert!(c.ramp_progress(4.0, 4.0) < 0.5);
    }

    #[test]
    fn a_zero_or_negative_duration_falls_back_to_the_scene() {
        let c = counter(Some(0.0), None);
        assert!((c.ramp_progress(2.0, 4.0) - 0.5).abs() < 1e-9);
    }

    // ─── Lot B, wave S: relative `font-size` units ─────────────────────────

    #[test]
    fn rem_font_size_paints_visible_ink() {
        // Reproduction: `font-size: "2rem"` used to resolve to 0px on the
        // context-free `font_size_px_or` path.
        let mut c = counter(None, None);
        c.style.font_size = Some(rustmotion_core::css::Length::String("2rem".into()));
        c.style.color = Some(rustmotion_core::css::style::Color::String("#FFFFFF".into()));

        const W: i32 = 300;
        const H: i32 = 150;
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        let ctx = PaintCtx {
            time: 1.0,
            scenario_time: 1.0,
            scene_duration: 2.0,
            frame_index: 30,
            fps: 30,
            video_width: 300,
            video_height: 150,
            stagger_offset: 0.0,
        };
        let props = AnimatedProperties::default();
        {
            let canvas = surface.canvas();
            c.paint(canvas, 200.0, 1.0, 2.0, &props, &ctx)
                .expect("paint succeeds");
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
        let lit = buf.chunks_exact(4).filter(|p| p[3] > 0).count();
        assert!(
            lit > 20,
            "counter at font-size: 2rem must paint visible ink, got {lit} lit pixels"
        );
    }
}
