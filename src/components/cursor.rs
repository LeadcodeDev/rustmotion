use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::Canvas;

use crate::engine::renderer::paint_from_hex;
use crate::layout::{Constraints, LayoutNode};
use crate::schema::LayerStyle;
use crate::traits::{RenderContext, TimingConfig, Widget};

/// A blinking cursor component (vertical bar).
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
        if self.blink > 0.0 {
            let cycle = (ctx.time as f32 % (self.blink * 2.0)) / self.blink;
            if cycle >= 1.0 {
                return Ok(()); // invisible half of blink
            }
        }

        let paint = paint_from_hex(&self.color);
        let rect = skia_safe::Rect::from_xywh(0.0, 0.0, self.width, self.height);
        let rrect = skia_safe::RRect::new_rect_xy(rect, self.radius, self.radius);
        canvas.draw_rrect(rrect, &paint);

        Ok(())
    }

    fn measure(&self, _constraints: &Constraints) -> (f32, f32) {
        (self.width, self.height)
    }
}
