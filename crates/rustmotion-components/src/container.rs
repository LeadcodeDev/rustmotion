use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::Canvas;

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

use crate::ChildComponent;

/// Invisible flex container — the HTML `<div>` equivalent.
/// No visual defaults (no background, border-radius, or shadow).
/// Gets `display: flex` automatically, like every other layout container.
/// Accepts `"type": "div"` as an alias in JSON.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ContainerComponent {
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

rustmotion_core::impl_traits!(ContainerComponent {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

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
