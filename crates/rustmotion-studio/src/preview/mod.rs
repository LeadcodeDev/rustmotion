mod app;
mod render_worker;
mod ui;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use winit::event_loop::EventLoop;
use winit::keyboard::ModifiersState;

use rustmotion::encode;
use rustmotion::engine;
use rustmotion::error::Result;
use rustmotion::error::RustmotionError;
use rustmotion::schema::ResolvedScenario;

use app::PreviewApp;
use render_worker::{RenderRequest, render_thread};
use ui::ExportState;

pub fn run_preview(
    scenario: ResolvedScenario,
    input_path: Option<PathBuf>,
    watch: bool,
) -> Result<()> {
    run_preview_inner(scenario, input_path, watch, None)
}

pub fn run_preview_with_error(
    initial_error: String,
    input_path: Option<PathBuf>,
    watch: bool,
) -> Result<()> {
    use rustmotion::schema::{VideoConfig, ResolvedView, ViewType};
    let scenario = ResolvedScenario {
        video: VideoConfig {
            width: 1920,
            height: 1080,
            fps: 30,
            background: "#000000".to_string(),
            codec: None,
            crf: None,
        },
        audio: vec![],
        fonts: vec![],
        views: vec![ResolvedView {
            view_type: ViewType::Slide,
            scenes: vec![],
            transition: None,
            background: Default::default(),
            camera_easing: rustmotion::schema::EasingType::Linear,
            camera_pan_duration: 0.0,
        }],
        included_paths: vec![],
    };
    run_preview_inner(scenario, input_path, watch, Some(initial_error))
}

fn run_preview_inner(
    scenario: ResolvedScenario,
    input_path: Option<PathBuf>,
    watch: bool,
    initial_error: Option<String>,
) -> Result<()> {
    // Prefetch assets
    for view in &scenario.views {
        engine::prefetch_icons(&view.scenes);
        engine::preextract_video_frames(&view.scenes, scenario.video.fps);
    }
    if !scenario.fonts.is_empty() {
        engine::renderer::load_custom_fonts(&scenario.fonts);
    }

    let fps = scenario.video.fps;
    let video_width = scenario.video.width;
    let video_height = scenario.video.height;
    let total_frames = encode::build_frame_tasks(&scenario).len() as u32;

    // Channels
    let (req_tx, req_rx) = mpsc::channel::<RenderRequest>();
    let (resp_tx, resp_rx) = mpsc::channel();
    let (reload_resp_tx, reload_resp_rx) = mpsc::channel();

    // Spawn render thread
    std::thread::spawn(move || {
        render_thread(req_rx, resp_tx, reload_resp_tx, scenario);
    });

    // File watcher (optional)
    let file_change_rx = if watch {
        if let Some(ref path) = input_path {
            let (tx, rx) = mpsc::channel();
            let watch_path = path.clone();
            std::thread::spawn(move || {
                use notify::{RecursiveMode, Watcher};
                let mut watcher = notify::recommended_watcher(
                    move |res: std::result::Result<notify::Event, notify::Error>| {
                        if let Ok(event) = res {
                            if event.kind.is_modify() || event.kind.is_create() {
                                let _ = tx.send(());
                            }
                        }
                    },
                )
                .expect("Failed to create file watcher");
                watcher
                    .watch(watch_path.as_ref(), RecursiveMode::NonRecursive)
                    .expect("Failed to watch file");
                loop {
                    std::thread::sleep(Duration::from_secs(3600));
                }
            });
            Some(rx)
        } else {
            None
        }
    } else {
        None
    };

    let event_loop = EventLoop::new()
        .map_err(|e| RustmotionError::PreviewWindow {
            reason: e.to_string(),
        })?;

    let mut app = PreviewApp {
        total_frames,
        fps,
        video_width,
        video_height,
        input_path,
        current_frame: 0,
        playing: false,
        last_frame_time: Instant::now(),
        frame_duration: Duration::from_secs_f64(1.0 / fps.max(1) as f64),
        frame_cache: HashMap::new(),
        rendered_width: video_width,
        rendered_height: video_height,
        render_tx: req_tx,
        render_rx: resp_rx,
        reload_rx: reload_resp_rx,
        pending_frame: None,
        file_change_rx,
        window: None,
        surface: None,
        display_width: video_width,
        display_height: video_height,
        scale: 1.0,
        modifiers: ModifiersState::empty(),
        timeline_dragging: false,
        cursor_x: 0.0,
        cursor_y: 0.0,
        error_message: initial_error,
        export_state: ExportState::Idle,
        export_done_rx: None,
        export_progress: 0.0,
        export_progress_label: String::new(),
        export_progress_rx: None,
    };

    event_loop
        .run_app(&mut app)
        .map_err(|e| RustmotionError::PreviewWindow {
            reason: e.to_string(),
        })?;

    Ok(())
}
