use anyhow::Result;
use skia_safe::{Canvas, PaintStyle, Rect};

use crate::engine::renderer::color4f_from_hex;
use crate::schema::QrCodeLayer;

pub fn render_qr_code(canvas: &Canvas, layer: &QrCodeLayer) -> Result<()> {
    use qrcode::QrCode;

    let code = QrCode::new(layer.content.as_bytes())
        .map_err(|e| anyhow::anyhow!("QR code generation failed: {}", e))?;

    let modules = code.to_colors();
    let module_count = code.width() as f32;
    let module_size = layer.size / module_count;

    let x = layer.position.x;
    let y = layer.position.y;

    // Background
    let mut bg_paint = skia_safe::Paint::new(color4f_from_hex(&layer.background_color), None);
    bg_paint.set_style(PaintStyle::Fill);
    canvas.draw_rect(Rect::from_xywh(x, y, layer.size, layer.size), &bg_paint);

    // Foreground modules
    let mut fg_paint = skia_safe::Paint::new(color4f_from_hex(&layer.foreground_color), None);
    fg_paint.set_style(PaintStyle::Fill);
    fg_paint.set_anti_alias(false); // Sharp edges for QR

    for (idx, &color) in modules.iter().enumerate() {
        if color == qrcode::Color::Dark {
            let col = (idx % code.width()) as f32;
            let row = (idx / code.width()) as f32;
            let rect = Rect::from_xywh(
                x + col * module_size,
                y + row * module_size,
                module_size,
                module_size,
            );
            canvas.draw_rect(rect, &fg_paint);
        }
    }

    Ok(())
}
