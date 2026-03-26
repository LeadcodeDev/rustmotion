use skia_safe::{Color4f, Paint};

/// Parse a hex color string (#RRGGBB or #RRGGBBAA) into RGBA components
pub fn parse_hex_color(hex: &str) -> (u8, u8, u8, u8) {
    let hex = hex.trim_start_matches('#');
    if hex.len() < 6 {
        return (0, 0, 0, 255);
    }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
    let a = if hex.len() >= 8 {
        u8::from_str_radix(&hex[6..8], 16).unwrap_or(255)
    } else {
        255
    };
    (r, g, b, a)
}

pub fn color4f_from_hex(hex: &str) -> Color4f {
    let (r, g, b, a) = parse_hex_color(hex);
    Color4f::new(
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        a as f32 / 255.0,
    )
}

pub fn paint_from_hex(hex: &str) -> Paint {
    let mut paint = Paint::new(color4f_from_hex(hex), None);
    paint.set_anti_alias(true);
    paint
}
