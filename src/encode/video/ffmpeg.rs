use rayon::prelude::*;
use std::io::Write;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::engine::prefetch_icons;
use crate::error::{Result, RustmotionError};
use crate::schema::ResolvedScenario as Scenario;
use crate::tui::TuiProgress;

use super::tasks::{build_frame_tasks, render_frame_task};
use super::EncodeProgress;

/// Encode using FFmpeg subprocess (for h265, vp9, prores, webm, mov, transparency)
pub fn encode_with_ffmpeg(
    scenario: &Scenario,
    output_path: &str,
    quiet: bool,
    codec: &str,
    crf: Option<u8>,
    transparent: bool,
) -> Result<()> {
    encode_with_ffmpeg_progress(scenario, output_path, quiet, codec, crf, transparent, None)
}

pub fn encode_with_ffmpeg_progress(
    scenario: &Scenario,
    output_path: &str,
    quiet: bool,
    codec: &str,
    crf: Option<u8>,
    transparent: bool,
    on_progress: Option<&(dyn Fn(EncodeProgress) + Send + Sync)>,
) -> Result<()> {
    let config = &scenario.video;
    let width = config.width;
    let height = config.height;
    let fps = config.fps;

    for view in &scenario.views {
        prefetch_icons(&view.scenes);
    }

    let tasks = build_frame_tasks(scenario);
    let total_frames = tasks.len() as u32;

    if total_frames == 0 {
        return Err(RustmotionError::NoFrames.into());
    }

    let mut tui = if !quiet {
        Some(TuiProgress::new(total_frames, output_path, width, height, fps, codec)?)
    } else {
        None
    };

    // Process audio
    let total_duration = total_frames as f64 / fps as f64;
    let audio_tmp_dir = if !scenario.audio.is_empty() {
        Some(std::env::temp_dir().join(format!("rustmotion_audio_{}", std::process::id())))
    } else {
        None
    };
    let pcm_data = if !scenario.audio.is_empty() {
        if let Some(ref tmp_dir) = audio_tmp_dir {
            std::fs::create_dir_all(tmp_dir)?;
        }
        super::super::audio::mix_audio_tracks(&scenario.audio, total_duration)?
    } else {
        None
    };

    // Build FFmpeg command
    let crf_val = crf.unwrap_or(23);

    let mut cmd = std::process::Command::new("ffmpeg");
    cmd.args([
        "-y",
        "-loglevel", "error",
        "-f", "rawvideo",
        "-pixel_format", "rgba",
        "-video_size", &format!("{}x{}", width, height),
        "-framerate", &fps.to_string(),
        "-i", "pipe:0",
    ]);

    match codec {
        "h265" | "hevc" => {
            cmd.args(["-c:v", "libx265", "-crf", &crf_val.to_string(), "-preset", "medium"]);
            if transparent {
                cmd.args(["-pix_fmt", "yuva420p"]);
            } else {
                cmd.args(["-pix_fmt", "yuv420p"]);
            }
        }
        "vp9" => {
            cmd.args(["-c:v", "libvpx-vp9", "-crf", &crf_val.to_string(), "-b:v", "0"]);
            if transparent {
                cmd.args(["-pix_fmt", "yuva420p"]);
            } else {
                cmd.args(["-pix_fmt", "yuv420p"]);
            }
        }
        "prores" => {
            cmd.args(["-c:v", "prores_ks", "-profile:v", "4"]);
            if transparent {
                cmd.args(["-pix_fmt", "yuva444p10le"]);
            } else {
                cmd.args(["-pix_fmt", "yuv422p10le"]);
            }
        }
        _ => {
            cmd.args(["-c:v", "libx264", "-crf", &crf_val.to_string(), "-preset", "medium", "-profile:v", "high10", "-pix_fmt", "yuv420p10le"]);
        }
    }

    if let Some(ref pcm) = pcm_data {
        let audio_path = audio_tmp_dir.as_ref().unwrap().join("audio.raw");
        std::fs::write(&audio_path, pcm)?;
        cmd.args([
            "-f", "s16le", "-ar", "44100", "-ac", "2", "-i",
            audio_path.to_str().unwrap(),
            "-c:a", "aac", "-b:a", "128k",
        ]);
    }

    cmd.arg(output_path);
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(if quiet { std::process::Stdio::null() } else { std::process::Stdio::inherit() });

    let mut child = cmd
        .spawn()
        .map_err(|e| RustmotionError::FfmpegSpawn { reason: e.to_string() })?;

    let mut stdin = child.stdin.take()
        .ok_or(RustmotionError::FfmpegPipe)?;

    // Render frames in parallel batches, pipe RGBA sequentially
    let batch_size = (rayon::current_num_threads() * 2).max(4);
    let counter = AtomicU32::new(0);
    let mut pipe_error: Option<RustmotionError> = None;

    for batch in tasks.chunks(batch_size) {
        if pipe_error.is_some() {
            break;
        }

        let results: Vec<Result<Vec<u8>>> = batch
            .par_iter()
            .map(|task| {
                let rgba = render_frame_task(config, scenario, task)?;
                counter.fetch_add(1, Ordering::Relaxed);
                Ok(rgba)
            })
            .collect();

        let rendered = counter.load(Ordering::Relaxed);
        if let Some(ref mut tui) = tui {
            tui.set_progress(rendered);
        }
        if let Some(cb) = &on_progress {
            cb(EncodeProgress::Rendering(rendered, total_frames));
        }

        for result in results {
            match result {
                Ok(rgba) => {
                    if let Err(e) = stdin.write_all(&rgba) {
                        pipe_error = Some(RustmotionError::FfmpegWrite { reason: e.to_string() });
                        break;
                    }
                }
                Err(e) => {
                    pipe_error = Some(e);
                    break;
                }
            }
        }
    }

    drop(stdin);

    if let Some(cb) = &on_progress {
        cb(EncodeProgress::Muxing);
    }

    let status = child.wait()
        .map_err(|e| RustmotionError::FfmpegWait { reason: e.to_string() })?;

    if let Some(ref tmp_dir) = audio_tmp_dir {
        let _ = std::fs::remove_dir_all(tmp_dir);
    }

    if let Some(e) = pipe_error {
        return Err(e);
    }

    if !status.success() {
        return Err(RustmotionError::FfmpegFailed.into());
    }

    if let Some(tui) = tui {
        tui.finish("Done!");
    }

    Ok(())
}
