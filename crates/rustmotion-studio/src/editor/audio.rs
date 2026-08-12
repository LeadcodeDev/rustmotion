//! Preview audio for the transport bar.
//!
//! What plays is the output of [`mix_audio_tracks`] — the same function the
//! encoder calls — so `start`/`end`, fades, per-track volume and volume
//! keyframes are heard in the studio exactly as they will be muxed. Anything
//! less would make the preview lie about the cut.
//!
//! The mix spans the whole scenario, silence included, which is what lets the
//! playhead follow the audio clock from the first frame to the last instead of
//! drifting against a timer (and freezing when a track ends early).
//!
//! cpal's output stream is not `Send`, so it lives on a dedicated thread driven
//! by a command channel; position and state are published as atomics the UI
//! reads without ever touching that thread.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use rustmotion::schema::ResolvedScenario;

enum Cmd {
    /// Interleaved stereo f32 for the whole scenario, and its sample rate.
    Load(Arc<Vec<f32>>, u32),
    /// Drop the current mix — the scenario has no audio at all.
    Clear,
    PlayFrom(f64),
    Stop,
    SetMuted(bool),
}

/// Playhead position in milliseconds, valid only while [`PLAYING`] is true.
static POSITION_MS: AtomicU64 = AtomicU64::new(0);
static PLAYING: AtomicBool = AtomicBool::new(false);
static HAS_AUDIO: AtomicBool = AtomicBool::new(false);
static MUTED: AtomicBool = AtomicBool::new(false);
/// Bumped per prepared scenario so a slow mix that finishes after a newer one
/// started cannot install itself over the newer result.
static MIX_TOKEN: AtomicUsize = AtomicUsize::new(0);

static TX: OnceLock<Option<Sender<Cmd>>> = OnceLock::new();

fn tx() -> Option<&'static Sender<Cmd>> {
    TX.get_or_init(spawn_audio_thread).as_ref()
}

fn spawn_audio_thread() -> Option<Sender<Cmd>> {
    let (tx, rx) = channel::<Cmd>();
    std::thread::Builder::new()
        .name("rm-preview-audio".into())
        .spawn(move || {
            // No output device (CI, headless, a machine with sound disabled) is
            // not an error: keep draining so senders never block, just silent.
            let stream = match rodio::stream::DeviceSinkBuilder::from_default_device()
                .and_then(|b| b.open_stream())
            {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("rustmotion-studio: no audio output device ({e}); preview is silent");
                    while rx.recv().is_ok() {}
                    return;
                }
            };

            let mut mix: Option<(Arc<Vec<f32>>, u32)> = None;
            let mut sink: Option<rodio::Player> = None;
            let mut start_offset = 0.0f64;

            loop {
                // Short timeout so the published position stays fresh enough to
                // drive the playhead at any sane fps.
                match rx.recv_timeout(Duration::from_millis(10)) {
                    Ok(Cmd::Load(pcm, rate)) => {
                        mix = Some((pcm, rate));
                        HAS_AUDIO.store(true, Ordering::Relaxed);
                    }
                    Ok(Cmd::Clear) => {
                        mix = None;
                        sink = None;
                        HAS_AUDIO.store(false, Ordering::Relaxed);
                        PLAYING.store(false, Ordering::Relaxed);
                    }
                    Ok(Cmd::PlayFrom(seconds)) => {
                        sink = None;
                        let Some((pcm, rate)) = mix.as_ref() else {
                            continue;
                        };
                        let frame = (seconds.max(0.0) * *rate as f64) as usize;
                        let offset = (frame * CHANNELS as usize).min(pcm.len());
                        let s = rodio::Player::connect_new(stream.mixer());
                        s.set_volume(if MUTED.load(Ordering::Relaxed) {
                            0.0
                        } else {
                            1.0
                        });
                        s.append(rodio::buffer::SamplesBuffer::new(
                            channel_count(),
                            sample_rate(*rate),
                            pcm[offset..].to_vec(),
                        ));
                        s.play();
                        start_offset = seconds.max(0.0);
                        sink = Some(s);
                        POSITION_MS.store((start_offset * 1000.0) as u64, Ordering::Relaxed);
                        PLAYING.store(true, Ordering::Relaxed);
                    }
                    Ok(Cmd::Stop) => {
                        sink = None;
                        PLAYING.store(false, Ordering::Relaxed);
                    }
                    Ok(Cmd::SetMuted(m)) => {
                        if let Some(s) = sink.as_ref() {
                            s.set_volume(if m { 0.0 } else { 1.0 });
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => return,
                }

                if let Some(s) = sink.as_ref() {
                    if s.empty() {
                        // Ran off the end of the mix: stop claiming a position
                        // so the transport falls back to its own clock.
                        sink = None;
                        PLAYING.store(false, Ordering::Relaxed);
                    } else {
                        let pos = start_offset + s.get_pos().as_secs_f64();
                        POSITION_MS.store((pos * 1000.0) as u64, Ordering::Relaxed);
                    }
                }
            }
        })
        .ok()?;
    Some(tx)
}

const CHANNELS: u16 = 2;

fn channel_count() -> rodio::ChannelCount {
    rodio::ChannelCount::new(CHANNELS).expect("stereo is non-zero")
}

fn sample_rate(rate: u32) -> rodio::SampleRate {
    rodio::SampleRate::new(rate).unwrap_or(rodio::SampleRate::new(44_100).unwrap())
}

/// Mix the scenario's audio in the background and hand it to the audio thread.
/// Cheap to call on every model reload: a scenario with no tracks just clears.
pub fn prepare(scenario: Arc<ResolvedScenario>, total_duration: f64) {
    let Some(tx) = tx() else { return };
    if scenario.audio.is_empty() {
        let _ = tx.send(Cmd::Clear);
        return;
    }
    let token = MIX_TOKEN.fetch_add(1, Ordering::SeqCst) + 1;
    let tx = tx.clone();
    // Mixing decodes and resamples every track: seconds on a long scenario, so
    // never on the UI thread.
    std::thread::spawn(move || {
        let mixed = rustmotion::encode::audio::mix_audio_tracks(&scenario.audio, total_duration);
        if MIX_TOKEN.load(Ordering::SeqCst) != token {
            return; // a newer scenario is already being prepared
        }
        match mixed {
            Ok(Some(pcm_bytes)) => {
                let pcm: Vec<f32> = pcm_bytes
                    .chunks_exact(2)
                    .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
                    .collect();
                let _ = tx.send(Cmd::Load(Arc::new(pcm), SAMPLE_RATE));
            }
            Ok(None) => {
                let _ = tx.send(Cmd::Clear);
            }
            Err(e) => {
                eprintln!("rustmotion-studio: preview audio unavailable: {e}");
                let _ = tx.send(Cmd::Clear);
            }
        }
    });
}

const SAMPLE_RATE: u32 = rustmotion::encode::audio::OUTPUT_SAMPLE_RATE;

/// Start (or restart) playback at `frame`. A no-op when the scenario is silent.
///
/// The position is published here rather than waiting for the audio thread to
/// pick the command up: a seek must not read back as the old position for the
/// handful of milliseconds in between, or the transport — which follows this
/// clock — would snap the playhead back before the sound catches up.
pub fn play_from_frame(frame: u32, fps: u32) {
    let seconds = frame as f64 / fps.max(1) as f64;
    POSITION_MS.store((seconds * 1000.0) as u64, Ordering::Relaxed);
    PLAYING.store(true, Ordering::Relaxed);
    if let Some(tx) = tx() {
        let _ = tx.send(Cmd::PlayFrom(seconds));
    }
}

pub fn stop() {
    if let Some(tx) = tx() {
        let _ = tx.send(Cmd::Stop);
    }
}

pub fn set_muted(muted: bool) {
    MUTED.store(muted, Ordering::Relaxed);
    if let Some(tx) = tx() {
        let _ = tx.send(Cmd::SetMuted(muted));
    }
}

pub fn has_audio() -> bool {
    HAS_AUDIO.load(Ordering::Relaxed)
}

/// The frame the audio is currently at, or `None` when nothing is playing —
/// in which case the transport advances on its own timer.
pub fn position_frame(fps: u32) -> Option<u32> {
    if !PLAYING.load(Ordering::Relaxed) {
        return None;
    }
    let secs = POSITION_MS.load(Ordering::Relaxed) as f64 / 1000.0;
    Some((secs * fps.max(1) as f64).round() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_is_none_while_stopped() {
        PLAYING.store(false, Ordering::Relaxed);
        assert_eq!(position_frame(30), None);
    }

    #[test]
    fn position_converts_milliseconds_to_frames() {
        PLAYING.store(true, Ordering::Relaxed);
        POSITION_MS.store(2_000, Ordering::Relaxed);
        assert_eq!(position_frame(30), Some(60));
        POSITION_MS.store(0, Ordering::Relaxed);
        assert_eq!(position_frame(30), Some(0));
        PLAYING.store(false, Ordering::Relaxed);
    }

    /// fps 0 would divide by zero on the way in and out.
    #[test]
    fn zero_fps_is_clamped() {
        PLAYING.store(true, Ordering::Relaxed);
        POSITION_MS.store(1_000, Ordering::Relaxed);
        assert_eq!(position_frame(0), Some(1));
        PLAYING.store(false, Ordering::Relaxed);
    }
}
