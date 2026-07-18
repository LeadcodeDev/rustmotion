// Re-export core and components for downstream users
pub use rustmotion_core as core;
pub use rustmotion_core::error;
pub use rustmotion_core::schema;
pub use rustmotion_core::traits;
pub use rustmotion_core::variables;

pub use rustmotion_components as components;

// Re-export engine from core + local extensions
pub mod engine {
    pub use rustmotion_core::engine::*;
    pub mod preload;
    pub mod render;
    pub mod world;
    pub use preload::{preextract_video_frames, prefetch_icons};
}

// Local modules
pub mod encode;
pub mod include;
pub mod loader;

#[cfg(test)]
mod tests;
