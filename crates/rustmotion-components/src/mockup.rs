use rustmotion_core::css::CssStyle;
use rustmotion_core::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Paint, PaintStyle, Path, RRect, Rect};

use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{asset_cache, paint_from_hex};
use rustmotion_core::error::RustmotionError;
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MockupDevice {
    Iphone,
    Android,
    Laptop,
    Browser,
}

impl Default for MockupDevice {
    fn default() -> Self {
        Self::Iphone
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MockupTheme {
    Dark,
    Light,
}

impl Default for MockupTheme {
    fn default() -> Self {
        Self::Dark
    }
}

impl MockupTheme {
    fn bezel_color(&self) -> &str {
        match self {
            MockupTheme::Dark => "#1A1A1A",
            MockupTheme::Light => "#E5E7EB",
        }
    }

    fn screen_bg(&self) -> &str {
        match self {
            MockupTheme::Dark => "#000000",
            MockupTheme::Light => "#FFFFFF",
        }
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Mockup {
    pub device: MockupDevice,
    pub src: String,
    #[serde(default)]
    pub theme: MockupTheme,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(Mockup {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

struct DeviceMetrics {
    corner_radius: f32,
    bezel_top: f32,
    bezel_bottom: f32,
    bezel_side: f32,
}

impl MockupDevice {
    fn metrics(&self) -> DeviceMetrics {
        match self {
            MockupDevice::Iphone => DeviceMetrics {
                corner_radius: 40.0,
                bezel_top: 44.0,
                bezel_bottom: 34.0,
                bezel_side: 12.0,
            },
            MockupDevice::Android => DeviceMetrics {
                corner_radius: 35.0,
                bezel_top: 32.0,
                bezel_bottom: 24.0,
                bezel_side: 10.0,
            },
            MockupDevice::Laptop => DeviceMetrics {
                corner_radius: 12.0,
                bezel_top: 32.0,
                bezel_bottom: 32.0,
                bezel_side: 16.0,
            },
            MockupDevice::Browser => DeviceMetrics {
                corner_radius: 10.0,
                bezel_top: 40.0,
                bezel_bottom: 0.0,
                bezel_side: 0.0,
            },
        }
    }
}

impl Mockup {
    fn load_content_image(&self) -> Result<skia_safe::Image> {
        let cache = asset_cache();
        if let Some(cached) = cache.get(&self.src) {
            return Ok(cached.clone());
        }

        let data = std::fs::read(&self.src).map_err(|e| RustmotionError::ImageLoad {
            path: self.src.clone(),
            reason: e.to_string(),
        })?;
        let skia_data = skia_safe::Data::new_copy(&data);
        let decoded = skia_safe::Image::from_encoded(skia_data).ok_or_else(|| {
            RustmotionError::ImageDecode {
                path: self.src.clone(),
            }
        })?;
        cache.insert(self.src.clone(), decoded.clone());
        Ok(decoded)
    }

    fn draw_content_image(&self, canvas: &Canvas, screen_rect: Rect) -> Result<()> {
        let img = self.load_content_image()?;
        let img_w = img.width() as f32;
        let img_h = img.height() as f32;
        let sw = screen_rect.width();
        let sh = screen_rect.height();

        // Cover fit
        let scale = (sw / img_w).max(sh / img_h);
        let draw_w = img_w * scale;
        let draw_h = img_h * scale;
        let offset_x = screen_rect.left + (sw - draw_w) / 2.0;
        let offset_y = screen_rect.top + (sh - draw_h) / 2.0;

        canvas.save();
        canvas.clip_rect(screen_rect, skia_safe::ClipOp::Intersect, true);
        let dst = Rect::from_xywh(offset_x, offset_y, draw_w, draw_h);
        canvas.draw_image_rect(img, None, dst, &Paint::default());
        canvas.restore();

        Ok(())
    }

    fn render_phone(&self, canvas: &Canvas, w: f32, h: f32, m: &DeviceMetrics) -> Result<()> {
        // Device body
        let body_rect = Rect::from_xywh(0.0, 0.0, w, h);
        let body_rrect = RRect::new_rect_xy(body_rect, m.corner_radius, m.corner_radius);
        let mut body_paint = paint_from_hex(self.theme.bezel_color());
        body_paint.set_style(PaintStyle::Fill);
        body_paint.set_anti_alias(true);
        canvas.draw_rrect(body_rrect, &body_paint);

        // Screen area
        let screen_rect = Rect::from_xywh(
            m.bezel_side,
            m.bezel_top,
            w - m.bezel_side * 2.0,
            h - m.bezel_top - m.bezel_bottom,
        );

        // Screen background
        let mut screen_bg = paint_from_hex(self.theme.screen_bg());
        screen_bg.set_style(PaintStyle::Fill);
        let screen_radius = m.corner_radius - m.bezel_side;
        let screen_rrect =
            RRect::new_rect_xy(screen_rect, screen_radius.max(0.0), screen_radius.max(0.0));
        canvas.draw_rrect(screen_rrect, &screen_bg);

        // Content image
        canvas.save();
        canvas.clip_rrect(screen_rrect, skia_safe::ClipOp::Intersect, true);
        self.draw_content_image(canvas, screen_rect)?;
        canvas.restore();

        // iPhone notch
        if matches!(self.device, MockupDevice::Iphone) {
            let notch_w = w * 0.35;
            let notch_h = 28.0;
            let notch_x = (w - notch_w) / 2.0;
            let notch_r = 12.0;

            let mut notch_paint = paint_from_hex(self.theme.bezel_color());
            notch_paint.set_style(PaintStyle::Fill);
            notch_paint.set_anti_alias(true);

            let notch_rect = Rect::from_xywh(notch_x, 0.0, notch_w, notch_h);
            let notch_rrect = RRect::new_rect_radii(
                notch_rect,
                &[
                    (0.0, 0.0).into(),
                    (0.0, 0.0).into(),
                    (notch_r, notch_r).into(),
                    (notch_r, notch_r).into(),
                ],
            );
            canvas.draw_rrect(notch_rrect, &notch_paint);
        }

        // Android punch-hole camera
        if matches!(self.device, MockupDevice::Android) {
            let mut cam_paint = paint_from_hex(self.theme.bezel_color());
            cam_paint.set_style(PaintStyle::Fill);
            cam_paint.set_anti_alias(true);
            canvas.draw_circle((w / 2.0, m.bezel_top / 2.0), 5.0, &cam_paint);
        }

        // Home indicator (iPhone)
        if matches!(self.device, MockupDevice::Iphone) {
            let indicator_w = w * 0.35;
            let indicator_h = 5.0;
            let indicator_x = (w - indicator_w) / 2.0;
            let indicator_y = h - m.bezel_bottom / 2.0 - indicator_h / 2.0;

            let mut indicator_paint = paint_from_hex("#888888");
            indicator_paint.set_style(PaintStyle::Fill);
            indicator_paint.set_anti_alias(true);

            let indicator_rect =
                Rect::from_xywh(indicator_x, indicator_y, indicator_w, indicator_h);
            let indicator_rrect =
                RRect::new_rect_xy(indicator_rect, indicator_h / 2.0, indicator_h / 2.0);
            canvas.draw_rrect(indicator_rrect, &indicator_paint);
        }

        Ok(())
    }

    fn render_laptop(&self, canvas: &Canvas, w: f32, h: f32, m: &DeviceMetrics) -> Result<()> {
        let screen_h = h * 0.85;
        let _base_h = h - screen_h;

        // Screen lid
        let lid_rect = Rect::from_xywh(0.0, 0.0, w, screen_h);
        let lid_rrect = RRect::new_rect_xy(lid_rect, m.corner_radius, m.corner_radius);
        let mut lid_paint = paint_from_hex(self.theme.bezel_color());
        lid_paint.set_style(PaintStyle::Fill);
        lid_paint.set_anti_alias(true);
        canvas.draw_rrect(lid_rrect, &lid_paint);

        // Screen area
        let screen_rect = Rect::from_xywh(
            m.bezel_side,
            m.bezel_top,
            w - m.bezel_side * 2.0,
            screen_h - m.bezel_top - 8.0,
        );

        let mut screen_bg = paint_from_hex(self.theme.screen_bg());
        screen_bg.set_style(PaintStyle::Fill);
        canvas.draw_rect(screen_rect, &screen_bg);

        // Content
        self.draw_content_image(canvas, screen_rect)?;

        // Camera dot
        let mut cam_paint = paint_from_hex("#333333");
        cam_paint.set_anti_alias(true);
        canvas.draw_circle((w / 2.0, m.bezel_top / 2.0), 3.0, &cam_paint);

        // Base (trapezoid)
        let base_y = screen_h;
        let inset = w * 0.05;
        let mut base_path = Path::new();
        base_path.move_to((inset, base_y));
        base_path.line_to((w - inset, base_y));
        base_path.line_to((w + inset, h));
        base_path.line_to((-inset, h));
        base_path.close();

        let mut base_paint = paint_from_hex(self.theme.bezel_color());
        base_paint.set_style(PaintStyle::Fill);
        base_paint.set_anti_alias(true);
        canvas.draw_path(&base_path, &base_paint);

        Ok(())
    }

    fn render_browser(&self, canvas: &Canvas, w: f32, h: f32, m: &DeviceMetrics) -> Result<()> {
        // Window background
        let body_rect = Rect::from_xywh(0.0, 0.0, w, h);
        let body_rrect = RRect::new_rect_xy(body_rect, m.corner_radius, m.corner_radius);
        let mut body_paint = paint_from_hex(self.theme.bezel_color());
        body_paint.set_style(PaintStyle::Fill);
        body_paint.set_anti_alias(true);
        canvas.draw_rrect(body_rrect, &body_paint);

        // Title bar
        let chrome_color = match self.theme {
            MockupTheme::Dark => "#2D2D2D",
            MockupTheme::Light => "#F3F3F3",
        };
        let mut chrome_paint = paint_from_hex(chrome_color);
        chrome_paint.set_style(PaintStyle::Fill);

        canvas.save();
        canvas.clip_rrect(body_rrect, skia_safe::ClipOp::Intersect, true);
        canvas.draw_rect(Rect::from_xywh(0.0, 0.0, w, m.bezel_top), &chrome_paint);
        canvas.restore();

        // Traffic lights
        let dot_colors = ["#FF5F57", "#FEBC2E", "#28C840"];
        let dot_y = m.bezel_top / 2.0;
        for (i, color) in dot_colors.iter().enumerate() {
            let dot_x = 16.0 + i as f32 * 20.0;
            let mut dot_paint = paint_from_hex(color);
            dot_paint.set_style(PaintStyle::Fill);
            dot_paint.set_anti_alias(true);
            canvas.draw_circle((dot_x, dot_y), 6.0, &dot_paint);
        }

        // URL bar
        let url_bar_color = match self.theme {
            MockupTheme::Dark => "#404040",
            MockupTheme::Light => "#FFFFFF",
        };
        let url_bar_x = 80.0;
        let url_bar_w = w - 160.0;
        let url_bar_h = 24.0;
        let url_bar_y = (m.bezel_top - url_bar_h) / 2.0;
        let url_rect = Rect::from_xywh(url_bar_x, url_bar_y, url_bar_w, url_bar_h);
        let url_rrect = RRect::new_rect_xy(url_rect, url_bar_h / 2.0, url_bar_h / 2.0);
        let mut url_paint = paint_from_hex(url_bar_color);
        url_paint.set_style(PaintStyle::Fill);
        url_paint.set_anti_alias(true);
        canvas.draw_rrect(url_rrect, &url_paint);

        // Content area
        let screen_rect = Rect::from_xywh(0.0, m.bezel_top, w, h - m.bezel_top);

        let mut screen_bg = paint_from_hex(self.theme.screen_bg());
        screen_bg.set_style(PaintStyle::Fill);
        canvas.save();
        canvas.clip_rrect(body_rrect, skia_safe::ClipOp::Intersect, true);
        canvas.draw_rect(screen_rect, &screen_bg);
        canvas.restore();

        // Content image
        canvas.save();
        canvas.clip_rrect(body_rrect, skia_safe::ClipOp::Intersect, true);
        self.draw_content_image(canvas, screen_rect)?;
        canvas.restore();

        Ok(())
    }
}

impl Painter for Mockup {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &AnimatedProperties,
        _ctx: &PaintCtx,
    ) {
        let w = layout.width;
        let h = layout.height;
        let m = self.device.metrics();

        let _ = match self.device {
            MockupDevice::Iphone | MockupDevice::Android => self.render_phone(canvas, w, h, &m),
            MockupDevice::Laptop => self.render_laptop(canvas, w, h, &m),
            MockupDevice::Browser => self.render_browser(canvas, w, h, &m),
        };
    }
}
