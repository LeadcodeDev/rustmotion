use minimp4::Mp4Muxer;
use std::fs::File;
use std::io::BufWriter;

use crate::error::Result;
use crate::schema::ResolvedScenario as Scenario;

#[allow(clippy::too_many_arguments)]
pub(super) fn mux_h264_to_mp4(
    h264_data: &[u8],
    output_path: &str,
    width: u32,
    height: u32,
    fps: u32,
    scenario: &Scenario,
    segment_duration: f64,
    scenario_total_duration: f64,
    segment_start: f64,
) -> Result<()> {
    let video_tracks = super::super::video_audio::collect_video_audio_tracks(scenario);
    let merged_audio: Vec<crate::schema::AudioTrack> = {
        let mut all = scenario.audio.clone();
        all.extend(video_tracks);
        all
    };

    let pcm_data = if !merged_audio.is_empty() {
        super::super::audio::mix_audio_tracks_segment(
            &merged_audio,
            scenario_total_duration,
            segment_start,
            segment_duration,
        )?
    } else {
        None
    };

    let file = File::create(output_path)?;
    let writer = BufWriter::new(file);
    let mut muxer = Mp4Muxer::new(writer);
    muxer.init_video(width as i32, height as i32, false, "rustmotion");
    if let Some(ref pcm) = pcm_data {
        muxer.init_audio(128000, crate::encode::audio::OUTPUT_SAMPLE_RATE, 2);
        muxer.write_video_with_audio(h264_data, fps, pcm);
    } else {
        muxer.write_video_with_fps(h264_data, fps);
    }
    muxer.close();

    Ok(())
}
