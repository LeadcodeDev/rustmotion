use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, Rect};

use crate::engine::renderer::paint_from_hex;
use crate::layout::{layout_flex, Constraints, LayoutNode};
use crate::schema::SizeDimension;
use crate::traits::{
    AnimationConfig, Border, Bordered, BorderedMut, Container, FlexConfig, FlexContainer,
    FlexContainerMut, RenderContext, Rounded, RoundedMut, Shadow, Shadowed, ShadowedMut,
    StyleConfig, TimingConfig, Widget,
};

use super::ChildComponent;

/// Flex size — each dimension can be fixed or auto.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FlexSize {
    pub width: SizeDimension,
    pub height: SizeDimension,
}

/// Flex container — children are positioned via flexbox layout.
/// Replaces the old `Card` / `Flex` layer.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Flex {
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
    // Layout
    #[serde(flatten)]
    pub flex: FlexConfig,
    // Behaviors
    #[serde(flatten)]
    pub animation: AnimationConfig,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(flatten)]
    pub style: StyleConfig,
}

crate::impl_traits!(Flex {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Container for Flex {
    fn children(&self) -> &[ChildComponent] {
        &self.layers
    }
}

impl FlexContainer for Flex {
    fn flex_config(&self) -> &FlexConfig {
        &self.flex
    }
}

impl FlexContainerMut for Flex {
    fn flex_config_mut(&mut self) -> &mut FlexConfig {
        &mut self.flex
    }
}

impl Bordered for Flex {
    fn border(&self) -> Option<&Border> {
        self.border.as_ref()
    }
}

impl BorderedMut for Flex {
    fn set_border(&mut self, border: Option<Border>) {
        self.border = border;
    }
}

impl Rounded for Flex {
    fn corner_radius(&self) -> f32 {
        self.corner_radius
    }
}

impl RoundedMut for Flex {
    fn set_corner_radius(&mut self, radius: f32) {
        self.corner_radius = radius;
    }
}

impl Shadowed for Flex {
    fn shadow(&self) -> Option<&Shadow> {
        self.shadow.as_ref()
    }
}

impl ShadowedMut for Flex {
    fn set_shadow(&mut self, shadow: Option<Shadow>) {
        self.shadow = shadow;
    }
}

impl crate::traits::Backgrounded for Flex {
    fn background(&self) -> Option<&str> {
        self.background.as_deref()
    }
}

impl crate::traits::BackgroundedMut for Flex {
    fn set_background(&mut self, bg: Option<String>) {
        self.background = bg;
    }
}

impl crate::traits::Clipped for Flex {
    fn clip(&self) -> bool {
        true // Flex always clips children to bounds
    }
}

impl Widget for Flex {
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
        let c = resolve_size_constraints(&self.size, constraints);
        layout_flex(self, &c)
    }
}

/// Resolve FlexSize dimensions into constraints.
pub(crate) fn resolve_size_constraints(size: &Option<FlexSize>, constraints: &Constraints) -> Constraints {
    let w = size.as_ref().and_then(|s| match &s.width {
        SizeDimension::Fixed(v) => Some(*v),
        SizeDimension::Auto(_) => None,
    });
    let h = size.as_ref().and_then(|s| match &s.height {
        SizeDimension::Fixed(v) => Some(*v),
        SizeDimension::Auto(_) => None,
    });
    Constraints {
        min_width: w.unwrap_or(constraints.min_width),
        max_width: w.unwrap_or(constraints.max_width),
        min_height: h.unwrap_or(constraints.min_height),
        max_height: h.unwrap_or(constraints.max_height),
    }
}

fn default_corner_radius() -> f32 { 12.0 }
