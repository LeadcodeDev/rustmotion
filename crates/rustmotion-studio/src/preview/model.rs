use std::sync::{Arc, Mutex};

use rustmotion::encode::video::FrameTask;
use rustmotion::schema::ResolvedScenario;

/// Live studio state shared between the file watcher and the UI/asset handler.
/// Wrapped in `Arc<Mutex<_>>` (`Shared`) so the watcher thread can swap in a
/// reloaded scenario while the webview reads frames.
pub struct StudioModel {
    pub scenario: ResolvedScenario,
    pub tasks: Vec<FrameTask>,
    pub total_frames: u32,
    pub error: Option<String>,
    /// Bumped on every hot-reload so the UI can detect a change.
    pub generation: u64,
}

pub type Shared = Arc<Mutex<StudioModel>>;

impl StudioModel {
    pub fn new(scenario: ResolvedScenario, error: Option<String>) -> Self {
        let tasks = rustmotion::encode::build_frame_tasks(&scenario);
        let total_frames = tasks.len() as u32;
        Self {
            scenario,
            tasks,
            total_frames,
            error,
            generation: 0,
        }
    }
}
