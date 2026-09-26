use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::Canvas;

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

use crate::ChildComponent;

/// The single layout container — the HTML `<div>` equivalent. Lays out
/// `children` via flex (default) or grid, decorated purely by `style`
/// (`background`, `border-radius`, `box-shadow`, ... — all opt-in, none
/// applied by default) and gets `display: flex` automatically when `style`
/// doesn't set one, like every other layout container.
///
/// Six spellings of `"type"` all deserialize into this same struct and
/// render identically: `"div"` (canonical), plus `"container"`, `"card"`,
/// `"flex"`, `"grid"`, and `"positioned"` kept as aliases for backward
/// compatibility. None of them carries different behavior — a `"card"`
/// with no `background` set is exactly as undecorated as a `"div"`, and a
/// `"positioned"` is exactly as capable of flex/grid flow as any other
/// spelling; `position: {x, y}` on a child works the same way inside all
/// six, since it's a property of the child, not of the container.
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
