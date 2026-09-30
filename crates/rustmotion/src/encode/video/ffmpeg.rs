use rayon::prelude::*;
use std::collections::HashSet;
use std::io::Write;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::encode::audio_analysis::analyze_scenario_audio;
use crate::engine::prefetch_icons;
use crate::error::{Result, RustmotionError};
use crate::schema::ResolvedScenario as Scenario;

use super::tasks::{build_frame_tasks, build_frame_tasks_range, render_frame_task};
use super::EncodeProgress;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HwEncoderFamily {
    VideoToolbox,
    Nvenc,
    Qsv,
    Amf,
}

impl HwEncoderFamily {
    const ALL: [HwEncoderFamily; 4] = [
        HwEncoderFamily::VideoToolbox,
        HwEncoderFamily::Nvenc,
        HwEncoderFamily::Qsv,
        HwEncoderFamily::Amf,
    ];

    fn encoder_name(self, codec: &str) -> Option<&'static str> {
        use HwEncoderFamily::*;
        match (self, codec) {
            (VideoToolbox, "h264") => Some("h264_videotoolbox"),
            (VideoToolbox, "h265" | "hevc") => Some("hevc_videotoolbox"),
            (Nvenc, "h264") => Some("h264_nvenc"),
            (Nvenc, "h265" | "hevc") => Some("hevc_nvenc"),
            (Qsv, "h264") => Some("h264_qsv"),
            (Qsv, "h265" | "hevc") => Some("hevc_qsv"),
            (Amf, "h264") => Some("h264_amf"),
            (Amf, "h265" | "hevc") => Some("hevc_amf"),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HardwareSelection {
    Use(String),
    NotRequested,
    Unsupported { reason: String },
    Unavailable { tried: Vec<String> },
}

fn select_hardware_encoder(
    requested: bool,
    codec: &str,
    transparent: bool,
    is_available: impl Fn(&str) -> bool,
) -> HardwareSelection {
    if !requested {
        return HardwareSelection::NotRequested;
    }
    if transparent {
        return HardwareSelection::Unsupported {
            reason: "no supported hardware encoder produces an alpha channel".to_string(),
        };
    }
    let mut tried = Vec::new();
    let mut any_family_supports_codec = false;
    for family in HwEncoderFamily::ALL {
        if let Some(name) = family.encoder_name(codec) {
            any_family_supports_codec = true;
            if is_available(name) {
                return HardwareSelection::Use(name.to_string());
            }
            tried.push(name.to_string());
        }
    }
    if !any_family_supports_codec {
        return HardwareSelection::Unsupported {
            reason: format!("no known hardware encoder exists for codec '{codec}'"),
        };
    }
    HardwareSelection::Unavailable { tried }
}

fn parse_encoder_names(text: &str) -> HashSet<String> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let flags = parts.next()?;
            if flags.len() < 2 || !flags.chars().all(|c| c == '.' || c.is_ascii_uppercase()) {
                return None;
            }
            let name = parts.next()?;
            if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return None;
            }
            Some(name.to_string())
        })
        .collect()
}

fn probe_ffmpeg_encoders() -> HashSet<String> {
    let output = std::process::Command::new("ffmpeg")
        .args(["-hide_banner", "-encoders"])
        .output();
    match output {
        Ok(out) if out.status.success() => {
            parse_encoder_names(&String::from_utf8_lossy(&out.stdout))
        }
        _ => HashSet::new(),
    }
}

fn hevc_sample_entry_tag(codec: &str, output_path: &str) -> Option<&'static str> {
    let container = std::path::Path::new(output_path)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    match (codec, container.as_deref()) {
        ("h265" | "hevc", Some("mp4" | "mov")) => Some("hvc1"),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
fn ffmpeg_args(
    width: u32,
    height: u32,
    fps: u32,
    codec: &str,
    crf_val: u8,
    transparent: bool,
    hw_encoder: Option<&str>,
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

    push(&["-y", "-loglevel", "error"], &mut args);
    push(&["-f", "rawvideo", "-pixel_format", "rgba"], &mut args);
    push(&["-video_size", &size], &mut args);
    push(&["-framerate", &framerate], &mut args);
    push(&["-i", "pipe:0"], &mut args);

    if let Some(path) = audio_input {
        let sample_rate = super::super::audio::OUTPUT_SAMPLE_RATE.to_string();
        args.extend(
            ["-f", "s16le", "-ar", &sample_rate, "-ac", "2", "-i", path]
                .into_iter()
                .map(str::to_string),
        );
    }

    let alpha_fmt = |with: &'static str, without: &'static str| {
        if transparent {
            with
        } else {
            without
        }
    };
    if let Some(hw_name) = hw_encoder {
        push(&["-c:v", hw_name], &mut args);
        push(&["-pix_fmt", "yuv420p"], &mut args);
    } else {
        match codec {
            "h265" | "hevc" => {
                push(
                    &["-c:v", "libx265", "-crf", &crf, "-preset", "medium"],
                    &mut args,
                );
                push(&["-pix_fmt", alpha_fmt("yuva420p", "yuv420p")], &mut args);
            }
            "vp9" => {
                push(
                    &["-c:v", "libvpx-vp9", "-crf", &crf, "-b:v", "0"],
                    &mut args,
                );
                push(&["-pix_fmt", alpha_fmt("yuva420p", "yuv420p")], &mut args);
            }
            "prores" => {
                push(&["-c:v", "prores_ks", "-profile:v", "4"], &mut args);
                push(
                    &["-pix_fmt", alpha_fmt("yuva444p10le", "yuv422p10le")],
                    &mut args,
                );
            }
            "h264_10bit" => {
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
                        "high",
                        "-pix_fmt",
                        "yuv420p",
                    ],
                    &mut args,
                );
            }
        }
    }

    if let Some(tag) = hevc_sample_entry_tag(codec, output_path) {
        push(&["-tag:v", tag], &mut args);
    }

    if audio_input.is_some() {
        push(&["-c:a", "aac", "-b:a", "128k"], &mut args);
    }

    args.push(output_path.to_string());
    args
}

fn audio_tmp_dir_name(pid: u32, seq: u32, nanos: u128) -> String {
    format!("rustmotion_audio_{pid}_{seq}_{nanos:x}")
}

fn ffmpeg_partial_output_path(output_path: &std::path::Path) -> std::path::PathBuf {
    let stem = output_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");
    let name = match output_path.extension().and_then(|s| s.to_str()) {
        Some(ext) => format!("{stem}.partial.{ext}"),
        None => format!("{stem}.partial"),
    };
    output_path.with_file_name(name)
}

pub fn encode_with_ffmpeg(
    scenario: &Scenario,
    output_path: &str,
    quiet: bool,
    codec: &str,
    crf: Option<u8>,
    transparent: bool,
    on_progress: Option<&mut dyn FnMut(EncodeProgress)>,
) -> Result<()> {
    encode_with_ffmpeg_hw(
        scenario,
        output_path,
        quiet,
        codec,
        crf,
        transparent,
        false,
        on_progress,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn encode_with_ffmpeg_hw(
    scenario: &Scenario,
    output_path: &str,
    quiet: bool,
    codec: &str,
    crf: Option<u8>,
    transparent: bool,
    hardware_acceleration: bool,
    on_progress: Option<&mut dyn FnMut(EncodeProgress)>,
) -> Result<()> {
    encode_with_ffmpeg_hw_impl(
        scenario,
        output_path,
        quiet,
        codec,
        crf,
        transparent,
        hardware_acceleration,
        None,
        on_progress,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn encode_with_ffmpeg_hw_range(
    scenario: &Scenario,
    output_path: &str,
    quiet: bool,
    codec: &str,
    crf: Option<u8>,
    transparent: bool,
    hardware_acceleration: bool,
    frame_range: (u32, u32),
    on_progress: Option<&mut dyn FnMut(EncodeProgress)>,
) -> Result<()> {
    encode_with_ffmpeg_hw_impl(
        scenario,
        output_path,
        quiet,
        codec,
        crf,
        transparent,
        hardware_acceleration,
        Some(frame_range),
        on_progress,
    )
}

#[allow(clippy::too_many_arguments)]
fn encode_with_ffmpeg_hw_impl(
    scenario: &Scenario,
    output_path: &str,
    quiet: bool,
    codec: &str,
    crf: Option<u8>,
    transparent: bool,
    hardware_acceleration: bool,
    frame_range: Option<(u32, u32)>,
    mut on_progress: Option<&mut dyn FnMut(EncodeProgress)>,
) -> Result<()> {
    let config = &scenario.video;
    let width = config.width;
    let height = config.height;
    let fps = config.fps;

    for view in &scenario.views {
        prefetch_icons(&view.scenes)?;
    }
    for failure in analyze_scenario_audio(scenario) {
        eprintln!("rustmotion: audio-reactive: {failure} — waveform/audio_spectrum will render flat for this track.");
    }

    let (tasks, full_total_frames, segment_start_frame) = match frame_range {
        Some((start, end)) => {
            let (tasks, total) = build_frame_tasks_range(scenario, start, end)?;
            (tasks, total, start)
        }
        None => {
            let tasks = build_frame_tasks(scenario);
            let total = tasks.len() as u32;
            if total == 0 {
                return Err(RustmotionError::NoFrames);
            }
            (tasks, total, 0)
        }
    };
    let total_frames = tasks.len() as u32;

    let scenario_total_duration = full_total_frames as f64 / fps as f64;
    let segment_duration = total_frames as f64 / fps as f64;
    let segment_start = segment_start_frame as f64 / fps as f64;
    let video_tracks = super::super::video_audio::collect_video_audio_tracks(scenario);
    let merged_audio: Vec<crate::schema::AudioTrack> = {
        let mut all = scenario.audio.clone();
        all.extend(video_tracks);
        all
    };

    static AUDIO_TMP_DIR_SEQ: AtomicU32 = AtomicU32::new(0);
    let audio_tmp_dir = if !merged_audio.is_empty() {
        let seq = AUDIO_TMP_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        Some(std::env::temp_dir().join(audio_tmp_dir_name(std::process::id(), seq, nanos)))
    } else {
        None
    };
    let pcm_data = if !merged_audio.is_empty() {
        if let Some(ref tmp_dir) = audio_tmp_dir {
            std::fs::create_dir(tmp_dir)?;
        }
        super::super::audio::mix_audio_tracks_segment(
            &merged_audio,
            scenario_total_duration,
            segment_start,
            segment_duration,
        )?
    } else {
        None
    };

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

    let crf_val = crf.unwrap_or(23);

    let available_encoders = if hardware_acceleration {
        Some(probe_ffmpeg_encoders())
    } else {
        None
    };
    let hw_selection = select_hardware_encoder(hardware_acceleration, codec, transparent, |name| {
        available_encoders
            .as_ref()
            .is_some_and(|set| set.contains(name))
    });
    let hw_encoder = match hw_selection {
        HardwareSelection::Use(name) => {
            if !quiet {
                eprintln!("Hardware acceleration: using {name}");
            }
            Some(name)
        }
        HardwareSelection::NotRequested => None,
        HardwareSelection::Unsupported { reason } => {
            if !quiet {
                eprintln!(
                    "Hardware acceleration requested but not applicable here ({reason}); \
                     continuing with the software encoder."
                );
            }
            None
        }
        HardwareSelection::Unavailable { tried } => {
            if !quiet {
                eprintln!(
                    "Hardware acceleration requested but this machine's ffmpeg does not offer \
                     any of the candidate encoders (tried: {}); continuing with the software \
                     encoder.",
                    tried.join(", ")
                );
            }
            None
        }
    };

    let partial_output_path = ffmpeg_partial_output_path(std::path::Path::new(output_path));
    let partial_output_str =
        partial_output_path
            .to_str()
            .ok_or_else(|| RustmotionError::NonUtf8Path {
                path: partial_output_path.to_string_lossy().into_owned(),
            })?;

    let mut cmd = std::process::Command::new("ffmpeg");
    cmd.args(ffmpeg_args(
        width,
        height,
        fps,
        codec,
        crf_val,
        transparent,
        hw_encoder.as_deref(),
        audio_input.as_deref(),
        partial_output_str,
    ));
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| RustmotionError::FfmpegSpawn {
        reason: e.to_string(),
    })?;

    let mut stdin = child.stdin.take().ok_or(RustmotionError::FfmpegPipe)?;

    let stderr_reader: Option<std::thread::JoinHandle<String>> =
        child.stderr.take().map(|mut h| {
            std::thread::spawn(move || {
                use std::io::Read;
                let mut s = String::new();
                let _ = h.read_to_string(&mut s);
                s
            })
        });

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
                            stderr: None,
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

    let status = if pipe_error.is_some() {
        let _ = child.kill();
        child.wait()
    } else {
        child.wait()
    }
    .map_err(|e| RustmotionError::FfmpegWait {
        reason: e.to_string(),
    })?;

    let stderr_text = stderr_reader.and_then(|h| h.join().ok());

    if let Some(ref tmp_dir) = audio_tmp_dir {
        let _ = std::fs::remove_dir_all(tmp_dir);
    }

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
        let _ = std::fs::remove_file(&partial_output_path);
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
        let _ = std::fs::remove_file(&partial_output_path);
        return Err(RustmotionError::FfmpegFailed {
            stderr: stderr_summary,
        });
    }

    std::fs::rename(&partial_output_path, output_path)?;

    Ok(())
}

pub fn concat_mp4_segments(inputs: &[std::path::PathBuf], output_path: &str) -> Result<()> {
    if inputs.is_empty() {
        return Err(RustmotionError::Generic(
            "concat requires at least one input segment".to_string(),
        ));
    }

    let mut list_contents = String::new();
    for input in inputs {
        let abs = input
            .canonicalize()
            .map_err(|e| RustmotionError::FileRead {
                path: input.display().to_string(),
                source: e,
            })?;
        let escaped = abs.to_string_lossy().replace('\'', r"'\''");
        list_contents.push_str(&format!("file '{escaped}'\n"));
    }

    let list_path = std::env::temp_dir().join(format!(
        "rustmotion_concat_{}_{}.txt",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::write(&list_path, &list_contents)?;

    let output = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-loglevel",
            "error",
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
        ])
        .arg(&list_path)
        .args(["-c", "copy"])
        .arg(output_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| RustmotionError::FfmpegSpawn {
            reason: e.to_string(),
        })?;

    let _ = std::fs::remove_file(&list_path);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let summary = stderr
            .lines()
            .rev()
            .take(8)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        return Err(RustmotionError::FfmpegFailed {
            stderr: (!summary.trim().is_empty()).then_some(summary),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        audio_tmp_dir_name, ffmpeg_args, ffmpeg_partial_output_path, parse_encoder_names,
        select_hardware_encoder, HardwareSelection,
    };

    #[test]
    fn audio_tmp_dir_name_differs_across_calls_that_share_pid_and_seq() {
        let a = audio_tmp_dir_name(1234, 0, 111);
        let b = audio_tmp_dir_name(1234, 0, 222);
        assert_ne!(
            a, b,
            "same pid+seq, different nanos, must differ: {a} vs {b}"
        );
    }

    #[test]
    fn audio_tmp_dir_name_is_stable_for_identical_inputs() {
        assert_eq!(audio_tmp_dir_name(1, 2, 3), audio_tmp_dir_name(1, 2, 3));
    }

    #[test]
    fn create_dir_refuses_an_already_occupied_path_that_create_dir_all_would_have_adopted() {
        let path = std::env::temp_dir().join(format!(
            "rustmotion_audit_ws_c_preexisting_dir_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir(&path).expect("set up a pre-existing directory at the target path");

        assert!(
            std::fs::create_dir_all(&path).is_ok(),
            "create_dir_all silently succeeding on a pre-existing directory is exactly the \
             behavior that let a hostile pre-planted directory (or symlink) be adopted"
        );
        assert!(
            std::fs::create_dir(&path).is_err(),
            "create_dir must refuse the same pre-existing path instead of adopting it"
        );

        let _ = std::fs::remove_dir_all(&path);
    }

    #[test]
    fn partial_path_keeps_the_original_extension_as_its_last_extension() {
        let cases = [
            ("/tmp/out.mp4", "/tmp/out.partial.mp4"),
            ("/tmp/out.mov", "/tmp/out.partial.mov"),
            ("/tmp/out.webm", "/tmp/out.partial.webm"),
            ("out.mp4", "out.partial.mp4"),
        ];
        for (input, expected) in cases {
            let got = ffmpeg_partial_output_path(std::path::Path::new(input));
            assert_eq!(
                got,
                std::path::PathBuf::from(expected),
                "input={input}: ffmpeg picks its muxer from the last extension, so it must \
                 survive unchanged"
            );
        }
    }

    #[test]
    fn partial_path_is_a_sibling_of_the_final_output_not_a_different_directory() {
        let got = ffmpeg_partial_output_path(std::path::Path::new("/a/b/c/out.mp4"));
        assert_eq!(
            got.parent(),
            Some(std::path::Path::new("/a/b/c")),
            "the rename onto output_path must stay on the same filesystem"
        );
    }

    #[test]
    fn partial_path_falls_back_gracefully_with_no_extension() {
        let got = ffmpeg_partial_output_path(std::path::Path::new("/tmp/out"));
        assert_eq!(got, std::path::PathBuf::from("/tmp/out.partial"));
    }

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
            let args = ffmpeg_args(
                320,
                240,
                30,
                codec,
                23,
                false,
                None,
                Some("/tmp/a.raw"),
                "o.mp4",
            );
            let inputs = input_positions(&args);
            assert_eq!(
                inputs.len(),
                2,
                "{codec}: expected a video and an audio input"
            );

            let audio_i = inputs[1];
            assert_eq!(args[audio_i - 1], "2", "{codec}: -ac lost before audio -i");
            assert_eq!(args[audio_i + 1], "/tmp/a.raw");

            let ar_pos = args.iter().position(|s| s == "-ar").unwrap();
            assert_eq!(
                args[ar_pos + 1],
                crate::encode::audio::OUTPUT_SAMPLE_RATE.to_string(),
                "{codec}: -ar must equal OUTPUT_SAMPLE_RATE, the rate mix_audio_tracks resamples to"
            );

            for opt in OUTPUT_OPTS {
                if let Some(pos) = args.iter().position(|s| s == opt) {
                    assert!(
                        pos > audio_i,
                        "{codec}: {opt} is emitted at {pos}, before the audio input at {audio_i} — \
                         ffmpeg would read it as an option of audio.raw and refuse to start"
                    );
                }
            }
            assert_eq!(
                args.last().unwrap(),
                "o.mp4",
                "{codec}: output must be last"
            );
        }
    }

    fn value_after(args: &[String], flag: &str) -> Option<String> {
        let at = args.iter().position(|s| s == flag)?;
        args.get(at + 1).cloned()
    }

    #[test]
    fn the_default_h264_output_is_eight_bit() {
        let args = ffmpeg_args(320, 240, 30, "h264", 23, false, None, None, "o.mp4");
        assert_eq!(
            value_after(&args, "-pix_fmt").as_deref(),
            Some("yuv420p"),
            "High 10 is what QuickTime and Safari refuse to play, and it was the default"
        );
        assert_eq!(value_after(&args, "-profile:v").as_deref(), Some("high"));
    }

    #[test]
    fn ten_bit_is_still_reachable_by_asking_for_it() {
        let args = ffmpeg_args(320, 240, 30, "h264_10bit", 23, false, None, None, "o.mp4");
        assert_eq!(
            value_after(&args, "-pix_fmt").as_deref(),
            Some("yuv420p10le"),
            "a dark gradient still needs 10-bit, so it stays available"
        );
        assert_eq!(value_after(&args, "-profile:v").as_deref(), Some("high10"));
        assert!(args.iter().any(|s| s == "libx264"), "still H.264");
    }

    #[test]
    fn an_unknown_codec_name_falls_back_to_the_playable_default() {
        let args = ffmpeg_args(320, 240, 30, "not-a-codec", 23, false, None, None, "o.mp4");
        assert_eq!(
            value_after(&args, "-pix_fmt").as_deref(),
            Some("yuv420p"),
            "the catch-all arm is what an unrecognised --codec lands in, so it must be the \
             playable one"
        );
    }

    #[test]
    fn a_silent_scenario_declares_a_single_input_and_no_audio_codec() {
        let args = ffmpeg_args(320, 240, 30, "h264", 23, false, None, None, "o.mp4");
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
                let a = ffmpeg_args(320, 240, 30, codec, 23, t, None, None, "o.mov");
                let i = a.iter().position(|s| s == "-pix_fmt").unwrap();
                a[i + 1].clone()
            };
            assert_eq!(pix(false), opaque, "{codec} opaque");
            assert_eq!(pix(true), alpha, "{codec} transparent");
        }
    }

    #[test]
    fn a_hardware_encoder_replaces_the_software_codec_and_its_rate_control() {
        let args = ffmpeg_args(
            320,
            240,
            30,
            "h264",
            23,
            false,
            Some("h264_videotoolbox"),
            None,
            "o.mp4",
        );
        let cv_pos = args
            .iter()
            .position(|s| s == "-c:v")
            .expect("-c:v must be present");
        assert_eq!(args[cv_pos + 1], "h264_videotoolbox");

        for absent in ["-crf", "-preset", "-profile:v"] {
            assert!(
                !args.iter().any(|s| s == absent),
                "hardware path must not emit {absent}: {args:?}"
            );
        }
        assert_eq!(args.last().unwrap(), "o.mp4");
    }

    #[test]
    fn a_hardware_encoder_still_sits_after_the_audio_input() {
        let args = ffmpeg_args(
            320,
            240,
            30,
            "h264",
            23,
            false,
            Some("h264_nvenc"),
            Some("/tmp/a.raw"),
            "o.mp4",
        );
        let audio_i = input_positions(&args)[1];
        let cv_pos = args.iter().position(|s| s == "-c:v").unwrap();
        assert!(
            cv_pos > audio_i,
            "-c:v (hardware) at {cv_pos} must come after the audio -i at {audio_i}"
        );
    }

    #[test]
    fn no_hardware_encoder_falls_back_to_the_existing_software_branch() {
        let with_none = ffmpeg_args(320, 240, 30, "h264", 23, false, None, None, "o.mp4");
        assert!(with_none.iter().any(|s| s == "libx264"));
        assert!(with_none.iter().any(|s| s == "-crf"));
    }

    #[test]
    fn hevc_into_an_apple_container_is_tagged_hvc1() {
        for codec in ["h265", "hevc"] {
            for (path, hw) in [
                ("o.mp4", None),
                ("o.mov", None),
                ("o.MP4", None),
                ("o.partial.mp4", None),
                ("o.mp4", Some("hevc_videotoolbox")),
                ("o.mp4", Some("hevc_nvenc")),
                ("o.mov", Some("hevc_qsv")),
                ("o.mp4", Some("hevc_amf")),
            ] {
                let args = ffmpeg_args(320, 240, 30, codec, 23, false, hw, None, path);
                assert_eq!(
                    value_after(&args, "-tag:v").as_deref(),
                    Some("hvc1"),
                    "{codec} -> {path} (hw {hw:?}): the muxer writes hev1 unless told, and \
                     AVFoundation opens nothing but hvc1"
                );
            }
        }
    }

    #[test]
    fn hevc_into_matroska_carries_no_sample_entry_tag() {
        for path in ["o.mkv", "o"] {
            let args = ffmpeg_args(320, 240, 30, "h265", 23, false, None, None, path);
            assert!(
                !args.iter().any(|s| s == "-tag:v"),
                "{path} is not an ISO-BMFF container, so the tag does not apply: {args:?}"
            );
        }
    }

    #[test]
    fn no_other_codec_is_retagged() {
        for codec in ["h264", "h264_10bit", "vp9", "prores", "not-a-codec"] {
            for path in ["o.mp4", "o.mov"] {
                let args = ffmpeg_args(320, 240, 30, codec, 23, false, None, None, path);
                assert!(
                    !args.iter().any(|s| s == "-tag:v"),
                    "{codec} already muxes under a tag its players accept: {args:?}"
                );
            }
        }
    }

    #[test]
    fn selection_is_a_noop_when_not_requested() {
        assert_eq!(
            select_hardware_encoder(false, "h264", false, |_| true),
            HardwareSelection::NotRequested
        );
    }

    #[test]
    fn selection_refuses_transparent_even_when_every_encoder_is_available() {
        let selection = select_hardware_encoder(true, "h264", true, |_| true);
        assert!(
            matches!(selection, HardwareSelection::Unsupported { .. }),
            "got {selection:?}"
        );
    }

    #[test]
    fn selection_refuses_codecs_with_no_hardware_family() {
        for codec in ["vp9", "prores"] {
            let selection = select_hardware_encoder(true, codec, false, |_| true);
            assert!(
                matches!(selection, HardwareSelection::Unsupported { .. }),
                "{codec}: got {selection:?}"
            );
        }
    }

    #[test]
    fn selection_picks_the_first_available_family_in_priority_order() {
        let selection = select_hardware_encoder(true, "h264", false, |name| {
            matches!(name, "h264_nvenc" | "h264_amf")
        });
        assert_eq!(selection, HardwareSelection::Use("h264_nvenc".to_string()));
    }

    #[test]
    fn selection_reports_unavailable_with_every_candidate_tried_when_none_match() {
        let selection = select_hardware_encoder(true, "h264", false, |_| false);
        match selection {
            HardwareSelection::Unavailable { tried } => {
                assert_eq!(
                    tried,
                    vec!["h264_videotoolbox", "h264_nvenc", "h264_qsv", "h264_amf"]
                );
            }
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[test]
    fn selection_covers_h265_and_hevc_as_the_same_codec() {
        for codec in ["h265", "hevc"] {
            let selection =
                select_hardware_encoder(true, codec, false, |name| name == "hevc_videotoolbox");
            assert_eq!(
                selection,
                HardwareSelection::Use("hevc_videotoolbox".to_string()),
                "{codec}"
            );
        }
    }

    #[test]
    fn parses_encoder_names_out_of_realistic_ffmpeg_encoders_output() {
        let sample = "\
Encoders:
 V..... = Video
 A..... = Audio
 S..... = Subtitle
 ------
 V..... a64_multi            Multicolor charset for Commodore 64 (codec a64_multi)
 V....S alias_pix            Alias/Wavefront PIX image
 V..... libx264              libx264 H.264 / AVC / MPEG-4 AVC (codec h264)
 V..... h264_videotoolbox    VideoToolbox H.264 Encoder
 V..... hevc_videotoolbox    VideoToolbox H.265 Encoder
 V..... h264_nvenc           NVIDIA NVENC H.264 encoder (codec h264)
 A..... aac                  AAC (Advanced Audio Coding)
";
        let names = parse_encoder_names(sample);
        for expect in [
            "libx264",
            "h264_videotoolbox",
            "hevc_videotoolbox",
            "h264_nvenc",
            "aac",
            "a64_multi",
            "alias_pix",
        ] {
            assert!(names.contains(expect), "missing {expect}: {names:?}");
        }
        assert!(!names.contains("="));
        assert!(names.iter().all(|n| n != "Video" && n != "Audio"));
    }

    fn ffmpeg_on_path() -> bool {
        std::process::Command::new("ffmpeg")
            .args(["-version"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn ffprobe_on_path() -> bool {
        std::process::Command::new("ffprobe")
            .args(["-version"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    fn ffprobe_video_codec_tag(path: &str) -> Option<String> {
        let out = std::process::Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=codec_tag_string",
                "-of",
                "default=noprint_wrappers=1:nokey=1",
                path,
            ])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    fn ffprobe_stream_duration(path: &str, selector: &str) -> Option<f64> {
        let out = std::process::Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                selector,
                "-show_entries",
                "stream=duration",
                "-of",
                "default=noprint_wrappers=1:nokey=1",
                path,
            ])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        String::from_utf8_lossy(&out.stdout)
            .trim()
            .parse::<f64>()
            .ok()
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
    fn encode_with_ffmpeg_produces_a_synced_two_stream_mp4() {
        if !ffmpeg_on_path() {
            eprintln!(
                "encode_with_ffmpeg_produces_a_synced_two_stream_mp4: ffmpeg not found — skipping"
            );
            return;
        }

        let wav_path = std::env::temp_dir().join(format!(
            "rm_ffmpeg_it_audio_{}_{}.wav",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        write_minimal_wav(&wav_path, 22_050, 22_050);

        let json = format!(
            r#"{{"video": {{"width": 32, "height": 32, "fps": 10}},
                 "audio": [{{"src": "{}"}}],
                 "scenes": [{{"duration": 1.0, "children": []}}]}}"#,
            wav_path.to_str().unwrap().replace('\\', "\\\\")
        );
        let scenario = crate::loader::load_scenario_from_source(None, Some(&json)).expect("load");

        let out = std::env::temp_dir().join(format!("rm_ffmpeg_it_out_{}.mp4", std::process::id()));
        let _ = std::fs::remove_file(&out);

        super::encode_with_ffmpeg(
            &scenario,
            out.to_str().unwrap(),
            true,
            "h264",
            None,
            false,
            None,
        )
        .expect("ffmpeg encode with an audio track must succeed (constat #1)");

        assert!(out.exists(), "output MP4 must exist");
        assert!(
            std::fs::metadata(&out).unwrap().len() > 0,
            "output MP4 must not be empty"
        );

        if ffprobe_on_path() {
            let video_dur = ffprobe_stream_duration(out.to_str().unwrap(), "v:0")
                .expect("must report a video stream duration");
            let audio_dur = ffprobe_stream_duration(out.to_str().unwrap(), "a:0").expect(
                "must report an audio stream duration — the MP4 must contain an audio stream \
                 at all (constat #1)",
            );
            assert!(
                (video_dur - audio_dur).abs() < 0.05,
                "audio/video duration must match within 50ms, got video={video_dur:.3}s \
                 audio={audio_dur:.3}s (constat #2: a sample-rate mismatch between the mixer \
                 and the muxer desyncs them)"
            );
        }

        let _ = std::fs::remove_file(&wav_path);
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn a_rendered_hevc_mp4_carries_the_hvc1_tag_on_disk() {
        if !ffmpeg_on_path() || !ffprobe_on_path() {
            eprintln!("a_rendered_hevc_mp4_carries_the_hvc1_tag_on_disk: ffmpeg/ffprobe not found — skipping");
            return;
        }
        if !super::probe_ffmpeg_encoders().contains("libx265") {
            eprintln!("a_rendered_hevc_mp4_carries_the_hvc1_tag_on_disk: this ffmpeg has no libx265 — skipping");
            return;
        }

        let json = r#"{"video": {"width": 64, "height": 64, "fps": 10},
                       "scenes": [{"duration": 0.5, "children": []}]}"#;
        let scenario = crate::loader::load_scenario_from_source(None, Some(json)).expect("load");

        let out = std::env::temp_dir().join(format!(
            "rm_hvc1_tag_{}_{}.mp4",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_file(&out);

        super::encode_with_ffmpeg(
            &scenario,
            out.to_str().unwrap(),
            true,
            "h265",
            None,
            false,
            None,
        )
        .expect("an h265 render must succeed");

        let tag = ffprobe_video_codec_tag(out.to_str().unwrap())
            .expect("ffprobe must report a video codec tag");
        assert_eq!(
            tag, "hvc1",
            "hev1 is what the muxer writes by default and what QuickTime, Safari and every \
             other AVFoundation player refuse to open"
        );

        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn hardware_probe_reports_what_this_machine_actually_offers() {
        if !ffmpeg_on_path() {
            eprintln!(
                "hardware_probe_reports_what_this_machine_actually_offers: ffmpeg not found — skipping"
            );
            return;
        }
        let available = super::probe_ffmpeg_encoders();
        let selection =
            super::select_hardware_encoder(true, "h264", false, |name| available.contains(name));
        match selection {
            HardwareSelection::Use(name) => {
                eprintln!("this machine's ffmpeg offers hardware encoder: {name}");
            }
            other => {
                eprintln!(
                    "this machine's ffmpeg offers no known h264 hardware encoder ({other:?}); \
                     the fallback path is covered by the pure tests above"
                );
            }
        }
    }

    #[test]
    fn encode_with_ffmpeg_hw_succeeds_whether_or_not_this_machine_has_a_hardware_encoder() {
        if !ffmpeg_on_path() {
            eprintln!(
                "encode_with_ffmpeg_hw_succeeds_whether_or_not_this_machine_has_a_hardware_encoder: \
                 ffmpeg not found — skipping"
            );
            return;
        }
        let json = r#"{"video": {"width": 32, "height": 32, "fps": 10},
             "scenes": [{"duration": 0.5, "children": []}]}"#;
        let scenario = crate::loader::load_scenario_from_source(None, Some(json)).expect("load");

        let out = std::env::temp_dir().join(format!(
            "rm_ffmpeg_hw_it_out_{}_{}.mp4",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_file(&out);

        super::encode_with_ffmpeg_hw(
            &scenario,
            out.to_str().unwrap(),
            true,
            "h264",
            None,
            false,
            true,
            None,
        )
        .expect(
            "hardware_acceleration=true must never fail the encode outright — available or \
                 not, it must fall back to software rather than abort",
        );

        assert!(out.exists(), "output MP4 must exist");
        assert!(
            std::fs::metadata(&out).unwrap().len() > 0,
            "output MP4 must not be empty"
        );

        let _ = std::fs::remove_file(&out);
    }

    fn ffprobe_frame_count(path: &str) -> Option<u32> {
        let out = std::process::Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-count_frames",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=nb_read_frames",
                "-of",
                "default=noprint_wrappers=1:nokey=1",
                path,
            ])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        String::from_utf8_lossy(&out.stdout)
            .trim()
            .parse::<u32>()
            .ok()
    }

    fn extract_center_pixel(path: &str, time_s: f64) -> Option<(u8, u8, u8)> {
        let png_path = std::env::temp_dir().join(format!(
            "rm_frame_range_pixel_{}_{}.png",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?
                .as_nanos()
        ));
        let status = std::process::Command::new("ffmpeg")
            .args(["-y", "-loglevel", "error", "-i", path, "-ss"])
            .arg(time_s.to_string())
            .args(["-frames:v", "1"])
            .arg(&png_path)
            .status()
            .ok()?;
        if !status.success() {
            return None;
        }
        let img = image::open(&png_path).ok()?.to_rgba8();
        let (w, h) = img.dimensions();
        let px = img.get_pixel(w / 2, h / 2);
        let _ = std::fs::remove_file(&png_path);
        Some((px[0], px[1], px[2]))
    }

    #[test]
    fn full_render_and_three_segments_concatenated_match_frame_count_and_audio_duration() {
        if !ffmpeg_on_path() || !ffprobe_on_path() {
            eprintln!(
                "full_render_and_three_segments_concatenated_match_frame_count_and_audio_duration: \
                 ffmpeg/ffprobe not found — skipping"
            );
            return;
        }

        let fps = 10u32;
        let width = 64u32;
        let height = 64u32;

        let wav_path = std::env::temp_dir().join(format!(
            "rm_frame_range_audio_{}_{}.wav",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        write_minimal_wav(&wav_path, 22_050, 22_050 * 3);

        let json = format!(
            r##"{{"video": {{"width": {width}, "height": {height}, "fps": {fps}}},
            "audio": [{{"src": "{}"}}],
            "scenes": [
                {{"duration": 1.0, "children": [
                    {{"type": "shape", "shape": "rect", "fill": "#ff0000",
                      "position": "absolute", "x": 0, "y": 0,
                      "style": {{"width": {width}, "height": {height}}}}}
                ]}},
                {{"duration": 1.0, "children": [
                    {{"type": "shape", "shape": "rect", "fill": "#00ff00",
                      "position": "absolute", "x": 0, "y": 0,
                      "style": {{"width": {width}, "height": {height}}}}}
                ]}},
                {{"duration": 1.0, "children": [
                    {{"type": "shape", "shape": "rect", "fill": "#0000ff",
                      "position": "absolute", "x": 0, "y": 0,
                      "style": {{"width": {width}, "height": {height}}}}}
                ]}}
            ]}}"##,
            wav_path.to_str().unwrap().replace('\\', "\\\\")
        );
        let scenario = crate::loader::load_scenario_from_source(None, Some(&json)).expect("load");

        let expected_total_frames = super::build_frame_tasks(&scenario).len() as u32;
        assert_eq!(expected_total_frames, 30, "3 scenes x 1.0s x 10fps");

        let pid = std::process::id();
        let full_out = std::env::temp_dir().join(format!("rm_frame_range_full_{pid}.mp4"));
        let seg_outs: Vec<std::path::PathBuf> = (0..3)
            .map(|i| std::env::temp_dir().join(format!("rm_frame_range_seg{i}_{pid}.mp4")))
            .collect();
        let concat_out = std::env::temp_dir().join(format!("rm_frame_range_concat_{pid}.mp4"));
        for p in [&full_out, &concat_out].into_iter().chain(seg_outs.iter()) {
            let _ = std::fs::remove_file(p);
        }

        super::encode_with_ffmpeg_hw(
            &scenario,
            full_out.to_str().unwrap(),
            true,
            "h264",
            None,
            false,
            false,
            None,
        )
        .expect("full render must succeed");

        for (i, (start, end)) in [(0u32, 9u32), (10, 19), (20, 29)].into_iter().enumerate() {
            super::encode_with_ffmpeg_hw_range(
                &scenario,
                seg_outs[i].to_str().unwrap(),
                true,
                "h264",
                None,
                false,
                false,
                (start, end),
                None,
            )
            .unwrap_or_else(|e| panic!("segment {i} ({start}-{end}) render must succeed: {e}"));
        }

        super::concat_mp4_segments(&seg_outs, concat_out.to_str().unwrap())
            .expect("concat must succeed");

        let full_frames = ffprobe_frame_count(full_out.to_str().unwrap())
            .expect("ffprobe must report the full render's frame count");
        let concat_frames = ffprobe_frame_count(concat_out.to_str().unwrap())
            .expect("ffprobe must report the concatenated render's frame count");
        assert_eq!(
            full_frames, expected_total_frames,
            "full render must have exactly the frames build_frame_tasks predicts"
        );
        assert_eq!(
            concat_frames, full_frames,
            "concatenated segments must have the exact same frame count as the full render"
        );

        let full_audio_dur = ffprobe_stream_duration(full_out.to_str().unwrap(), "a:0")
            .expect("full render must have an audio stream");
        let concat_audio_dur = ffprobe_stream_duration(concat_out.to_str().unwrap(), "a:0")
            .expect("concatenated render must have an audio stream");
        assert!(
            (full_audio_dur - concat_audio_dur).abs() < 0.05,
            "audio duration must match within 50ms: full={full_audio_dur:.3}s \
             concat={concat_audio_dur:.3}s"
        );

        let full_px = extract_center_pixel(full_out.to_str().unwrap(), 1.5)
            .expect("must extract a frame from the full render");
        let concat_px = extract_center_pixel(concat_out.to_str().unwrap(), 1.5)
            .expect("must extract a frame from the concatenated render");
        let close = |a: (u8, u8, u8), b: (u8, u8, u8)| {
            (a.0 as i32 - b.0 as i32).abs() <= 20
                && (a.1 as i32 - b.1 as i32).abs() <= 20
                && (a.2 as i32 - b.2 as i32).abs() <= 20
        };
        assert!(
            close(full_px, concat_px),
            "pixel at t=1.5s must match between full and concatenated renders: \
             full={full_px:?} concat={concat_px:?}"
        );
        assert!(
            close(full_px, (0, 255, 0)),
            "t=1.5s sits inside the solid-green second scene: expected ~green, \
             got full={full_px:?}"
        );

        for p in [&full_out, &concat_out].into_iter().chain(seg_outs.iter()) {
            let _ = std::fs::remove_file(p);
        }
        let _ = std::fs::remove_file(&wav_path);
    }
}
