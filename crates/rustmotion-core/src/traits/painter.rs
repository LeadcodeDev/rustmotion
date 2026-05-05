//! `Painter` trait — replaces the old `Widget::render`. A component implements
//! `Painter` to draw its **content** onto a Skia canvas. Box decorations
//! (background, border, shadow) are handled generically by the paint pass.
//!
//! Layout is computed by taffy from the component's `CssStyle`; the resulting
//! [`BoxLayout`] is passed in.

use skia_safe::Canvas;

use crate::engine::animator::AnimatedProperties;
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
    ///
    /// `props` carries the per-component animation state at the current
    /// frame. Outer paint properties (transform / opacity / filter) have
    /// already been applied to the canvas by the engine via CSS overrides;
    /// `props` exposes the *internal-only* fields (`draw_progress`,
    /// `stroke_width`, `char_animation`, `visible_chars*`, `font_size`,
    /// `color`, `border_radius`, etc.) that components use to drive their
    /// own painting.
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        props: &AnimatedProperties,
        ctx: &PaintCtx,
    );

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
