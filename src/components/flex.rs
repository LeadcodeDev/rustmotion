use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, Rect};

use crate::engine::renderer::paint_from_hex;
use crate::layout::{layout_flex, Constraints, LayoutNode};
use crate::schema::{LayerStyle, SizeDimension};
use crate::traits::{
    AnimationConfig, Border, Bordered, BorderedMut, Container, FlexConfig, FlexContainer,
    FlexContainerMut, RenderContext, Rounded, RoundedMut, Shadow, Shadowed, ShadowedMut,
    TimingConfig, Widget,
};

use super::ChildComponent;

/// Flex size — each dimension can be fixed or auto.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FlexSize {
    pub width: SizeDimension,
    pub height: SizeDimension,
}

/// Flex container — children are positioned via flexbox layout.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Flex {
    #[serde(default)]
    pub children: Vec<ChildComponent>,
    #[serde(default)]
    pub size: Option<FlexSize>,
    // Behaviors
    #[serde(flatten)]
    pub animation: AnimationConfig,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

crate::impl_traits!(Flex {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Container for Flex {
    fn children(&self) -> &[ChildComponent] {
        &self.children
    }
}

impl FlexContainer for Flex {
    fn flex_config(&self) -> &FlexConfig {
        // We need to construct a FlexConfig from LayerStyle on the fly.
        // Since FlexContainer returns a reference, we use a thread-local for the config.
        // This is a workaround - the layout code will be updated to read from LayerStyle directly.
        unreachable!("Use style directly for flex config")
    }
}

impl FlexContainerMut for Flex {
    fn flex_config_mut(&mut self) -> &mut FlexConfig {
        unreachable!("Use style directly for flex config")
    }
}

impl Bordered for Flex {
    fn border(&self) -> Option<&Border> {
        // Border in LayerStyle uses CardBorder, but trait uses Border.
        // They have the same shape. We use unsafe transmute or just return None and handle in render.
        None // Handled directly in render via self.style.border
    }
}

impl BorderedMut for Flex {
    fn set_border(&mut self, _border: Option<Border>) {}
}

impl Rounded for Flex {
    fn corner_radius(&self) -> f32 {
        self.style.border_radius_or(12.0)
    }
}

impl RoundedMut for Flex {
    fn set_corner_radius(&mut self, radius: f32) {
        self.style.border_radius = Some(radius);
    }
}

impl Shadowed for Flex {
    fn shadow(&self) -> Option<&Shadow> {
        // CardShadow and Shadow have the same shape
        None // Handled directly in render via self.style.box_shadow
    }
}

impl ShadowedMut for Flex {
    fn set_shadow(&mut self, _shadow: Option<Shadow>) {}
}

impl crate::traits::Backgrounded for Flex {
    fn background(&self) -> Option<&str> {
        self.style.background.as_deref()
    }
}

impl crate::traits::BackgroundedMut for Flex {
    fn set_background(&mut self, bg: Option<String>) {
        self.style.background = bg;
    }
}

impl crate::traits::Clipped for Flex {
    fn clip(&self) -> bool {
        true
    }
}

impl Widget for Flex {
    fn render(&self, canvas: &Canvas, layout: &LayoutNode, ctx: &RenderContext) -> Result<()> {
        let corner_radius = self.style.border_radius_or(12.0);
        let rect = Rect::from_xywh(0.0, 0.0, layout.width, layout.height);
        let rrect = skia_safe::RRect::new_rect_xy(rect, corner_radius, corner_radius);

        // 1. Shadow
        if let Some(ref shadow) = self.style.box_shadow {
            let shadow_rect = Rect::from_xywh(
                shadow.offset_x, shadow.offset_y,
                layout.width, layout.height,
            );
            let shadow_rrect = skia_safe::RRect::new_rect_xy(
                shadow_rect, corner_radius, corner_radius,
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
        if let Some(ref bg) = self.style.background {
            let bg_paint = paint_from_hex(bg);
            canvas.draw_rrect(rrect, &bg_paint);
        }

        // 3. Clip to rounded rect for children
        canvas.save();
        canvas.clip_rrect(rrect, skia_safe::ClipOp::Intersect, true);

        // 4. Render children with animation support
        crate::engine::render_v2::render_children(canvas, &self.children, layout, ctx)?;

        canvas.restore();

        // 5. Border
        if let Some(ref border) = self.style.border {
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
        SizeDimension::Percent(_) | SizeDimension::Auto => None,
    });
    let h = size.as_ref().and_then(|s| match &s.height {
        SizeDimension::Fixed(v) => Some(*v),
        SizeDimension::Percent(_) | SizeDimension::Auto => None,
    });
    Constraints {
        min_width: w.unwrap_or(constraints.min_width),
        max_width: w.unwrap_or(constraints.max_width),
        min_height: h.unwrap_or(constraints.min_height),
        max_height: h.unwrap_or(constraints.max_height),
    }
}
