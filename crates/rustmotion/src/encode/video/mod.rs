mod ffmpeg;
mod formats;
mod h264;
mod mux;
mod tasks;

pub use ffmpeg::*;
pub use formats::*;
pub use h264::*;
pub use tasks::*;

pub enum EncodeProgress {
    Rendering(u32, u32),
    Encoding(u32, u32),
    Muxing,
}
