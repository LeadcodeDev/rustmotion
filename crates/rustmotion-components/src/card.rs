use rustmotion_core::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, Rect};

use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::paint_from_hex;
use rustmotion_core::schema::{CardDisplay, LayerStyle};
use rustmotion_core::traits::{Border, Bordered, BorderedMut, Container, FlexConfig, FlexContainer, FlexContainerMut, GridConfig, PaintCtx, Painter, Rounded, RoundedMut, Shadow, Shadowed, ShadowedMut, TimingConfig};

use crate::flex::FlexSize;
use crate::ChildComponent;

/// Card container — backward-compatible with v1 `"type": "card"`.
/// Supports both flex and grid display modes via the `display` style field.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Card {
    #[serde(default)]
    pub children: Vec<ChildComponent>,
    #[serde(default)]
    pub size: Option<FlexSize>,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

rustmotion_core::impl_traits!(Card {
    Animatable => style,
    Timed => timing,
    Styled => style,
});

impl Container for Card {}

impl FlexContainer for Card {
    fn flex_config(&self) -> &FlexConfig {
        unreachable!("Use style directly for flex config")
    }
}

impl FlexContainerMut for Card {
    fn flex_config_mut(&mut self) -> &mut FlexConfig {
        unreachable!("Use style directly for flex config")
    }
}

impl Card {
    fn grid_config_owned(&self) -> GridConfig {
        GridConfig {
            grid_template_columns: self.style.grid_template_columns.clone(),
            grid_template_rows: self.style.grid_template_rows.clone(),
            gap: self.style.gap_or(0.0),
        }
    }
}

impl Bordered for Card {
    fn border(&self) -> Option<&Border> {
        None // Handled directly in render via self.style.border
    }
}

impl BorderedMut for Card {
    fn set_border(&mut self, _border: Option<Border>) {}
}

impl Rounded for Card {
    fn corner_radius(&self) -> f32 {
        self.style.border_radius_or(12.0)
    }
}

impl RoundedMut for Card {
    fn set_corner_radius(&mut self, radius: f32) {
        self.style.border_radius = Some(radius);
    }
}

impl Shadowed for Card {
    fn shadow(&self) -> Option<&Shadow> {
        None // Handled directly in render via self.style.box_shadow
    }
}

impl ShadowedMut for Card {
    fn set_shadow(&mut self, _shadow: Option<Shadow>) {}
}

impl rustmotion_core::traits::Backgrounded for Card {
    fn background(&self) -> Option<&str> {
        self.style.background.as_deref()
    }
}

impl rustmotion_core::traits::BackgroundedMut for Card {
    fn set_background(&mut self, bg: Option<String>) {
        self.style.background = bg;
    }
}

impl rustmotion_core::traits::Clipped for Card {
    fn clip(&self) -> bool {
        true
    }
}

impl Painter for Card {
    fn paint_content(
        &self,
        _canvas: &Canvas,
        _layout: &BoxLayout,
        _props: &rustmotion_core::engine::animator::AnimatedProperties,
        _ctx: &PaintCtx,
    ) {
    }
}
