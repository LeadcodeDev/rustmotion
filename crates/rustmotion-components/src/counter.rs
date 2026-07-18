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

        let start = self.timing.start_at.unwrap_or(0.0);
        let elapsed = (time - start).max(0.0);
        let remaining_duration = scene_duration - start;
        let t = if remaining_duration > 0.0 {
            (elapsed / remaining_duration).clamp(0.0, 1.0)
        } else {
            1.0
        };

        let progress = ease(t, &self.easing);
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

        let letter_spacing = self.style.letter_spacing_px();

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
