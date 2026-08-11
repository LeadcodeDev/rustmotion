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
///
/// Equivalent to [`mix_audio_tracks_segment`] with `segment_start = 0.0` and
/// `segment_duration = total_duration` — i.e. "the whole scenario is the
/// segment". Kept as its own entry point for API stability (existing
/// callers, this module's own unit test).
pub fn mix_audio_tracks(tracks: &[AudioTrack], total_duration: f64) -> Result<Option<Vec<u8>>> {
    mix_audio_tracks_segment(tracks, total_duration, 0.0, total_duration)
}

/// Mix multiple audio tracks, but only materialize the samples that fall
/// inside `[segment_start, segment_start + segment_duration)` of the
/// *scenario's* own timeline — i.e. sample 0 of the returned buffer is
/// `segment_start` seconds into the scenario, not into each track.
///
/// This is what makes a frame-range render (`rustmotion render --frames
/// a-b`) carry the audio that actually plays at that point in the full
/// scenario instead of the audio from t=0: without it, every segment's mux
/// step called the same `mix_audio_tracks(tracks, segment_duration)` that
/// the whole-video path uses, which always places sample 0 of every track
/// at sample 0 of the output — correct for a full render, silently wrong
/// for a segment starting anywhere past frame 0.
///
/// `scenario_total_duration` is deliberately a separate parameter from
/// `segment_duration`: a track with no explicit `end` plays until the end
/// of the *scenario*, not the end of this segment. Bounding it by
/// `segment_duration` instead would make every segment boundary look like
/// the track's own natural end and trigger its `fade_out` early, once per
/// segment, instead of once at the point it actually ends.
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

    // Absolute sample index (interleaved) of the segment's first sample
    // within the scenario's own timeline — the anchor every track's
    // scenario-relative `start`/`end` is translated against below.
    let segment_offset_samples =
        (segment_start * TARGET_SAMPLE_RATE as f64).round() as i64 * TARGET_CHANNELS as i64;

    // Bound used when a track has no explicit `end`, and a clamp on an
    // explicit one: the scenario's own length, never this segment's.
    let scenario_samples = (scenario_total_duration * TARGET_SAMPLE_RATE as f64).ceil() as usize
        * TARGET_CHANNELS as usize;

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

        // Track start/end, in absolute samples on the *scenario* timeline
        // (not yet translated into this segment's buffer).
        let track_start_abs =
            (track.start * TARGET_SAMPLE_RATE as f64) as usize * TARGET_CHANNELS as usize;
        let track_end_abs = track
            .end
            .map(|e| (e * TARGET_SAMPLE_RATE as f64) as usize * TARGET_CHANNELS as usize)
            .unwrap_or(scenario_samples)
            .min(scenario_samples);

        let fade_in_samples = track.fade_in.unwrap_or(0.0) * TARGET_SAMPLE_RATE as f64;
        let fade_out_samples = track.fade_out.unwrap_or(0.0) * TARGET_SAMPLE_RATE as f64;

        // How much of the track is ever audible in the scenario, regardless
        // of which segment we are materializing right now. Fades are
        // computed against this, not against the segment's own bounds.
        let src_len = resampled.len();
        let available = track_end_abs.saturating_sub(track_start_abs);
        let copy_len = src_len.min(available);
        let total_frames = copy_len / TARGET_CHANNELS as usize;

        for (i, &src_sample) in resampled.iter().enumerate().take(copy_len) {
            // Absolute position of this sample on the scenario timeline,
            // translated into this segment's own buffer coordinates.
            let abs_idx = track_start_abs as i64 + i as i64;
            let dst_idx = abs_idx - segment_offset_samples;
            if dst_idx < 0 {
                // Before this segment's window — earlier segments (or a
                // future re-render of an earlier range) own this sample.
                continue;
            }
            let dst_idx = dst_idx as usize;
            if dst_idx >= mix_buffer.len() {
                // Past this segment's window. `abs_idx` only increases as
                // `i` does, so nothing later in this track falls inside
                // this segment either.
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

            // Apply fade out — against the track's own total audible
            // frames in the scenario, not this segment's length.
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

    /// Write a mono PCM WAV with deterministic, non-silent content:
    /// `sample[i] = ((i % 2000) - 1000) * 30`. Unlike `write_minimal_wav`'s
    /// silence, an offset applied to this content is detectable — silence
    /// shifted by any amount is still silence, which would make a
    /// byte-equality check pass trivially even with a broken offset.
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
        buf.extend_from_slice(&1u16.to_le_bytes()); // PCM
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

    /// The core proof of the frame-range audio fix (brief's constat #2): a
    /// segment starting partway through the scenario must carry the audio
    /// that actually plays at that point, not audio restarted from t=0.
    ///
    /// Mixing one 2.0s track for the whole scenario in a single call must
    /// produce byte-identical PCM to mixing the *same* track in three
    /// independent segment calls (0.7s + 0.7s + 0.6s) and concatenating the
    /// results — each boundary lands on a whole sample count at 44100Hz
    /// (30870 / 30870 / 26460, summing exactly to 88200), so nothing here
    /// can hide behind rounding. `fade_in`/`fade_out` are set on the track
    /// specifically to also prove fades key off the *scenario*'s bound, not
    /// each segment's own edges (a segment boundary must never look like
    /// the track's natural end and trigger an early fade-out).
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

        // The equality above is only meaningful if the fixture is not
        // silent — otherwise it would hold trivially regardless of offsets.
        assert!(
            whole.iter().any(|&b| b != 0),
            "fixture must contain non-silent audio or the byte-equality check above proves nothing"
        );

        let _ = std::fs::remove_file(&wav_path);
    }

    /// A track's explicit `end` is scenario-relative. A segment sitting
    /// entirely after that `end` must be silent — proving `track_end_abs`
    /// clamps to the *scenario* bound (needed so a track with no `end` at
    /// all keeps playing across segment boundaries) without also letting a
    /// track that DOES have an end ignore it past its own segment.
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

        // Segment [1.0, 2.0) sits entirely after the track's own end.
        let seg = mix_audio_tracks_segment(&tracks, scenario_duration, 1.0, 1.0)
            .expect("mix must succeed")
            .expect("must return Some(pcm)");
        assert!(
            seg.iter().all(|&b| b == 0),
            "a segment entirely after the track's own `end` must be silent"
        );

        let _ = std::fs::remove_file(&wav_path);
    }

    /// Reproduces the brief's exact bug shape, quantified. Before
    /// `mix_audio_tracks_segment` existed, `mux_h264_to_mp4` /
    /// `encode_with_ffmpeg_hw` had no offset to give the mixer at all —
    /// every segment's mux step could only call `mix_audio_tracks(tracks,
    /// segment_duration)`, which is exactly `mix_audio_tracks_segment`
    /// with an implicit `segment_start = 0.0`. For a second segment that
    /// actually starts at t=0.7s in the scenario, that call mixes the
    /// track as if the *segment itself* were the whole timeline starting
    /// at 0 — i.e. "a segment starting at frame 300 would receive the
    /// audio from the start of the scenario" (the brief's own framing).
    /// This test calls the old, still-present, offset-less
    /// `mix_audio_tracks` the way that old mux code path would have, and
    /// shows — with an actual byte-difference count, not just "it's
    /// different" — how far that is from the correct windowed segment.
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

        // Segment 2, correctly windowed: [0.7s, 2.0s) of the scenario.
        let segment_start = 0.7_f64;
        let segment_duration = 1.3_f64;
        let correct =
            mix_audio_tracks_segment(&tracks, scenario_duration, segment_start, segment_duration)
                .expect("mix must succeed")
                .expect("must return Some(pcm)");

        // The bug: mixing the same segment's own duration with no offset —
        // exactly the call shape available before this fix existed.
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
}
