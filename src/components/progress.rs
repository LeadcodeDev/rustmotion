use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, Rect, RRect};

use crate::engine::renderer::color4f_from_hex;
use crate::layout::{Constraints, LayoutNode};
use crate::schema::LayerStyle;
use crate::traits::{RenderContext, TimingConfig, Widget};

fn default_progress_width() -> f32 {
    300.0
}
fn default_progress_height() -> f32 {
    20.0
}
fn default_progress_bg() -> String {
    "#333333".to_string()
}
fn default_progress_fill() -> String {
    "#4CAF50".to_string()
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Progress {
    #[serde(default)]
    pub progress: f64,
    #[serde(default = "default_progress_width")]
    pub width: f32,
    #[serde(default = "default_progress_height")]
    pub height: f32,
    #[serde(default = "default_progress_bg")]
    pub background_color: String,
    #[serde(default = "default_progress_fill")]
    pub fill_color: String,
    #[serde(default)]
    pub border_radius: f32,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

crate::impl_traits!(Progress {
    Animatable => style,
    Timed => timing,
    Styled => style,
});

impl Widget for Progress {
    fn render(
        &self,
        canvas: &Canvas,
        _layout: &LayoutNode,
        _ctx: &RenderContext,
        _props: &crate::engine::animator::AnimatedProperties,
    ) -> Result<()> {
        let w = self.width;
        let h = self.height;
        let radius = self.border_radius;
        let progress = self.progress.clamp(0.0, 1.0) as f32;

        // Background
        let mut bg_paint = skia_safe::Paint::new(color4f_from_hex(&self.background_color), None);
        bg_paint.set_style(PaintStyle::Fill);
        bg_paint.set_anti_alias(true);

        let bg_rect = Rect::from_xywh(0.0, 0.0, w, h);
        let bg_rrect = RRect::new_rect_xy(bg_rect, radius, radius);
        canvas.draw_rrect(bg_rrect, &bg_paint);

        // Fill (progress)
        if progress > 0.001 {
            let mut fill_paint =
                skia_safe::Paint::new(color4f_from_hex(&self.fill_color), None);
            fill_paint.set_style(PaintStyle::Fill);
            fill_paint.set_anti_alias(true);

            let fill_w = w * progress;
            let fill_rect = Rect::from_xywh(0.0, 0.0, fill_w, h);

            canvas.save();
            canvas.clip_rrect(bg_rrect, skia_safe::ClipOp::Intersect, true);
            canvas.draw_rect(fill_rect, &fill_paint);
            canvas.restore();
        }

        Ok(())
    }

    fn measure(&self, _constraints: &Constraints) -> (f32, f32) {
        (self.width, self.height)
    }
}
