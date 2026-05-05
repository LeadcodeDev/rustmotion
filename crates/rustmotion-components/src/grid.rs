use rustmotion_core::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, Rect};

use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::paint_from_hex;
use rustmotion_core::schema::LayerStyle;
use rustmotion_core::traits::{Border, Bordered, BorderedMut, Container, GridConfig, GridContainer, GridContainerMut, PaintCtx, Painter, Rounded, RoundedMut, Shadow, Shadowed, ShadowedMut, TimingConfig};

use crate::flex::FlexSize;
use crate::ChildComponent;

/// Grid container — children are positioned via CSS-like grid layout.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Grid {
    #[serde(default)]
    pub children: Vec<ChildComponent>,
    #[serde(default)]
    pub size: Option<FlexSize>,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

rustmotion_core::impl_traits!(Grid {
    Animatable => style,
    Timed => timing,
    Styled => style,
});

impl Container for Grid {}

impl GridContainer for Grid {
    fn grid_config(&self) -> &GridConfig {
        unreachable!("Use style directly for grid config")
    }
}

impl GridContainerMut for Grid {
    fn grid_config_mut(&mut self) -> &mut GridConfig {
        unreachable!("Use style directly for grid config")
    }
}

impl Bordered for Grid {
    fn border(&self) -> Option<&Border> {
        None // Handled directly in render via self.style.border
    }
}

impl BorderedMut for Grid {
    fn set_border(&mut self, _border: Option<Border>) {}
}

impl Rounded for Grid {
    fn corner_radius(&self) -> f32 {
        self.style.border_radius_or(12.0)
    }
}

impl RoundedMut for Grid {
    fn set_corner_radius(&mut self, radius: f32) {
        self.style.border_radius = Some(radius);
    }
}

impl Shadowed for Grid {
    fn shadow(&self) -> Option<&Shadow> {
        None // Handled directly in render via self.style.box_shadow
    }
}

impl ShadowedMut for Grid {
    fn set_shadow(&mut self, _shadow: Option<Shadow>) {}
}

impl rustmotion_core::traits::Backgrounded for Grid {
    fn background(&self) -> Option<&str> {
        self.style.background.as_deref()
    }
}

impl rustmotion_core::traits::BackgroundedMut for Grid {
    fn set_background(&mut self, bg: Option<String>) {
        self.style.background = bg;
    }
}

impl rustmotion_core::traits::Clipped for Grid {
    fn clip(&self) -> bool {
        true
    }
}

impl Painter for Grid {
    fn paint_content(
        &self,
        _canvas: &Canvas,
        _layout: &BoxLayout,
        _props: &rustmotion_core::engine::animator::AnimatedProperties,
        _ctx: &PaintCtx,
    ) {
    }
}
