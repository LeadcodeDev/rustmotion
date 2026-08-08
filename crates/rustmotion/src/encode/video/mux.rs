use minimp4::Mp4Muxer;
use std::fs::File;
use std::io::BufWriter;

use crate::error::Result;
use crate::schema::ResolvedScenario as Scenario;

/// Mux H.264 data (with optional audio) into an MP4 file.
pub(super) fn mux_h264_to_mp4(
    h264_data: &[u8],
    output_path: &str,
    width: u32,
    height: u32,
    fps: u32,
    scenario: &Scenario,
    total_duration: f64,
) -> Result<()> {
    // Collect audio from embedded video components and merge with scenario.audio.
    let video_tracks = super::super::video_audio::collect_video_audio_tracks(scenario);
    let merged_audio: Vec<crate::schema::AudioTrack> = {
        let mut all = scenario.audio.clone();
        all.extend(video_tracks);
        all
    };

    let pcm_data = if !merged_audio.is_empty() {
        super::super::audio::mix_audio_tracks(&merged_audio, total_duration)?
    } else {
        None
    };

    let file = File::create(output_path)?;
    let writer = BufWriter::new(file);
    let mut muxer = Mp4Muxer::new(writer);
    muxer.init_video(width as i32, height as i32, false, "rustmotion");
    if let Some(ref pcm) = pcm_data {
        // Same constant the mixer resamples to. Hardcoding it here is how the
        // 48kHz/44.1kHz desync got in: two sites declared a rate, one of them
        // silently disagreed with the PCM it was handed.
        muxer.init_audio(128000, crate::encode::audio::OUTPUT_SAMPLE_RATE, 2);
        muxer.write_video_with_audio(h264_data, fps, pcm);
    } else {
        muxer.write_video_with_fps(h264_data, fps);
    }
    muxer.close();

    Ok(())
}
