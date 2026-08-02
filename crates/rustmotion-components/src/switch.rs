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

/// `#[serde(from = "SwitchRaw")]`: `paint()` draws the label at
/// `self.width + 8`, entirely outside the track — and, like `slider` and
/// `countdown`, this painter draws from its own fields (`self.width`/
/// `self.height`), never `layout.width`/`layout.height`, so the box
/// `box_builder.rs` assigns (`css.width = Px(c.width)`, no label
/// awareness — see #127) needs to be widened to fit the label, not the
/// paint code changed to fit a box it doesn't consult. Going through a raw
/// shadow struct lets that extra width be computed once (with the same
/// font metrics `paint()` uses) and folded into `style.width` before
/// `box_builder.rs` ever sees this component, without touching that file.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(from = "SwitchRaw")]
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
struct SwitchRaw {
    #[serde(default)]
    value: bool,
    #[serde(default)]
    toggle_at: Option<f64>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default = "default_switch_width")]
    width: f32,
    #[serde(default = "default_switch_height")]
    height: f32,
    #[serde(default = "default_track_color_on")]
    track_color_on: String,
    #[serde(default = "default_track_color_off")]
    track_color_off: String,
    #[serde(default = "default_thumb_color")]
    thumb_color: String,
    #[serde(default = "default_transition_duration")]
    transition_duration: f64,
    #[serde(flatten)]
    timing: TimingConfig,
    #[serde(default)]
    style: CssStyle,
    #[serde(default)]
    timeline: Vec<TimelineStep>,
    #[serde(default)]
    stagger: Option<f32>,
}

impl From<SwitchRaw> for Switch {
    fn from(raw: SwitchRaw) -> Self {
        let mut style = raw.style;
        if style.width.is_none() {
            if let Some(extra) = raw
                .label
                .as_deref()
                .and_then(|label| switch_label_extra_width(label, raw.height))
            {
                style.width = Some(CSize::Length(CLP::Px(raw.width + extra)));
            }
        }
        Switch {
            value: raw.value,
            toggle_at: raw.toggle_at,
            label: raw.label,
            width: raw.width,
            height: raw.height,
            track_color_on: raw.track_color_on,
            track_color_off: raw.track_color_off,
            thumb_color: raw.thumb_color,
            transition_duration: raw.transition_duration,
            timing: raw.timing,
            style,
            timeline: raw.timeline,
            stagger: raw.stagger,
        }
    }
}

/// Extra width (gap + label text) `paint()` needs past the track — mirrors
/// its `font_size = (h*0.5).max(12.0)` / `text_x = w + 8.0` exactly, so the
/// box reserved here always matches what gets drawn. `None` on font-load
/// failure (e.g. a headless test env with no fonts) — `style.width` is then
/// left unset and `box_builder.rs`'s plain `c.width` fallback applies, same
/// as before this fix.
fn switch_label_extra_width(label: &str, height: f32) -> Option<f32> {
    let font_size = (height * 0.5).max(12.0);
    let typeface = typeface_with_fallback("Inter", skia_safe::FontStyle::normal()).ok()?;
    let font = skia_safe::Font::from_typeface(typeface, font_size);
    let emoji_font = emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, font_size));
    let label_w = measure_text_with_fallback(label, &font, &emoji_font, 0.0);
    Some(8.0 + label_w)
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
            let font_style = skia_safe::FontStyle::normal();
            let Ok(typeface) = typeface_with_fallback("Inter", font_style) else {
                return;
            };
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

#[cfg(test)]
mod tests {
    use super::*;
    use rustmotion_core::css::style::Size as SzCheck;

    fn parse(json: &str) -> Switch {
        serde_json::from_str(json).expect("switch should deserialize")
    }

    #[test]
    fn no_label_leaves_style_width_unset() {
        // No extra room needed — box_builder.rs's plain `c.width` fallback
        // should still apply, exactly as before this fix.
        let s = parse(r#"{"type":"switch"}"#);
        assert!(s.style.width.is_none());
    }

    #[test]
    fn label_widens_style_width_past_the_track() {
        // #127: `paint()` draws the label at `self.width + 8`, entirely
        // outside a box sized only for the track (64×34 assigned vs.
        // 155×34 painted in the audit). `style.width` must now reserve at
        // least `width` (the track) plus *some* label room.
        let s = parse(r#"{"type":"switch","label":"Dark mode","width":64,"height":34}"#);
        let SzCheck::Length(rustmotion_core::css::LengthPercentage::Px(w)) =
            s.style.width.expect("label should reserve style.width")
        else {
            panic!("expected an explicit px width");
        };
        assert!(
            w > s.width,
            "reserved width {w} should exceed the bare track width {}",
            s.width
        );
    }

    #[test]
    fn explicit_style_width_is_never_overridden() {
        let s = parse(
            r#"{"type":"switch","label":"Dark mode","width":64,"height":34,"style":{"width":500}}"#,
        );
        let SzCheck::Length(rustmotion_core::css::LengthPercentage::Px(w)) =
            s.style.width.expect("width should still be set")
        else {
            panic!("expected an explicit px width");
        };
        assert_eq!(w, 500.0, "author's explicit style.width must win");
    }
}
