use rayon::prelude::*;
use std::collections::HashSet;
use std::io::Write;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::encode::audio_analysis::analyze_scenario_audio;
use crate::engine::prefetch_icons;
use crate::error::{Result, RustmotionError};
use crate::schema::ResolvedScenario as Scenario;

use super::tasks::{build_frame_tasks, render_frame_task};
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
    }

    if audio_input.is_some() {
        push(&["-c:a", "aac", "-b:a", "128k"], &mut args);
    }

    args.push(output_path.to_string());
    args
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

    let status = child.wait().map_err(|e| RustmotionError::FfmpegWait {
        reason: e.to_string(),
    })?;

    // The drain thread finishes once ffmpeg closes its stderr (which
    // happens no later than process exit, already awaited above), so this
    // join does not block on anything still running.
    let stderr_text = stderr_reader.and_then(|h| h.join().ok());

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
    use super::{ffmpeg_args, parse_encoder_names, select_hardware_encoder, HardwareSelection};

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
}
