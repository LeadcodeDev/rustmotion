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

/// A hardware encoder family ffmpeg can drive, in probe priority order.
/// Deliberately not gated by `cfg(target_os)`: the machine that compiled
/// rustmotion is not necessarily the machine that will run it, a macOS box
/// can have a VideoToolbox-less ffmpeg build, and a Linux box can have an
/// nvenc-capable ffmpeg without a working NVIDIA driver. `probe_ffmpeg_encoders`
/// asks the actual binary instead of guessing from the target triple.
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

    /// The concrete ffmpeg encoder name for this family + base codec, or
    /// `None` when this family has no hardware path for that codec. vp9 and
    /// prores stay software-only here: their hardware paths are far less
    /// standard across ffmpeg builds than h264/h265's, and getting one
    /// wrong means a confusing ffmpeg failure instead of a clean fallback.
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

/// What `ffmpeg_args` should do about hardware acceleration, decided once
/// up front and passed in as a plain value. Kept separate from the probe
/// (machine-dependent I/O, see `probe_ffmpeg_encoders`) so the *decision*
/// — which family to pick, and why not when none applies — is a pure
/// function (`select_hardware_encoder`) testable with a fake availability
/// set, no ffmpeg binary required.
#[derive(Debug, Clone, PartialEq, Eq)]
enum HardwareSelection {
    /// Use this concrete ffmpeg encoder (e.g. "h264_videotoolbox").
    Use(String),
    /// `--hardware-acceleration` was not requested.
    NotRequested,
    /// Requested, but this codec/transparency combination has no hardware
    /// path at all — no known hardware encoder here produces an alpha
    /// channel, and vp9/prores have no hardware family wired in.
    Unsupported { reason: String },
    /// Requested, and the codec supports it in principle, but this
    /// machine's `ffmpeg -encoders` didn't list any of the candidates.
    Unavailable { tried: Vec<String> },
}

/// Decide which hardware encoder (if any) to use. Pure: everything
/// machine-dependent comes in through `is_available`, so this is exercised
/// in tests with a fake set instead of a real probe.
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

/// Parse the encoder names out of `ffmpeg -encoders` output. Pure — the
/// real probe (`probe_ffmpeg_encoders`) is the only caller that touches a
/// process; this half is exercised with a captured sample of real ffmpeg
/// output, no binary required.
///
/// Each encoder line looks like ` V..... h264_videotoolbox   VideoToolbox
/// H.264 Encoder` (a flags column, the name, then a free-text description);
/// the legend above it looks like ` V..... = Video`, which has the same
/// flags shape but a bare `=` where a name would be — filtered out
/// explicitly rather than relied on to fail some other check.
fn parse_encoder_names(text: &str) -> HashSet<String> {
    text.lines()
        .filter_map(|line| {
            // `split_whitespace` already skips leading whitespace.
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

/// Ask this machine's actual ffmpeg what it offers, rather than assuming
/// from the compiled target platform (see `HwEncoderFamily`'s doc comment
/// for why that assumption is unsafe). Returns an empty set — never an
/// error — when ffmpeg can't be run or produces unexpected output: an
/// empty set makes `select_hardware_encoder` report `Unavailable`, which
/// falls back to software. Probing must never be the reason an encode that
/// would otherwise have worked in software fails outright.
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

    // ---- inputs ------------------------------------------------------------
    push(&["-y", "-loglevel", "error"], &mut args);
    push(&["-f", "rawvideo", "-pixel_format", "rgba"], &mut args);
    push(&["-video_size", &size], &mut args);
    push(&["-framerate", &framerate], &mut args);
    push(&["-i", "pipe:0"], &mut args);

    if let Some(path) = audio_input {
        // The PCM `mix_audio_tracks` hands us is fixed at `OUTPUT_SAMPLE_RATE`
        // (constat #2) — declaring anything else here would desync the muxed
        // audio track from the video without ffmpeg ever raising an error.
        let sample_rate = super::super::audio::OUTPUT_SAMPLE_RATE.to_string();
        args.extend(
            ["-f", "s16le", "-ar", &sample_rate, "-ac", "2", "-i", path]
                .into_iter()
                .map(str::to_string),
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
    if let Some(hw_name) = hw_encoder {
        // Hardware encoders are quality/bitrate-driven, not CRF-driven —
        // VideoToolbox reasons in `-q:v`, NVENC in `-cq`/`-b:v`, QSV/AMF in
        // `-global_quality`/`-b:v` — and the mapping between "CRF 23" and
        // each of those is not a clean, verifiable translation. Emitting
        // none of them and letting the encoder use its own default rate
        // control is more honest than inventing one; `check_crf` tells the
        // caller up front that `--crf` has no effect on this path.
        //
        // Likewise `-preset`/`-profile:v` are libx264/libx265 AVOptions —
        // several hardware encoders (VideoToolbox in particular) reject an
        // unrecognized option outright and abort, so the software knobs are
        // not reused here at all, not even as a best-effort translation.
        //
        // None of the families wired into `HwEncoderFamily` support an
        // alpha channel, so this branch always targets a fixed opaque
        // `yuv420p` — `select_hardware_encoder` already refuses to select a
        // hardware encoder when `transparent` is set, forcing the software
        // branch below instead, so `transparent` is never silently dropped
        // here.
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
                let prores_profile_matching_pix_fmt = alpha_fmt("4", "3");
                push(
                    &[
                        "-c:v",
                        "prores_ks",
                        "-profile:v",
                        prores_profile_matching_pix_fmt,
                    ],
                    &mut args,
                );
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
    }

    if audio_input.is_some() {
        let container = std::path::Path::new(output_path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("mp4");
        push(
            &["-c:a", audio_codec_for_container(container), "-b:a", "128k"],
            &mut args,
        );
    }

    args.push(output_path.to_string());
    args
}

/// Name of the scratch directory a single audio-bearing render call writes
/// its materialised PCM into. `pid` repeats across the machine's uptime and
/// `seq` is a small monotonic counter starting at zero, so together they are
/// a key an outside process could realistically pre-compute and occupy
/// ahead of time; folding in a nanosecond timestamp neither of those two
/// alone carries closes that gap without needing a random-number
/// dependency this crate doesn't already have.
fn audio_tmp_dir_name(pid: u32, seq: u32, nanos: u128) -> String {
    format!("rustmotion_audio_{pid}_{seq}_{nanos:x}")
}

/// Scratch path ffmpeg actually writes to; promoted (renamed) onto the
/// caller's real `output_path` only after a clean exit with no `pipe_error`.
/// Kept as a sibling of `output_path` (same directory, same filesystem, so
/// the promotion is a plain rename) and keeps `output_path`'s own extension
/// as the *final* extension — mirrors `video_audio::partial_wav_path`'s doc:
/// ffmpeg picks its output muxer from the last extension, so a bare
/// `.partial` suffix appended after it makes ffmpeg refuse to start with
/// "Unable to choose an output format" instead of the encode failure this
/// path exists to isolate.
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

/// Encode using FFmpeg subprocess (for h265, vp9, prores, webm, mov, transparency).
///
/// Software-only. Kept with its original signature so existing callers
/// (the studio's exporter among them) are unaffected by hardware
/// acceleration support; see [`encode_with_ffmpeg_hw`] for the switch.
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

/// Same as [`encode_with_ffmpeg`], with an opt-in `hardware_acceleration`
/// switch. When set, probes this machine's ffmpeg for a matching hardware
/// encoder (see `select_hardware_encoder` / `probe_ffmpeg_encoders`) and
/// uses it if found; otherwise — or when the codec/transparency combination
/// has no hardware path at all — falls back to the software encoder and
/// says so on stderr unless `quiet`. Never fails just because hardware
/// acceleration was requested but unavailable.
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

/// Same as [`encode_with_ffmpeg_hw`], restricted to the inclusive frame
/// index range `[frame_range.0, frame_range.1]` — the same index space
/// `--frame N` already addresses via `build_frame_tasks(...).get(N)`. This
/// is the default (ffmpeg-driven) render path — the one actually used
/// unless ffmpeg is absent from `PATH` — so it, not just the native
/// `encode_video_range`, has to window its audio the same way: see
/// `mix_audio_tracks_segment`'s doc for why a segment carries the audio
/// that plays at that point in the *full* scenario instead of audio
/// restarted from t=0.
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

fn transparency_unsupported_reason(codec: &str) -> Option<&'static str> {
    match codec {
        "vp9" | "prores" => None,
        "h265" | "hevc" => Some(
            "H.265 alpha encoding is not supported by libx265; use --codec prores or vp9, or \
             drop --transparent",
        ),
        _ => Some("h264 has no alpha channel; use --codec prores or vp9, or drop --transparent"),
    }
}

/// Refuses `--transparent` for a codec whose ffmpeg pipeline cannot carry an alpha channel.
pub fn check_transparent_codec(codec: &str, transparent: bool) -> Result<()> {
    if !transparent {
        return Ok(());
    }
    if let Some(reason) = transparency_unsupported_reason(codec) {
        return Err(RustmotionError::Generic(format!(
            "--transparent cannot be honoured with --codec {codec} — {reason}"
        )));
    }
    Ok(())
}

struct RemoveDirAllOnDrop(Option<std::path::PathBuf>);

impl Drop for RemoveDirAllOnDrop {
    fn drop(&mut self) {
        if let Some(dir) = self.0.take() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
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
    check_transparent_codec(codec, transparent)?;
    let container = std::path::Path::new(output_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("mp4");
    super::formats::check_codec_container(codec, container)?;

    let config = &scenario.video;
    let width = config.width;
    let height = config.height;
    let fps = config.fps;

    for view in &scenario.views {
        prefetch_icons(&view.scenes);
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

    // Process audio — merge scenario.audio with tracks extracted from embedded
    // video components. `scenario_total_duration` stays separate from
    // `segment_duration`: a track with no explicit `end` plays until the end
    // of the *scenario*, and fades key off that same bound (see
    // `mix_audio_tracks_segment`'s doc) — not this segment's own edges.
    let scenario_total_duration = full_total_frames as f64 / fps as f64;
    let segment_duration = total_frames as f64 / fps as f64;
    let segment_start = segment_start_frame as f64 / fps as f64;
    let video_tracks = super::super::video_audio::collect_video_audio_tracks(scenario);
    let merged_audio: Vec<crate::schema::AudioTrack> = {
        let mut all = scenario.audio.clone();
        all.extend(video_tracks);
        all
    };

    // PID alone is not a unique directory name: several audio-bearing
    // encodes can run concurrently *within* one process (parallel test
    // threads today; `--frames` segments rendered concurrently by a future
    // distributed worker tomorrow — the exact shape this feature exists to
    // enable). Two calls sharing a PID-only path would each try to create
    // the same directory, then whichever finishes first would
    // `remove_dir_all` it out from under the other mid-write, surfacing as
    // a bare `NotFound` on `std::fs::write` below. A monotonic counter on
    // top of PID makes every call's directory distinct regardless of
    // timing; a nanosecond timestamp on top of *that* keeps the full key
    // from being small enough for something outside this process to
    // pre-compute and occupy ahead of time — pid space and a
    // monotonic-from-zero counter both are. `create_dir` below (not
    // `_all`) is what actually refuses to proceed if something is already
    // sitting at the computed path, symlink included; the timestamp only
    // raises the cost of ever landing on that path in the first place.
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
    let _audio_tmp_dir_cleanup = RemoveDirAllOnDrop(audio_tmp_dir.clone());
    let pcm_data = if !merged_audio.is_empty() {
        if let Some(ref tmp_dir) = audio_tmp_dir {
            std::fs::create_dir(tmp_dir)?;
        }
        super::super::audio::mix_audio_tracks_segment(
            &merged_audio,
            scenario_total_duration,
            segment_start,
            segment_duration,
            quiet,
        )?
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

    // Probing shells out to `ffmpeg -encoders`, so it only runs when
    // hardware acceleration was actually requested — an unconditional probe
    // would pay that cost on every encode for nothing.
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
    // Always capture stderr so failures surface a useful diagnostic. We tee to
    // the user terminal in non-quiet mode below by reading the captured buffer
    // only on failure.
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| RustmotionError::FfmpegSpawn {
        reason: e.to_string(),
    })?;

    let mut stdin = child.stdin.take().ok_or(RustmotionError::FfmpegPipe)?;

    // Drain stderr on a dedicated thread, started immediately after spawn —
    // not after `child.wait()`. `-loglevel error` keeps ffmpeg's stderr
    // small in the common case, but a pipe is only ~64KiB: if ffmpeg ever
    // writes enough to fill it while nobody is reading, it blocks on that
    // write. We are, at the same moment, blocked writing RGBA frames to its
    // stdin below — two processes each waiting on the other's pipe is a
    // deadlock neither side can recover from. Draining concurrently removes
    // the second pipe from that equation entirely (constat #11).
    let stderr_reader: Option<std::thread::JoinHandle<String>> =
        child.stderr.take().map(|mut h| {
            std::thread::spawn(move || {
                use std::io::Read;
                let mut s = String::new();
                let _ = h.read_to_string(&mut s);
                s
            })
        });

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

    // A `pipe_error` means the render already failed and `partial_output_path`
    // will be discarded either way, so there is nothing left for ffmpeg to
    // usefully finish — killing it here instead of waiting for it to
    // gracefully encode and finalize a file nobody will ever read avoids
    // burning time on a result already known to be thrown away.
    let status = if pipe_error.is_some() {
        let _ = child.kill();
        child.wait()
    } else {
        child.wait()
    }
    .map_err(|e| RustmotionError::FfmpegWait {
        reason: e.to_string(),
    })?;

    // The drain thread finishes once ffmpeg closes its stderr (which
    // happens no later than process exit, already awaited above), so this
    // join does not block on anything still running.
    let stderr_text = stderr_reader.and_then(|h| h.join().ok());

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
        let _ = std::fs::remove_file(&partial_output_path);
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
        let _ = std::fs::remove_file(&partial_output_path);
        return Err(RustmotionError::FfmpegFailed {
            stderr: stderr_summary,
        });
    }

    // Only now, with a clean exit and no pipe error, does `output_path` ever
    // see this render's bytes — promote-on-success, the same discipline
    // `video_audio::extract_audio_to_wav` already applies to its cached WAVs.
    std::fs::rename(&partial_output_path, output_path)?;

    Ok(())
}

fn audio_codec_for_container(container: &str) -> &'static str {
    match container {
        "webm" => "libopus",
        _ => "aac",
    }
}

fn ffmpeg_stderr_summary(stderr: &[u8]) -> Option<String> {
    let stderr = String::from_utf8_lossy(stderr);
    let summary = stderr
        .lines()
        .rev()
        .take(8)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    (!summary.trim().is_empty()).then_some(summary)
}

struct ConcatVideoFormat {
    codec_name: String,
    width: u32,
    height: u32,
    pix_fmt: String,
}

fn probe_video_format(path: &std::path::Path) -> Result<ConcatVideoFormat> {
    let out = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=codec_name,width,height,pix_fmt",
            "-of",
            "default=nw=1",
        ])
        .arg(path)
        .output()
        .map_err(|e| RustmotionError::FfmpegSpawn {
            reason: e.to_string(),
        })?;
    if !out.status.success() {
        return Err(RustmotionError::Generic(format!(
            "could not probe '{}' for its video format — it may be unreadable or corrupt: {}",
            path.display(),
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut codec_name: Option<String> = None;
    let mut width: Option<u32> = None;
    let mut height: Option<u32> = None;
    let mut pix_fmt: Option<String> = None;
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("codec_name=") {
            codec_name = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("width=") {
            width = v.trim().parse().ok();
        } else if let Some(v) = line.strip_prefix("height=") {
            height = v.trim().parse().ok();
        } else if let Some(v) = line.strip_prefix("pix_fmt=") {
            pix_fmt = Some(v.trim().to_string());
        }
    }
    match (codec_name, width, height, pix_fmt) {
        (Some(codec_name), Some(width), Some(height), Some(pix_fmt)) => Ok(ConcatVideoFormat {
            codec_name,
            width,
            height,
            pix_fmt,
        }),
        _ => Err(RustmotionError::Generic(format!(
            "could not read '{}' video codec/resolution/pixel format from ffprobe output — it \
             may be unreadable or corrupt: {text}",
            path.display()
        ))),
    }
}

fn check_inputs_share_video_format(inputs: &[std::path::PathBuf]) -> Result<()> {
    let mut formats = inputs
        .iter()
        .map(|p| Ok::<_, RustmotionError>((p, probe_video_format(p)?)));
    let Some(first) = formats.next() else {
        return Ok(());
    };
    let (first_path, first_format) = first?;
    for entry in formats {
        let (path, format) = entry?;
        if format.codec_name != first_format.codec_name
            || format.width != first_format.width
            || format.height != first_format.height
            || format.pix_fmt != first_format.pix_fmt
        {
            return Err(RustmotionError::Generic(format!(
                "concat requires every segment to share codec/resolution/pixel format — '{}' \
                 is {}x{} {}/{}, but '{}' is {}x{} {}/{}",
                path.display(),
                format.width,
                format.height,
                format.codec_name,
                format.pix_fmt,
                first_path.display(),
                first_format.width,
                first_format.height,
                first_format.codec_name,
                first_format.pix_fmt,
            )));
        }
    }
    Ok(())
}

fn segment_has_audio_stream(path: &std::path::Path) -> bool {
    std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "a:0",
            "-show_entries",
            "stream=index",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .map(|out| out.status.success() && !out.stdout.is_empty())
        .unwrap_or(false)
}

fn probe_video_frames_and_duration_s(path: &std::path::Path) -> Result<(u32, f64)> {
    let out = std::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-count_frames",
            "-show_entries",
            "stream=nb_read_frames,r_frame_rate",
            "-of",
            "default=nw=1",
        ])
        .arg(path)
        .output()
        .map_err(|e| RustmotionError::FfmpegSpawn {
            reason: e.to_string(),
        })?;
    if !out.status.success() {
        return Err(RustmotionError::Generic(format!(
            "could not probe '{}' for its exact video frame count — it may be unreadable or \
             corrupt: {}",
            path.display(),
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let mut nb_frames: Option<u32> = None;
    let mut frame_rate: Option<f64> = None;
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("nb_read_frames=") {
            nb_frames = v.trim().parse().ok();
        } else if let Some(v) = line.strip_prefix("r_frame_rate=") {
            frame_rate = v.trim().split_once('/').and_then(|(n, d)| {
                let n: f64 = n.parse().ok()?;
                let d: f64 = d.parse().ok()?;
                (d != 0.0).then_some(n / d)
            });
        }
    }
    match (nb_frames, frame_rate) {
        (Some(frames), Some(fps)) if fps > 0.0 && frames > 0 => Ok((frames, frames as f64 / fps)),
        _ => Err(RustmotionError::Generic(format!(
            "could not read '{}' video frame count / frame rate from ffprobe output — it may \
             be unreadable or corrupt: {text}",
            path.display()
        ))),
    }
}

fn decode_segment_pcm_s16le(
    path: &std::path::Path,
    sample_rate: u32,
    channels: u16,
) -> Result<Vec<u8>> {
    let out = std::process::Command::new("ffmpeg")
        .args(["-y", "-loglevel", "error", "-i"])
        .arg(path)
        .args(["-map", "a:0", "-f", "s16le", "-acodec", "pcm_s16le"])
        .args([
            "-ar",
            &sample_rate.to_string(),
            "-ac",
            &channels.to_string(),
        ])
        .arg("-")
        .output()
        .map_err(|e| RustmotionError::FfmpegSpawn {
            reason: e.to_string(),
        })?;
    if !out.status.success() {
        return Err(RustmotionError::FfmpegFailed {
            stderr: ffmpeg_stderr_summary(&out.stderr),
        });
    }
    Ok(out.stdout)
}

fn concat_with_retimed_audio(
    inputs: &[std::path::PathBuf],
    list_path: &std::path::Path,
    output_path: &str,
) -> Result<()> {
    let sample_rate = super::super::audio::OUTPUT_SAMPLE_RATE;
    let channels: u16 = 2;
    let bytes_per_frame = channels as usize * 2;

    let mut joined_pcm: Vec<u8> = Vec::new();
    let mut expected_frames: u32 = 0;
    for input in inputs {
        let (frames, duration_s) = probe_video_frames_and_duration_s(input)?;
        expected_frames += frames;
        let target_bytes = (duration_s * sample_rate as f64).round() as usize * bytes_per_frame;
        let mut pcm = if segment_has_audio_stream(input) {
            decode_segment_pcm_s16le(input, sample_rate, channels)?
        } else {
            Vec::new()
        };
        pcm.resize(target_bytes, 0);
        joined_pcm.extend_from_slice(&pcm);
    }

    let pcm_path = std::env::temp_dir().join(format!(
        "rustmotion_concat_audio_{}_{}.raw",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::write(&pcm_path, &joined_pcm)?;

    let audio_codec = audio_codec_for_container(
        std::path::Path::new(output_path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("mp4"),
    );

    let spawned = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-loglevel",
            "error",
            "-xerror",
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
        ])
        .arg(list_path)
        .args([
            "-f",
            "s16le",
            "-ar",
            &sample_rate.to_string(),
            "-ac",
            &channels.to_string(),
            "-i",
        ])
        .arg(&pcm_path)
        .args(["-map", "0:v", "-map", "1:a"])
        .args(["-c:v", "copy", "-c:a", audio_codec, "-b:a", "128k"])
        .arg(output_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| RustmotionError::FfmpegSpawn {
            reason: e.to_string(),
        });

    let _ = std::fs::remove_file(&pcm_path);

    let out = spawned?;
    if !out.status.success() {
        return Err(RustmotionError::FfmpegFailed {
            stderr: ffmpeg_stderr_summary(&out.stderr),
        });
    }
    if let Some(summary) = ffmpeg_stderr_summary(&out.stderr) {
        eprintln!("Warning: ffmpeg reported (concat succeeded regardless): {summary}");
    }

    verify_concat_frame_count(output_path, expected_frames)
}

fn verify_concat_frame_count(output_path: &str, expected_frames: u32) -> Result<()> {
    let (actual_frames, _) = probe_video_frames_and_duration_s(std::path::Path::new(output_path))?;
    if actual_frames != expected_frames {
        let _ = std::fs::remove_file(output_path);
        return Err(RustmotionError::Generic(format!(
            "concat reported success but '{output_path}' has {actual_frames} frame(s), not the \
             {expected_frames} the input segments actually total — ffmpeg's concat demuxer can \
             silently drop an unreadable segment while still exiting 0"
        )));
    }
    Ok(())
}

/// Join MP4 segments — such as ones produced by consecutive `render
/// --frames a-b` calls against the same scenario — into one file via
/// ffmpeg's concat demuxer, remuxing video (`-c:v copy`) instead of
/// re-encoding.
///
/// ## Why the demuxer, not a raw H.264 bitstream join
///
/// The other way to concatenate video segments is to concatenate their raw
/// Annex-B H.264 bitstreams directly and mux the result once. That only
/// works when every segment's bitstream is independently decodable at its
/// boundary — in practice, every frame at every segment boundary has to be
/// a keyframe, and the segments' encoder settings (profile, resolution,
/// pixel format) have to match exactly. Neither `encode_video_range` (the
/// native openh264 path, GOP-encoded like any full render) nor this
/// function's actual callers — the ffmpeg path
/// (`encode_with_ffmpeg_hw_range`), the one `render` actually uses whenever
/// ffmpeg is on `PATH` (the CLI's default) — give any per-frame intra
/// control, so segment boundaries are not guaranteed keyframes and a raw
/// bitstream join would silently produce an undecodable or corrupted joint
/// at some cuts. Making bitstream concatenation reliable needs an
/// encoding-side change (forcing a keyframe at every segment boundary, or
/// exposing a GOP-alignment knob) that does not exist yet.
///
/// The concat demuxer sidesteps all of that: it trusts each segment's own
/// container-level framing and restitches the streams, so it works
/// regardless of GOP layout. The cost is an extra remux pass, cheap for
/// video (`-c:v copy` touches no pixels, so no re-encode and no quality
/// loss) — and the requirement that every segment share codec, resolution,
/// and pixel format, which segments of the *same* scenario rendered with
/// the *same* `render` flags always do.
pub fn concat_mp4_segments(inputs: &[std::path::PathBuf], output_path: &str) -> Result<()> {
    if inputs.is_empty() {
        return Err(RustmotionError::Generic(
            "concat requires at least one input segment".to_string(),
        ));
    }
    check_inputs_share_video_format(inputs)?;

    // The concat demuxer reads a text list of `file '<path>'` lines. Paths
    // are canonicalized so the list works regardless of the process's
    // current directory, and single quotes are escaped the way ffmpeg's own
    // docs prescribe for its concat protocol.
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

    let result = if inputs.iter().any(|p| segment_has_audio_stream(p)) {
        concat_with_retimed_audio(inputs, &list_path, output_path)
    } else {
        concat_video_only(inputs, &list_path, output_path)
    };

    let _ = std::fs::remove_file(&list_path);
    result
}

fn concat_video_only(
    inputs: &[std::path::PathBuf],
    list_path: &std::path::Path,
    output_path: &str,
) -> Result<()> {
    let mut expected_frames: u32 = 0;
    for input in inputs {
        let (frames, _) = probe_video_frames_and_duration_s(input)?;
        expected_frames += frames;
    }

    let out = std::process::Command::new("ffmpeg")
        .args([
            "-y",
            "-loglevel",
            "error",
            "-xerror",
            "-f",
            "concat",
            "-safe",
            "0",
            "-i",
        ])
        .arg(list_path)
        .args(["-c", "copy"])
        .arg(output_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| RustmotionError::FfmpegSpawn {
            reason: e.to_string(),
        })?;

    if !out.status.success() {
        return Err(RustmotionError::FfmpegFailed {
            stderr: ffmpeg_stderr_summary(&out.stderr),
        });
    }
    if let Some(summary) = ffmpeg_stderr_summary(&out.stderr) {
        eprintln!("Warning: ffmpeg reported (concat succeeded regardless): {summary}");
    }

    verify_concat_frame_count(output_path, expected_frames)
}

#[cfg(test)]
mod tests {
    use super::{
        audio_tmp_dir_name, check_transparent_codec, ffmpeg_args, ffmpeg_partial_output_path,
        parse_encoder_names, segment_has_audio_stream, select_hardware_encoder, HardwareSelection,
        RemoveDirAllOnDrop,
    };

    // ── audio scratch directory naming: not fully predictable from outside ──

    #[test]
    fn audio_tmp_dir_name_differs_across_calls_that_share_pid_and_seq() {
        // A pid+seq pair is small enough to pre-seed exhaustively from
        // outside the process; folding in a nanosecond timestamp neither of
        // those two alone carries means a name computed ahead of time from
        // pid+seq no longer identifies the exact directory this process
        // will actually create.
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

    /// Characterizes the exact property this fix depends on: swapping
    /// `create_dir_all` for `create_dir` at the audio scratch directory's
    /// creation site turns "adopt whatever is already there" into "refuse
    /// outright" the moment something — attacker-planted symlink included —
    /// already occupies that path.
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

    // ── partial-output-path naming (pure) ────────────────────────────────────

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

            // The audio input keeps its own format options immediately ahead of it.
            let audio_i = inputs[1];
            assert_eq!(args[audio_i - 1], "2", "{codec}: -ac lost before audio -i");
            assert_eq!(args[audio_i + 1], "/tmp/a.raw");

            // Constat #2: the declared PCM rate must match what
            // `mix_audio_tracks` actually produces (`audio::OUTPUT_SAMPLE_RATE`),
            // not an independent literal that can drift out of sync with it.
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

    #[test]
    fn a_silent_scenario_declares_a_single_input_and_no_audio_codec() {
        let args = ffmpeg_args(320, 240, 30, "h264", 23, false, None, None, "o.mp4");
        assert_eq!(input_positions(&args).len(), 1);
        assert!(!args.iter().any(|s| s == "-c:a" || s == "-b:a"));
        assert_eq!(args.last().unwrap(), "o.mp4");
    }

    #[test]
    fn the_audio_codec_matches_the_output_container() {
        let webm_args = ffmpeg_args(
            320,
            240,
            30,
            "vp9",
            23,
            false,
            None,
            Some("/tmp/a.raw"),
            "o.webm",
        );
        let audio_codec_at = |args: &[String]| {
            let i = args
                .iter()
                .position(|s| s == "-c:a")
                .expect("-c:a must be present");
            args[i + 1].clone()
        };
        assert_eq!(
            audio_codec_at(&webm_args),
            "libopus",
            "webm cannot mux aac — libx264/libx265/prores's own mp4/mov output is unaffected"
        );

        let mp4_args = ffmpeg_args(
            320,
            240,
            30,
            "h264",
            23,
            false,
            None,
            Some("/tmp/a.raw"),
            "o.mp4",
        );
        assert_eq!(
            audio_codec_at(&mp4_args),
            "aac",
            "mp4/mov keep aac as before"
        );
    }

    #[test]
    fn transparency_selects_an_alpha_pixel_format() {
        for (codec, opaque, alpha) in [
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
    fn opaque_prores_uses_the_422_hq_profile_not_4444() {
        let profile_at = |t: bool| {
            let a = ffmpeg_args(320, 240, 30, "prores", 23, t, None, None, "o.mov");
            let i = a.iter().position(|s| s == "-profile:v").unwrap();
            a[i + 1].clone()
        };
        assert_eq!(
            profile_at(false),
            "3",
            "opaque prores must encode profile 3 (422 HQ), which actually pairs with the \
             yuv422p10le pixel format this path emits — profile 4 (4444) forces ffmpeg to \
             silently upgrade to a 4:4:4/12-bit format instead of honouring what was asked"
        );
        assert_eq!(
            profile_at(true),
            "4",
            "transparent prores still needs profile 4 (4444) — it is the only ProRes profile \
             with an alpha channel"
        );
    }

    #[test]
    fn check_transparent_codec_only_refuses_h264_and_h265() {
        for codec in ["h264", "h265", "hevc"] {
            let err = check_transparent_codec(codec, true)
                .expect_err(&format!("{codec}: --transparent must be refused"));
            let msg = err.to_string();
            assert!(msg.contains("--transparent"), "{codec}: {msg}");
            assert!(msg.to_lowercase().contains("alpha"), "{codec}: {msg}");
        }
        for codec in ["vp9", "prores"] {
            check_transparent_codec(codec, true)
                .unwrap_or_else(|e| panic!("{codec}: --transparent must be accepted: {e}"));
        }
        for codec in ["h264", "h265", "vp9", "prores", "anything"] {
            check_transparent_codec(codec, false)
                .unwrap_or_else(|e| panic!("{codec}: opaque must never be refused: {e}"));
        }
    }

    #[test]
    fn transparent_is_refused_up_front_for_codecs_without_alpha() {
        let json = r#"{"video": {"width": 32, "height": 32, "fps": 10},
             "scenes": [{"duration": 0.3, "children": []}]}"#;
        let scenario = crate::loader::load_scenario_from_source(None, Some(json)).expect("load");

        for codec in ["h264", "h265"] {
            let out = std::env::temp_dir().join(format!(
                "rm_ffmpeg_transparent_refused_{codec}_{}_{}.mov",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            let _ = std::fs::remove_file(&out);

            let result = super::encode_with_ffmpeg_hw(
                &scenario,
                out.to_str().unwrap(),
                true,
                codec,
                None,
                true,
                false,
                None,
            );

            let err = result.expect_err(&format!(
                "--transparent with --codec {codec} must be refused up front (F-ENCODE-4) \
                 instead of silently producing an opaque file (h264) or rendering every frame \
                 before ffmpeg fails to open libx265 with an alpha layer (h265)"
            ));
            let msg = err.to_string();
            assert!(
                msg.to_lowercase().contains("alpha") && msg.contains("--transparent"),
                "{codec}: the refusal must name the actual defect, not surface a generic ffmpeg \
                 failure: {msg}"
            );
            assert!(
                !out.exists(),
                "{codec}: refusing --transparent must happen before any frame is rendered or \
                 piped to ffmpeg, so no output file should exist"
            );
            let _ = std::fs::remove_file(&out);
        }
    }

    #[test]
    fn encode_with_ffmpeg_hw_range_refuses_a_codec_the_container_cannot_hold_before_rendering() {
        let json = r#"{"video": {"width": 32, "height": 32, "fps": 10},
             "scenes": [{"duration": 1.0, "children": []}]}"#;
        let scenario = crate::loader::load_scenario_from_source(None, Some(json)).expect("load");

        let out = std::env::temp_dir().join(format!(
            "rm_ffmpeg_range_codec_container_{}_{}.mp4",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_file(&out);

        let result = super::encode_with_ffmpeg_hw_range(
            &scenario,
            out.to_str().unwrap(),
            true,
            "prores",
            None,
            false,
            false,
            (0, 4),
            None,
        );

        let err = result.expect_err(
            "--frames with a codec/container pair check_codec_container refuses (prores into \
             .mp4) must be caught before any frame is rendered — the same guard `render` gets \
             without --frames — instead of surfacing as a raw ffmpeg failure after the whole \
             segment renders",
        );
        let msg = err.to_string();
        assert!(msg.contains("prores"), "{msg}");
        assert!(
            msg.contains(".mov"),
            "the message must name what works: {msg}"
        );
        assert!(
            !out.exists(),
            "refusing the pair must happen before any frame is rendered or piped to ffmpeg"
        );

        let _ = std::fs::remove_file(&out);
    }

    // ── Hardware acceleration: pure argument construction ───────────────────
    // No ffmpeg binary involved — `hw_encoder` is a plain `Option<&str>` the
    // caller already resolved, exactly like `select_hardware_encoder`'s
    // tests below resolve it without a real probe.

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

        // Software-only AVOptions: several hardware encoders (VideoToolbox
        // among them) reject an unrecognized option outright and abort, so
        // none of these may be reused as-is on the hardware path.
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
        // Same load-bearing ordering invariant as the software path: an
        // output option before the last `-i` gets attached to that input by
        // ffmpeg and aborts the process.
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
        // `hw_encoder: None` must reproduce byte-for-byte what the pre-hardware
        // code emitted — the software branch is untouched, only wrapped.
        let with_none = ffmpeg_args(320, 240, 30, "h264", 23, false, None, None, "o.mp4");
        assert!(with_none.iter().any(|s| s == "libx264"));
        assert!(with_none.iter().any(|s| s == "-crf"));
    }

    // ── Hardware acceleration: encoder selection (pure, no probe) ───────────

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
        // Only nvenc and amf "available" — videotoolbox and qsv are tried
        // first (per `HwEncoderFamily::ALL`) but rejected, so nvenc wins.
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

    // ── Hardware acceleration: `ffmpeg -encoders` parsing (pure) ────────────

    #[test]
    fn parses_encoder_names_out_of_realistic_ffmpeg_encoders_output() {
        // A trimmed, representative capture of `ffmpeg -hide_banner -encoders`:
        // a legend (flags-shaped but no real name, just "="), a separator
        // line, and a handful of real entries including the hardware ones
        // this module knows about.
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
        // The legend's bare "=" and the "Encoders:"/"------" scaffolding
        // must never be mistaken for encoder names.
        assert!(!names.contains("="));
        assert!(names.iter().all(|n| n != "Video" && n != "Audio"));
    }

    // ── Integration test (gated on ffmpeg + ffprobe) ────────────────────────
    //
    // Ties constat #1 (audio input declared before every output option — a
    // scenario with audio must actually produce a file) and constat #2
    // (the mixer and the muxer must agree on the sample rate) together
    // end-to-end, matching what the audit's own suggested fix asked for:
    // "Ajouter un test d'intégration gaté sur ffmpeg qui rend un scénario
    // avec piste audio et vérifie que le MP4 existe et contient deux flux."

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

    /// Write a minimal, hand-rolled canonical PCM WAV file (16-bit, mono) —
    /// no ffmpeg needed to produce the *input* fixture, only to encode it.
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
        buf.extend(std::iter::repeat_n(0u8, data_size as usize));

        std::fs::write(path, &buf).expect("write fixture wav");
    }

    fn write_wav_with_bursts(
        path: &std::path::Path,
        sample_rate: u32,
        total_duration_s: f64,
        burst_times_s: &[f64],
        burst_duration_s: f64,
    ) {
        let num_samples = (total_duration_s * sample_rate as f64).round() as u32;
        let burst_len = (burst_duration_s * sample_rate as f64).round() as u32;
        let amplitude = 20_000i16;
        let freq_hz = 1000.0;

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

        let burst_starts: Vec<u32> = burst_times_s
            .iter()
            .map(|t| (t * sample_rate as f64).round() as u32)
            .collect();
        for n in 0..num_samples {
            let in_burst = burst_starts
                .iter()
                .any(|&start| n >= start && n < start + burst_len);
            let sample = if in_burst {
                let t = n as f64 / sample_rate as f64;
                (amplitude as f64 * (2.0 * std::f64::consts::PI * freq_hz * t).sin()) as i16
            } else {
                0i16
            };
            buf.extend_from_slice(&sample.to_le_bytes());
        }

        std::fs::write(path, &buf).expect("write fixture wav");
    }

    fn decode_pcm_s16le_mono(path: &str, sample_rate: u32) -> Option<Vec<i16>> {
        let out = std::process::Command::new("ffmpeg")
            .args(["-y", "-loglevel", "error", "-i", path, "-map", "a:0"])
            .args(["-f", "s16le", "-acodec", "pcm_s16le"])
            .args(["-ar", &sample_rate.to_string(), "-ac", "1"])
            .arg("-")
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        Some(
            out.stdout
                .chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]))
                .collect(),
        )
    }

    fn find_onset_s(
        samples: &[i16],
        sample_rate: u32,
        expected_s: f64,
        window_s: f64,
        threshold: i16,
    ) -> Option<f64> {
        let lo = ((expected_s - window_s).max(0.0) * sample_rate as f64) as usize;
        let hi = (((expected_s + window_s) * sample_rate as f64) as usize).min(samples.len());
        samples
            .get(lo..hi)?
            .iter()
            .position(|&s| s.abs() > threshold)
            .map(|i| (lo + i) as f64 / sample_rate as f64)
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
        // Source at a rate different from OUTPUT_SAMPLE_RATE, to also
        // exercise the resampler rather than a same-rate passthrough.
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

    // ── Hardware acceleration: machine-dependent, gated on ffmpeg ───────────
    //
    // These exercise the real probe (`probe_ffmpeg_encoders`) and the real
    // spawn against whatever this machine's ffmpeg actually offers — unlike
    // the pure tests above, their outcome legitimately varies by machine, so
    // neither asserts a specific encoder was picked. What they do assert
    // (the probe doesn't panic; the encode succeeds and produces a file
    // either way) holds on any machine, hardware-capable or not, which is
    // what makes them safe to run in CI even though CI has no GPU.

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

    #[test]
    fn a_failed_audio_decode_does_not_leak_the_scratch_directory() {
        let bad_audio = std::env::temp_dir().join(format!(
            "rm_ffmpeg_leak_bad_audio_{}_{}.wav",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&bad_audio, b"not actually audio").expect("write garbage audio file");

        let json = format!(
            r#"{{"video": {{"width": 32, "height": 32, "fps": 10}},
                 "audio": [{{"src": "{}"}}],
                 "scenes": [{{"duration": 0.2, "children": []}}]}}"#,
            bad_audio.to_str().unwrap().replace('\\', "\\\\")
        );
        let scenario = crate::loader::load_scenario_from_source(None, Some(&json)).expect("load");

        let out = std::env::temp_dir().join(format!(
            "rm_ffmpeg_leak_out_{}_{}.mp4",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let result = super::encode_with_ffmpeg_hw(
            &scenario,
            out.to_str().unwrap(),
            true,
            "h264",
            None,
            false,
            false,
            None,
        );
        assert!(
            result.is_err(),
            "a garbage audio source must fail the decode, not silently succeed — this is the \
             precondition the leak actually needs: `create_dir` runs before the decode, so the \
             scratch directory exists by the time it fails"
        );

        let _ = std::fs::remove_file(&bad_audio);
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn remove_dir_all_on_drop_removes_the_directory_when_it_goes_out_of_scope() {
        let dir = std::env::temp_dir().join(format!(
            "rustmotion_remove_dir_all_on_drop_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).expect("set up a directory to guard");
        assert!(dir.exists());

        {
            let _guard = RemoveDirAllOnDrop(Some(dir.clone()));
        }

        assert!(
            !dir.exists(),
            "the directory must be gone once the guard drops — this is what closes the leak an \
             early `?` return (e.g. a failed audio decode, before ffmpeg is ever spawned) used \
             to skip, since the only cleanup used to sit after `child.wait()`"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── Frame-range render + concat: the brief's "test that matters most" ──
    //
    // "rendre un scénario en un seul morceau, puis le même en N segments
    // concaténés, et comparer. Les deux doivent avoir le même nombre de
    // frames et la même durée audio." A scenario with an audio track is
    // part of this test on purpose — it is the only way to exercise the
    // segment-audio-offset fix (`mix_audio_tracks_segment`) through the
    // actual default (ffmpeg) render path, not just the pure mixer unit
    // tests in `encode::audio`.

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

    /// Extract the frame at `time_s` into `path` as a PNG and return its
    /// center pixel's RGB. Accurate (post-`-i`) seeking, not the fast
    /// keyframe-snapping `-ss`-before`-i` form — this scenario's scenes are
    /// each a full second of one flat color, so any frame within a scene's
    /// window has the same color regardless of exactly which one lands, but
    /// accurate seeking keeps the test honest about which scene it read.
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

        // 3 scenes x 1.0s x 10fps = 30 frames, no transitions, so segment
        // boundaries land exactly on scene boundaries: (0,9)=red, (10,19)=green,
        // (20,29)=blue. Frame-range indices are the same index space
        // `build_frame_tasks` (and `--frame N`) already use.
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
        // 3.0s of audio at the video's own duration, so a whole-track (no
        // explicit `end`) plays across all three segments — exactly the
        // shape that needs the segment-audio-offset fix to sound right.
        let burst_times = [0.5, 1.5, 2.5];
        write_wav_with_bursts(
            &wav_path,
            crate::encode::audio::OUTPUT_SAMPLE_RATE,
            3.0,
            &burst_times,
            0.05,
        );

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

        // 1. Render the whole scenario in one piece.
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

        // 2. Render the same scenario as three independent frame-range segments.
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

        // 3. Concatenate the three segments.
        super::concat_mp4_segments(&seg_outs, concat_out.to_str().unwrap())
            .expect("concat must succeed");

        // 4. Same frame count.
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

        // 5. Same decoded audio onsets.
        let sr = crate::encode::audio::OUTPUT_SAMPLE_RATE;
        let full_samples =
            decode_pcm_s16le_mono(full_out.to_str().unwrap(), sr).expect("decode full render");
        let concat_samples = decode_pcm_s16le_mono(concat_out.to_str().unwrap(), sr)
            .expect("decode concatenated render");

        for &expected in &burst_times {
            let full_onset = find_onset_s(&full_samples, sr, expected, 0.3, 5_000)
                .unwrap_or_else(|| panic!("full render: no burst detected near {expected}s"));
            let concat_onset = find_onset_s(&concat_samples, sr, expected, 0.3, 5_000)
                .unwrap_or_else(|| {
                    panic!("concatenated render: no burst detected near {expected}s")
                });
            let drift = (full_onset - concat_onset).abs();
            assert!(
                drift < 0.02,
                "burst near {expected}s must decode to the same position in both renders \
                 (tighter than one seam's ~33-45ms drift): full={full_onset:.4}s \
                 concat={concat_onset:.4}s, drift={drift:.4}s"
            );
        }

        let len_diff_s =
            (full_samples.len() as f64 - concat_samples.len() as f64).abs() / sr as f64;
        assert!(
            len_diff_s < 0.02,
            "decoded sample count must match within 20ms: full={} concat={} ({len_diff_s:.4}s)",
            full_samples.len(),
            concat_samples.len()
        );

        // 6. Pixel check at the middle of segment 2 (t=1.5s, inside the solid-
        // green scene): the concatenated output's content there must match
        // the full render's, proving the split didn't shift which frames
        // land where.
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

    #[test]
    fn concat_still_joins_silent_segments_with_no_audio_probing() {
        if !ffmpeg_on_path() || !ffprobe_on_path() {
            eprintln!(
                "concat_still_joins_silent_segments_with_no_audio_probing: \
                 ffmpeg/ffprobe not found — skipping"
            );
            return;
        }

        let json = r#"{"video": {"width": 32, "height": 32, "fps": 10},
             "scenes": [
                 {"duration": 1.0, "children": []},
                 {"duration": 1.0, "children": []}
             ]}"#;
        let scenario = crate::loader::load_scenario_from_source(None, Some(json)).expect("load");

        let pid = std::process::id();
        let seg_outs: Vec<std::path::PathBuf> = (0..2)
            .map(|i| std::env::temp_dir().join(format!("rm_concat_silent_seg{i}_{pid}.mp4")))
            .collect();
        let concat_out = std::env::temp_dir().join(format!("rm_concat_silent_out_{pid}.mp4"));
        for p in [&concat_out].into_iter().chain(seg_outs.iter()) {
            let _ = std::fs::remove_file(p);
        }

        for (i, (start, end)) in [(0u32, 9u32), (10, 19)].into_iter().enumerate() {
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
            .unwrap_or_else(|e| panic!("segment {i} render must succeed: {e}"));
        }

        super::concat_mp4_segments(&seg_outs, concat_out.to_str().unwrap())
            .expect("concat of silent segments must succeed");

        let frames = ffprobe_frame_count(concat_out.to_str().unwrap())
            .expect("ffprobe must report the concatenated render's frame count");
        assert_eq!(frames, 20, "2 segments x 10 frames each");
        assert!(
            !segment_has_audio_stream(&concat_out),
            "no input segment had audio, so the joined output must not gain one"
        );

        for p in [&concat_out].into_iter().chain(seg_outs.iter()) {
            let _ = std::fs::remove_file(p);
        }
    }

    #[test]
    fn concat_refuses_to_silently_drop_an_unreadable_segment() {
        if !ffmpeg_on_path() || !ffprobe_on_path() {
            eprintln!(
                "concat_refuses_to_silently_drop_an_unreadable_segment: \
                 ffmpeg/ffprobe not found — skipping"
            );
            return;
        }

        let json = r#"{"video": {"width": 32, "height": 32, "fps": 10},
             "scenes": [{"duration": 1.5, "children": []}]}"#;
        let scenario = crate::loader::load_scenario_from_source(None, Some(json)).expect("load");

        let pid = std::process::id();
        let valid_seg = std::env::temp_dir().join(format!("rm_concat_corrupt_valid_{pid}.mp4"));
        let corrupt_seg = std::env::temp_dir().join(format!("rm_concat_corrupt_garbage_{pid}.mp4"));
        let out = std::env::temp_dir().join(format!("rm_concat_corrupt_out_{pid}.mp4"));
        for p in [&valid_seg, &corrupt_seg, &out] {
            let _ = std::fs::remove_file(p);
        }

        super::encode_with_ffmpeg_hw(
            &scenario,
            valid_seg.to_str().unwrap(),
            true,
            "h264",
            None,
            false,
            false,
            None,
        )
        .expect("valid segment render must succeed");
        std::fs::write(&corrupt_seg, [0x2a_u8; 500]).expect("write garbage segment");

        let result = super::concat_mp4_segments(
            &[valid_seg.clone(), corrupt_seg.clone()],
            out.to_str().unwrap(),
        );

        assert!(
            result.is_err(),
            "concat must fail loudly on an unreadable segment instead of silently dropping it"
        );

        for p in [&valid_seg, &corrupt_seg, &out] {
            let _ = std::fs::remove_file(p);
        }
    }

    #[test]
    fn concat_refuses_segments_with_different_resolutions() {
        if !ffmpeg_on_path() || !ffprobe_on_path() {
            eprintln!(
                "concat_refuses_segments_with_different_resolutions: ffmpeg/ffprobe not found \
                 — skipping"
            );
            return;
        }

        let json_at = |size: u32| {
            format!(
                r#"{{"video": {{"width": {size}, "height": {size}, "fps": 10}},
                     "scenes": [{{"duration": 0.3, "children": []}}]}}"#
            )
        };

        let pid = std::process::id();
        let small_seg = std::env::temp_dir().join(format!("rm_concat_reso_small_{pid}.mp4"));
        let big_seg = std::env::temp_dir().join(format!("rm_concat_reso_big_{pid}.mp4"));
        let out = std::env::temp_dir().join(format!("rm_concat_reso_out_{pid}.mp4"));
        for p in [&small_seg, &big_seg, &out] {
            let _ = std::fs::remove_file(p);
        }

        for (size, path) in [(64u32, &small_seg), (128u32, &big_seg)] {
            let scenario =
                crate::loader::load_scenario_from_source(None, Some(&json_at(size))).expect("load");
            super::encode_with_ffmpeg_hw(
                &scenario,
                path.to_str().unwrap(),
                true,
                "h264",
                None,
                false,
                false,
                None,
            )
            .unwrap_or_else(|e| panic!("{size}x{size} segment render must succeed: {e}"));
        }

        let result = super::concat_mp4_segments(
            &[small_seg.clone(), big_seg.clone()],
            out.to_str().unwrap(),
        );

        let err = result.expect_err(
            "concat must refuse segments of different resolutions instead of producing a file \
             whose header contradicts its frames",
        );
        let msg = err.to_string();
        assert!(
            msg.contains("64") && msg.contains("128"),
            "the refusal must name the actual mismatching dimensions: {msg}"
        );
        assert!(
            !out.exists(),
            "no output should be left behind when concat refuses the input"
        );

        for p in [&small_seg, &big_seg, &out] {
            let _ = std::fs::remove_file(p);
        }
    }
}
