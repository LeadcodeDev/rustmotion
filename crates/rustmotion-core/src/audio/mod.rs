//! Declarative audio synthesis (issue #331): a scenario carries its own
//! soundtrack, synthesised from oscillators/noise/filters on the *same*
//! beat grid as the cuts — no audio file required. An LLM can author a
//! [`score::Score`] (a JSON object of `voices` + `score` + `master`); it
//! cannot hand over a WAV.
//!
//! - [`dsp`] — dependency-free primitives: oscillators, a biquad filter,
//!   an ADSR envelope, a compressor, a limiter.
//! - [`voices`] — [`voices::Voice`], one instrument definition, and the
//!   code that renders one trigger of it into a grain of samples.
//! - [`score`] — [`score::Score`]/[`score::ScoreEvent`], the JSON schema
//!   for the timeline, and the [`crate::schema::time::TimePoint`]
//!   resolution that expands a repeating event into concrete hit instants.
//! - [`synth`] — [`synth::render`], the single entry point that ties the
//!   above together into a finished, mixed, mastered buffer.
//!
//! `schema::scenario::AudioConfig` (the object form of `Scenario::audio`)
//! is this module's only caller inside `rustmotion-core`; the offline
//! render-to-WAV-then-mux bridge lives in `rustmotion`'s `encode` crate,
//! which joins the synthesised buffer into the existing file-based
//! [`crate::schema::AudioTrack`] mixer rather than replacing it.

pub mod dsp;
pub mod score;
pub mod synth;
pub mod voices;

pub use score::{CompressorConfig, MasterBus, Score, ScoreError, ScoreEvent};
pub use synth::{render, SYNTH_SAMPLE_RATE};
pub use voices::{FilterKind, FilterSpec, FreqSpec, OscKind, Voice};
