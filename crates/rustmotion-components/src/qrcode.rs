use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, Rect};

use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::color4f_from_hex;
use rustmotion_core::schema::LayerStyle;
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
    pub style: LayerStyle,
}

rustmotion_core::impl_traits!(QrCode {
    Animatable => style,
    Timed => timing,
    Styled => style,
});

impl Painter for QrCode {
    fn paint_content(
        &self,
        canvas: &Canvas,
        _layout: &BoxLayout,
        _props: &AnimatedProperties,
        _ctx: &PaintCtx,
    ) {
        use qrcode::QrCode as QrCodeLib;

        let Ok(code) = QrCodeLib::new(self.content.as_bytes()) else { return };

        let modules = code.to_colors();
        let module_count = code.width() as f32;
        let module_size = self.size / module_count;

        let mut bg_paint = skia_safe::Paint::new(color4f_from_hex(&self.background_color), None);
        bg_paint.set_style(PaintStyle::Fill);
        canvas.draw_rect(
            Rect::from_xywh(0.0, 0.0, self.size, self.size),
            &bg_paint,
        );

        let mut fg_paint = skia_safe::Paint::new(color4f_from_hex(&self.foreground_color), None);
        fg_paint.set_style(PaintStyle::Fill);
        fg_paint.set_anti_alias(false);

        for (idx, &color) in modules.iter().enumerate() {
            if color == qrcode::Color::Dark {
                let col = (idx % code.width()) as f32;
                let row = (idx / code.width()) as f32;
                let rect = Rect::from_xywh(
                    col * module_size,
                    row * module_size,
                    module_size,
                    module_size,
                );
                canvas.draw_rect(rect, &fg_paint);
            }
        }
    }
}
