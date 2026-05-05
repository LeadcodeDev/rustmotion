use rustmotion_core::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, ColorType, ImageInfo, Paint, PaintStyle, Rect};

use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    asset_cache, draw_text_with_fallback, emoji_typeface, fetch_icon_svg, font_mgr, paint_from_hex,
};
use rustmotion_core::layout::{Constraints, LayoutNode};
use rustmotion_core::schema::LayerStyle;
use rustmotion_core::traits::{PaintCtx, Painter, RenderContext, TimingConfig, Widget};

fn default_gap() -> f32 {
    16.0
}
fn default_icon_size() -> f32 {
    20.0
}
fn default_icon_color() -> String {
    "#22C55E".to_string()
}
fn default_unchecked_color() -> String {
    "#6B7280".to_string()
}
fn default_width() -> f32 {
    400.0
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ListVariant {
    Bullet,
    Numbered,
    Checklist,
}

impl Default for ListVariant {
    fn default() -> Self {
        Self::Bullet
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListItem {
    pub text: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub checked: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct List {
    pub items: Vec<ListItem>,
    #[serde(default)]
    pub variant: ListVariant,
    #[serde(default = "default_gap")]
    pub gap: f32,
    #[serde(default = "default_icon_size")]
    pub icon_size: f32,
    #[serde(default = "default_icon_color")]
    pub icon_color: String,
    #[serde(default = "default_unchecked_color")]
    pub unchecked_color: String,
    #[serde(default = "default_width")]
    pub width: f32,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

rustmotion_core::impl_traits!(List {
    Animatable => style,
    Timed => timing,
    Styled => style,
});

impl List {
    fn resolved_font_size(&self) -> f32 {
        self.style.font_size.unwrap_or(16.0)
    }

    fn make_font(&self) -> skia_safe::Font {
        let fm = font_mgr();
        let font_style = skia_safe::FontStyle::normal();
        let family = self.style.font_family.as_deref().unwrap_or("Inter");
        let typeface = fm
            .match_family_style(family, font_style)
            .or_else(|| fm.match_family_style("Helvetica", font_style))
            .unwrap_or_else(|| fm.legacy_make_typeface(None, font_style).unwrap());
        skia_safe::Font::from_typeface(typeface, self.resolved_font_size())
    }

    fn render_icon_svg(
        &self,
        canvas: &Canvas,
        icon_id: &str,
        color: &str,
        x: f32,
        y: f32,
    ) -> Result<()> {
        let icon_w = self.icon_size.round() as u32;
        let icon_h = self.icon_size.round() as u32;
        let cache_key = format!("icon:{}:{}:{}x{}", icon_id, color, icon_w, icon_h);

        let cache = asset_cache();
        let img = if let Some(cached) = cache.get(&cache_key) {
            cached.clone()
        } else if let Ok(svg_data) = fetch_icon_svg(icon_id, color, icon_w, icon_h) {
            let opt = usvg::Options::default();
            if let Ok(tree) = usvg::Tree::from_data(&svg_data, &opt) {
                let svg_size = tree.size();
                if let Some(mut pixmap) = tiny_skia::Pixmap::new(icon_w, icon_h) {
                    let sx = icon_w as f32 / svg_size.width();
                    let sy = icon_h as f32 / svg_size.height();
                    resvg::render(
                        &tree,
                        tiny_skia::Transform::from_scale(sx, sy),
                        &mut pixmap.as_mut(),
                    );
                    let img_data = skia_safe::Data::new_copy(pixmap.data());
                    let info = ImageInfo::new(
                        (icon_w as i32, icon_h as i32),
                        ColorType::RGBA8888,
                        skia_safe::AlphaType::Premul,
                        None,
                    );
                    if let Some(decoded) = skia_safe::images::raster_from_data(
                        &info,
                        img_data,
                        icon_w as usize * 4,
                    ) {
                        cache.insert(cache_key, decoded.clone());
                        decoded
                    } else {
                        return Ok(());
                    }
                } else {
                    return Ok(());
                }
            } else {
                return Ok(());
            }
        } else {
            return Ok(());
        };

        let dst = Rect::from_xywh(x, y, self.icon_size, self.icon_size);
        canvas.draw_image_rect(img, None, dst, &Paint::default());
        Ok(())
    }
}

impl List {
    fn paint(&self, canvas: &Canvas) -> Result<()> {
        let font = self.make_font();
        let font_size = self.resolved_font_size();
        let emoji_font = emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, font_size));
        let text_color = self.style.color.as_deref().unwrap_or("#FFFFFF");
        let mut text_paint = paint_from_hex(text_color);
        text_paint.set_anti_alias(true);

        let (_, metrics) = font.metrics();
        let line_height = font_size * 1.3;
        let left_margin = self.icon_size + 12.0;

        let mut y_offset = 0.0;

        for (i, item) in self.items.iter().enumerate() {
            let text_y = y_offset + (line_height + (-metrics.ascent)) / 2.0;
            let icon_y = y_offset + (line_height - self.icon_size) / 2.0;

            match &self.variant {
                ListVariant::Bullet => {
                    let bullet_r = 4.0;
                    let bullet_cx = self.icon_size / 2.0;
                    let bullet_cy = y_offset + line_height / 2.0;
                    let mut bullet_paint = paint_from_hex(&self.icon_color);
                    bullet_paint.set_style(PaintStyle::Fill);
                    bullet_paint.set_anti_alias(true);
                    canvas.draw_circle((bullet_cx, bullet_cy), bullet_r, &bullet_paint);
                }
                ListVariant::Numbered => {
                    let num_text = format!("{}.", i + 1);
                    let mut num_paint = paint_from_hex(&self.icon_color);
                    num_paint.set_anti_alias(true);
                    draw_text_with_fallback(
                        canvas,
                        &num_text,
                        &font,
                        &emoji_font,
                        0.0,
                        0.0,
                        text_y,
                        &num_paint,
                    );
                }
                ListVariant::Checklist => {
                    let checked = item.checked.unwrap_or(false);
                    if let Some(icon_id) = &item.icon {
                        let color = if checked {
                            &self.icon_color
                        } else {
                            &self.unchecked_color
                        };
                        self.render_icon_svg(canvas, icon_id, color, 0.0, icon_y)?;
                    } else {
                        let circle_r = self.icon_size / 2.0 - 2.0;
                        let circle_cx = self.icon_size / 2.0;
                        let circle_cy = y_offset + line_height / 2.0;
                        if checked {
                            let mut fill_paint = paint_from_hex(&self.icon_color);
                            fill_paint.set_style(PaintStyle::Fill);
                            fill_paint.set_anti_alias(true);
                            canvas.draw_circle((circle_cx, circle_cy), circle_r, &fill_paint);
                        } else {
                            let mut stroke_paint = paint_from_hex(&self.unchecked_color);
                            stroke_paint.set_style(PaintStyle::Stroke);
                            stroke_paint.set_stroke_width(2.0);
                            stroke_paint.set_anti_alias(true);
                            canvas.draw_circle((circle_cx, circle_cy), circle_r, &stroke_paint);
                        }
                    }
                }
            }

            draw_text_with_fallback(
                canvas,
                &item.text,
                &font,
                &emoji_font,
                0.0,
                left_margin,
                text_y,
                &text_paint,
            );

            y_offset += line_height + self.gap;
        }

        Ok(())
    }
}

impl Widget for List {
    fn render(
        &self,
        canvas: &Canvas,
        _layout: &LayoutNode,
        _ctx: &RenderContext,
        _props: &AnimatedProperties,
        _pipeline: &dyn rustmotion_core::traits::RenderPipeline,
    ) -> Result<()> {
        self.paint(canvas)
    }

    fn measure(&self, _constraints: &Constraints) -> (f32, f32) {
        let font_size = self.resolved_font_size();
        let line_height = font_size * 1.3;
        let total_h = if self.items.is_empty() {
            0.0
        } else {
            self.items.len() as f32 * (line_height + self.gap) - self.gap
        };
        (self.width, total_h)
    }
}

impl Painter for List {
    fn paint_content(
        &self,
        canvas: &Canvas,
        _layout: &BoxLayout,
        _props: &AnimatedProperties,
        _ctx: &PaintCtx,
    ) {
        let _ = self.paint(canvas);
    }
}
