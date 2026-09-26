use skia_safe::Canvas;

use crate::engine::animator::AnimatedProperties;
use crate::engine::box_tree::AvailableSpace;
use crate::engine::layout_pass::BoxLayout;

#[derive(Debug, Clone, Copy)]
pub struct PaintCtx {
    pub time: f64,
    pub scenario_time: f64,
    pub scene_duration: f64,
    pub frame_index: u32,
    pub fps: u32,
    pub video_width: u32,
    pub video_height: u32,
    pub stagger_offset: f64,
}

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

pub trait Painter {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        props: &AnimatedProperties,
        ctx: &PaintCtx,
    );

    fn intrinsic_size(&self, _available: AvailableSize, _ctx: &MeasureCtx) -> Option<(f32, f32)> {
        None
    }
}
