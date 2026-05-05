use rustmotion_core::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::Canvas;

use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::layout::{Constraints, LayoutNode};
use rustmotion_core::schema::LayerStyle;
use rustmotion_core::traits::{Container, PaintCtx, Painter, RenderContext, Widget};

use crate::ChildComponent;

/// Positioned container — children are placed at fixed absolute coordinates.
/// Like Flutter's Stack/Positioned: each child uses its `position: {x, y}` field.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Positioned {
    #[serde(default)]
    pub children: Vec<ChildComponent>,
    #[serde(default)]
    pub style: LayerStyle,
}

rustmotion_core::impl_traits!(Positioned {
    Styled => style,
});

impl Container for Positioned {}

impl Widget for Positioned {
    fn layout(&self, constraints: &Constraints) -> LayoutNode {
        rustmotion_core::layout::stack::layout_stack(&self.children, &self.style, constraints)
    }

    fn render(&self, canvas: &Canvas, layout: &LayoutNode, ctx: &RenderContext, _props: &rustmotion_core::engine::animator::AnimatedProperties, pipeline: &dyn rustmotion_core::traits::RenderPipeline) -> Result<()> {
        pipeline.render_children(canvas, &self.children as &dyn std::any::Any, layout, ctx, None)?;
        Ok(())
    }

    fn measure(&self, _constraints: &Constraints) -> (f32, f32) {
        let mut max_x = 0.0f32;
        let mut max_y = 0.0f32;
        for child in &self.children {
            let (cw, ch) = child.component.as_widget().measure(_constraints);
            let (cx, cy) = child.absolute_position().unwrap_or((0.0, 0.0));
            max_x = max_x.max(cx + cw);
            max_y = max_y.max(cy + ch);
        }
        (max_x, max_y)
    }
}

impl Painter for Positioned {
    fn paint_content(&self, _canvas: &Canvas, _layout: &BoxLayout, _ctx: &PaintCtx) {
        // Containers paint nothing of their own. Box decorations are
        // handled by paint_pass; children are recursed by paint_tree.
    }
}
