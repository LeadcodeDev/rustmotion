use rayon::prelude::*;
use std::io::Write;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::encode::audio_analysis::analyze_scenario_audio;
use crate::engine::prefetch_icons;
use crate::error::{Result, RustmotionError};
use crate::schema::ResolvedScenario as Scenario;

use super::tasks::{build_frame_tasks, render_frame_task};
use super::EncodeProgress;

/// Assemble FFmpeg's argument vector.
///
/// The order is load-bearing. FFmpeg parses argv positionally: an option applies
/// to the *next* `-i` that follows it, or to the output when no input follows. So
/// the whole input section — including the audio input and its `-f s16le -ar -ac`
/// — has to be emitted before the first output option. Emitting the codec block
/// between the two inputs makes ffmpeg reject `-profile:v` as an input option for
/// audio.raw and refuse to start, which silently broke every scenario carrying an
/// audio track.
///
/// Kept separate from the spawn so the ordering invariant is unit-testable without
/// an ffmpeg binary on the machine.
#[allow(clippy::too_many_arguments)]
fn ffmpeg_args(
    width: u32,
    height: u32,
    fps: u32,
    codec: &str,
    crf_val: u8,
    transparent: bool,
    audio_input: Option<&str>,
    output_path: &str,
) -> Vec<String> {
    let size = format!("{}x{}", width, height);
    let framerate = fps.to_string();
    let crf = crf_val.to_string();
    let mut args: Vec<String> = Vec::new();
    fn push(xs: &[&str], out: &mut Vec<String>) {
        out.extend(xs.iter().map(|s| (*s).to_string()))
    }

    // ---- inputs ------------------------------------------------------------
    push(&["-y", "-loglevel", "error"], &mut args);
    push(&["-f", "rawvideo", "-pixel_format", "rgba"], &mut args);
    push(&["-video_size", &size], &mut args);
    push(&["-framerate", &framerate], &mut args);
    push(&["-i", "pipe:0"], &mut args);

    if let Some(path) = audio_input {
        push(
            &["-f", "s16le", "-ar", "44100", "-ac", "2", "-i", path],
            &mut args,
        );
    }

    // ---- output options ----------------------------------------------------
    let alpha_fmt = |with: &'static str, without: &'static str| {
        if transparent {
            with
        } else {
            without
        }
    };
    match codec {
        "h265" | "hevc" => {
            push(
                &["-c:v", "libx265", "-crf", &crf, "-preset", "medium"],
                &mut args,
            );
            push(
                &["-pix_fmt", alpha_fmt("yuva420p", "yuv420p")],
                &mut args,
            );
        }
        "vp9" => {
            push(
                &["-c:v", "libvpx-vp9", "-crf", &crf, "-b:v", "0"],
                &mut args,
            );
            push(
                &["-pix_fmt", alpha_fmt("yuva420p", "yuv420p")],
                &mut args,
            );
        }
        "prores" => {
            push(&["-c:v", "prores_ks", "-profile:v", "4"], &mut args);
            push(
                &["-pix_fmt", alpha_fmt("yuva444p10le", "yuv422p10le")],
                &mut args,
            );
        }
        _ => {
            push(
                &[
                    "-c:v",
                    "libx264",
                    "-crf",
                    &crf,
                    "-preset",
                    "medium",
                    "-profile:v",
                    "high10",
                    "-pix_fmt",
                    "yuv420p10le",
                ],
                &mut args,
            );
        }
    }

    if audio_input.is_some() {
        push(&["-c:a", "aac", "-b:a", "128k"], &mut args);
    }

    args.push(output_path.to_string());
    args
}

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
    analyze_scenario_audio(scenario);

    let tasks = build_frame_tasks(scenario);
    let total_frames = tasks.len() as u32;

    if total_frames == 0 {
        return Err(RustmotionError::NoFrames);
    }

    // Process audio — merge scenario.audio with tracks extracted from embedded
    // video components.
    let total_duration = total_frames as f64 / fps as f64;
    let video_tracks = super::super::video_audio::collect_video_audio_tracks(scenario);
    let merged_audio: Vec<crate::schema::AudioTrack> = {
        let mut all = scenario.audio.clone();
        all.extend(video_tracks);
        all
    };

    let audio_tmp_dir = if !merged_audio.is_empty() {
        Some(std::env::temp_dir().join(format!("rustmotion_audio_{}", std::process::id())))
    } else {
        None
    };
    let pcm_data = if !merged_audio.is_empty() {
        if let Some(ref tmp_dir) = audio_tmp_dir {
            std::fs::create_dir_all(tmp_dir)?;
        }
        super::super::audio::mix_audio_tracks(&merged_audio, total_duration)?
    } else {
        None
    };

    // Materialise the mixed PCM before the command is assembled: the audio input
    // has to be declared next to the video input, ahead of every output option.
    let audio_input: Option<String> = match (&pcm_data, &audio_tmp_dir) {
        (Some(pcm), Some(tmp_dir)) => {
            let audio_path = tmp_dir.join("audio.raw");
            std::fs::write(&audio_path, pcm)?;
            Some(
                audio_path
                    .to_str()
                    .ok_or_else(|| RustmotionError::NonUtf8Path {
                        path: audio_path.to_string_lossy().into_owned(),
                    })?
                    .to_owned(),
            )
        }
        _ => None,
    };

    // Build FFmpeg command
    let crf_val = crf.unwrap_or(23);

    let mut cmd = std::process::Command::new("ffmpeg");
    cmd.args(ffmpeg_args(
        width,
        height,
        fps,
        codec,
        crf_val,
        transparent,
        audio_input.as_deref(),
        output_path,
    ));
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::null());
    // Always capture stderr so failures surface a useful diagnostic. We tee to
    // the user terminal in non-quiet mode below by reading the captured buffer
    // only on failure.
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| RustmotionError::FfmpegSpawn {
        reason: e.to_string(),
    })?;

    let mut stdin = child.stdin.take().ok_or(RustmotionError::FfmpegPipe)?;
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
                        pipe_error = Some(RustmotionError::FfmpegWrite {
                            reason: e.to_string(),
                            stderr: None, // filled in below, once stderr is drained
                        });
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

    let status = child.wait().map_err(|e| RustmotionError::FfmpegWait {
        reason: e.to_string(),
    })?;

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

    // ffmpeg's actual complaint sits in the last few lines of stderr. Build the
    // summary once: every failure path needs it, and `--quiet` must not be the
    // difference between a diagnosable error and "Broken pipe".
    let stderr_summary = stderr_text
        .as_ref()
        .map(|s| {
            let lines: Vec<&str> = s.lines().rev().take(8).collect();
            lines.into_iter().rev().collect::<Vec<_>>().join("\n")
        })
        .filter(|s| !s.trim().is_empty());

    let tee_stderr = || {
        if !quiet {
            if let Some(ref text) = stderr_text {
                if !text.trim().is_empty() {
                    eprintln!("{}", text);
                }
            }
        }
    };

    if let Some(e) = pipe_error {
        tee_stderr();
        // A broken pipe means ffmpeg is already gone — its own error says why,
        // ours only says we could not keep writing. Carry both.
        return Err(match e {
            RustmotionError::FfmpegWrite { reason, .. } => RustmotionError::FfmpegWrite {
                reason,
                stderr: stderr_summary,
            },
            other => other,
        });
    }

    if !status.success() {
        tee_stderr();
        return Err(RustmotionError::FfmpegFailed {
            stderr: stderr_summary,
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::ffmpeg_args;

    /// Every option that describes the *output* has to sit after the last `-i`.
    /// Put one before it and ffmpeg attaches it to the following input instead,
    /// then aborts with "Option ... cannot be applied to input url".
    const OUTPUT_OPTS: [&str; 6] = ["-c:v", "-crf", "-preset", "-profile:v", "-c:a", "-b:a"];

    fn input_positions(args: &[String]) -> Vec<usize> {
        args.iter()
            .enumerate()
            .filter(|(_, s)| s.as_str() == "-i")
            .map(|(i, _)| i)
            .collect()
    }

    #[test]
    fn the_audio_input_is_declared_before_every_output_option() {
        for codec in ["h264", "h265", "vp9", "prores"] {
            let args = ffmpeg_args(320, 240, 30, codec, 23, false, Some("/tmp/a.raw"), "o.mp4");
            let inputs = input_positions(&args);
            assert_eq!(inputs.len(), 2, "{codec}: expected a video and an audio input");

            // The audio input keeps its own format options immediately ahead of it.
            let audio_i = inputs[1];
            assert_eq!(args[audio_i - 1], "2", "{codec}: -ac lost before audio -i");
            assert_eq!(args[audio_i + 1], "/tmp/a.raw");

            for opt in OUTPUT_OPTS {
                if let Some(pos) = args.iter().position(|s| s == opt) {
                    assert!(
                        pos > audio_i,
                        "{codec}: {opt} is emitted at {pos}, before the audio input at {audio_i} — \
                         ffmpeg would read it as an option of audio.raw and refuse to start"
                    );
                }
            }
            assert_eq!(args.last().unwrap(), "o.mp4", "{codec}: output must be last");
        }
    }

    #[test]
    fn a_silent_scenario_declares_a_single_input_and_no_audio_codec() {
        let args = ffmpeg_args(320, 240, 30, "h264", 23, false, None, "o.mp4");
        assert_eq!(input_positions(&args).len(), 1);
        assert!(!args.iter().any(|s| s == "-c:a" || s == "-b:a"));
        assert_eq!(args.last().unwrap(), "o.mp4");
    }

    #[test]
    fn transparency_selects_an_alpha_pixel_format() {
        for (codec, opaque, alpha) in [
            ("h265", "yuv420p", "yuva420p"),
            ("vp9", "yuv420p", "yuva420p"),
            ("prores", "yuv422p10le", "yuva444p10le"),
        ] {
            let pix = |t: bool| {
                let a = ffmpeg_args(320, 240, 30, codec, 23, t, None, "o.mov");
                let i = a.iter().position(|s| s == "-pix_fmt").unwrap();
                a[i + 1].clone()
            };
            assert_eq!(pix(false), opaque, "{codec} opaque");
            assert_eq!(pix(true), alpha, "{codec} transparent");
        }
    }
}
