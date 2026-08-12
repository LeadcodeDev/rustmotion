use crate::tui;
use crate::OutputFormat;
use rustmotion::encode;
use rustmotion::engine;
use rustmotion::error::{Result, RustmotionError};
use rustmotion::schema::ResolvedScenario;
use std::path::{Path, PathBuf};

use crate::commands::validation::{self, ValidationSource};

/// Load + validate a scenario for watch mode. On validation failure prints the
/// report and returns the typed error so the caller can decide how to handle it.
///
/// `strict_attrs` is accepted for CLI-surface parity with `validate` (M5,
/// issue #110) but is a no-op in practice: unknown component attributes
/// block by default now (`ValidationReport::is_blocking`), and `--watch`
/// mode never writes a `--report` JSON file, so there is no bucket left for
/// `promote_attr_warnings` to affect.
#[allow(clippy::too_many_arguments)]
fn load_for_watch(
    input: &Path,
    no_validate: bool,
    lenient: bool,
    strict_anim: bool,
    strict_attrs: bool,
) -> Result<ResolvedScenario> {
    let loaded = validation::load(ValidationSource::File(input))?;
    if !no_validate {
        let mut report = validation::run_checks(&loaded, strict_anim);
        if strict_attrs {
            report.promote_attr_warnings();
        }
        if !report.is_clean() {
            validation::print_report(&report, &input.display().to_string());
        }
        if report.is_blocking(lenient) {
            return Err(report.to_error());
        }
    }
    Ok(loaded.scenario)
}

#[allow(clippy::too_many_arguments)]
pub fn cmd_render(
    scenario: ResolvedScenario,
    output: &Path,
    frame: Option<u32>,
    output_format: Option<&OutputFormat>,
    quiet: bool,
    codec: Option<String>,
    crf: Option<u8>,
    format: Option<String>,
    transparent: bool,
    hardware_acceleration: bool,
) -> Result<()> {
    // Refuse a codec the container cannot hold before rendering anything:
    // ffmpeg otherwise discovers it after every frame is done, and reports it
    // as a raw -22 with no output file.
    //
    // `--frame` is exempt: it writes a PNG still through `render_single_frame`
    // and never reaches an encoder, so the codec is not part of that operation.
    if frame.is_none() {
        let container = format
            .as_deref()
            .unwrap_or_else(|| output.extension().and_then(|e| e.to_str()).unwrap_or("mp4"));
        encode::check_codec_container(codec.as_deref().unwrap_or("h264"), container)?;
    }

    let start = std::time::Instant::now();

    // Load custom fonts if defined
    if !scenario.fonts.is_empty() {
        engine::renderer::load_custom_fonts(&scenario.fonts);
    }

    // Create parent directories if they don't exist
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    if let Some(frame_num) = frame {
        // Single frame render to PNG
        let png_path = if output.extension().map(|e| e == "mp4").unwrap_or(false) {
            output.with_extension("png")
        } else {
            output.to_path_buf()
        };
        render_single_frame(&scenario, frame_num, &png_path)?;
        if !quiet {
            eprintln!("Frame {} saved to {}", frame_num, png_path.display());
        }
    } else {
        // Determine output format
        let fmt = format
            .as_deref()
            .unwrap_or_else(|| output.extension().and_then(|e| e.to_str()).unwrap_or("mp4"));

        let config = &scenario.video;
        let output_str = output
            .to_str()
            .ok_or_else(|| RustmotionError::NonUtf8Path {
                path: output.to_string_lossy().into_owned(),
            })?;

        // Helper: create TUI and wrap encode call with progress
        let make_tui = |codec_label: &str| -> Option<tui::TuiProgress> {
            if quiet {
                return None;
            }
            let total = encode::build_frame_tasks(&scenario).len() as u32;
            tui::TuiProgress::new(
                total,
                output_str,
                config.width,
                config.height,
                config.fps,
                codec_label,
            )
            .ok()
        };

        if hardware_acceleration && matches!(fmt, "png-seq" | "gif" | "raw") && !quiet {
            eprintln!(
                "Warning: --hardware-acceleration has no effect on `{fmt}` output — only the \
                 ffmpeg-driven video encode step (mp4/webm/mov) can use a hardware encoder."
            );
        }

        match fmt {
            "png-seq" => {
                let mut tui = make_tui("png");
                let mut cb = |p: encode::EncodeProgress| {
                    if let (Some(ref mut t), encode::EncodeProgress::Rendering(c, _)) =
                        (&mut tui, &p)
                    {
                        t.set_progress(*c);
                    }
                };
                encode::encode_png_sequence(
                    &scenario,
                    output_str,
                    quiet,
                    transparent,
                    Some(&mut cb),
                )?;
                if let Some(ref mut t) = tui {
                    t.finish("Done!");
                }
            }
            "gif" => {
                let mut tui = make_tui("gif");
                let mut cb = |p: encode::EncodeProgress| {
                    if let (Some(ref mut t), encode::EncodeProgress::Rendering(c, _)) =
                        (&mut tui, &p)
                    {
                        t.set_progress(*c);
                    }
                };
                encode::encode_gif(&scenario, output_str, quiet, Some(&mut cb))?;
                if let Some(ref mut t) = tui {
                    t.finish("Done!");
                }
            }
            "raw" => {
                encode::encode_raw_stdout(&scenario, false)?;
            }
            _ => {
                let ffmpeg_available = std::process::Command::new("ffmpeg")
                    .arg("-version")
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false);

                let codec_str = codec.as_deref().unwrap_or("h264");

                if ffmpeg_available {
                    let mut tui = make_tui(codec_str);
                    let mut cb = |p: encode::EncodeProgress| {
                        if let Some(ref mut t) = tui {
                            match &p {
                                encode::EncodeProgress::Rendering(c, _) => t.set_progress(*c),
                                encode::EncodeProgress::Muxing => t.set_status("Muxing"),
                                _ => {}
                            }
                        }
                    };
                    encode::video::encode_with_ffmpeg_hw(
                        &scenario,
                        output_str,
                        quiet,
                        codec_str,
                        crf,
                        transparent,
                        hardware_acceleration,
                        Some(&mut cb),
                    )?;
                    if let Some(ref mut t) = tui {
                        t.finish("Done!");
                    }
                } else {
                    if hardware_acceleration && !quiet {
                        eprintln!(
                            "Hardware acceleration requested but ffmpeg was not found on PATH \
                             (only ffmpeg can drive a hardware encoder); continuing with the \
                             bundled software encoder."
                        );
                    }
                    let mut tui = make_tui("h264");
                    let mut cb = |p: encode::EncodeProgress| {
                        if let Some(ref mut t) = tui {
                            match &p {
                                encode::EncodeProgress::Rendering(c, _) => t.set_progress(*c),
                                encode::EncodeProgress::Encoding(c, _) => {
                                    t.set_status("Encoding H.264");
                                    t.set_progress(*c);
                                }
                                encode::EncodeProgress::Muxing => t.set_status("Muxing to MP4"),
                            }
                        }
                    };
                    encode::encode_video(&scenario, output_str, quiet, Some(&mut cb))?;
                    if let Some(ref mut t) = tui {
                        t.finish("Done!");
                    }
                }
            }
        }
    }

    let elapsed = start.elapsed();

    if let Some(OutputFormat::Json) = output_format {
        let result = serde_json::json!({
            "status": "success",
            "output": output.to_string_lossy(),
            "duration_ms": elapsed.as_millis(),
        });
        println!("{}", serde_json::to_string(&result)?);
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn cmd_watch(
    input: &PathBuf,
    output: &Path,
    frame: Option<u32>,
    output_format: Option<&OutputFormat>,
    quiet: bool,
    codec: Option<String>,
    crf: Option<u8>,
    format: Option<String>,
    transparent: bool,
    hardware_acceleration: bool,
    no_validate: bool,
    lenient: bool,
    strict_anim: bool,
    strict_attrs: bool,
) -> Result<()> {
    use notify::{RecursiveMode, Watcher};
    use std::sync::mpsc;

    // Refuse a codec the container cannot hold before rendering anything (see
    // `cmd_render`; `--frame` is exempt for the same reason).
    if frame.is_none() {
        let container = format
            .as_deref()
            .unwrap_or_else(|| output.extension().and_then(|e| e.to_str()).unwrap_or("mp4"));
        encode::check_codec_container(codec.as_deref().unwrap_or("h264"), container)?;
    }

    // Determine if we can use incremental rendering (native h264 only).
    // Hardware acceleration is an ffmpeg-only feature (see
    // `encode::video::encode_with_ffmpeg_hw`): the incremental/native path
    // never shells out to ffmpeg at all, so routing a hardware-acceleration
    // request there would silently do nothing. Forcing `use_ffmpeg` here
    // keeps that request meaningful under `--watch` too, same as it already
    // is for `codec`/`format`/`transparent`.
    let fmt = format
        .as_deref()
        .unwrap_or_else(|| output.extension().and_then(|e| e.to_str()).unwrap_or("mp4"));
    let use_ffmpeg = codec.as_deref().is_some_and(|c| c != "h264")
        || matches!(fmt, "webm" | "mov")
        || transparent
        || hardware_acceleration;
    let can_incremental =
        frame.is_none() && !matches!(fmt, "png-seq" | "gif" | "raw") && !use_ffmpeg;

    let output_str = output
        .to_str()
        .ok_or_else(|| RustmotionError::NonUtf8Path {
            path: output.to_string_lossy().into_owned(),
        })?
        .to_string();

    // State for incremental rendering
    let mut prev_segments: Option<Vec<encode::SceneSegment>> = None;
    let mut prev_config_hash: Option<u64> = None;

    // Initialize TUI for watch mode
    let codec_label = codec.as_deref().unwrap_or("h264");
    let mut tui_watch: Option<tui::TuiWatch> = None;

    // Track included file paths for watch mode
    let mut initial_includes: Vec<PathBuf> = Vec::new();

    // Initial render
    match load_for_watch(input, no_validate, lenient, strict_anim, strict_attrs) {
        Ok(scenario) => {
            initial_includes = scenario.included_paths.clone();

            // Load custom fonts if defined
            if !scenario.fonts.is_empty() {
                engine::renderer::load_custom_fonts(&scenario.fonts);
            }

            if !quiet {
                tui_watch = tui::TuiWatch::new(
                    &input.display().to_string(),
                    &output.display().to_string(),
                    scenario.video.width,
                    scenario.video.height,
                    scenario.video.fps,
                    codec_label,
                )
                .ok();
            }

            if can_incremental {
                let config_hash = encode::hash_video_config(&scenario.video);
                if let Some(ref mut tui) = tui_watch {
                    tui.set_phase(tui::WatchPhase::InitialRender);
                }
                let mut encoding_started = false;
                let mut cb = |progress: encode::EncodeProgress| {
                    if let Some(ref mut tui) = tui_watch {
                        match progress {
                            encode::EncodeProgress::Rendering(current, total) => {
                                tui.set_frame_progress(current, total)
                            }
                            encode::EncodeProgress::Encoding(current, total) => {
                                if !encoding_started {
                                    tui.set_phase(tui::WatchPhase::Encoding);
                                    encoding_started = true;
                                }
                                tui.set_frame_progress(current, total);
                            }
                            encode::EncodeProgress::Muxing => {
                                tui.set_phase(tui::WatchPhase::Muxing)
                            }
                        }
                    }
                };
                match encode::encode_video_incremental(
                    &scenario,
                    &output_str,
                    quiet,
                    None,
                    Some(&mut cb),
                ) {
                    Ok(segments) => {
                        prev_segments = Some(segments);
                        prev_config_hash = Some(config_hash);
                        if let Some(ref mut tui) = tui_watch {
                            tui.finish_render();
                        }
                    }
                    Err(e) => eprintln!("Render error: {}", e),
                }
            } else {
                engine::clear_asset_cache();
                if let Err(e) = cmd_render(
                    scenario,
                    output,
                    frame,
                    output_format,
                    quiet,
                    codec.clone(),
                    crf,
                    format.clone(),
                    transparent,
                    hardware_acceleration,
                ) {
                    eprintln!("Render error: {}", e);
                }
                if let Some(ref mut tui) = tui_watch {
                    tui.finish_render();
                }
            }
        }
        Err(e) => eprintln!("Load error: {}", e),
    }

    let (tx, rx) = mpsc::channel();

    let mut watcher = notify::recommended_watcher(
        move |res: std::result::Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                if event.kind.is_modify() || event.kind.is_create() {
                    let _ = tx.send(());
                }
            }
        },
    )?;

    watcher.watch(input.as_ref(), RecursiveMode::NonRecursive)?;

    // Watch included files from initial render
    let mut watched_includes: Vec<PathBuf> = initial_includes;
    for inc in &watched_includes {
        let _ = watcher.watch(inc.as_ref(), RecursiveMode::NonRecursive);
    }

    // Debounce: wait for changes, then re-render
    let mut last_err_repr: Option<String> = None;
    let mut consecutive_err_count: u32 = 0;
    let mut suppressed = false;
    loop {
        // Block until a change event
        rx.recv().map_err(|_| RustmotionError::WatcherClosed)?;

        // Drain any additional events (debounce)
        std::thread::sleep(std::time::Duration::from_millis(100));
        while rx.try_recv().is_ok() {}

        match load_for_watch(input, no_validate, lenient, strict_anim, strict_attrs) {
            Ok(scenario) => {
                // Reset error backoff on a successful load
                if consecutive_err_count > 0 && suppressed {
                    eprintln!("Recovered from previous errors.");
                }
                last_err_repr = None;
                consecutive_err_count = 0;
                suppressed = false;
                // Update watched includes: unwatch old, watch new
                for old in &watched_includes {
                    let _ = watcher.unwatch(old.as_ref());
                }
                watched_includes = scenario.included_paths.clone();
                for inc in &watched_includes {
                    let _ = watcher.watch(inc.as_ref(), RecursiveMode::NonRecursive);
                }

                if can_incremental {
                    let config_hash = encode::hash_video_config(&scenario.video);
                    // If video config changed (resolution/fps), do full re-render
                    let use_prev = if prev_config_hash == Some(config_hash) {
                        prev_segments.as_deref()
                    } else {
                        engine::clear_asset_cache();
                        None
                    };

                    // Count changed scenes for the TUI
                    let view0_scenes = scenario.views.first().map(|v| &v.scenes[..]).unwrap_or(&[]);
                    let num_scenes = view0_scenes.len();
                    let scene_hashes: Vec<u64> = view0_scenes
                        .iter()
                        .map(encode::hash_video_config_scene)
                        .collect();
                    let changed = if let Some(prev) = use_prev {
                        if prev.len() == num_scenes {
                            (0..num_scenes)
                                .filter(|&i| {
                                    let hash_changed = scene_hashes[i] != prev[i].scene_hash;
                                    let next_changed = if i + 1 < num_scenes {
                                        scene_hashes[i + 1] != prev[i + 1].scene_hash
                                            && view0_scenes[i + 1].transition.is_some()
                                    } else {
                                        false
                                    };
                                    hash_changed || next_changed
                                })
                                .count()
                        } else {
                            num_scenes
                        }
                    } else {
                        num_scenes
                    };

                    if let Some(ref mut tui) = tui_watch {
                        tui.set_phase(tui::WatchPhase::Rerendering {
                            changed,
                            total: num_scenes,
                        });
                    }

                    let mut encoding_started = false;
                    let mut cb = |progress: encode::EncodeProgress| {
                        if let Some(ref mut tui) = tui_watch {
                            match progress {
                                encode::EncodeProgress::Rendering(current, total) => {
                                    tui.set_frame_progress(current, total)
                                }
                                encode::EncodeProgress::Encoding(current, total) => {
                                    if !encoding_started {
                                        tui.set_phase(tui::WatchPhase::Encoding);
                                        encoding_started = true;
                                    }
                                    tui.set_frame_progress(current, total);
                                }
                                encode::EncodeProgress::Muxing => {
                                    tui.set_phase(tui::WatchPhase::Muxing)
                                }
                            }
                        }
                    };
                    match encode::encode_video_incremental(
                        &scenario,
                        &output_str,
                        quiet,
                        use_prev,
                        Some(&mut cb),
                    ) {
                        Ok(segments) => {
                            prev_segments = Some(segments);
                            prev_config_hash = Some(config_hash);
                            if let Some(ref mut tui) = tui_watch {
                                tui.finish_render();
                            }
                        }
                        Err(e) => eprintln!("Render error: {}", e),
                    }
                } else {
                    engine::clear_asset_cache();
                    if let Err(e) = cmd_render(
                        scenario,
                        output,
                        frame,
                        output_format,
                        quiet,
                        codec.clone(),
                        crf,
                        format.clone(),
                        transparent,
                        hardware_acceleration,
                    ) {
                        eprintln!("Render error: {}", e);
                    }
                    if let Some(ref mut tui) = tui_watch {
                        tui.finish_render();
                    }
                }
            }
            Err(e) => {
                let repr = e.to_string();
                if last_err_repr.as_deref() == Some(&repr) {
                    consecutive_err_count += 1;
                    if consecutive_err_count > 3 {
                        if !suppressed {
                            eprintln!("Load error: {} (further identical errors suppressed)", repr);
                            suppressed = true;
                        }
                    } else {
                        eprintln!("Load error: {}", repr);
                    }
                } else {
                    eprintln!("Load error: {}", repr);
                    last_err_repr = Some(repr);
                    consecutive_err_count = 1;
                    suppressed = false;
                }
            }
        }
    }
}

fn render_single_frame(
    scenario: &ResolvedScenario,
    frame_num: u32,
    output: &PathBuf,
) -> Result<()> {
    // Use the same task pipeline as the encoder so `--frame N` always shows
    // exactly what frame N of the rendered MP4 contains. Summing scene
    // durations directly would skip transition overlaps and drift the
    // numbering by `transition_duration * fps` per scene boundary.
    let config = &scenario.video;
    let tasks = encode::build_frame_tasks(scenario);
    let total = tasks.len() as u32;
    let task = tasks
        .get(frame_num as usize)
        .ok_or(RustmotionError::FrameOutOfRange {
            frame: frame_num,
            total,
        })?;

    let rgba = encode::render_frame_task_scaled(config, scenario, task, 1.0)?;
    let img = image::RgbaImage::from_raw(config.width, config.height, rgba)
        .ok_or(RustmotionError::PixelImage)?;
    img.save(output)?;
    Ok(())
}
