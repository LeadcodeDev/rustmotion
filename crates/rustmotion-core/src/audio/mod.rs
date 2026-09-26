pub mod dsp;
pub mod score;
pub mod synth;
pub mod voices;

pub use score::{CompressorConfig, MasterBus, Score, ScoreError, ScoreEvent};
pub use synth::{render, SYNTH_SAMPLE_RATE};
pub use voices::{FilterKind, FilterSpec, FreqSpec, OscKind, Voice};
