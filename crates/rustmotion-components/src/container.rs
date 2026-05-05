use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::Canvas;

use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::schema::LayerStyle;
use rustmotion_core::traits::{Border, Bordered, BorderedMut, PaintCtx, Painter, Rounded, RoundedMut, Shadow, Shadowed, ShadowedMut, TimingConfig};

use crate::flex::FlexSize;
use crate::ChildComponent;

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

rustmotion_core::impl_traits!(ContainerComponent {
    Animatable => style,
    Timed => timing,
    Styled => style,
});

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

impl rustmotion_core::traits::Backgrounded for ContainerComponent {
    fn background(&self) -> Option<&str> {
        None
    }
}

impl rustmotion_core::traits::BackgroundedMut for ContainerComponent {
    fn set_background(&mut self, _bg: Option<String>) {}
}

impl rustmotion_core::traits::Clipped for ContainerComponent {
    fn clip(&self) -> bool {
        false
    }
}

impl Painter for ContainerComponent {
    fn paint_content(
        &self,
        _canvas: &Canvas,
        _layout: &BoxLayout,
        _props: &rustmotion_core::engine::animator::AnimatedProperties,
        _ctx: &PaintCtx,
    ) {
    }
}
