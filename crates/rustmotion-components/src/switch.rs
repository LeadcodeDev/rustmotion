use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, RRect, Rect};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, font_mgr, paint_from_hex,
};
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

fn default_switch_width() -> f32 {
    52.0
}
fn default_switch_height() -> f32 {
    28.0
}
fn default_track_color_on() -> String {
    "#4CAF50".to_string()
}
fn default_track_color_off() -> String {
    "#CCCCCC".to_string()
}
fn default_thumb_color() -> String {
    "#FFFFFF".to_string()
}
fn default_transition_duration() -> f64 {
    0.3
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Switch {
    #[serde(default)]
    pub value: bool,
    #[serde(default)]
    pub toggle_at: Option<f64>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default = "default_switch_width")]
    pub width: f32,
    #[serde(default = "default_switch_height")]
    pub height: f32,
    #[serde(default = "default_track_color_on")]
    pub track_color_on: String,
    #[serde(default = "default_track_color_off")]
    pub track_color_off: String,
    #[serde(default = "default_thumb_color")]
    pub thumb_color: String,
    #[serde(default = "default_transition_duration")]
    pub transition_duration: f64,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(Switch {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Switch {
    fn ease_out_cubic(t: f64) -> f64 {
        1.0 - (1.0 - t).powi(3)
    }

    fn current_state_at(&self, time: f64) -> f64 {
        let target = match self.toggle_at {
            Some(toggle_time) if time >= toggle_time => !self.value,
            _ => self.value,
        };

        let target_val = if target { 1.0 } else { 0.0 };

        match self.toggle_at {
            Some(toggle_time) if time >= toggle_time => {
                let elapsed = time - toggle_time;
                let progress = (elapsed / self.transition_duration).clamp(0.0, 1.0);
                let eased = Self::ease_out_cubic(progress);
                let start_val = if self.value { 1.0 } else { 0.0 };
                start_val + (target_val - start_val) * eased
            }
            _ => target_val,
        }
    }
}

impl Switch {
    fn paint(&self, canvas: &Canvas, time: f64) {
        let w = self.width;
        let h = self.height;
        let radius = h / 2.0;
        let state = self.current_state_at(time) as f32;

        // Interpolate track color
        let track_color = if state > 0.5 {
            &self.track_color_on
        } else {
            &self.track_color_off
        };

        // Track
        let mut track_paint = paint_from_hex(track_color);
        track_paint.set_style(PaintStyle::Fill);
        track_paint.set_anti_alias(true);

        let track_rect = Rect::from_xywh(0.0, 0.0, w, h);
        let track_rrect = RRect::new_rect_xy(track_rect, radius, radius);
        canvas.draw_rrect(track_rrect, &track_paint);

        // Thumb
        let thumb_radius = (h - 4.0) / 2.0;
        let thumb_x_min = 2.0 + thumb_radius;
        let thumb_x_max = w - 2.0 - thumb_radius;
        let thumb_cx = thumb_x_min + (thumb_x_max - thumb_x_min) * state;
        let thumb_cy = h / 2.0;

        let mut thumb_paint = paint_from_hex(&self.thumb_color);
        thumb_paint.set_style(PaintStyle::Fill);
        thumb_paint.set_anti_alias(true);
        canvas.draw_circle((thumb_cx, thumb_cy), thumb_radius, &thumb_paint);

        // Label
        if let Some(label) = &self.label {
            let font_size = (h * 0.5).max(12.0);
            let fm = font_mgr();
            let font_style = skia_safe::FontStyle::normal();
            let typeface = fm
                .match_family_style("Inter", font_style)
                .or_else(|| fm.match_family_style("Helvetica", font_style))
                .or_else(|| fm.match_family_style("Arial", font_style))
                .unwrap_or_else(|| fm.legacy_make_typeface(None, font_style).unwrap());
            let font = skia_safe::Font::from_typeface(typeface, font_size);
            let emoji_font =
                emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, font_size));

            let mut text_paint = paint_from_hex("#FFFFFF");
            text_paint.set_anti_alias(true);

            let (_, metrics) = font.metrics();
            let text_x = w + 8.0;
            let text_y = h / 2.0 + (-metrics.ascent) / 2.0;

            draw_text_with_fallback(
                canvas,
                label,
                &font,
                &emoji_font,
                0.0,
                text_x,
                text_y,
                &text_paint,
            );
        }
    }
}

impl Painter for Switch {
    fn paint_content(
        &self,
        canvas: &Canvas,
        _layout: &BoxLayout,
        _props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        self.paint(canvas, ctx.time);
    }
}
