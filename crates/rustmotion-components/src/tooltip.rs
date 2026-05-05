use rustmotion_core::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, Path, Rect};

use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, font_mgr, measure_text_with_fallback, paint_from_hex,
};
use rustmotion_core::layout::{Constraints, LayoutNode};
use rustmotion_core::schema::LayerStyle;
use rustmotion_core::traits::{PaintCtx, Painter, RenderContext, TimingConfig, Widget};

fn default_font_size() -> f32 {
    13.0
}

fn default_bg_color() -> String {
    "#1E293B".to_string()
}

fn default_text_color() -> String {
    "#E2E8F0".to_string()
}

fn default_arrow_size() -> f32 {
    8.0
}

/// Arrow direction — where the arrow points (toward the target element).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TooltipArrow {
    Top,
    Bottom,
    Left,
    Right,
    None,
}

impl Default for TooltipArrow {
    fn default() -> Self {
        Self::Bottom
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Tooltip {
    pub text: String,
    #[serde(default)]
    pub arrow: TooltipArrow,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default = "default_bg_color")]
    pub background_color: String,
    #[serde(default = "default_text_color")]
    pub text_color: String,
    #[serde(default = "default_arrow_size")]
    pub arrow_size: f32,
    #[serde(default)]
    pub border_color: Option<String>,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

rustmotion_core::impl_traits!(Tooltip {
    Animatable => style,
    Timed => timing,
    Styled => style,
});

impl Tooltip {
    fn make_font(&self) -> skia_safe::Font {
        let fs = self.style.font_size.unwrap_or(self.font_size);
        let fm = font_mgr();
        let font_style = skia_safe::FontStyle::normal();
        let family = self.style.font_family.as_deref().unwrap_or("Inter");
        let typeface = fm
            .match_family_style(family, font_style)
            .or_else(|| fm.match_family_style("Helvetica", font_style))
            .or_else(|| fm.match_family_style("Arial", font_style))
            .unwrap_or_else(|| fm.legacy_make_typeface(None, font_style).unwrap());
        skia_safe::Font::from_typeface(typeface, fs)
    }

    fn measure_content(&self) -> (f32, f32) {
        let font = self.make_font();
        let fs = self.style.font_size.unwrap_or(self.font_size);
        let emoji_font = emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, fs));
        let text_w = measure_text_with_fallback(&self.text, &font, &emoji_font, 0.0);
        let h_pad = fs * 0.8;
        let v_pad = fs * 0.5;
        let body_w = text_w + h_pad * 2.0;
        let body_h = fs * 1.4 + v_pad * 2.0;

        let (total_w, total_h) = match self.arrow {
            TooltipArrow::Top | TooltipArrow::Bottom => (body_w, body_h + self.arrow_size),
            TooltipArrow::Left | TooltipArrow::Right => (body_w + self.arrow_size, body_h),
            TooltipArrow::None => (body_w, body_h),
        };
        (total_w, total_h)
    }
}

impl Tooltip {
    fn paint(&self, canvas: &Canvas, layout_w: f32, layout_h: f32) {
        let w = layout_w;
        let h = layout_h;
        let bg_color = self.style.background.as_deref().unwrap_or(&self.background_color);
        let radius = self.style.border_radius.unwrap_or(8.0);
        let arrow_sz = self.arrow_size;

        // Compute body rect (excluding arrow area)
        let (body_x, body_y, body_w, body_h) = match self.arrow {
            TooltipArrow::Bottom => (0.0, 0.0, w, h - arrow_sz),
            TooltipArrow::Top => (0.0, arrow_sz, w, h - arrow_sz),
            TooltipArrow::Right => (0.0, 0.0, w - arrow_sz, h),
            TooltipArrow::Left => (arrow_sz, 0.0, w - arrow_sz, h),
            TooltipArrow::None => (0.0, 0.0, w, h),
        };

        // Body rounded rect
        let body_rect = Rect::from_xywh(body_x, body_y, body_w, body_h);
        let body_rrect = skia_safe::RRect::new_rect_xy(body_rect, radius, radius);

        let mut bg_paint = paint_from_hex(bg_color);
        bg_paint.set_style(PaintStyle::Fill);
        bg_paint.set_anti_alias(true);
        canvas.draw_rrect(body_rrect, &bg_paint);

        // Border
        if let Some(bc) = &self.border_color {
            let mut border_paint = paint_from_hex(bc);
            border_paint.set_style(PaintStyle::Stroke);
            border_paint.set_stroke_width(1.0);
            border_paint.set_anti_alias(true);
            canvas.draw_rrect(body_rrect, &border_paint);
        }

        // Arrow triangle
        if !matches!(self.arrow, TooltipArrow::None) {
            let mut arrow_path = Path::new();
            match self.arrow {
                TooltipArrow::Bottom => {
                    let cx = body_x + body_w / 2.0;
                    let ay = body_y + body_h;
                    arrow_path.move_to((cx - arrow_sz, ay));
                    arrow_path.line_to((cx, ay + arrow_sz));
                    arrow_path.line_to((cx + arrow_sz, ay));
                    arrow_path.close();
                }
                TooltipArrow::Top => {
                    let cx = body_x + body_w / 2.0;
                    let ay = body_y;
                    arrow_path.move_to((cx - arrow_sz, ay));
                    arrow_path.line_to((cx, ay - arrow_sz));
                    arrow_path.line_to((cx + arrow_sz, ay));
                    arrow_path.close();
                }
                TooltipArrow::Right => {
                    let cy = body_y + body_h / 2.0;
                    let ax = body_x + body_w;
                    arrow_path.move_to((ax, cy - arrow_sz));
                    arrow_path.line_to((ax + arrow_sz, cy));
                    arrow_path.line_to((ax, cy + arrow_sz));
                    arrow_path.close();
                }
                TooltipArrow::Left => {
                    let cy = body_y + body_h / 2.0;
                    let ax = body_x;
                    arrow_path.move_to((ax, cy - arrow_sz));
                    arrow_path.line_to((ax - arrow_sz, cy));
                    arrow_path.line_to((ax, cy + arrow_sz));
                    arrow_path.close();
                }
                TooltipArrow::None => {}
            }
            canvas.draw_path(&arrow_path, &bg_paint);
        }

        // Text centered in body
        let font = self.make_font();
        let fs = self.style.font_size.unwrap_or(self.font_size);
        let emoji_font = emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, fs));

        let text_color = self.style.color.as_deref().unwrap_or(&self.text_color);
        let mut text_paint = paint_from_hex(text_color);
        text_paint.set_anti_alias(true);

        let text_w = measure_text_with_fallback(&self.text, &font, &emoji_font, 0.0);
        let (_, metrics) = font.metrics();
        let text_x = body_x + (body_w - text_w) / 2.0;
        let text_y = body_y + (body_h + (-metrics.ascent)) / 2.0;

        draw_text_with_fallback(
            canvas,
            &self.text,
            &font,
            &emoji_font,
            0.0,
            text_x,
            text_y,
            &text_paint,
        );
    }
}

impl Widget for Tooltip {
    fn render(
        &self,
        canvas: &Canvas,
        layout: &LayoutNode,
        _ctx: &RenderContext,
        _props: &AnimatedProperties,
        _pipeline: &dyn rustmotion_core::traits::RenderPipeline,
    ) -> Result<()> {
        self.paint(canvas, layout.width, layout.height);
        Ok(())
    }

    fn measure(&self, _constraints: &Constraints) -> (f32, f32) {
        self.measure_content()
    }
}

impl Painter for Tooltip {
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
