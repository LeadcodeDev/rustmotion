use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, Rect};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, measure_text_with_fallback, paint_from_hex,
    typeface_with_fallback,
};
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

fn default_background_color() -> String {
    "#0F172A".to_string()
}

fn default_dot_color() -> String {
    "#334155".to_string()
}

fn default_dot_spacing() -> f32 {
    8.0
}

fn default_dot_radius() -> f32 {
    1.5
}

fn default_animated() -> bool {
    true
}

fn default_animation_duration() -> f64 {
    1.5
}

fn default_show_world() -> bool {
    true
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct MapPoint {
    /// Latitude (-90 to 90, positive = north)
    pub lat: f64,
    /// Longitude (-180 to 180, positive = east)
    pub lng: f64,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub size: Option<f32>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub pulse: Option<bool>,
}

const LAT_MAX: f64 = 85.0;
const LAT_MIN: f64 = -85.0;

fn geo_to_screen(lat: f64, lng: f64) -> (f32, f32) {
    let x = ((lng + 180.0) / 360.0) as f32;
    let y = ((LAT_MAX - lat) / (LAT_MAX - LAT_MIN)) as f32;
    (x, y)
}

fn screen_to_geo(nx: f32, ny: f32) -> (f64, f64) {
    let lng = nx as f64 * 360.0 - 180.0;
    let lat = LAT_MAX - ny as f64 * (LAT_MAX - LAT_MIN);
    (lat, lng)
}

fn geo_is_land(lat: f64, lng: f64) -> bool {
    let bitmap = super::world_bitmap::land_bitmap();
    let col = ((lng + 180.0) / 360.0 * 180.0) as usize;
    let row = ((90.0 - lat) / 180.0 * 90.0) as usize;
    if col >= 180 || row >= 90 {
        return false;
    }
    bitmap[row * 180 + col] == 1
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct DotMap {
    pub points: Vec<MapPoint>,
    #[serde(default = "default_background_color")]
    pub background_color: String,
    /// Color of the world map dots
    #[serde(default = "default_dot_color")]
    pub world_dot_color: String,
    /// Spacing between world map dots in pixels
    #[serde(default = "default_dot_spacing")]
    pub dot_spacing: f32,
    /// Radius of each world map dot
    #[serde(default = "default_dot_radius")]
    pub dot_radius: f32,
    /// Show world map dot pattern
    #[serde(default = "default_show_world")]
    pub show_world: bool,
    #[serde(default = "default_animated")]
    pub animated: bool,
    #[serde(default = "default_animation_duration")]
    pub animation_duration: f64,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(DotMap {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

fn is_land_screen(nx: f32, ny: f32) -> bool {
    let (lat, lng) = screen_to_geo(nx, ny);
    geo_is_land(lat, lng)
}

impl DotMap {
    fn progress_at(&self, time: f64) -> f32 {
        if !self.animated {
            return 1.0;
        }
        let start = self.timing.start_at.unwrap_or(0.0);
        let elapsed = (time - start).max(0.0);
        let p = (elapsed / self.animation_duration).clamp(0.0, 1.0) as f32;
        1.0 - (1.0 - p).powi(3)
    }

    fn paint(&self, canvas: &Canvas, layout_w: f32, layout_h: f32, time: f64) {
        let w = layout_w;
        let h = layout_h;
        let progress = self.progress_at(time);

        let mut bg_paint = paint_from_hex(&self.background_color);
        bg_paint.set_style(PaintStyle::Fill);
        bg_paint.set_anti_alias(true);
        canvas.draw_rect(Rect::from_xywh(0.0, 0.0, w, h), &bg_paint);

        if self.show_world {
            let mut world_paint = paint_from_hex(&self.world_dot_color);
            world_paint.set_style(PaintStyle::Fill);
            world_paint.set_anti_alias(true);

            let spacing = if self.dot_spacing.is_finite() && self.dot_spacing >= 1.0 {
                self.dot_spacing
            } else {
                1.0
            };
            let radius = self.dot_radius;
            let margin = spacing;

            const MAX_DOTS_PER_AXIS: u32 = 4096;
            let cols = (((w - margin * 2.0) / spacing) as u32).min(MAX_DOTS_PER_AXIS);
            let rows = (((h - margin * 2.0) / spacing) as u32).min(MAX_DOTS_PER_AXIS);

            for row in 0..rows {
                for col in 0..cols {
                    let px = margin + col as f32 * spacing;
                    let py = margin + row as f32 * spacing;

                    let nx = px / w;
                    let ny = py / h;

                    if is_land_screen(nx, ny) {
                        canvas.draw_circle((px, py), radius, &world_paint);
                    }
                }
            }
        }

        let default_color = "#3B82F6";
        let default_dot_size = 10.0_f32;
        let label_font_size = 12.0_f32;
        let point_count = self.points.len();

        let font_style = skia_safe::FontStyle::normal();
        let Ok(typeface) = typeface_with_fallback("Inter", font_style) else {
            return;
        };
        let label_font = skia_safe::Font::from_typeface(typeface, label_font_size);
        let emoji_font =
            emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, label_font_size));

        for (i, point) in self.points.iter().enumerate() {
            let dot_alpha = if self.animated {
                let stagger_delay = if point_count > 1 {
                    (i as f64 / point_count as f64) * 0.6
                } else {
                    0.0
                };
                let dot_progress = ((time - stagger_delay) / (self.animation_duration * 0.4))
                    .clamp(0.0, 1.0) as f32;
                dot_progress * progress
            } else {
                1.0
            };

            if dot_alpha <= 0.0 {
                continue;
            }

            let (nx, ny) = geo_to_screen(point.lat, point.lng);
            let px = nx * w;
            let py = ny * h;
            let dot_size = point.size.unwrap_or(default_dot_size);
            let color_str = point.color.as_deref().unwrap_or(default_color);

            if point.pulse.unwrap_or(false) {
                let num_rings = 2;
                for ring in 0..num_rings {
                    let phase = ((time * 1.5 + ring as f64 * 0.5).fract()) as f32;
                    let ring_radius = dot_size * (1.0 + phase * 2.5);
                    let ring_alpha = (1.0 - phase).max(0.0) * 0.4 * dot_alpha;

                    let mut pulse_paint = paint_from_hex(color_str);
                    pulse_paint.set_style(PaintStyle::Stroke);
                    pulse_paint.set_stroke_width(2.0);
                    pulse_paint.set_anti_alias(true);
                    pulse_paint.set_alpha_f(ring_alpha);
                    canvas.draw_circle((px, py), ring_radius, &pulse_paint);
                }
            }

            let mut dot_paint = paint_from_hex(color_str);
            dot_paint.set_style(PaintStyle::Fill);
            dot_paint.set_anti_alias(true);
            dot_paint.set_alpha_f(dot_alpha);
            canvas.draw_circle((px, py), dot_size / 2.0, &dot_paint);

            let mut border_paint = paint_from_hex("#FFFFFF");
            border_paint.set_style(PaintStyle::Stroke);
            border_paint.set_stroke_width(1.5);
            border_paint.set_anti_alias(true);
            border_paint.set_alpha_f(dot_alpha * 0.6);
            canvas.draw_circle((px, py), dot_size / 2.0, &border_paint);

            if let Some(label) = &point.label {
                let mut label_paint = paint_from_hex("#FFFFFF");
                label_paint.set_anti_alias(true);
                label_paint.set_alpha_f(dot_alpha * 0.9);

                let text_w = measure_text_with_fallback(label, &label_font, &emoji_font, 0.0);
                let label_x = px - text_w / 2.0;
                let label_y = py + dot_size / 2.0 + label_font_size + 4.0;

                draw_text_with_fallback(
                    canvas,
                    label,
                    &label_font,
                    &emoji_font,
                    0.0,
                    label_x,
                    label_y,
                    &label_paint,
                );
            }
        }
    }
}

impl Painter for DotMap {
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
