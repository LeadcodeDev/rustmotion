use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::Canvas;

use crate::layout::{Constraints, LayoutNode};
use crate::schema::LayerStyle;
use crate::traits::{Container, RenderContext, Widget};

use super::ChildComponent;

/// Stack container — children are positioned absolutely (like CSS `position: absolute`).
/// Replaces the old `Group` layer.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Stack {
    #[serde(default)]
    pub children: Vec<ChildComponent>,
    #[serde(default)]
    pub style: LayerStyle,
}

crate::impl_traits!(Stack {
    Styled => style,
});

impl Container for Stack {
    fn children(&self) -> &[ChildComponent] {
        &self.children
    }
}

impl Widget for Stack {
    fn render(&self, canvas: &Canvas, layout: &LayoutNode, ctx: &RenderContext, _props: &crate::engine::animator::AnimatedProperties) -> Result<()> {
        crate::engine::render_v2::render_children(canvas, &self.children, layout, ctx)?;
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
