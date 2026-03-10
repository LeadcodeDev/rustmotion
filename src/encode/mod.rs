pub mod audio;
pub mod video;

pub use video::encode_video;
pub use video::encode_png_sequence;
pub use video::encode_gif;
pub use video::encode_with_ffmpeg;
pub use video::encode_raw_stdout;
pub use video::encode_video_incremental;
pub use video::hash_video_config;
pub use video::SceneSegment;
pub use video::hash_scene as hash_video_config_scene;
pub use video::EncodeProgress;
pub use video::build_frame_tasks;
pub use video::render_frame_task;
pub use video::render_frame_task_scaled;
