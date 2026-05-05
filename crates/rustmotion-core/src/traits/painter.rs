//! `Painter` trait — replaces the old `Widget::render`. A component implements
//! `Painter` to draw its **content** onto a Skia canvas. Box decorations
//! (background, border, shadow) are handled generically by the paint pass.
//!
//! Layout is computed by taffy from the component's `CssStyle`; the resulting
//! [`BoxLayout`] is passed in.

use skia_safe::Canvas;

use crate::engine::box_tree::AvailableSpace;
use crate::engine::layout_pass::BoxLayout;

/// Frame-level info passed into every paint call.
#[derive(Debug, Clone, Copy)]
pub struct PaintCtx {
    pub time: f64,
    pub scene_duration: f64,
    pub frame_index: u32,
    pub fps: u32,
    pub video_width: u32,
    pub video_height: u32,
    /// Stagger offset accumulated by parent containers (seconds).
    pub stagger_offset: f64,
}

/// Available space hint passed to `intrinsic_size` (proxy for taffy's enum).
#[derive(Debug, Clone, Copy)]
pub struct AvailableSize {
    pub width: AvailableSpace,
    pub height: AvailableSpace,
}

#[derive(Debug, Clone, Copy)]
pub struct MeasureCtx {
    pub video_width: u32,
    pub video_height: u32,
}

/// Component paint contract.
pub trait Painter {
    /// Paint the component's *content* into the canvas. The canvas is
    /// already translated to the content-box origin and clipped if
    /// `overflow: hidden` was set. Generic decorations (bg, border,
    /// shadow) are already painted by the engine.
    fn paint_content(&self, canvas: &Canvas, layout: &BoxLayout, ctx: &PaintCtx);

    /// Optional intrinsic measurement for leaves like `text`, `image`,
    /// `codeblock`. Return `None` to let taffy compute the size from the
    /// CSS style alone. `available` mirrors taffy's `AvailableSpace`.
    fn intrinsic_size(
        &self,
        _available: AvailableSize,
        _ctx: &MeasureCtx,
    ) -> Option<(f32, f32)> {
        None
    }
}
