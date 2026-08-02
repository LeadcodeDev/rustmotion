use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, Rect};

use rustmotion_core::css::style::AlignSelf;
use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, measure_text_with_fallback, paint_from_hex,
    typeface_with_fallback,
};
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

fn default_font_size() -> f32 {
    14.0
}

fn default_bg_color() -> String {
    "#1E293B".to_string()
}

fn default_border_color() -> String {
    "#475569".to_string()
}

fn default_text_color() -> String {
    "#E2E8F0".to_string()
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Kbd {
    pub key: String,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default = "default_bg_color")]
    pub background_color: String,
    #[serde(default = "default_border_color")]
    pub border_color: String,
    #[serde(default = "default_text_color")]
    pub text_color: String,
    #[serde(flatten)]
    pub timing: TimingConfig,
    /// `align-self` defaults to `flex-start` (not the flex container's
    /// `stretch`) — a keycap is an atomic chip that must keep its natural
    /// intrinsic width even inside a `flex`/`card` column; without this,
    /// the default cross-axis `stretch` wins over `KbdIntrinsic` and the
    /// key renders as a full-width slab. An author-specified `align-self`
    /// in JSON is always respected (this only fills the gap when absent).
    #[serde(
        default = "default_kbd_style",
        deserialize_with = "deserialize_no_stretch_style"
    )]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

fn default_kbd_style() -> CssStyle {
    CssStyle {
        align_self: Some(AlignSelf::FlexStart),
        ..CssStyle::default()
    }
}

/// Deserializes `style` normally, then defaults `align-self` to
/// `flex-start` when the author didn't set it explicitly — see the doc
/// comment on [`Kbd::style`].
fn deserialize_no_stretch_style<'de, D>(deserializer: D) -> Result<CssStyle, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let mut style = CssStyle::deserialize(deserializer)?;
    if style.align_self.is_none() {
        style.align_self = Some(AlignSelf::FlexStart);
    }
    Ok(style)
}

rustmotion_core::impl_traits!(Kbd {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Kbd {
    fn make_font(&self) -> Option<skia_safe::Font> {
        let fs = self.style.font_size_px_or(self.font_size);
        let font_style = skia_safe::FontStyle::normal();
        let family = self.style.font_family.as_deref().unwrap_or("SF Mono");
        let typeface = typeface_with_fallback(family, font_style).ok()?;
        Some(skia_safe::Font::from_typeface(typeface, fs))
    }
}

impl Kbd {
    fn paint(&self, canvas: &Canvas, layout_w: f32, layout_h: f32) {
        let w = layout_w;
        let h = layout_h;
        let radius = 6.0;

        let bg_color = self
            .style
            .background_color_str()
            .unwrap_or(&self.background_color);

        // Shadow (bottom edge to simulate physical key depth), and the key
        // face sitting on top of it. Both are inset to `h - shadow_h` tall
        // so the "3D lip" the shadow peeks out from under the face stays
        // inside the assigned box — the face used to be drawn full-height
        // with the shadow offset *below* it, bleeding `shadow_h` px past
        // the box bottom every frame.
        let shadow_h = 3.0_f32.min(h);
        let cap_h = (h - shadow_h).max(0.0);
        let shadow_rect = Rect::from_xywh(0.0, shadow_h, w, cap_h);
        let shadow_rrect = skia_safe::RRect::new_rect_xy(shadow_rect, radius, radius);
        let mut shadow_paint = paint_from_hex(&self.border_color);
        shadow_paint.set_style(PaintStyle::Fill);
        shadow_paint.set_anti_alias(true);
        canvas.draw_rrect(shadow_rrect, &shadow_paint);

        // Key face background
        let face_rect = Rect::from_xywh(0.0, 0.0, w, cap_h);
        let face_rrect = skia_safe::RRect::new_rect_xy(face_rect, radius, radius);
        let mut face_paint = paint_from_hex(bg_color);
        face_paint.set_style(PaintStyle::Fill);
        face_paint.set_anti_alias(true);
        canvas.draw_rrect(face_rrect, &face_paint);

        // Border
        let mut border_paint = paint_from_hex(&self.border_color);
        border_paint.set_style(PaintStyle::Stroke);
        border_paint.set_stroke_width(1.0);
        border_paint.set_anti_alias(true);
        canvas.draw_rrect(face_rrect, &border_paint);

        // Text centered
        let Some(font) = self.make_font() else {
            return;
        };
        let fs = self.style.font_size_px_or(self.font_size);
        let emoji_font = emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, fs));

        let text_color = self.style.color_str().unwrap_or(&self.text_color);
        let mut text_paint = paint_from_hex(text_color);
        text_paint.set_anti_alias(true);

        let text_w = measure_text_with_fallback(&self.key, &font, &emoji_font, 0.0);
        let (_, metrics) = font.metrics();
        let text_x = (w - text_w) / 2.0;
        let text_y = (cap_h + (-metrics.ascent)) / 2.0;

        draw_text_with_fallback(
            canvas,
            &self.key,
            &font,
            &emoji_font,
            0.0,
            text_x,
            text_y,
            &text_paint,
        );
    }
}

impl Painter for Kbd {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &AnimatedProperties,
        _ctx: &PaintCtx,
    ) {
        self.paint(canvas, layout.width, layout.height);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Kbd {
        serde_json::from_str(json).expect("kbd should deserialize")
    }

    #[test]
    fn style_defaults_to_flex_start_when_absent() {
        // #127: same fix as `badge` — a keycap must keep its intrinsic
        // width (`KbdIntrinsic`) instead of stretching to the flex
        // container's full cross-axis size.
        let kbd = parse(r#"{"type":"kbd","key":"K"}"#);
        assert_eq!(kbd.style.align_self, Some(AlignSelf::FlexStart));
    }

    #[test]
    fn style_defaults_to_flex_start_with_other_style_keys_present() {
        let kbd = parse(r##"{"type":"kbd","key":"K","style":{"color":"#f00"}}"##);
        assert_eq!(kbd.style.align_self, Some(AlignSelf::FlexStart));
        assert_eq!(kbd.style.color_str(), Some("#f00"));
    }

    #[test]
    fn explicit_align_self_is_respected() {
        let kbd = parse(r#"{"type":"kbd","key":"K","style":{"align-self":"stretch"}}"#);
        assert_eq!(kbd.style.align_self, Some(AlignSelf::Stretch));
    }

    #[test]
    fn shadow_never_extends_past_the_assigned_box() {
        // The shadow "lip" used to be offset 3px *below* a full-height
        // face (`shadow_rect` at y=3 with height=h), so its bottom edge
        // sat at `h + 3` — 3px past whatever box the layout gave this
        // component. `paint()` now insets the face to `h - shadow_h` and
        // draws the shadow directly beneath it, so nothing should paint
        // past `h` any more. Render at a tiny height where a 3px miss
        // would be highly visible relative to the box.
        let kbd = Kbd {
            key: "K".to_string(),
            font_size: default_font_size(),
            background_color: default_bg_color(),
            border_color: default_border_color(),
            text_color: default_text_color(),
            timing: Default::default(),
            style: CssStyle::default(),
            timeline: Vec::new(),
            stagger: None,
        };
        const W: i32 = 100;
        const H: i32 = 20;
        let box_w = 60.0_f32;
        let box_h = 10.0_f32; // shorter than the 3px shadow inset would tolerate if unfixed by a wide margin
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        {
            let canvas = surface.canvas();
            canvas.translate((20.0, 5.0));
            kbd.paint(canvas, box_w, box_h);
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
        // Box bottom edge is at absolute y = 5 (translate) + 10 (box_h) = 15.
        // Nothing painted should reach row 16 or beyond.
        let box_bottom = 15;
        for y in (box_bottom + 1)..H {
            for x in 0..W {
                let a = buf[((y * W + x) * 4 + 3) as usize];
                assert_eq!(
                    a, 0,
                    "kbd painted past its assigned box bottom at ({x},{y}), box_bottom={box_bottom}"
                );
            }
        }
    }
}
