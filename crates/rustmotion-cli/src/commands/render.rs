use rustmotion::encode;
use rustmotion::engine;
use rustmotion::error::{Result, RustmotionError};
use rustmotion::schema::{ResolvedScenario, ViewType};
use rustmotion::loader::load_scenario;
use crate::tui;
use crate::OutputFormat;
use std::path::PathBuf;

pub fn cmd_render(
    scenario: ResolvedScenario,
    output: &PathBuf,
    frame: Option<u32>,
    output_format: Option<&OutputFormat>,
    quiet: bool,
    codec: Option<String>,
    crf: Option<u8>,
    format: Option<String>,
    transparent: bool,
) -> Result<()> {
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
            output.clone()
        };
        render_single_frame(&scenario, frame_num, &png_path)?;
        if !quiet {
            eprintln!("Frame {} saved to {}", frame_num, png_path.display());
        }
    } else {
        // Determine output format
        let fmt = format.as_deref().unwrap_or_else(|| {
            output.extension()
                .and_then(|e| e.to_str())
                .unwrap_or("mp4")
        });

        let config = &scenario.video;
        let output_str = output.to_str().unwrap();

        // Helper: create TUI and wrap encode call with progress
        let make_tui = |codec_label: &str| -> Option<tui::TuiProgress> {
            if quiet { return None; }
            let total = encode::build_frame_tasks(&scenario).len() as u32;
            tui::TuiProgress::new(total, output_str, config.width, config.height, config.fps, codec_label).ok()
        };

        match fmt {
            "png-seq" => {
                let mut tui = make_tui("png");
                let mut cb = |p: encode::EncodeProgress| {
                    if let (Some(ref mut t), encode::EncodeProgress::Rendering(c, _)) = (&mut tui, &p) {
                        t.set_progress(*c);
                    }
                };
                encode::encode_png_sequence(&scenario, output_str, quiet, transparent, Some(&mut cb))?;
                if let Some(ref mut t) = tui { t.finish("Done!"); }
            }
            "gif" => {
                let mut tui = make_tui("gif");
                let mut cb = |p: encode::EncodeProgress| {
                    if let (Some(ref mut t), encode::EncodeProgress::Rendering(c, _)) = (&mut tui, &p) {
                        t.set_progress(*c);
                    }
                };
                encode::encode_gif(&scenario, output_str, quiet, Some(&mut cb))?;
                if let Some(ref mut t) = tui { t.finish("Done!"); }
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
                    encode::encode_with_ffmpeg(&scenario, output_str, quiet, codec_str, crf, transparent, Some(&mut cb))?;
                    if let Some(ref mut t) = tui { t.finish("Done!"); }
                } else {
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
                    if let Some(ref mut t) = tui { t.finish("Done!"); }
                }
            }
        }
    }

    let elapsed = start.elapsed();

    if let Some(OutputFormat::Json) = output_format {
        let result = serde_json::json!({
            "status": "success",
            "output": output.to_str().unwrap(),
            "duration_ms": elapsed.as_millis(),
        });
        println!("{}", serde_json::to_string(&result)?);
    }

    Ok(())
}

pub fn cmd_watch(
    input: &PathBuf,
    output: &PathBuf,
    frame: Option<u32>,
    output_format: Option<&OutputFormat>,
    quiet: bool,
    codec: Option<String>,
    crf: Option<u8>,
    format: Option<String>,
    transparent: bool,
) -> Result<()> {
    use notify::{Watcher, RecursiveMode};
    use std::sync::mpsc;

    // Determine if we can use incremental rendering (native h264 only)
    let fmt = format.as_deref().unwrap_or_else(|| {
        output.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("mp4")
    });
    let use_ffmpeg = codec.as_deref().map_or(false, |c| c != "h264")
        || matches!(fmt, "webm" | "mov")
        || transparent;
    let can_incremental = frame.is_none()
        && !matches!(fmt, "png-seq" | "gif" | "raw")
        && !use_ffmpeg;

    // State for incremental rendering
    let mut prev_segments: Option<Vec<encode::SceneSegment>> = None;
    let mut prev_config_hash: Option<u64> = None;

    // Initialize TUI for watch mode
    let codec_label = codec.as_deref().unwrap_or("h264");
    let mut tui_watch: Option<tui::TuiWatch> = None;

    // Track included file paths for watch mode
    let mut initial_includes: Vec<PathBuf> = Vec::new();

    // Initial render
    match load_scenario(input) {
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
                ).ok();
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
                            encode::EncodeProgress::Rendering(current, total) => tui.set_frame_progress(current, total),
                            encode::EncodeProgress::Encoding(current, total) => {
                                if !encoding_started {
                                    tui.set_phase(tui::WatchPhase::Encoding);
                                    encoding_started = true;
                                }
                                tui.set_frame_progress(current, total);
                            }
                            encode::EncodeProgress::Muxing => tui.set_phase(tui::WatchPhase::Muxing),
                        }
                    }
                };
                match encode::encode_video_incremental(&scenario, output.to_str().unwrap(), quiet, None, Some(&mut cb)) {
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
                if let Err(e) = cmd_render(scenario, output, frame, output_format, quiet, codec.clone(), crf, format.clone(), transparent) {
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

    let mut watcher = notify::recommended_watcher(move |res: std::result::Result<notify::Event, notify::Error>| {
        if let Ok(event) = res {
            if event.kind.is_modify() || event.kind.is_create() {
                let _ = tx.send(());
            }
        }
    })?;

    watcher.watch(input.as_ref(), RecursiveMode::NonRecursive)?;

    // Watch included files from initial render
    let mut watched_includes: Vec<PathBuf> = initial_includes;
    for inc in &watched_includes {
        let _ = watcher.watch(inc.as_ref(), RecursiveMode::NonRecursive);
    }

    // Debounce: wait for changes, then re-render
    loop {
        // Block until a change event
        rx.recv().map_err(|_| RustmotionError::WatcherClosed)?;

        // Drain any additional events (debounce)
        std::thread::sleep(std::time::Duration::from_millis(100));
        while rx.try_recv().is_ok() {}

        match load_scenario(input) {
            Ok(scenario) => {
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
                    let view0_scenes = scenario.views.get(0).map(|v| &v.scenes[..]).unwrap_or(&[]);
                    let num_scenes = view0_scenes.len();
                    let scene_hashes: Vec<u64> = view0_scenes.iter()
                        .map(|s| encode::hash_video_config_scene(s))
                        .collect();
                    let changed = if let Some(ref prev) = use_prev {
                        if prev.len() == num_scenes {
                            (0..num_scenes).filter(|&i| {
                                let hash_changed = scene_hashes[i] != prev[i].scene_hash;
                                let next_changed = if i + 1 < num_scenes {
                                    scene_hashes[i + 1] != prev[i + 1].scene_hash
                                        && view0_scenes[i + 1].transition.is_some()
                                } else { false };
                                hash_changed || next_changed
                            }).count()
                        } else { num_scenes }
                    } else { num_scenes };

                    if let Some(ref mut tui) = tui_watch {
                        tui.set_phase(tui::WatchPhase::Rerendering { changed, total: num_scenes });
                    }

                    let mut encoding_started = false;
                    let mut cb = |progress: encode::EncodeProgress| {
                        if let Some(ref mut tui) = tui_watch {
                            match progress {
                                encode::EncodeProgress::Rendering(current, total) => tui.set_frame_progress(current, total),
                                encode::EncodeProgress::Encoding(current, total) => {
                                    if !encoding_started {
                                        tui.set_phase(tui::WatchPhase::Encoding);
                                        encoding_started = true;
                                    }
                                    tui.set_frame_progress(current, total);
                                }
                                encode::EncodeProgress::Muxing => tui.set_phase(tui::WatchPhase::Muxing),
                            }
                        }
                    };
                    match encode::encode_video_incremental(&scenario, output.to_str().unwrap(), quiet, use_prev, Some(&mut cb)) {
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
                    if let Err(e) = cmd_render(scenario, output, frame, output_format, quiet, codec.clone(), crf, format.clone(), transparent) {
                        eprintln!("Render error: {}", e);
                    }
                    if let Some(ref mut tui) = tui_watch {
                        tui.finish_render();
                    }
                }
            }
            Err(e) => eprintln!("Load error: {}", e),
        }
    }
}

fn render_single_frame(scenario: &ResolvedScenario, frame_num: u32, output: &PathBuf) -> Result<()> {
    let config = &scenario.video;
    let fps = config.fps;

    // Find which view and frame this belongs to
    let mut frame_offset = 0u32;
    for view in &scenario.views {
        match view.view_type {
            ViewType::World => {
                // World view: all scenes share a single continuous timeline
                let timeline = engine::world::WorldTimeline::build(view, fps, config.width, config.height);
                let view_frames = timeline.total_frames(fps);
                if frame_num < frame_offset + view_frames {
                    let frame_in_view = frame_num - frame_offset;
                    let rgba = engine::render::render_world_frame_scaled(
                        config, view, &timeline, frame_in_view, 1.0,
                    )?;

                    let img = image::RgbaImage::from_raw(config.width, config.height, rgba)
                        .ok_or(RustmotionError::PixelImage)?;
                    img.save(output)?;
                    return Ok(());
                }
                frame_offset += view_frames;
            }
            ViewType::Slide => {
                // Slide view: each scene is independent
                for (scene_idx, scene) in view.scenes.iter().enumerate() {
                    let scene_frames = (scene.duration * fps as f64).round() as u32;
                    if frame_num < frame_offset + scene_frames {
                        let local_frame = frame_num - frame_offset;
                        let prev_bg = if scene_idx > 0 {
                            let prev = &view.scenes[scene_idx - 1];
                            Some((&prev.resolved_background, prev.duration))
                        } else {
                            None
                        };

                        let rgba = engine::render::render_scene_frame_scaled_with_prev_bg(
                            config, scene, local_frame, scene_frames, 1.0, prev_bg,
                        )?;

                        let img = image::RgbaImage::from_raw(config.width, config.height, rgba)
                            .ok_or(RustmotionError::PixelImage)?;
                        img.save(output)?;
                        return Ok(());
                    }
                    frame_offset += scene_frames;
                }
            }
        }
    }

    Err(RustmotionError::FrameOutOfRange { frame: frame_num, total: frame_offset }.into())
}
