use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::Canvas;

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

use crate::ChildComponent;

/// Positioned container — children are placed at fixed absolute coordinates.
/// Like Flutter's Stack/Positioned: each child uses its `position: {x, y}` field.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Positioned {
    #[serde(default)]
    pub children: Vec<ChildComponent>,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
}

rustmotion_core::impl_traits!(Positioned {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Painter for Positioned {
    fn paint_content(
        &self,
        _canvas: &Canvas,
        _layout: &BoxLayout,
        _props: &rustmotion_core::engine::animator::AnimatedProperties,
        _ctx: &PaintCtx,
    ) {
        // Containers paint nothing of their own. Box decorations are
        // handled by paint_pass; children are recursed by paint_tree.
    }
}
