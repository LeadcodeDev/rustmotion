use crate::error::Result;
use skia_safe::Canvas;

use crate::engine::animator::AnimatedProperties;
use crate::layout::{Constraints, LayoutNode};

/// Context passed to every component during the render pass.
#[derive(Debug, Clone)]
pub struct RenderContext {
    /// Current time within the scene (seconds).
    pub time: f64,
    /// Total scene duration (seconds).
    pub scene_duration: f64,
    /// Current frame index within the scene.
    pub frame_index: u32,
    /// Video FPS.
    pub fps: u32,
    /// Video canvas width.
    pub video_width: u32,
    /// Video canvas height.
    pub video_height: u32,
    /// Extra animation delay offset from parent stagger (seconds).
    pub stagger_offset: f64,
}

/// Trait to break the cycle between components and the render engine.
/// Containers call `pipeline.render_children()` instead of importing `crate::engine::render::*`.
pub trait RenderPipeline {
    fn render_children(
        &self,
        canvas: &Canvas,
        children: &dyn std::any::Any,
        layout: &LayoutNode,
        ctx: &RenderContext,
        stagger: Option<f32>,
    ) -> Result<()>;
}

/// Base trait for all renderable components.
pub trait Widget {
    /// Render this component onto the Skia canvas.
    /// The canvas is already translated/transformed by the layout engine.
    fn render(
        &self,
        canvas: &Canvas,
        layout: &LayoutNode,
        ctx: &RenderContext,
        props: &AnimatedProperties,
        pipeline: &dyn RenderPipeline,
    ) -> Result<()>;

    /// Measure this component given parent constraints.
    /// Returns (width, height) — the desired size within the constraints.
    fn measure(&self, constraints: &Constraints) -> (f32, f32);

    /// Compute the full layout tree for this component.
    /// Containers override this to return a LayoutNode with children.
    /// Leaf components return a simple node with their measured size.
    fn layout(&self, constraints: &Constraints) -> LayoutNode {
        let (w, h) = self.measure(constraints);
        LayoutNode::new(0.0, 0.0, w, h)
    }
}
