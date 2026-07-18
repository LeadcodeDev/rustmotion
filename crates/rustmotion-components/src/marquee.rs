use rustmotion_core::css::CssStyle;
use rustmotion_core::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Rect};

use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, measure_text_with_fallback, paint_from_hex,
    typeface_with_fallback,
};
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

fn default_speed() -> f32 {
    100.0
}

fn default_font_size() -> f32 {
    24.0
}

fn default_color() -> String {
    "#FFFFFF".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum MarqueeDirection {
    #[default]
    Left,
    Right,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Marquee {
    pub content: String,
    #[serde(default = "default_speed")]
    pub speed: f32,
    #[serde(default)]
    pub direction: MarqueeDirection,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default)]
    pub separator: Option<String>,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(Marquee {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Marquee {
    fn paint(&self, canvas: &Canvas, layout_w: f32, layout_h: f32, time: f64) -> Result<()> {
        let w = layout_w;
        let h = layout_h;

        let fs = self.style.font_size_px_or(self.font_size);
        let font_style = skia_safe::FontStyle::normal();
        let family = self.style.font_family_or("Inter");
        let typeface = typeface_with_fallback(family, font_style)?;
        let font = skia_safe::Font::from_typeface(typeface, fs);
        let emoji_font = emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, fs));

        let separator = self.separator.as_deref().unwrap_or("     ");
        let full_text = format!("{}{}", self.content, separator);
        let text_w = measure_text_with_fallback(&full_text, &font, &emoji_font, 0.0);

        if text_w < 1.0 {
            return Ok(());
        }

        let color = self.style.color_str().unwrap_or(&self.color);
        let mut text_paint = paint_from_hex(color);
        text_paint.set_anti_alias(true);

        let (_, metrics) = font.metrics();
        let ascent = -metrics.ascent;
        let text_y = (h + ascent) / 2.0;

        // Calculate offset based on time and direction
        let offset = (time as f32 * self.speed) % text_w;
        let start_x = match self.direction {
            MarqueeDirection::Left => -offset,
            MarqueeDirection::Right => offset - text_w,
        };

        // Clip to bounds
        canvas.save();
        canvas.clip_rect(
            Rect::from_xywh(0.0, 0.0, w, h),
            skia_safe::ClipOp::Intersect,
            false,
        );

        // Draw enough copies to fill the width
        let copies = ((w / text_w).ceil() as i32 + 2).max(2);
        for i in 0..copies {
            let x = start_x + i as f32 * text_w;
            if x > w {
                break;
            }
            if x + text_w < 0.0 {
                continue;
            }
            draw_text_with_fallback(
                canvas,
                &full_text,
                &font,
                &emoji_font,
                0.0,
                x,
                text_y,
                &text_paint,
            );
        }

        canvas.restore();
        Ok(())
    }
}

impl Painter for Marquee {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        let _ = self.paint(canvas, layout.width, layout.height, ctx.time);
    }
}
