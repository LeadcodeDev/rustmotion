mod app_ui;
mod frames;
mod model;

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustmotion::engine;
use rustmotion::error::Result;
use rustmotion::schema::ResolvedScenario;

use app_ui::StudioApp;
use model::{Shared, StudioModel};

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
    use rustmotion::schema::{ResolvedView, VideoConfig, ViewType};
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
    // Prefetch assets (unchanged from the winit version).
    for view in &scenario.views {
        engine::prefetch_icons(&view.scenes);
        engine::preextract_video_frames(&view.scenes, scenario.video.fps);
    }
    if !scenario.fonts.is_empty() {
        engine::renderer::load_custom_fonts(&scenario.fonts);
    }

    let shared: Shared = Arc::new(Mutex::new(StudioModel::new(scenario, initial_error)));

    // Optional file watcher: on change, reload and swap into the shared model.
    if watch {
        if let Some(path) = input_path.clone() {
            let shared_w = shared.clone();
            std::thread::spawn(move || watch_loop(path, shared_w));
        }
    }

    // Launch the Dioxus desktop app, providing the shared model as root context.
    dioxus::LaunchBuilder::desktop()
        .with_context(shared)
        .launch(StudioApp);

    Ok(())
}

fn watch_loop(path: PathBuf, shared: Shared) {
    use notify::{RecursiveMode, Watcher};
    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = match notify::recommended_watcher(
        move |res: std::result::Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                if event.kind.is_modify() || event.kind.is_create() {
                    let _ = tx.send(());
                }
            }
        },
    ) {
        Ok(w) => w,
        Err(_) => return,
    };
    if watcher.watch(path.as_ref(), RecursiveMode::NonRecursive).is_err() {
        return;
    }
    while rx.recv().is_ok() {
        if let Ok(scenario) = rustmotion::loader::load_scenario(&path) {
            if let Ok(mut m) = shared.lock() {
                let generation = m.generation.wrapping_add(1);
                *m = StudioModel::new(scenario, None);
                m.generation = generation;
            }
        }
        // Drain rapid duplicate events.
        std::thread::sleep(Duration::from_millis(50));
        while rx.try_recv().is_ok() {}
    }
}
