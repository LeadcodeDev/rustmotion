use anyhow::Result;
use std::fs::File;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::schema::AudioTrack;

const TARGET_SAMPLE_RATE: u32 = 48000;
const TARGET_CHANNELS: u32 = 2;

/// Decode an audio file into PCM i16 samples (stereo, 44100Hz, interleaved)
fn decode_audio_file(path: &str) -> Result<(Vec<f32>, u32, u32)> {
    let file = File::open(path)
        .map_err(|e| anyhow::anyhow!("Failed to open audio file '{}': {}", path, e))?;

    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = std::path::Path::new(path).extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| anyhow::anyhow!("Failed to probe audio format for '{}': {}", path, e))?;

    let mut format = probed.format;

    let track = format
        .default_track()
        .ok_or_else(|| anyhow::anyhow!("No audio track found in '{}'", path))?;

    let track_id = track.id;
    let sample_rate = track.codec_params.sample_rate.unwrap_or(44100);
    let channels = track.codec_params.channels.map(|c| c.count() as u32).unwrap_or(2);

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| anyhow::anyhow!("Failed to create decoder for '{}': {}", path, e))?;

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
        let start_sample = (track.start * TARGET_SAMPLE_RATE as f64) as usize * TARGET_CHANNELS as usize;
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

        for i in 0..copy_len {
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
            let mut sample = resampled[i] * vol;

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

    use rubato::{Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction};

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
    let mut channel_data: Vec<Vec<f64>> = vec![Vec::with_capacity(src_frames); channels];
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

        match resampler.process(&chunk, None) {
            Ok(out) => {
                let expected_out = (remaining as f64 * ratio).ceil() as usize;
                for (ch, data) in out.iter().enumerate() {
                    let take = expected_out.min(data.len());
                    output_channels[ch].extend_from_slice(&data[..take]);
                }
            }
            Err(_) => {}
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
