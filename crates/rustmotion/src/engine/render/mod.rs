mod background;
mod canvas_guard;
mod scene;

pub(crate) use canvas_guard::CanvasGuard;

#[allow(unused_imports)]
pub use scene::{
    deserialize_children, prepare_scene, render_frame_v2, render_frame_v2_scaled,
    render_scene_bg_scaled, render_scene_fg_scaled, render_scene_frame, render_scene_frame_scaled,
    render_scene_frame_scaled_with_prev_bg, render_scene_hits, render_world_frame_scaled,
    root_style,
};
