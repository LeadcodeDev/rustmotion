use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::Canvas;

use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::schema::{LayerStyle, SizeDimension};
use rustmotion_core::traits::{Border, Bordered, BorderedMut, Container, FlexConfig, FlexContainer, FlexContainerMut, PaintCtx, Painter, Rounded, RoundedMut, Shadow, Shadowed, ShadowedMut, TimingConfig};

use crate::ChildComponent;

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
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

rustmotion_core::impl_traits!(Flex {
    Animatable => style,
    Timed => timing,
    Styled => style,
});

impl Container for Flex {}

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

impl rustmotion_core::traits::Backgrounded for Flex {
    fn background(&self) -> Option<&str> {
        self.style.background.as_deref()
    }
}

impl rustmotion_core::traits::BackgroundedMut for Flex {
    fn set_background(&mut self, bg: Option<String>) {
        self.style.background = bg;
    }
}

impl rustmotion_core::traits::Clipped for Flex {
    fn clip(&self) -> bool {
        true
    }
}

impl Painter for Flex {
    fn paint_content(
        &self,
        _canvas: &Canvas,
        _layout: &BoxLayout,
        _props: &rustmotion_core::engine::animator::AnimatedProperties,
        _ctx: &PaintCtx,
    ) {
    }
}

