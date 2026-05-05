use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::Canvas;

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::schema::{AnimationEffect, TimelineStep};
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

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
    pub style: CssStyle,
    #[serde(default, deserialize_with = "rustmotion_core::schema::deserialize_animation_effects")]
    pub animation: Vec<AnimationEffect>,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
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
