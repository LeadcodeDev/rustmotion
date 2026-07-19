use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::Canvas;

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::schema::{SizeDimension, TimelineStep};
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

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
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
    #[serde(default)]
    pub time_scale: Option<f64>,
    #[serde(default)]
    pub time_offset: Option<f64>,
}

rustmotion_core::impl_traits!(Flex {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

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
