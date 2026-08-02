use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, RRect, Rect};

use rustmotion_core::css::style::Size as CSize;
use rustmotion_core::css::{CssStyle, LengthPercentage as CLP};
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, measure_text_with_fallback, paint_from_hex,
    typeface_with_fallback,
};
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

fn default_slider_value() -> f64 {
    0.5
}
fn default_animation_duration() -> f64 {
    1.0
}
fn default_slider_width() -> f32 {
    300.0
}
fn default_slider_height() -> f32 {
    8.0
}
fn default_track_color() -> String {
    "#333333".to_string()
}
fn default_fill_color() -> String {
    "#3B82F6".to_string()
}
fn default_thumb_size() -> f32 {
    20.0
}
fn default_thumb_color() -> String {
    "#FFFFFF".to_string()
}

/// `#[serde(from = "SliderRaw")]`: like `switch`/`countdown`, `paint()`
/// draws from this component's own fields, never `layout.width`/
/// `layout.height` — box_builder.rs's `css.width/height = Px(c.width /
/// c.height)` only budgets for the thin *track*, not the round thumb
/// (`thumb_size`, typically bigger than the track) that can sit centered
/// on either end, nor the `show_value` "N%" label that floats above it
/// (#127 measured 396×7 assigned vs. 417×31 painted). The raw shadow
/// struct computes the true bounding box once — same font metrics
/// `paint()` uses — and folds it into `style.width`/`style.height` before
/// `box_builder.rs` ever runs, without editing that file. `paint()` itself
/// is updated below to offset its drawing into that box instead of
/// treating local (0,0) as the thumb's top-left.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(from = "SliderRaw")]
pub struct Slider {
    #[serde(default = "default_slider_value")]
    pub value: f64,
    #[serde(default)]
    pub animate_to: Option<f64>,
    #[serde(default)]
    pub animate_at: Option<f64>,
    #[serde(default = "default_animation_duration")]
    pub animation_duration: f64,
    #[serde(default = "default_slider_width")]
    pub width: f32,
    #[serde(default = "default_slider_height")]
    pub height: f32,
    #[serde(default = "default_track_color")]
    pub track_color: String,
    #[serde(default = "default_fill_color")]
    pub fill_color: String,
    #[serde(default = "default_thumb_size")]
    pub thumb_size: f32,
    #[serde(default = "default_thumb_color")]
    pub thumb_color: String,
    #[serde(default)]
    pub show_value: bool,
    #[serde(default)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

#[derive(Debug, Deserialize)]
struct SliderRaw {
    #[serde(default = "default_slider_value")]
    value: f64,
    #[serde(default)]
    animate_to: Option<f64>,
    #[serde(default)]
    animate_at: Option<f64>,
    #[serde(default = "default_animation_duration")]
    animation_duration: f64,
    #[serde(default = "default_slider_width")]
    width: f32,
    #[serde(default = "default_slider_height")]
    height: f32,
    #[serde(default = "default_track_color")]
    track_color: String,
    #[serde(default = "default_fill_color")]
    fill_color: String,
    #[serde(default = "default_thumb_size")]
    thumb_size: f32,
    #[serde(default = "default_thumb_color")]
    thumb_color: String,
    #[serde(default)]
    show_value: bool,
    #[serde(flatten)]
    timing: TimingConfig,
    #[serde(default)]
    style: CssStyle,
    #[serde(default)]
    timeline: Vec<TimelineStep>,
    #[serde(default)]
    stagger: Option<f32>,
}

impl From<SliderRaw> for Slider {
    fn from(raw: SliderRaw) -> Self {
        let thumb_r = raw.thumb_size / 2.0;
        let mut style = raw.style;

        if style.width.is_none() {
            let h_margin = slider_value_label_half_width(raw.thumb_size)
                .unwrap_or(0.0)
                .max(thumb_r);
            style.width = Some(CSize::Length(CLP::Px(raw.width + h_margin * 2.0)));
        }
        if style.height.is_none() {
            let top_margin = if raw.show_value {
                slider_value_label_line_height(raw.thumb_size).unwrap_or(0.0)
            } else {
                0.0
            };
            style.height = Some(CSize::Length(CLP::Px(top_margin + raw.thumb_size)));
        }

        Slider {
            value: raw.value,
            animate_to: raw.animate_to,
            animate_at: raw.animate_at,
            animation_duration: raw.animation_duration,
            width: raw.width,
            height: raw.height,
            track_color: raw.track_color,
            fill_color: raw.fill_color,
            thumb_size: raw.thumb_size,
            thumb_color: raw.thumb_color,
            show_value: raw.show_value,
            timing: raw.timing,
            style,
            timeline: raw.timeline,
            stagger: raw.stagger,
        }
    }
}

/// Half-width of the widest value label ("100%") `paint()` can draw,
/// using its exact `font_size = (thumb_size*0.7).max(12.0)` formula — the
/// thumb can sit centered at either end of the track, so this (or the
/// thumb radius, whichever is larger) is how much horizontal margin the
/// box needs past the track on both sides.
fn slider_value_label_half_width(thumb_size: f32) -> Option<f32> {
    let font_size = (thumb_size * 0.7).max(12.0);
    let typeface = typeface_with_fallback("Inter", skia_safe::FontStyle::normal()).ok()?;
    let font = skia_safe::Font::from_typeface(typeface, font_size);
    let emoji_font = emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, font_size));
    let w = measure_text_with_fallback("100%", &font, &emoji_font, 0.0);
    Some(w / 2.0)
}

/// Vertical room `paint()`'s value label needs above the thumb (ascent +
/// descent + the 4px gap it's drawn with), using the same font size.
fn slider_value_label_line_height(thumb_size: f32) -> Option<f32> {
    let font_size = (thumb_size * 0.7).max(12.0);
    let typeface = typeface_with_fallback("Inter", skia_safe::FontStyle::normal()).ok()?;
    let font = skia_safe::Font::from_typeface(typeface, font_size);
    let (_, metrics) = font.metrics();
    Some(-metrics.ascent + metrics.descent + 4.0)
}

rustmotion_core::impl_traits!(Slider {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Slider {
    fn ease_out_cubic(t: f64) -> f64 {
        1.0 - (1.0 - t).powi(3)
    }

    fn current_value_at(&self, time: f64) -> f64 {
        match (self.animate_to, self.animate_at) {
            (Some(target), Some(start_time)) if time >= start_time => {
                let elapsed = time - start_time;
                let progress = (elapsed / self.animation_duration).clamp(0.0, 1.0);
                let eased = Self::ease_out_cubic(progress);
                self.value + (target - self.value) * eased
            }
            _ => self.value,
        }
    }
}

impl Slider {
    fn paint(&self, canvas: &Canvas, layout: &BoxLayout, time: f64) {
        let w = self.width;
        let h = self.height;
        let thumb_r = self.thumb_size / 2.0;
        let radius = h / 2.0;
        let current = self.current_value_at(time).clamp(0.0, 1.0) as f32;

        // The track used to be drawn flush with local (0,0) — the thumb
        // (radius `thumb_r`, usually bigger than the thin track) and the
        // `show_value` label both extend past that origin on every side
        // (#127 measured a 396×7 assigned box vs. 417×31 painted). Offset
        // everything by the same margins reserved on the `Slider` struct
        // (see its `From<SliderRaw>` impl) so the whole thing — track,
        // thumb at either end, and the widest possible "100%" label — sits
        // inside `[0, layout.width] x [0, layout.height]`.
        let h_margin = slider_value_label_half_width(self.thumb_size)
            .unwrap_or(0.0)
            .max(thumb_r);
        let top_margin = if self.show_value {
            slider_value_label_line_height(self.thumb_size).unwrap_or(0.0)
        } else {
            0.0
        };

        canvas.save();
        if layout.width > 0.0 && layout.height > 0.0 {
            canvas.clip_rect(
                Rect::from_xywh(0.0, 0.0, layout.width, layout.height),
                skia_safe::ClipOp::Intersect,
                true,
            );
        }
        canvas.translate((h_margin, top_margin));

        let track_y = thumb_r - h / 2.0;

        let mut track_paint = paint_from_hex(&self.track_color);
        track_paint.set_style(PaintStyle::Fill);
        track_paint.set_anti_alias(true);

        let track_rect = Rect::from_xywh(0.0, track_y, w, h);
        let track_rrect = RRect::new_rect_xy(track_rect, radius, radius);
        canvas.draw_rrect(track_rrect, &track_paint);

        if current > 0.001 {
            let mut fill_paint = paint_from_hex(&self.fill_color);
            fill_paint.set_style(PaintStyle::Fill);
            fill_paint.set_anti_alias(true);

            let fill_w = w * current;
            let fill_rect = Rect::from_xywh(0.0, track_y, fill_w, h);

            canvas.save();
            canvas.clip_rrect(track_rrect, skia_safe::ClipOp::Intersect, true);
            canvas.draw_rect(fill_rect, &fill_paint);
            canvas.restore();
        }

        let thumb_cx = w * current;
        let thumb_cy = thumb_r;

        let mut thumb_paint = paint_from_hex(&self.thumb_color);
        thumb_paint.set_style(PaintStyle::Fill);
        thumb_paint.set_anti_alias(true);
        canvas.draw_circle((thumb_cx, thumb_cy), thumb_r, &thumb_paint);

        if self.show_value {
            let text = format!("{}%", (current * 100.0).round() as i32);
            let font_size = (self.thumb_size * 0.7).max(12.0);
            let font_style = skia_safe::FontStyle::normal();
            let Ok(typeface) = typeface_with_fallback("Inter", font_style) else {
                canvas.restore();
                return;
            };
            let font = skia_safe::Font::from_typeface(typeface, font_size);
            let emoji_font =
                emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, font_size));

            let mut text_paint = paint_from_hex(&self.fill_color);
            text_paint.set_anti_alias(true);

            let text_w = measure_text_with_fallback(&text, &font, &emoji_font, 0.0);
            let (_, metrics) = font.metrics();
            let text_x = thumb_cx - text_w / 2.0;
            let text_y = thumb_cy - thumb_r - 4.0 - (-metrics.descent);

            draw_text_with_fallback(
                canvas,
                &text,
                &font,
                &emoji_font,
                0.0,
                text_x,
                text_y,
                &text_paint,
            );
        }

        canvas.restore();
    }
}

impl Painter for Slider {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        self.paint(canvas, layout, ctx.time);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Slider {
        serde_json::from_str(json).expect("slider should deserialize")
    }

    fn px_width(s: &Slider) -> f32 {
        let CSize::Length(CLP::Px(w)) = s.style.width.clone().expect("style.width should be set")
        else {
            panic!("expected an explicit px width");
        };
        w
    }

    fn px_height(s: &Slider) -> f32 {
        let CSize::Length(CLP::Px(h)) = s.style.height.clone().expect("style.height should be set")
        else {
            panic!("expected an explicit px height");
        };
        h
    }

    #[test]
    fn box_reserves_room_for_the_thumb_past_the_track() {
        // #127: the thumb (`thumb_size`, usually bigger than the thin
        // track) can be centered at either end of the track, so it always
        // overhangs a box sized only to the track (396×7 assigned vs.
        // 417×31 painted in the audit).
        let s = parse(r#"{"type":"slider","width":396,"height":7,"thumb_size":20}"#);
        let w = px_width(&s);
        assert!(
            w >= s.width + s.thumb_size,
            "reserved width {w} should cover the track ({}) plus at least one full thumb ({})",
            s.width,
            s.thumb_size
        );
        let h = px_height(&s);
        assert!(
            h >= s.thumb_size,
            "reserved height {h} should cover the thumb ({})",
            s.thumb_size
        );
    }

    #[test]
    fn show_value_reserves_extra_height_above_the_thumb() {
        let plain = parse(r#"{"type":"slider","thumb_size":20}"#);
        let with_value = parse(r#"{"type":"slider","thumb_size":20,"show_value":true}"#);
        assert!(
            px_height(&with_value) > px_height(&plain),
            "show_value must reserve extra height for the floating label"
        );
    }

    #[test]
    fn explicit_style_size_is_never_overridden() {
        let s = parse(
            r#"{"type":"slider","thumb_size":20,"show_value":true,"style":{"width":900,"height":90}}"#,
        );
        assert_eq!(px_width(&s), 900.0);
        assert_eq!(px_height(&s), 90.0);
    }
}
