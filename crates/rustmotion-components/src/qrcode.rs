use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, Rect};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::color4f_from_hex;
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

fn default_qr_size() -> f32 {
    200.0
}
fn default_qr_fg() -> String {
    "#000000".to_string()
}
fn default_qr_bg() -> String {
    "#FFFFFF".to_string()
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct QrCode {
    pub content: String,
    #[serde(default = "default_qr_size")]
    pub size: f32,
    #[serde(default = "default_qr_fg")]
    pub foreground_color: String,
    #[serde(default = "default_qr_bg")]
    pub background_color: String,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(QrCode {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Painter for QrCode {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &AnimatedProperties,
        _ctx: &PaintCtx,
    ) {
        use qrcode::QrCode as QrCodeLib;

        let Ok(code) = QrCodeLib::new(self.content.as_bytes()) else {
            return;
        };

        let modules = code.to_colors();
        let module_count = code.width() as f32;
        let w = layout.width;
        let h = layout.height;
        const ISO_18004_QUIET_ZONE_MODULES: f32 = 4.0;
        let module_size_x = w / (module_count + ISO_18004_QUIET_ZONE_MODULES * 2.0);
        let module_size_y = h / (module_count + ISO_18004_QUIET_ZONE_MODULES * 2.0);
        let offset_x = ISO_18004_QUIET_ZONE_MODULES * module_size_x;
        let offset_y = ISO_18004_QUIET_ZONE_MODULES * module_size_y;

        let mut bg_paint = skia_safe::Paint::new(color4f_from_hex(&self.background_color), None);
        bg_paint.set_style(PaintStyle::Fill);
        canvas.draw_rect(Rect::from_xywh(0.0, 0.0, w, h), &bg_paint);

        let mut fg_paint = skia_safe::Paint::new(color4f_from_hex(&self.foreground_color), None);
        fg_paint.set_style(PaintStyle::Fill);
        fg_paint.set_anti_alias(false);

        for (idx, &color) in modules.iter().enumerate() {
            if color == qrcode::Color::Dark {
                let col = (idx % code.width()) as f32;
                let row = (idx / code.width()) as f32;
                let rect = Rect::from_xywh(
                    offset_x + col * module_size_x,
                    offset_y + row * module_size_y,
                    module_size_x,
                    module_size_y,
                );
                canvas.draw_rect(rect, &fg_paint);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmotion_core::css::CssStyle;

    fn qr(content: &str, size: f32) -> QrCode {
        QrCode {
            content: content.to_string(),
            size,
            foreground_color: default_qr_fg(),
            background_color: default_qr_bg(),
            timing: Default::default(),
            style: CssStyle::default(),
            timeline: Vec::new(),
            stagger: None,
        }
    }

    fn ink_max_xy(surface: &mut skia_safe::Surface, w: i32, h: i32) -> (i32, i32) {
        let snapshot = surface.image_snapshot();
        let info = skia_safe::ImageInfo::new(
            (w, h),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let ok = snapshot.read_pixels(
            &info,
            &mut buf,
            (w * 4) as usize,
            skia_safe::IPoint::new(0, 0),
            skia_safe::image::CachingHint::Disallow,
        );
        assert!(ok, "pixel read should succeed");
        let (mut maxx, mut maxy) = (0, 0);
        for y in 0..h {
            for x in 0..w {
                if buf[((y * w + x) * 4 + 3) as usize] > 0 {
                    maxx = maxx.max(x);
                    maxy = maxy.max(y);
                }
            }
        }
        (maxx, maxy)
    }

    fn pixel_at(surface: &mut skia_safe::Surface, w: i32, x: i32, y: i32) -> [u8; 4] {
        let snapshot = surface.image_snapshot();
        let info = skia_safe::ImageInfo::new(
            (w, surface.height()),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut buf = vec![0u8; (w * surface.height() * 4) as usize];
        let ok = snapshot.read_pixels(
            &info,
            &mut buf,
            (w * 4) as usize,
            skia_safe::IPoint::new(0, 0),
            skia_safe::image::CachingHint::Disallow,
        );
        assert!(ok, "pixel read should succeed");
        let i = ((y * w + x) * 4) as usize;
        buf[i..i + 4].try_into().expect("pixel")
    }

    #[test]
    fn a_quiet_zone_margin_surrounds_the_code() {
        let component = qr("https://example.com", 200.0);

        const W: i32 = 200;
        const H: i32 = 200;
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        {
            let canvas = surface.canvas();
            let layout = BoxLayout {
                x: 0.0,
                y: 0.0,
                width: W as f32,
                height: H as f32,
                ..Default::default()
            };
            component.paint_content(
                canvas,
                &layout,
                &AnimatedProperties::default(),
                &PaintCtx {
                    time: 0.0,
                    scenario_time: 0.0,
                    scene_duration: 1.0,
                    frame_index: 0,
                    fps: 30,
                    video_width: W as u32,
                    video_height: H as u32,
                    stagger_offset: 0.0,
                },
            );
        }

        let px = pixel_at(&mut surface, W, 2, 2);
        assert_eq!(
            px,
            [255, 255, 255, 255],
            "expected background-colored quiet zone at (2,2), got {px:?} — the finder pattern \
             must not border the scene directly"
        );
    }

    #[test]
    fn paints_within_the_layout_box_not_its_own_size_field() {
        let code = qr("https://example.com", 300.0);
        const W: i32 = 200;
        const H: i32 = 200;
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        {
            let canvas = surface.canvas();
            let layout = BoxLayout {
                x: 0.0,
                y: 0.0,
                width: 60.0,
                height: 60.0,
                ..Default::default()
            };
            code.paint_content(
                canvas,
                &layout,
                &AnimatedProperties::default(),
                &PaintCtx {
                    time: 0.0,
                    scenario_time: 0.0,
                    scene_duration: 1.0,
                    frame_index: 0,
                    fps: 30,
                    video_width: 200,
                    video_height: 200,
                    stagger_offset: 0.0,
                },
            );
        }
        let (maxx, maxy) = ink_max_xy(&mut surface, W, H);
        assert!(
            maxx < 60 && maxy < 60,
            "expected the code to stay within the 60x60 layout box, got ink up to ({maxx}, {maxy})"
        );
    }
}
