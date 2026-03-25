mod tasks;
mod h264;
mod ffmpeg;
mod formats;
mod mux;

pub use tasks::*;
pub use h264::*;
pub use ffmpeg::*;
pub use formats::*;

/// Progress events emitted during encoding
pub enum EncodeProgress {
    /// Rendering frames: (current, total)
    Rendering(u32, u32),
    /// Encoding H.264: (current, total)
    Encoding(u32, u32),
    /// Muxing MP4
    Muxing,
}
