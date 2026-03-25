use crate::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, Path};

use crate::engine::renderer::paint_from_hex;
use crate::layout::{Constraints, LayoutNode};
use crate::schema::LayerStyle;
use crate::traits::{RenderContext, TimingConfig, Widget};

fn default_max() -> u32 {
    5
}
fn default_star_size() -> f32 {
    32.0
}
fn default_gap() -> f32 {
    4.0
}
fn default_filled_color() -> String {
    "#F59E0B".to_string()
}
fn default_empty_color() -> String {
    "#374151".to_string()
}
fn default_animated() -> bool {
    true
}
fn default_animation_duration() -> f64 {
    1.0
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Rating {
    #[serde(default)]
    pub value: f64,
    #[serde(default = "default_max")]
    pub max: u32,
    #[serde(default = "default_star_size")]
    pub size: f32,
    #[serde(default = "default_gap")]
    pub gap: f32,
    #[serde(default = "default_filled_color")]
    pub filled_color: String,
    #[serde(default = "default_empty_color")]
    pub empty_color: String,
    #[serde(default = "default_animated")]
    pub animated: bool,
    #[serde(default = "default_animation_duration")]
    pub animation_duration: f64,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

crate::impl_traits!(Rating {
    Animatable => style,
    Timed => timing,
    Styled => style,
});

impl Rating {
    fn ease_out_cubic(t: f64) -> f64 {
        1.0 - (1.0 - t).powi(3)
    }

    fn displayed_value(&self, ctx: &RenderContext) -> f64 {
        if !self.animated {
            return self.value;
        }
        let progress = (ctx.time / self.animation_duration).clamp(0.0, 1.0);
        let eased = Self::ease_out_cubic(progress);
        self.value * eased
    }

    fn star_path(cx: f32, cy: f32, outer_radius: f32) -> Path {
        let inner_radius = outer_radius * 0.4;
        let mut path = Path::new();

        for i in 0..10 {
            let angle =
                -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
            let r = if i % 2 == 0 {
                outer_radius
            } else {
                inner_radius
            };
            let x = cx + r * angle.cos();
            let y = cy + r * angle.sin();

            if i == 0 {
                path.move_to((x, y));
            } else {
                path.line_to((x, y));
            }
        }
        path.close();
        path
    }
}

impl Widget for Rating {
    fn render(
        &self,
        canvas: &Canvas,
        _layout: &LayoutNode,
        ctx: &RenderContext,
        _props: &crate::engine::animator::AnimatedProperties,
    ) -> Result<()> {
        let displayed = self.displayed_value(ctx);
        let outer_radius = self.size / 2.0;

        for i in 0..self.max {
            let cx = outer_radius + i as f32 * (self.size + self.gap);
            let cy = outer_radius;
            let star_fill = displayed - i as f64;

            if star_fill >= 1.0 {
                // Fully filled star
                let path = Self::star_path(cx, cy, outer_radius);
                let mut paint = paint_from_hex(&self.filled_color);
                paint.set_style(PaintStyle::Fill);
                paint.set_anti_alias(true);
                canvas.draw_path(&path, &paint);
            } else if star_fill > 0.0 {
                // Empty background
                let path = Self::star_path(cx, cy, outer_radius);
                let mut empty_paint = paint_from_hex(&self.empty_color);
                empty_paint.set_style(PaintStyle::Fill);
                empty_paint.set_anti_alias(true);
                canvas.draw_path(&path, &empty_paint);

                // Partial fill with clip
                let fill_fraction = star_fill as f32;
                let clip_x = cx - outer_radius;
                let clip_w = self.size * fill_fraction;
                let clip_rect =
                    skia_safe::Rect::from_xywh(clip_x, 0.0, clip_w, self.size);

                canvas.save();
                canvas.clip_rect(clip_rect, skia_safe::ClipOp::Intersect, true);
                let path = Self::star_path(cx, cy, outer_radius);
                let mut fill_paint = paint_from_hex(&self.filled_color);
                fill_paint.set_style(PaintStyle::Fill);
                fill_paint.set_anti_alias(true);
                canvas.draw_path(&path, &fill_paint);
                canvas.restore();
            } else {
                // Empty star
                let path = Self::star_path(cx, cy, outer_radius);
                let mut paint = paint_from_hex(&self.empty_color);
                paint.set_style(PaintStyle::Fill);
                paint.set_anti_alias(true);
                canvas.draw_path(&path, &paint);
            }
        }

        Ok(())
    }

    fn measure(&self, _constraints: &Constraints) -> (f32, f32) {
        let total_width = self.max as f32 * self.size + (self.max.saturating_sub(1)) as f32 * self.gap;
        (total_width, self.size)
    }
}
