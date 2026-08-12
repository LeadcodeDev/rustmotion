use rustmotion_core::css::CssStyle;
use rustmotion_core::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, RRect, Rect};

use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    color4f_from_hex, draw_text_with_fallback, emoji_typeface, measure_text_with_fallback,
    paint_from_hex, typeface_with_fallback,
};
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

fn default_progress_width() -> f32 {
    300.0
}
fn default_progress_height() -> f32 {
    20.0
}
fn default_progress_bg() -> String {
    "#333333".to_string()
}
fn default_progress_fill() -> String {
    "#4CAF50".to_string()
}
fn default_track_width() -> f32 {
    8.0
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum ProgressVariant {
    #[default]
    Linear,
    Circular,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Progress {
    #[serde(default)]
    pub progress: f64,
    #[serde(default)]
    pub variant: ProgressVariant,
    #[serde(default = "default_progress_width")]
    pub width: f32,
    #[serde(default = "default_progress_height")]
    pub height: f32,
    #[serde(default = "default_progress_bg")]
    pub background_color: String,
    #[serde(default = "default_progress_fill")]
    pub fill_color: String,
    #[serde(default)]
    pub border_radius: f32,
    #[serde(default = "default_track_width")]
    pub track_width: f32,
    #[serde(default)]
    pub show_value: bool,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(Progress {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Progress {
    fn paint(&self, canvas: &Canvas, w: f32, h: f32) -> Result<()> {
        match self.variant {
            ProgressVariant::Linear => self.render_linear(canvas, w, h),
            ProgressVariant::Circular => self.render_circular(canvas, w, h),
        }
    }
}

impl Painter for Progress {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &AnimatedProperties,
        _ctx: &PaintCtx,
    ) {
        // `self.width`/`self.height` only seed the *intrinsic* size in
        // `box_builder` (promoted to CSS when `style.width`/`style.height`
        // are absent) — the box taffy actually assigns can differ whenever
        // an author sets `style.width`/`style.height` or a flex-grow
        // idiom directly, which `html-css-mental-model.md` recommends.
        // Painting at `self.width`/`self.height` regardless left the fill
        // sized to whichever one happened to be smaller, filling only part
        // of its own box (or overflowing it) instead of the box.
        let _ = self.paint(canvas, layout.width, layout.height);
    }
}

impl Progress {
    fn render_linear(&self, canvas: &Canvas, w: f32, h: f32) -> Result<()> {
        let radius = self.border_radius;
        let progress = self.progress.clamp(0.0, 1.0) as f32;

        // Background
        let mut bg_paint = skia_safe::Paint::new(color4f_from_hex(&self.background_color), None);
        bg_paint.set_style(PaintStyle::Fill);
        bg_paint.set_anti_alias(true);

        let bg_rect = Rect::from_xywh(0.0, 0.0, w, h);
        let bg_rrect = RRect::new_rect_xy(bg_rect, radius, radius);
        canvas.draw_rrect(bg_rrect, &bg_paint);

        // Fill (progress)
        if progress > 0.001 {
            let mut fill_paint = skia_safe::Paint::new(color4f_from_hex(&self.fill_color), None);
            fill_paint.set_style(PaintStyle::Fill);
            fill_paint.set_anti_alias(true);

            let fill_w = w * progress;
            let fill_rect = Rect::from_xywh(0.0, 0.0, fill_w, h);

            canvas.save();
            canvas.clip_rrect(bg_rrect, skia_safe::ClipOp::Intersect, true);
            canvas.draw_rect(fill_rect, &fill_paint);
            canvas.restore();
        }

        Ok(())
    }

    fn render_circular(&self, canvas: &Canvas, w: f32, h: f32) -> Result<()> {
        let progress = self.progress.clamp(0.0, 1.0) as f32;

        let cx = w / 2.0;
        let cy = h / 2.0;
        let radius = (cx.min(cy) - self.track_width / 2.0 - 2.0).max(0.0);
        let oval = Rect::from_xywh(cx - radius, cy - radius, radius * 2.0, radius * 2.0);

        // Track
        let mut track_paint = paint_from_hex(&self.background_color);
        track_paint.set_style(PaintStyle::Stroke);
        track_paint.set_stroke_width(self.track_width);
        track_paint.set_stroke_cap(skia_safe::paint::Cap::Round);
        track_paint.set_anti_alias(true);
        canvas.draw_arc(oval, 0.0, 360.0, false, &track_paint);

        // Fill arc
        if progress > 0.001 {
            let sweep = 360.0 * progress;
            let mut fill_paint = paint_from_hex(&self.fill_color);
            fill_paint.set_style(PaintStyle::Stroke);
            fill_paint.set_stroke_width(self.track_width);
            fill_paint.set_stroke_cap(skia_safe::paint::Cap::Round);
            fill_paint.set_anti_alias(true);
            canvas.draw_arc(oval, -90.0, sweep, false, &fill_paint);
        }

        // Value text
        if self.show_value {
            let text = format!("{}%", (progress * 100.0).round() as i32);
            let font_size = (radius * 0.5).max(10.0);
            let font_style = skia_safe::FontStyle::bold();
            let Ok(typeface) = typeface_with_fallback("Inter", font_style) else {
                return Ok(());
            };
            let font = skia_safe::Font::from_typeface(typeface, font_size);
            let emoji_font =
                emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, font_size));

            let mut text_paint = paint_from_hex(&self.fill_color);
            text_paint.set_anti_alias(true);

            let text_w = measure_text_with_fallback(&text, &font, &emoji_font, 0.0);
            let (_, metrics) = font.metrics();
            let text_x = cx - text_w / 2.0;
            let text_y = cy + (-metrics.ascent) / 2.0;
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

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_progress(variant: ProgressVariant) -> Progress {
        Progress {
            progress: 0.5,
            variant,
            width: default_progress_width(),
            height: default_progress_height(),
            background_color: default_progress_bg(),
            fill_color: default_progress_fill(),
            border_radius: 0.0,
            track_width: default_track_width(),
            show_value: false,
            timing: TimingConfig::default(),
            style: CssStyle::default(),
            timeline: Vec::new(),
            stagger: None,
        }
    }

    fn base_ctx() -> PaintCtx {
        PaintCtx {
            time: 0.0,
            scenario_time: 0.0,
            scene_duration: 2.0,
            frame_index: 0,
            fps: 30,
            video_width: 900,
            video_height: 200,
            stagger_offset: 0.0,
        }
    }

    fn ink_bounds(
        surface: &mut skia_safe::Surface,
        w: i32,
        h: i32,
    ) -> Option<(i32, i32, i32, i32)> {
        let snapshot = surface.image_snapshot();
        let info = skia_safe::ImageInfo::new(
            (w, h),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut buf = vec![0u8; (w * h * 4) as usize];
        snapshot.read_pixels(
            &info,
            &mut buf,
            (w * 4) as usize,
            skia_safe::IPoint::new(0, 0),
            skia_safe::image::CachingHint::Disallow,
        );
        let (mut minx, mut maxx, mut miny, mut maxy) = (i32::MAX, i32::MIN, i32::MAX, i32::MIN);
        for y in 0..h {
            for x in 0..w {
                if buf[((y * w + x) * 4 + 3) as usize] > 0 {
                    minx = minx.min(x);
                    maxx = maxx.max(x);
                    miny = miny.min(y);
                    maxy = maxy.max(y);
                }
            }
        }
        (minx <= maxx).then_some((minx, maxx, miny, maxy))
    }

    #[test]
    fn linear_progress_fills_the_layout_box_not_its_own_width_height() {
        // #4's exact repro: `render_linear` always drew at `self.width` x
        // `self.height` (defaults 300x20) regardless of the box taffy
        // actually assigned it. `box_builder` only promotes `c.width`/
        // `c.height` to CSS when `style.width`/`style.height` are absent —
        // so a `progress` sized via `style.width: 800` (the project's
        // CSS-first idiom) painted a 300px-wide bar sitting inside an
        // 800px-wide box, filling only 37% of it at `progress: 0.5`.
        let progress = base_progress(ProgressVariant::Linear);
        const BOX_W: f32 = 800.0;
        const BOX_H: f32 = 24.0;
        let layout = BoxLayout {
            width: BOX_W,
            height: BOX_H,
            ..Default::default()
        };
        let ctx = base_ctx();
        let props = AnimatedProperties::default();

        let mut surface =
            skia_safe::surfaces::raster_n32_premul((900, 200)).expect("raster surface");
        {
            let canvas = surface.canvas();
            progress.paint_content(canvas, &layout, &props, &ctx);
        }
        let (_minx, maxx, _miny, _maxy) =
            ink_bounds(&mut surface, 900, 200).expect("progress must paint something");
        // At progress 0.5 the fill should reach roughly the middle of the
        // 800px box (~400px), not the middle of the component's own
        // `width` field (300px -> 150px).
        assert!(
            maxx as f32 > BOX_W * 0.4,
            "fill did not scale to the box's own width: max ink x = {maxx}, box width = {BOX_W}"
        );
    }

    #[test]
    fn circular_progress_fits_the_layout_box_not_its_own_width_height() {
        let progress = base_progress(ProgressVariant::Circular);
        const BOX_W: f32 = 60.0;
        const BOX_H: f32 = 60.0;
        let layout = BoxLayout {
            width: BOX_W,
            height: BOX_H,
            ..Default::default()
        };
        let ctx = base_ctx();
        let props = AnimatedProperties::default();

        let mut surface =
            skia_safe::surfaces::raster_n32_premul((300, 300)).expect("raster surface");
        {
            let canvas = surface.canvas();
            progress.paint_content(canvas, &layout, &props, &ctx);
        }
        let (minx, maxx, miny, maxy) =
            ink_bounds(&mut surface, 300, 300).expect("progress must paint something");
        // The ring must be centered on the 60x60 box's own center (30, 30),
        // not on the component's own `width`/`height` fields' center
        // (150, 10 for the 300x20 defaults) — the un-fixed painter puts the
        // whole ring outside a small box entirely.
        let center_x = (minx + maxx) as f32 / 2.0;
        let center_y = (miny + maxy) as f32 / 2.0;
        assert!(
            (center_x - BOX_W / 2.0).abs() < 5.0 && (center_y - BOX_H / 2.0).abs() < 5.0,
            "ring is not centered on the {BOX_W}x{BOX_H} box: center=({center_x}, {center_y})"
        );
    }
}
