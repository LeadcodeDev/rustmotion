use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::Canvas;

use crate::layout::{layout_flex, Constraints, LayoutNode};
use crate::schema::LayerStyle;
use crate::traits::{
    Border, Bordered, BorderedMut, Container, FlexConfig, FlexContainer,
    FlexContainerMut, RenderContext, Rounded, RoundedMut, Shadow, Shadowed, ShadowedMut,
    TimingConfig, Widget,
};

use super::flex::FlexSize;
use super::ChildComponent;

/// Invisible container — groups children and applies transforms (scale, opacity, etc.)
/// to the group as a whole. Equivalent of an HTML `<div>` with no visual styling.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ContainerComponent {
    #[serde(default)]
    pub children: Vec<ChildComponent>,
    #[serde(default)]
    pub size: Option<FlexSize>,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

crate::impl_traits!(ContainerComponent {
    Animatable => style,
    Timed => timing,
    Styled => style,
});

impl Container for ContainerComponent {
    fn children(&self) -> &[ChildComponent] {
        &self.children
    }
}

impl FlexContainer for ContainerComponent {
    fn flex_config(&self) -> &FlexConfig {
        unreachable!("Use style directly for flex config")
    }
}

impl FlexContainerMut for ContainerComponent {
    fn flex_config_mut(&mut self) -> &mut FlexConfig {
        unreachable!("Use style directly for flex config")
    }
}

impl Bordered for ContainerComponent {
    fn border(&self) -> Option<&Border> {
        None
    }
}

impl BorderedMut for ContainerComponent {
    fn set_border(&mut self, _border: Option<Border>) {}
}

impl Rounded for ContainerComponent {
    fn corner_radius(&self) -> f32 {
        self.style.border_radius_or(0.0)
    }
}

impl RoundedMut for ContainerComponent {
    fn set_corner_radius(&mut self, radius: f32) {
        self.style.border_radius = Some(radius);
    }
}

impl Shadowed for ContainerComponent {
    fn shadow(&self) -> Option<&Shadow> {
        None
    }
}

impl ShadowedMut for ContainerComponent {
    fn set_shadow(&mut self, _shadow: Option<Shadow>) {}
}

impl crate::traits::Backgrounded for ContainerComponent {
    fn background(&self) -> Option<&str> {
        None
    }
}

impl crate::traits::BackgroundedMut for ContainerComponent {
    fn set_background(&mut self, _bg: Option<String>) {}
}

impl crate::traits::Clipped for ContainerComponent {
    fn clip(&self) -> bool {
        false
    }
}

impl Widget for ContainerComponent {
    fn render(&self, canvas: &Canvas, layout: &LayoutNode, ctx: &RenderContext, _props: &crate::engine::animator::AnimatedProperties) -> Result<()> {
        // No background, no border, no shadow, no clip — just render children
        crate::engine::render::render_children_with_stagger(canvas, &self.children, layout, ctx, self.style.stagger)
    }

    fn measure(&self, constraints: &Constraints) -> (f32, f32) {
        let layout = self.layout(constraints);
        (layout.width, layout.height)
    }

    fn layout(&self, constraints: &Constraints) -> LayoutNode {
        let c = super::flex::resolve_size_constraints(&self.size, constraints);
        layout_flex(self, &c)
    }
}
