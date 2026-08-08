use crate::error::Result;
use std::fs::File;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::error::RustmotionError;
use crate::schema::AudioTrack;

/// Sample rate `mix_audio_tracks` resamples every track to and sizes its PCM
/// output buffer from. Both downstream muxers declare this exact rate as
/// fixed metadata rather than reading it from the PCM itself: the ffmpeg
/// path (`crates/rustmotion/src/encode/video/ffmpeg.rs`, PCM input `-ar`)
/// and the minimp4 path (`crates/rustmotion/src/encode/video/mux.rs`,
/// `init_audio(_, 44100, _)`). A mismatch here does not fail loudly — it
/// plays back at the wrong speed and pitch, because the container reports
/// the declared rate while decoding PCM produced at a different one
/// (constat #2: was 48000 here vs. 44100 in both muxers, an 8.8% duration
/// drift and a half-tone pitch shift on every video with audio). `mux.rs`
/// is outside this fix's ownership boundary and still hardcodes `44100` as
/// a literal — keep it in sync with this constant if either ever changes.
pub const OUTPUT_SAMPLE_RATE: u32 = 44_100;
const TARGET_SAMPLE_RATE: u32 = OUTPUT_SAMPLE_RATE;
const TARGET_CHANNELS: u32 = 2;

/// Decode an audio file into PCM i16 samples (stereo, 44100Hz, interleaved)
pub(crate) fn decode_audio_file(path: &str) -> Result<(Vec<f32>, u32, u32)> {
    let file = File::open(path).map_err(|e| RustmotionError::AudioOpen {
        path: path.to_string(),
        reason: e.to_string(),
    })?;

    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
    {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| RustmotionError::AudioProbe {
            path: path.to_string(),
            reason: e.to_string(),
        })?;

    let mut format = probed.format;

    let track = format
        .default_track()
        .ok_or_else(|| RustmotionError::AudioNoTrack {
            path: path.to_string(),
        })?;

    let track_id = track.id;
    let sample_rate = track.codec_params.sample_rate.unwrap_or(44100);
    let channels = track
        .codec_params
        .channels
        .map(|c| c.count() as u32)
        .unwrap_or(2);

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| RustmotionError::AudioDecoder {
            path: path.to_string(),
            reason: e.to_string(),
        })?;

    let mut all_samples: Vec<f32> = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(symphonia::core::errors::Error::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(_) => break,
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(_) => continue,
        };

        let spec = *decoded.spec();
        let duration = decoded.capacity();

        let mut sample_buf = SampleBuffer::<f32>::new(duration as u64, spec);
        sample_buf.copy_interleaved_ref(decoded);

        all_samples.extend_from_slice(sample_buf.samples());
    }

    Ok((all_samples, sample_rate, channels))
}

/// Mix multiple audio tracks into a single PCM i16 buffer for minimp4.
/// Output: interleaved i16, stereo, 44100Hz.
pub fn mix_audio_tracks(tracks: &[AudioTrack], total_duration: f64) -> Result<Option<Vec<u8>>> {
    if tracks.is_empty() {
        return Ok(None);
    }

    let total_samples = (total_duration * TARGET_SAMPLE_RATE as f64).ceil() as usize;
    let mut mix_buffer = vec![0.0f32; total_samples * TARGET_CHANNELS as usize];

    for track in tracks {
        eprintln!("  Loading audio: {}", track.src);

        let (samples, src_rate, src_channels) = decode_audio_file(&track.src)?;

        // Convert to stereo if needed
        let stereo_samples = to_stereo(&samples, src_channels);

        // Resample if needed
        let resampled = if src_rate != TARGET_SAMPLE_RATE {
            resample(&stereo_samples, src_rate, TARGET_SAMPLE_RATE)
        } else {
            stereo_samples
        };

        // Calculate start and end offsets in the mix buffer
        let start_sample =
            (track.start * TARGET_SAMPLE_RATE as f64) as usize * TARGET_CHANNELS as usize;
        let end_sample = track
            .end
            .map(|e| (e * TARGET_SAMPLE_RATE as f64) as usize * TARGET_CHANNELS as usize)
            .unwrap_or(mix_buffer.len());

        let fade_in_samples = track.fade_in.unwrap_or(0.0) * TARGET_SAMPLE_RATE as f64;
        let fade_out_samples = track.fade_out.unwrap_or(0.0) * TARGET_SAMPLE_RATE as f64;

        // Mix into buffer
        let src_len = resampled.len();
        let available = end_sample.min(mix_buffer.len()) - start_sample.min(mix_buffer.len());
        let copy_len = src_len.min(available);

        for (i, &src_sample) in resampled.iter().enumerate().take(copy_len) {
            let dst_idx = start_sample + i;
            if dst_idx >= mix_buffer.len() {
                break;
            }

            let frame = i / TARGET_CHANNELS as usize;
            let current_time = track.start + (frame as f64 / TARGET_SAMPLE_RATE as f64);
            let vol = if !track.volume_keyframes.is_empty() {
                interpolate_volume_keyframes(&track.volume_keyframes, current_time)
            } else {
                track.volume
            };
            let mut sample = src_sample * vol;

            // Apply fade in
            if fade_in_samples > 0.0 && (frame as f64) < fade_in_samples {
                sample *= frame as f32 / fade_in_samples as f32;
            }

            // Apply fade out
            let total_frames = copy_len / TARGET_CHANNELS as usize;
            let frames_from_end = total_frames - frame;
            if fade_out_samples > 0.0 && (frames_from_end as f64) < fade_out_samples {
                sample *= frames_from_end as f32 / fade_out_samples as f32;
            }

            mix_buffer[dst_idx] += sample;
        }
    }

    // Convert f32 to i16 PCM (interleaved, little-endian bytes)
    let mut pcm_bytes = Vec::with_capacity(mix_buffer.len() * 2);
    for &sample in &mix_buffer {
        let clamped = sample.clamp(-1.0, 1.0);
        let i16_val = (clamped * 32767.0) as i16;
        pcm_bytes.extend_from_slice(&i16_val.to_le_bytes());
    }

    Ok(Some(pcm_bytes))
}

/// Interpolate volume at a given time using volume keyframes with easing
fn interpolate_volume_keyframes(keyframes: &[crate::schema::VolumeKeyframe], time: f64) -> f32 {
    if keyframes.is_empty() {
        return 1.0;
    }
    if time <= keyframes[0].time {
        return keyframes[0].volume;
    }
    if time >= keyframes.last().unwrap().time {
        return keyframes.last().unwrap().volume;
    }
    for i in 0..keyframes.len() - 1 {
        let kf0 = &keyframes[i];
        let kf1 = &keyframes[i + 1];
        if time >= kf0.time && time <= kf1.time {
            let duration = kf1.time - kf0.time;
            if duration < 1e-9 {
                return kf1.volume;
            }
            let t = (time - kf0.time) / duration;
            let progress = crate::engine::animator::ease(t, &kf0.easing);
            return kf0.volume + (kf1.volume - kf0.volume) * progress as f32;
        }
    }
    keyframes.last().unwrap().volume
}

fn to_stereo(samples: &[f32], channels: u32) -> Vec<f32> {
    match channels {
        1 => {
            let mut stereo = Vec::with_capacity(samples.len() * 2);
            for &s in samples {
                stereo.push(s);
                stereo.push(s);
            }
            stereo
        }
        2 => samples.to_vec(),
        n => {
            // Downmix to stereo: take first two channels
            let mut stereo = Vec::with_capacity(samples.len() / n as usize * 2);
            for chunk in samples.chunks(n as usize) {
                stereo.push(chunk.first().copied().unwrap_or(0.0));
                stereo.push(chunk.get(1).copied().unwrap_or(chunk[0]));
            }
            stereo
        }
    }
}

fn resample(samples: &[f32], src_rate: u32, dst_rate: u32) -> Vec<f32> {
    if src_rate == dst_rate {
        return samples.to_vec();
    }

    use rubato::{
        Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
    };

    let channels = 2usize;
    let src_frames = samples.len() / channels;

    let params = SincInterpolationParameters {
        sinc_len: 256,
        f_cutoff: 0.95,
        interpolation: SincInterpolationType::Linear,
        oversampling_factor: 256,
        window: WindowFunction::BlackmanHarris2,
    };

    let ratio = dst_rate as f64 / src_rate as f64;
    let chunk_size = 1024.min(src_frames);

    let mut resampler = match SincFixedIn::<f64>::new(ratio, 2.0, params, chunk_size, channels) {
        Ok(r) => r,
        Err(_) => {
            // Fallback to linear interpolation if rubato fails to initialize
            return resample_linear(samples, src_rate, dst_rate);
        }
    };

    // Deinterleave samples into per-channel vectors
    let mut channel_data: Vec<Vec<f64>> = (0..channels)
        .map(|_| Vec::with_capacity(src_frames))
        .collect();
    for (i, &s) in samples.iter().enumerate() {
        channel_data[i % channels].push(s as f64);
    }

    let mut output_channels: Vec<Vec<f64>> = vec![Vec::new(); channels];

    // Process in chunks
    let mut pos = 0;
    while pos + chunk_size <= src_frames {
        let chunk: Vec<Vec<f64>> = channel_data
            .iter()
            .map(|ch| ch[pos..pos + chunk_size].to_vec())
            .collect();

        match resampler.process(&chunk, None) {
            Ok(out) => {
                for (ch, data) in out.iter().enumerate() {
                    output_channels[ch].extend_from_slice(data);
                }
            }
            Err(_) => break,
        }
        pos += chunk_size;
    }

    // Process remaining samples
    if pos < src_frames {
        let remaining = src_frames - pos;
        let chunk: Vec<Vec<f64>> = channel_data
            .iter()
            .map(|ch| {
                let mut v = ch[pos..].to_vec();
                v.resize(chunk_size, 0.0);
                v
            })
            .collect();

        if let Ok(out) = resampler.process(&chunk, None) {
            let expected_out = (remaining as f64 * ratio).ceil() as usize;
            for (ch, data) in out.iter().enumerate() {
                let take = expected_out.min(data.len());
                output_channels[ch].extend_from_slice(&data[..take]);
            }
        }
    }

    // Re-interleave
    let out_frames = output_channels[0].len();
    let mut result = Vec::with_capacity(out_frames * channels);
    for i in 0..out_frames {
        for ch in &output_channels {
            result.push(ch.get(i).copied().unwrap_or(0.0) as f32);
        }
    }

    result
}

fn resample_linear(samples: &[f32], src_rate: u32, dst_rate: u32) -> Vec<f32> {
    let ratio = dst_rate as f64 / src_rate as f64;
    let channels = 2usize;
    let src_frames = samples.len() / channels;
    let dst_frames = (src_frames as f64 * ratio) as usize;
    let mut result = Vec::with_capacity(dst_frames * channels);

    for frame in 0..dst_frames {
        let src_pos = frame as f64 / ratio;
        let src_frame = src_pos as usize;
        let frac = (src_pos - src_frame as f64) as f32;

        for ch in 0..channels {
            let idx0 = src_frame * channels + ch;
            let idx1 = ((src_frame + 1) * channels + ch).min(samples.len() - 1);

            let s0 = samples.get(idx0).copied().unwrap_or(0.0);
            let s1 = samples.get(idx1).copied().unwrap_or(s0);

            result.push(s0 + (s1 - s0) * frac);
        }
    }

    result
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Write a minimal, hand-rolled canonical PCM WAV file (16-bit, mono) —
    /// no ffmpeg and no extra crate needed, `symphonia`'s built-in WAV demuxer
    /// decodes this directly.
    fn write_minimal_wav(path: &std::path::Path, sample_rate: u32, num_samples: u32) {
        let bits_per_sample: u16 = 16;
        let num_channels: u16 = 1;
        let byte_rate = sample_rate * num_channels as u32 * bits_per_sample as u32 / 8;
        let block_align = num_channels * bits_per_sample / 8;
        let data_size = num_samples * block_align as u32;

        let mut buf = Vec::with_capacity(44 + data_size as usize);
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&(36 + data_size).to_le_bytes());
        buf.extend_from_slice(b"WAVE");
        buf.extend_from_slice(b"fmt ");
        buf.extend_from_slice(&16u32.to_le_bytes());
        buf.extend_from_slice(&1u16.to_le_bytes()); // PCM
        buf.extend_from_slice(&num_channels.to_le_bytes());
        buf.extend_from_slice(&sample_rate.to_le_bytes());
        buf.extend_from_slice(&byte_rate.to_le_bytes());
        buf.extend_from_slice(&block_align.to_le_bytes());
        buf.extend_from_slice(&bits_per_sample.to_le_bytes());
        buf.extend_from_slice(b"data");
        buf.extend_from_slice(&data_size.to_le_bytes());
        // Silence is a fine fixture: this test exercises PCM buffer sizing,
        // not audio content.
        buf.extend(std::iter::repeat_n(0u8, data_size as usize));

        std::fs::write(path, &buf).expect("write fixture wav");
    }

    /// Constat #2: `mix_audio_tracks` resamples to `TARGET_SAMPLE_RATE` and
    /// sizes its output buffer from it, but both downstream muxers declare a
    /// *different*, hardcoded rate as the PCM's metadata:
    /// `crates/rustmotion/src/encode/video/ffmpeg.rs` ("-ar 44100") and
    /// `crates/rustmotion/src/encode/video/mux.rs` (`init_audio(_, 44100,
    /// _)`). A mismatch plays the mixed track back at the wrong speed and
    /// desyncs it from the video (measured: +8.8% duration drift, pitch
    /// shifted down a half-tone). This test ties the mixer's output size
    /// directly to `TARGET_SAMPLE_RATE` so a regression back to a rate the
    /// muxers don't expect fails loudly here instead of silently at
    /// playback.
    #[test]
    fn mixed_pcm_is_sized_for_the_rate_both_muxers_declare() {
        assert_eq!(
            TARGET_SAMPLE_RATE, 44_100,
            "both muxers (ffmpeg.rs '-ar 44100', mux.rs init_audio(.., 44100, ..)) \
             declare 44100Hz as fixed metadata — TARGET_SAMPLE_RATE must match or \
             every video with audio plays back at the wrong speed"
        );

        let wav_path = std::env::temp_dir().join(format!(
            "rm_audio_rate_test_{}_{}.wav",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        // Source at a rate different from the target, to also exercise the
        // resampler rather than short-circuiting on a same-rate copy.
        write_minimal_wav(&wav_path, 22_050, 22_050);

        let track = AudioTrack {
            src: wav_path.to_str().unwrap().to_string(),
            start: 0.0,
            end: None,
            volume: 1.0,
            fade_in: None,
            fade_out: None,
            volume_keyframes: Vec::new(),
        };

        let total_duration = 1.0_f64;
        let pcm = mix_audio_tracks(&[track], total_duration)
            .expect("mix must succeed")
            .expect("must return Some(pcm) for a non-empty track list");

        let expected_len = (total_duration * TARGET_SAMPLE_RATE as f64).ceil() as usize
            * TARGET_CHANNELS as usize
            * 2; // i16 = 2 bytes/sample
        assert_eq!(
            pcm.len(),
            expected_len,
            "PCM buffer length must be computed from TARGET_SAMPLE_RATE={TARGET_SAMPLE_RATE}; \
             a caller that assumes 44100Hz (both muxers do) will read this buffer at the \
             wrong duration/pitch if the constant disagrees"
        );

        let _ = std::fs::remove_file(&wav_path);
    }
}
