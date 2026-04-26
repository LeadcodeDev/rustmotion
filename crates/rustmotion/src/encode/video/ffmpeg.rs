use rayon::prelude::*;
use std::io::Write;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::engine::prefetch_icons;
use crate::error::{Result, RustmotionError};
use crate::schema::ResolvedScenario as Scenario;

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
    mut on_progress: Option<&mut dyn FnMut(EncodeProgress)>,
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
        let audio_path_str = audio_path.to_str().ok_or_else(|| RustmotionError::NonUtf8Path {
            path: audio_path.to_string_lossy().into_owned(),
        })?;
        cmd.args([
            "-f", "s16le", "-ar", "44100", "-ac", "2", "-i",
            audio_path_str,
            "-c:a", "aac", "-b:a", "128k",
        ]);
    }

    cmd.arg(output_path);
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::null());
    // Always capture stderr so failures surface a useful diagnostic. We tee to
    // the user terminal in non-quiet mode below by reading the captured buffer
    // only on failure.
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| RustmotionError::FfmpegSpawn { reason: e.to_string() })?;

    let mut stdin = child.stdin.take()
        .ok_or(RustmotionError::FfmpegPipe)?;
    let stderr_handle = child.stderr.take();

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
        if let Some(ref mut cb) = on_progress {
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

    if let Some(ref mut cb) = on_progress {
        cb(EncodeProgress::Muxing);
    }

    let status = child.wait()
        .map_err(|e| RustmotionError::FfmpegWait { reason: e.to_string() })?;

    // Drain stderr (best effort)
    let stderr_text = stderr_handle.map(|mut h| {
        use std::io::Read;
        let mut s = String::new();
        let _ = h.read_to_string(&mut s);
        s
    });

    if let Some(ref tmp_dir) = audio_tmp_dir {
        let _ = std::fs::remove_dir_all(tmp_dir);
    }

    if let Some(e) = pipe_error {
        if !quiet {
            if let Some(ref text) = stderr_text {
                if !text.trim().is_empty() {
                    eprintln!("{}", text);
                }
            }
        }
        return Err(e);
    }

    if !status.success() {
        // Extract the last few lines of stderr — ffmpeg's actual error message
        // typically appears in the final 5-10 lines.
        let stderr_summary = stderr_text.as_ref().map(|s| {
            let lines: Vec<&str> = s.lines().rev().take(8).collect();
            lines.into_iter().rev().collect::<Vec<_>>().join("\n")
        }).filter(|s| !s.trim().is_empty());

        if !quiet {
            if let Some(ref text) = stderr_text {
                if !text.trim().is_empty() {
                    eprintln!("{}", text);
                }
            }
        }
        return Err(RustmotionError::FfmpegFailed { stderr: stderr_summary }.into());
    }

    Ok(())
}
