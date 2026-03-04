use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, Rect};

use crate::engine::renderer::paint_from_hex;
use crate::layout::{layout_grid, Constraints, LayoutNode};
use crate::traits::{
    AnimationConfig, Border, Bordered, BorderedMut, Container, GridConfig, GridContainer,
    GridContainerMut, RenderContext, Rounded, RoundedMut, Shadow, Shadowed, ShadowedMut,
    StyleConfig, TimingConfig, Widget,
};


use super::flex::FlexSize;
use super::ChildComponent;

/// Grid container — children are positioned via CSS-like grid layout.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Grid {
    #[serde(default)]
    pub layers: Vec<ChildComponent>,
    #[serde(default)]
    pub size: Option<FlexSize>,
    // Visual
    #[serde(default)]
    pub background: Option<String>,
    #[serde(default = "default_corner_radius")]
    pub corner_radius: f32,
    #[serde(default)]
    pub border: Option<Border>,
    #[serde(default)]
    pub shadow: Option<Shadow>,
    // Grid config
    #[serde(flatten)]
    pub grid: GridConfig,
    // Behaviors
    #[serde(flatten)]
    pub animation: AnimationConfig,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(flatten)]
    pub style: StyleConfig,
}

crate::impl_traits!(Grid {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Container for Grid {
    fn children(&self) -> &[ChildComponent] {
        &self.layers
    }
}

impl GridContainer for Grid {
    fn grid_config(&self) -> &GridConfig {
        &self.grid
    }
}

impl GridContainerMut for Grid {
    fn grid_config_mut(&mut self) -> &mut GridConfig {
        &mut self.grid
    }
}

impl Bordered for Grid {
    fn border(&self) -> Option<&Border> {
        self.border.as_ref()
    }
}

impl BorderedMut for Grid {
    fn set_border(&mut self, border: Option<Border>) {
        self.border = border;
    }
}

impl Rounded for Grid {
    fn corner_radius(&self) -> f32 {
        self.corner_radius
    }
}

impl RoundedMut for Grid {
    fn set_corner_radius(&mut self, radius: f32) {
        self.corner_radius = radius;
    }
}

impl Shadowed for Grid {
    fn shadow(&self) -> Option<&Shadow> {
        self.shadow.as_ref()
    }
}

impl ShadowedMut for Grid {
    fn set_shadow(&mut self, shadow: Option<Shadow>) {
        self.shadow = shadow;
    }
}

impl crate::traits::Backgrounded for Grid {
    fn background(&self) -> Option<&str> {
        self.background.as_deref()
    }
}

impl crate::traits::BackgroundedMut for Grid {
    fn set_background(&mut self, bg: Option<String>) {
        self.background = bg;
    }
}

impl crate::traits::Clipped for Grid {
    fn clip(&self) -> bool {
        true
    }
}

impl Widget for Grid {
    fn render(&self, canvas: &Canvas, layout: &LayoutNode, ctx: &RenderContext) -> Result<()> {
        let rect = Rect::from_xywh(0.0, 0.0, layout.width, layout.height);
        let rrect = skia_safe::RRect::new_rect_xy(rect, self.corner_radius, self.corner_radius);

        // 1. Shadow
        if let Some(ref shadow) = self.shadow {
            let shadow_rect = Rect::from_xywh(
                shadow.offset_x, shadow.offset_y,
                layout.width, layout.height,
            );
            let shadow_rrect = skia_safe::RRect::new_rect_xy(
                shadow_rect, self.corner_radius, self.corner_radius,
            );
            let mut shadow_paint = paint_from_hex(&shadow.color);
            if shadow.blur > 0.0 {
                shadow_paint.set_mask_filter(skia_safe::MaskFilter::blur(
                    skia_safe::BlurStyle::Normal,
                    shadow.blur / 2.0,
                    false,
                ));
            }
            canvas.draw_rrect(shadow_rrect, &shadow_paint);
        }

        // 2. Background
        if let Some(ref bg) = self.background {
            let bg_paint = paint_from_hex(bg);
            canvas.draw_rrect(rrect, &bg_paint);
        }

        // 3. Clip to rounded rect for children
        canvas.save();
        canvas.clip_rrect(rrect, skia_safe::ClipOp::Intersect, true);

        // 4. Render children with animation support
        crate::engine::render_v2::render_children(canvas, &self.layers, layout, ctx)?;

        canvas.restore(); // undo clip

        // 5. Border (on top of children)
        if let Some(ref border) = self.border {
            let mut border_paint = paint_from_hex(&border.color);
            border_paint.set_style(PaintStyle::Stroke);
            border_paint.set_stroke_width(border.width);
            canvas.draw_rrect(rrect, &border_paint);
        }

        Ok(())
    }

    fn measure(&self, constraints: &Constraints) -> (f32, f32) {
        let layout = self.layout(constraints);
        (layout.width, layout.height)
    }

    fn layout(&self, constraints: &Constraints) -> LayoutNode {
        let c = super::flex::resolve_size_constraints(&self.size, constraints);
        layout_grid(self, &c)
    }
}

fn default_corner_radius() -> f32 { 12.0 }
