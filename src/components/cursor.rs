use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::Canvas;

use crate::engine::renderer::paint_from_hex;
use crate::layout::{Constraints, LayoutNode};
use crate::schema::LayerStyle;
use crate::traits::{RenderContext, TimingConfig, Widget};

/// A blinking cursor component (vertical bar) with optional motion path and click events.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Cursor {
    #[serde(default = "default_cursor_width")]
    pub width: f32,
    #[serde(default = "default_cursor_height")]
    pub height: f32,
    #[serde(default = "default_cursor_color")]
    pub color: String,
    /// Blink interval in seconds (0 = no blink, always visible).
    #[serde(default = "default_blink_interval")]
    pub blink: f32,
    #[serde(default = "default_cursor_radius")]
    pub radius: f32,
    /// Timestamps (seconds) at which the cursor "clicks" (scale bounce effect).
    #[serde(default)]
    pub click_at: Vec<f64>,
    /// Click animation duration in seconds.
    #[serde(default = "default_click_duration")]
    pub click_duration: f32,
    /// Visual cursor style: "default" (arrow) or "pointer" (hand).
    /// Currently both render as a bar; this is metadata for future SVG cursors.
    #[serde(default = "default_cursor_style")]
    pub cursor_style: String,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

fn default_cursor_width() -> f32 {
    3.0
}

fn default_cursor_height() -> f32 {
    40.0
}

fn default_cursor_color() -> String {
    "#FFFFFF".to_string()
}

fn default_blink_interval() -> f32 {
    0.5
}

fn default_cursor_radius() -> f32 {
    1.5
}

fn default_click_duration() -> f32 {
    0.3
}

fn default_cursor_style() -> String {
    "default".to_string()
}

crate::impl_traits!(Cursor {
    Animatable => style,
    Timed => timing,
    Styled => style,
});

impl Widget for Cursor {
    fn render(
        &self,
        canvas: &Canvas,
        _layout: &LayoutNode,
        ctx: &RenderContext,
        _props: &crate::engine::animator::AnimatedProperties,
    ) -> Result<()> {
        // Blink logic: visible during first half of each blink cycle
        // Disable blink during click animation
        let in_click = self.click_at.iter().any(|&t| {
            let dt = ctx.time - t;
            dt >= 0.0 && dt < self.click_duration as f64
        });

        if self.blink > 0.0 && !in_click {
            let cycle = (ctx.time as f32 % (self.blink * 2.0)) / self.blink;
            if cycle >= 1.0 {
                return Ok(()); // invisible half of blink
            }
        }

        // Click bounce: scale up then back down
        let click_scale = if in_click {
            let closest_click = self.click_at.iter()
                .filter(|&&t| ctx.time >= t && ctx.time < t + self.click_duration as f64)
                .copied()
                .last()
                .unwrap_or(0.0);
            let progress = ((ctx.time - closest_click) / self.click_duration as f64) as f32;
            // Bounce: scale up to 1.5x at progress=0.3, then back to 1.0
            if progress < 0.3 {
                1.0 + 0.5 * (progress / 0.3)
            } else {
                1.5 - 0.5 * ((progress - 0.3) / 0.7)
            }
        } else {
            1.0
        };

        if (click_scale - 1.0).abs() > 0.001 {
            let cx = self.width / 2.0;
            let cy = self.height / 2.0;
            canvas.save();
            canvas.translate((cx, cy));
            canvas.scale((click_scale, click_scale));
            canvas.translate((-cx, -cy));
        }

        let paint = paint_from_hex(&self.color);
        let rect = skia_safe::Rect::from_xywh(0.0, 0.0, self.width, self.height);
        let rrect = skia_safe::RRect::new_rect_xy(rect, self.radius, self.radius);
        canvas.draw_rrect(rrect, &paint);

        if (click_scale - 1.0).abs() > 0.001 {
            canvas.restore();
        }

        Ok(())
    }

    fn measure(&self, _constraints: &Constraints) -> (f32, f32) {
        (self.width, self.height)
    }
}
