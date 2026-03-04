use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::Canvas;

use crate::layout::{Constraints, LayoutNode};
use crate::traits::{Container, RenderContext, StyleConfig, Widget};

use super::ChildComponent;

/// Stack container — children are positioned absolutely (like CSS `position: absolute`).
/// Replaces the old `Group` layer.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Stack {
    #[serde(default)]
    pub layers: Vec<ChildComponent>,
    #[serde(flatten)]
    pub style: StyleConfig,
}

crate::impl_traits!(Stack {
    Styled => style,
});

impl Container for Stack {
    fn children(&self) -> &[ChildComponent] {
        &self.layers
    }
}

impl Widget for Stack {
    fn render(&self, canvas: &Canvas, layout: &LayoutNode, ctx: &RenderContext) -> Result<()> {
        // Stack renders children with animation support
        crate::engine::render_v2::render_children(canvas, &self.layers, layout, ctx)?;

        Ok(())
    }

    fn measure(&self, _constraints: &Constraints) -> (f32, f32) {
        // Stack takes the size of its constraints (or bounding box of children)
        let mut max_x = 0.0f32;
        let mut max_y = 0.0f32;
        for child in &self.layers {
            let (cw, ch) = child.component.as_widget().measure(_constraints);
            let (cx, cy) = child.absolute_position().unwrap_or((0.0, 0.0));
            max_x = max_x.max(cx + cw);
            max_y = max_y.max(cy + ch);
        }
        (max_x, max_y)
    }
}
