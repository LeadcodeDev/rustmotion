use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, Rect, RRect};

use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, font_mgr, measure_text_with_fallback, paint_from_hex,
    parse_hex_color,
};
use rustmotion_core::schema::{LayerStyle, Size};
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

fn default_left_color() -> String {
    "#3B82F6".to_string()
}

fn default_right_color() -> String {
    "#EF4444".to_string()
}

fn default_divider_position() -> f64 {
    0.5
}

fn default_animation_duration() -> f64 {
    2.0
}

fn default_divider_color() -> String {
    "#FFFFFF".to_string()
}

fn default_divider_width() -> f32 {
    3.0
}

fn default_border_radius() -> f32 {
    12.0
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Comparison {
    #[serde(default = "default_left_color")]
    pub left_color: String,
    #[serde(default = "default_right_color")]
    pub right_color: String,
    #[serde(default)]
    pub left_label: Option<String>,
    #[serde(default)]
    pub right_label: Option<String>,
    #[serde(default = "default_divider_position")]
    pub divider_position: f64,
    #[serde(default)]
    pub animate_from: Option<f64>,
    #[serde(default)]
    pub animate_to: Option<f64>,
    #[serde(default)]
    pub animate_at: Option<f64>,
    #[serde(default = "default_animation_duration")]
    pub animation_duration: f64,
    #[serde(default = "default_divider_color")]
    pub divider_color: String,
    #[serde(default = "default_divider_width")]
    pub divider_width: f32,
    #[serde(default)]
    pub size: Option<Size>,
    #[serde(default = "default_border_radius")]
    pub border_radius: f32,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

rustmotion_core::impl_traits!(Comparison {
    Animatable => style,
    Timed => timing,
    Styled => style,
});

impl Comparison {
    fn current_divider_position_at(&self, time: f64) -> f64 {
        let from = self.animate_from.unwrap_or(self.divider_position);
        let to = self.animate_to.unwrap_or(self.divider_position);
        let start = self.animate_at.unwrap_or(0.0);

        if time < start {
            return from;
        }

        if self.animation_duration <= 0.0 {
            return to;
        }

        let elapsed = time - start;
        let t = (elapsed / self.animation_duration).clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - t).powi(3);
        from + (to - from) * eased
    }
}

impl Comparison {
    fn paint(&self, canvas: &Canvas, layout_w: f32, layout_h: f32, time: f64) {
        let w = layout_w;
        let h = layout_h;
        let pos = self.current_divider_position_at(time).clamp(0.0, 1.0) as f32;
        let divider_x = w * pos;

        let outer_rect = Rect::from_xywh(0.0, 0.0, w, h);
        let outer_rrect = RRect::new_rect_xy(outer_rect, self.border_radius, self.border_radius);

        canvas.save();
        canvas.clip_rrect(outer_rrect, skia_safe::ClipOp::Intersect, true);

        let mut left_paint = paint_from_hex(&self.left_color);
        left_paint.set_style(PaintStyle::Fill);
        let left_rect = Rect::from_xywh(0.0, 0.0, divider_x, h);
        canvas.draw_rect(left_rect, &left_paint);

        let mut right_paint = paint_from_hex(&self.right_color);
        right_paint.set_style(PaintStyle::Fill);
        let right_rect = Rect::from_xywh(divider_x, 0.0, w - divider_x, h);
        canvas.draw_rect(right_rect, &right_paint);

        let font_size = (h * 0.08).max(16.0).min(36.0);
        let fm = font_mgr();
        let font_style = skia_safe::FontStyle::bold();
        let typeface = fm
            .match_family_style("Inter", font_style)
            .or_else(|| fm.match_family_style("Helvetica", font_style))
            .unwrap_or_else(|| fm.legacy_make_typeface(None, font_style).unwrap());
        let font = skia_safe::Font::from_typeface(typeface, font_size);
        let emoji_font = emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, font_size));

        let mut label_paint = paint_from_hex("#FFFFFF");
        label_paint.set_anti_alias(true);

        if let Some(ref label) = self.left_label {
            let text_w = measure_text_with_fallback(label, &font, &emoji_font, 0.0);
            let (_, metrics) = font.metrics();
            let text_x = divider_x / 2.0 - text_w / 2.0;
            let text_y = h / 2.0 + (-metrics.ascent) / 2.0;
            draw_text_with_fallback(
                canvas, label, &font, &emoji_font, 0.0, text_x, text_y, &label_paint,
            );
        }

        if let Some(ref label) = self.right_label {
            let text_w = measure_text_with_fallback(label, &font, &emoji_font, 0.0);
            let (_, metrics) = font.metrics();
            let text_x = divider_x + (w - divider_x) / 2.0 - text_w / 2.0;
            let text_y = h / 2.0 + (-metrics.ascent) / 2.0;
            draw_text_with_fallback(
                canvas, label, &font, &emoji_font, 0.0, text_x, text_y, &label_paint,
            );
        }

        let mut divider_paint = paint_from_hex(&self.divider_color);
        divider_paint.set_style(PaintStyle::Stroke);
        divider_paint.set_stroke_width(self.divider_width);
        divider_paint.set_anti_alias(true);
        canvas.draw_line(
            skia_safe::Point::new(divider_x, 0.0),
            skia_safe::Point::new(divider_x, h),
            &divider_paint,
        );

        let handle_radius = (self.divider_width * 4.0).max(8.0);
        let mut handle_paint = paint_from_hex(&self.divider_color);
        handle_paint.set_style(PaintStyle::Fill);
        handle_paint.set_anti_alias(true);
        canvas.draw_circle(
            skia_safe::Point::new(divider_x, h / 2.0),
            handle_radius,
            &handle_paint,
        );

        let (r, g, b, _) = parse_hex_color(&self.divider_color);
        let mut border_paint = skia_safe::Paint::default();
        border_paint.set_style(PaintStyle::Stroke);
        border_paint.set_stroke_width(2.0);
        border_paint.set_anti_alias(true);
        border_paint.set_color(skia_safe::Color::from_argb(80, r, g, b));
        canvas.draw_circle(
            skia_safe::Point::new(divider_x, h / 2.0),
            handle_radius,
            &border_paint,
        );

        canvas.restore();
    }
}

impl Painter for Comparison {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        self.paint(canvas, layout.width, layout.height, ctx.time);
    }
}
