use skia_safe::{Canvas, Font, FontStyle, Rect};

use rustmotion_core::engine::renderer::{draw_text_with_fallback, emoji_typeface, font_mgr, measure_text_with_fallback, paint_from_hex};
use crate::components::Codeblock;

pub(super) fn draw_chrome(
    canvas: &Canvas,
    layer: &Codeblock,
    x: f32,
    y: f32,
    width: f32,
    corner_radius: f32,
) {
    let chrome = layer.chrome.as_ref().unwrap();
    let chrome_height = 36.0;

    let bar_color = chrome.color.as_deref().unwrap_or("#343d46");
    let bar_paint = paint_from_hex(bar_color);
    let bar_rect = Rect::from_xywh(x, y, width, chrome_height);
    let radii = [
        skia_safe::Point::new(corner_radius, corner_radius),
        skia_safe::Point::new(corner_radius, corner_radius),
        skia_safe::Point::new(0.0, 0.0),
        skia_safe::Point::new(0.0, 0.0),
    ];
    let rrect = skia_safe::RRect::new_rect_radii(bar_rect, &radii);
    canvas.draw_rrect(rrect, &bar_paint);

    let dot_y = y + chrome_height / 2.0;
    let dot_radius = 6.0;
    let dot_start_x = x + 16.0;
    let dot_spacing = 20.0;
    let dot_colors = ["#FF5F56", "#FFBD2E", "#27C93F"];
    for (i, color) in dot_colors.iter().enumerate() {
        let dot_x = dot_start_x + i as f32 * dot_spacing;
        canvas.draw_circle((dot_x, dot_y), dot_radius, &paint_from_hex(color));
    }

    if let Some(ref title) = chrome.title {
        let fm = font_mgr();
        let typeface = fm
            .match_family_style("Inter", FontStyle::normal())
            .or_else(|| fm.match_family_style("Helvetica", FontStyle::normal()))
            .or_else(|| fm.match_family_style("Arial", FontStyle::normal()))
            .unwrap_or_else(|| {
                fm
                    .match_family_style("sans-serif", FontStyle::normal())
                    .unwrap()
            });
        let title_font = Font::from_typeface(typeface, 13.0);
        let emoji_font = emoji_typeface().map(|tf| Font::from_typeface(tf, 13.0));
        let title_width = measure_text_with_fallback(title, &title_font, &emoji_font, 0.0);
        let title_x = x + width / 2.0 - title_width / 2.0;
        let title_y = dot_y + 4.0;
        let mut title_paint = paint_from_hex("#999999");
        title_paint.set_anti_alias(true);
        draw_text_with_fallback(canvas, title, &title_font, &emoji_font, 0.0, title_x, title_y, &title_paint);
    }
}
