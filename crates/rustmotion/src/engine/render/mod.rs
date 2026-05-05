mod background;
mod canvas_guard;
mod scene;

pub(crate) use canvas_guard::CanvasGuard;

#[allow(unused_imports)]
pub use scene::{
    render_frame_v2, render_frame_v2_scaled,
    render_scene_frame, render_scene_frame_scaled,
    render_scene_frame_scaled_with_prev_bg,
    render_world_frame_scaled,
    render_scene_bg_scaled, render_scene_fg_scaled,
    compute_root_layout, compute_root_layout_all_flow,
    prepare_scene, deserialize_children, root_style,
};
