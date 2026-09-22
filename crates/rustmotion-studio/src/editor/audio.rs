use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use rustmotion::schema::{AudioTrack, ResolvedScenario};

enum Cmd {
    Load(Arc<Vec<f32>>, u32),
    Clear,
    PlayFrom(f64),
    Stop,
    SetMuted(bool),
}

static POSITION_MS: AtomicU64 = AtomicU64::new(0);
static PLAYING: AtomicBool = AtomicBool::new(false);
static HAS_AUDIO: AtomicBool = AtomicBool::new(false);
static MUTED: AtomicBool = AtomicBool::new(false);
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

pub fn audio_fingerprint(audio: &[AudioTrack], total_duration: f64) -> u64 {
    let mut hasher = DefaultHasher::new();
    serde_json::to_string(audio)
        .unwrap_or_default()
        .hash(&mut hasher);
    total_duration.to_bits().hash(&mut hasher);
    hasher.finish()
}

pub fn prepare(scenario: Arc<ResolvedScenario>, total_duration: f64) {
    let Some(tx) = tx() else { return };
    if scenario.audio.is_empty() {
        let _ = tx.send(Cmd::Clear);
        return;
    }
    let token = MIX_TOKEN.fetch_add(1, Ordering::SeqCst) + 1;
    let tx = tx.clone();
    std::thread::spawn(move || {
        let mixed = rustmotion::encode::audio::mix_audio_tracks(&scenario.audio, total_duration);
        if MIX_TOKEN.load(Ordering::SeqCst) != token {
            return;
        }
        match mixed {
            Ok(Some(pcm_bytes)) => {
                let pcm: Vec<f32> = pcm_bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
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

    #[test]
    fn zero_fps_is_clamped() {
        PLAYING.store(true, Ordering::Relaxed);
        POSITION_MS.store(1_000, Ordering::Relaxed);
        assert_eq!(position_frame(0), Some(1));
        PLAYING.store(false, Ordering::Relaxed);
    }
}
