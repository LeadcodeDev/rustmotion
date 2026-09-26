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

pub const OUTPUT_SAMPLE_RATE: u32 = 44_100;
const TARGET_SAMPLE_RATE: u32 = OUTPUT_SAMPLE_RATE;
const TARGET_CHANNELS: u32 = 2;

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
    let header_sample_rate = track.codec_params.sample_rate;
    let header_channels = track.codec_params.channels.map(|c| c.count() as u32);

    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| RustmotionError::AudioDecoder {
            path: path.to_string(),
            reason: e.to_string(),
        })?;

    let mut all_samples: Vec<f32> = Vec::new();
    let mut decoded_spec: Option<(u32, u32)> = None;

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

        if decoded_spec.is_none() {
            decoded_spec = Some((spec.rate, spec.channels.count() as u32));
        }
    }

    let (sample_rate, channels) =
        resolve_decoded_spec(decoded_spec, header_sample_rate, header_channels);

    Ok((all_samples, sample_rate, channels))
}

fn resolve_decoded_spec(
    decoded_spec: Option<(u32, u32)>,
    header_sample_rate: Option<u32>,
    header_channels: Option<u32>,
) -> (u32, u32) {
    decoded_spec.unwrap_or((
        header_sample_rate.unwrap_or(44100),
        header_channels.unwrap_or(2),
    ))
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AudioProbe {
    pub duration_secs: f64,
    pub sample_rate: u32,
    pub channels: u32,
}

pub fn probe_audio_metadata(path: &str) -> Result<AudioProbe> {
    let (samples, sample_rate, channels) = decode_audio_file(path)?;
    let channels = channels.max(1);
    let frames = samples.len() as f64 / channels as f64;
    let duration_secs = frames / sample_rate.max(1) as f64;
    Ok(AudioProbe {
        duration_secs,
        sample_rate,
        channels,
    })
}

pub fn mix_audio_tracks(tracks: &[AudioTrack], total_duration: f64) -> Result<Option<Vec<u8>>> {
    mix_audio_tracks_segment(tracks, total_duration, 0.0, total_duration)
}

pub fn mix_audio_tracks_segment(
    tracks: &[AudioTrack],
    scenario_total_duration: f64,
    segment_start: f64,
    segment_duration: f64,
) -> Result<Option<Vec<u8>>> {
    if tracks.is_empty() {
        return Ok(None);
    }

    let segment_samples = (segment_duration * TARGET_SAMPLE_RATE as f64).ceil() as usize;
    let mut mix_buffer = vec![0.0f32; segment_samples * TARGET_CHANNELS as usize];

    let segment_offset_samples =
        (segment_start * TARGET_SAMPLE_RATE as f64).round() as i64 * TARGET_CHANNELS as i64;

    let scenario_samples = (scenario_total_duration * TARGET_SAMPLE_RATE as f64).ceil() as usize
        * TARGET_CHANNELS as usize;

    for track in tracks {
        eprintln!("  Loading audio: {}", track.src);

        let (samples, src_rate, src_channels) = decode_audio_file(&track.src)?;

        let stereo_samples = to_stereo(&samples, src_channels);

        let resampled = if src_rate != TARGET_SAMPLE_RATE {
            resample(&stereo_samples, src_rate, TARGET_SAMPLE_RATE)
        } else {
            stereo_samples
        };

        let track_start_abs =
            (track.start * TARGET_SAMPLE_RATE as f64) as usize * TARGET_CHANNELS as usize;
        let track_end_abs = track
            .end
            .map(|e| (e * TARGET_SAMPLE_RATE as f64) as usize * TARGET_CHANNELS as usize)
            .unwrap_or(scenario_samples)
            .min(scenario_samples);

        let src_len = resampled.len();
        let available = track_end_abs.saturating_sub(track_start_abs);
        let copy_len = src_len.min(available);
        let total_frames = copy_len / TARGET_CHANNELS as usize;

        for (i, &src_sample) in resampled.iter().enumerate().take(copy_len) {
            let abs_idx = track_start_abs as i64 + i as i64;
            let dst_idx = abs_idx - segment_offset_samples;
            if dst_idx < 0 {
                continue;
            }
            let dst_idx = dst_idx as usize;
            if dst_idx >= mix_buffer.len() {
                break;
            }

            let frame = i / TARGET_CHANNELS as usize;
            let sample = src_sample
                * track_gain_at(
                    track,
                    frame as f64 / TARGET_SAMPLE_RATE as f64,
                    total_frames as f64 / TARGET_SAMPLE_RATE as f64,
                );

            mix_buffer[dst_idx] += sample;
        }
    }

    let mut pcm_bytes = Vec::with_capacity(mix_buffer.len() * 2);
    for &sample in &mix_buffer {
        let clamped = sample.clamp(-1.0, 1.0);
        let i16_val = (clamped * 32767.0) as i16;
        pcm_bytes.extend_from_slice(&i16_val.to_le_bytes());
    }

    Ok(Some(pcm_bytes))
}

pub(crate) fn track_gain_at(
    track: &crate::schema::AudioTrack,
    t_in_track: f64,
    audible: f64,
) -> f32 {
    let mut gain = if track.volume_keyframes.is_empty() {
        track.volume
    } else {
        interpolate_volume_keyframes(&track.volume_keyframes, track.start + t_in_track)
    };

    if let Some(fade_in) = track.fade_in {
        if fade_in > 0.0 && t_in_track < fade_in {
            gain *= (t_in_track / fade_in) as f32;
        }
    }
    if let Some(fade_out) = track.fade_out {
        let remaining = audible - t_in_track;
        if fade_out > 0.0 && remaining < fade_out {
            gain *= (remaining.max(0.0) / fade_out) as f32;
        }
    }
    gain
}

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
            return resample_linear(samples, src_rate, dst_rate);
        }
    };

    let mut channel_data: Vec<Vec<f64>> = (0..channels)
        .map(|_| Vec::with_capacity(src_frames))
        .collect();
    for (i, &s) in samples.iter().enumerate() {
        channel_data[i % channels].push(s as f64);
    }

    let mut output_channels: Vec<Vec<f64>> = vec![Vec::new(); channels];

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

fn synth_cache_path(
    cfg: &crate::schema::AudioConfig,
    ctx: &rustmotion_core::schema::time::TimeCtx,
    duration_secs: f64,
) -> std::path::PathBuf {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    let score = cfg.as_score();
    let sorted_voices: std::collections::BTreeMap<_, _> = score.voices.iter().collect();
    if let Ok(json) = serde_json::to_string(&sorted_voices) {
        json.hash(&mut hasher);
    }
    if let Ok(json) = serde_json::to_string(&score.score) {
        json.hash(&mut hasher);
    }
    if let Ok(json) = serde_json::to_string(&score.master) {
        json.hash(&mut hasher);
    }
    ctx.bpm.map(f64::to_bits).hash(&mut hasher);
    ctx.beat_offset.to_bits().hash(&mut hasher);
    duration_secs.to_bits().hash(&mut hasher);

    std::env::temp_dir().join(format!("rustmotion_synth_{:016x}.wav", hasher.finish()))
}

fn write_wav_pcm16(
    path: &std::path::Path,
    samples: &[i16],
    sample_rate: u32,
    channels: u16,
) -> std::io::Result<()> {
    let bits_per_sample: u16 = 16;
    let byte_rate = sample_rate * channels as u32 * bits_per_sample as u32 / 8;
    let block_align = channels * bits_per_sample / 8;
    let data_size = (samples.len() * 2) as u32;

    let mut buf = Vec::with_capacity(44 + data_size as usize);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&(36 + data_size).to_le_bytes());
    buf.extend_from_slice(b"WAVE");
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&channels.to_le_bytes());
    buf.extend_from_slice(&sample_rate.to_le_bytes());
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&block_align.to_le_bytes());
    buf.extend_from_slice(&bits_per_sample.to_le_bytes());
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_size.to_le_bytes());
    for &s in samples {
        buf.extend_from_slice(&s.to_le_bytes());
    }
    std::fs::write(path, &buf)
}

pub fn synthesize_score_into_track(
    resolved: &mut crate::schema::ResolvedScenario,
    cfg: &crate::schema::AudioConfig,
    scenario_bpm: Option<f64>,
    scenario_beat_offset: f64,
) -> Result<()> {
    if !cfg.has_synth() {
        return Ok(());
    }

    let ctx = rustmotion_core::schema::time::TimeCtx {
        bpm: cfg.bpm.or(scenario_bpm),
        beat_offset: cfg.beat_offset.unwrap_or(scenario_beat_offset),
        scene_start: 0.0,
    };
    let duration_secs = crate::encode::video_audio::resolved_scenario_duration(resolved);
    if duration_secs <= 0.0 {
        return Ok(());
    }

    let path = synth_cache_path(cfg, &ctx, duration_secs);
    if !path.exists() {
        let score = cfg.as_score();
        let stereo = rustmotion_core::audio::render(&score, ctx, duration_secs)?;
        let samples_i16: Vec<i16> = stereo
            .iter()
            .map(|&s| (s.clamp(-1.0, 1.0) * 32767.0) as i16)
            .collect();
        write_wav_pcm16(
            &path,
            &samples_i16,
            rustmotion_core::audio::SYNTH_SAMPLE_RATE,
            2,
        )
        .map_err(|e| RustmotionError::AudioSynthWrite {
            path: path.display().to_string(),
            reason: e.to_string(),
        })?;
    }

    resolved.audio.push(AudioTrack {
        src: path.display().to_string(),
        start: 0.0,
        end: None,
        volume: 1.0,
        fade_in: None,
        fade_out: None,
        volume_keyframes: Vec::new(),
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_decoded_spec_prefers_the_decoded_packets_own_spec_over_the_header() {
        let decoded = Some((48_000, 1));
        let header_rate = Some(44_100);
        let header_channels = Some(2);
        assert_eq!(
            resolve_decoded_spec(decoded, header_rate, header_channels),
            (48_000, 1),
            "a packet actually decoded — all_samples came from its spec, so the \
             return value must match it, not the header's own guess"
        );
    }

    #[test]
    fn resolve_decoded_spec_falls_back_to_the_header_when_nothing_ever_decoded() {
        assert_eq!(
            resolve_decoded_spec(None, Some(22_050), Some(1)),
            (22_050, 1)
        );
    }

    #[test]
    fn resolve_decoded_spec_falls_back_to_the_hardcoded_default_as_a_last_resort() {
        assert_eq!(resolve_decoded_spec(None, None, None), (44_100, 2));
    }

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
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&num_channels.to_le_bytes());
        buf.extend_from_slice(&sample_rate.to_le_bytes());
        buf.extend_from_slice(&byte_rate.to_le_bytes());
        buf.extend_from_slice(&block_align.to_le_bytes());
        buf.extend_from_slice(&bits_per_sample.to_le_bytes());
        buf.extend_from_slice(b"data");
        buf.extend_from_slice(&data_size.to_le_bytes());
        buf.extend(std::iter::repeat_n(0u8, data_size as usize));

        std::fs::write(path, &buf).expect("write fixture wav");
    }

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
            * 2;
        assert_eq!(
            pcm.len(),
            expected_len,
            "PCM buffer length must be computed from TARGET_SAMPLE_RATE={TARGET_SAMPLE_RATE}; \
             a caller that assumes 44100Hz (both muxers do) will read this buffer at the \
             wrong duration/pitch if the constant disagrees"
        );

        let _ = std::fs::remove_file(&wav_path);
    }

    fn write_tone_wav(path: &std::path::Path, sample_rate: u32, num_samples: u32) {
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
        buf.extend_from_slice(&1u16.to_le_bytes());
        buf.extend_from_slice(&num_channels.to_le_bytes());
        buf.extend_from_slice(&sample_rate.to_le_bytes());
        buf.extend_from_slice(&byte_rate.to_le_bytes());
        buf.extend_from_slice(&block_align.to_le_bytes());
        buf.extend_from_slice(&bits_per_sample.to_le_bytes());
        buf.extend_from_slice(b"data");
        buf.extend_from_slice(&data_size.to_le_bytes());
        for i in 0..num_samples {
            let v: i16 = (((i % 2000) as i32 - 1000) * 30) as i16;
            buf.extend_from_slice(&v.to_le_bytes());
        }

        std::fs::write(path, &buf).expect("write fixture wav");
    }

    #[test]
    fn segment_mixing_concatenates_to_exactly_the_whole_scenario_mix() {
        let sample_rate = TARGET_SAMPLE_RATE;
        let scenario_duration = 2.0_f64;
        let num_samples = (scenario_duration * sample_rate as f64) as u32;

        let wav_path = std::env::temp_dir().join(format!(
            "rm_audio_segment_concat_test_{}_{}.wav",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        write_tone_wav(&wav_path, sample_rate, num_samples);

        let track = AudioTrack {
            src: wav_path.to_str().unwrap().to_string(),
            start: 0.3,
            end: None,
            volume: 0.8,
            fade_in: Some(0.1),
            fade_out: Some(0.1),
            volume_keyframes: Vec::new(),
        };
        let tracks = [track];

        let whole = mix_audio_tracks_segment(&tracks, scenario_duration, 0.0, scenario_duration)
            .expect("whole mix must succeed")
            .expect("must return Some(pcm)");

        let bounds = [(0.0, 0.7), (0.7, 0.7), (1.4, 0.6)];
        let mut concatenated = Vec::new();
        for (start, duration) in bounds {
            let seg = mix_audio_tracks_segment(&tracks, scenario_duration, start, duration)
                .expect("segment mix must succeed")
                .expect("must return Some(pcm)");
            concatenated.extend_from_slice(&seg);
        }

        assert_eq!(
            concatenated.len(),
            whole.len(),
            "segment PCM lengths must sum to the whole mix's length"
        );
        assert_eq!(
            concatenated, whole,
            "concatenating three independently-mixed segments must reproduce the whole-scenario \
             mix byte-for-byte — a mismatch means a segment is carrying audio from the wrong \
             offset (the frame-range bug this function exists to close)"
        );

        assert!(
            whole.iter().any(|&b| b != 0),
            "fixture must contain non-silent audio or the byte-equality check above proves nothing"
        );

        let _ = std::fs::remove_file(&wav_path);
    }

    #[test]
    fn segment_mix_silences_a_track_after_its_own_explicit_end() {
        let sample_rate = TARGET_SAMPLE_RATE;
        let scenario_duration = 2.0_f64;
        let num_samples = (scenario_duration * sample_rate as f64) as u32;

        let wav_path = std::env::temp_dir().join(format!(
            "rm_audio_segment_end_test_{}_{}.wav",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        write_tone_wav(&wav_path, sample_rate, num_samples);

        let track = AudioTrack {
            src: wav_path.to_str().unwrap().to_string(),
            start: 0.0,
            end: Some(1.0),
            volume: 1.0,
            fade_in: None,
            fade_out: None,
            volume_keyframes: Vec::new(),
        };
        let tracks = [track];

        let seg = mix_audio_tracks_segment(&tracks, scenario_duration, 1.0, 1.0)
            .expect("mix must succeed")
            .expect("must return Some(pcm)");
        assert!(
            seg.iter().all(|&b| b == 0),
            "a segment entirely after the track's own `end` must be silent"
        );

        let _ = std::fs::remove_file(&wav_path);
    }

    #[test]
    fn without_the_offset_a_second_segment_would_wrongly_replay_the_track_from_the_start() {
        let sample_rate = TARGET_SAMPLE_RATE;
        let scenario_duration = 2.0_f64;
        let num_samples = (scenario_duration * sample_rate as f64) as u32;

        let wav_path = std::env::temp_dir().join(format!(
            "rm_audio_naive_bug_repro_{}_{}.wav",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        write_tone_wav(&wav_path, sample_rate, num_samples);

        let track = AudioTrack {
            src: wav_path.to_str().unwrap().to_string(),
            start: 0.3,
            end: None,
            volume: 0.8,
            fade_in: None,
            fade_out: None,
            volume_keyframes: Vec::new(),
        };
        let tracks = [track];

        let segment_start = 0.7_f64;
        let segment_duration = 1.3_f64;
        let correct =
            mix_audio_tracks_segment(&tracks, scenario_duration, segment_start, segment_duration)
                .expect("mix must succeed")
                .expect("must return Some(pcm)");

        let buggy = mix_audio_tracks(&tracks, segment_duration)
            .expect("mix must succeed")
            .expect("must return Some(pcm)");

        assert_eq!(
            correct.len(),
            buggy.len(),
            "same segment duration, so same buffer size — the bug is about content, not length"
        );

        let differing_bytes = correct
            .iter()
            .zip(buggy.iter())
            .filter(|(a, b)| a != b)
            .count();
        let total_bytes = correct.len();
        eprintln!(
            "without the offset, {differing_bytes}/{total_bytes} bytes \
             ({:.1}%) of segment 2 would have been wrong",
            100.0 * differing_bytes as f64 / total_bytes as f64
        );
        assert!(
            differing_bytes * 4 > total_bytes,
            "expected the offset-less (buggy) mix to differ substantially from the correctly \
             windowed segment — only {differing_bytes}/{total_bytes} bytes differed, which would \
             mean the offset barely matters (it should matter for nearly the whole buffer here)"
        );

        let _ = std::fs::remove_file(&wav_path);
    }

    #[test]
    fn probe_audio_metadata_reports_duration_rate_and_channels() {
        let wav_path = std::env::temp_dir().join(format!(
            "rm_audio_probe_test_{}_{}.wav",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        write_minimal_wav(&wav_path, 22_050, 11_025);

        let probe = probe_audio_metadata(wav_path.to_str().unwrap()).expect("must probe wav");
        assert_eq!(probe.sample_rate, 22_050);
        assert_eq!(probe.channels, 1);
        assert!(
            (probe.duration_secs - 0.5).abs() < 0.01,
            "duration: {}",
            probe.duration_secs
        );

        let _ = std::fs::remove_file(&wav_path);
    }

    #[test]
    fn probe_audio_metadata_on_a_missing_file_is_an_error_not_a_panic() {
        let path = std::env::temp_dir().join(format!(
            "rm_audio_probe_missing_{}_{}.wav",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let result = probe_audio_metadata(path.to_str().unwrap());
        assert!(
            matches!(result, Err(RustmotionError::AudioOpen { .. })),
            "expected AudioOpen for a missing file, got: {result:?}"
        );
    }

    #[test]
    fn probe_audio_metadata_on_garbage_bytes_is_an_error_not_a_panic() {
        let path = std::env::temp_dir().join(format!(
            "rm_audio_probe_garbage_{}_{}.wav",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, b"this is not an audio file at all").unwrap();
        let result = probe_audio_metadata(path.to_str().unwrap());
        assert!(
            result.is_err(),
            "unreadable content must be an error, not a panic: {result:?}"
        );
        let _ = std::fs::remove_file(&path);
    }
}
