use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::Canvas;

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::schema::{AnimationEffect, TimelineStep};
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

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
    pub style: CssStyle,
    #[serde(default, deserialize_with = "rustmotion_core::schema::deserialize_animation_effects")]
    pub animation: Vec<AnimationEffect>,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(Card {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

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
