mod overlays;
mod root;
pub mod state;
mod window;

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use rustmotion::engine;
use rustmotion::error::Result;
use rustmotion::schema::ResolvedScenario;

use crate::library::{LibraryState, SharedLibrary, WatchMsg};
use crate::scenario::{empty_scenario, Shared, StudioModel, View};

fn default_workspace() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

pub fn run_preview(
    scenario: ResolvedScenario,
    input_path: Option<PathBuf>,
    watch: bool,
) -> Result<()> {
    let workspace = workspace_for(&input_path);
    run_preview_root(scenario, None, input_path, workspace, true, watch)
}

pub fn run_preview_with_error(
    initial_error: String,
    input_path: Option<PathBuf>,
    watch: bool,
) -> Result<()> {
    let workspace = workspace_for(&input_path);
    run_preview_root(
        empty_scenario(),
        Some(initial_error),
        input_path,
        workspace,
        true,
        watch,
    )
}

fn workspace_for(input_path: &Option<PathBuf>) -> PathBuf {
    input_path
        .as_ref()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or_else(default_workspace)
}

pub fn run_preview_root(
    scenario: ResolvedScenario,
    initial_error: Option<String>,
    input_path: Option<PathBuf>,
    workspace: PathBuf,
    start_in_editor: bool,
    watch: bool,
) -> Result<()> {
    if !scenario.fonts.is_empty() {
        engine::renderer::load_custom_fonts(&scenario.fonts);
    }

    let shared: Shared = Arc::new(Mutex::new(StudioModel::new(
        scenario,
        initial_error,
        input_path.clone(),
    )));
    let library: SharedLibrary =
        Arc::new(Mutex::new(LibraryState::new(workspace, start_in_editor)));

    spawn_scenario_warmup(shared.clone());

    if watch {
        let tx = spawn_watcher(shared.clone());
        if let Some(p) = input_path.clone() {
            let _ = tx.send(WatchMsg::Retarget(p));
        }
        library.lock().unwrap_or_else(|e| e.into_inner()).watch_tx = Some(tx);
    }

    let view = if start_in_editor {
        View::Editor
    } else {
        View::Library
    };
    let theme_pref = crate::theme::persist::load_theme_pref();

    gpui_kit::application()
        .with_assets(gpui_kit::assets::AllAssets::new(""))
        .run(move |cx| {
            gpui_kit::init(cx);
            cx.spawn(async move |cx| {
                window::open(shared, library, view, theme_pref, cx);
            })
            .detach();
        });

    Ok(())
}

pub fn spawn_scenario_warmup(shared: Shared) {
    std::thread::spawn(move || {
        let (scenario, fps) = {
            let m = shared.lock().unwrap_or_else(|e| e.into_inner());
            (m.scenario.clone(), m.scenario.video.fps)
        };
        for view in &scenario.views {
            if let Err(unresolvable_icon) = engine::try_prefetch_icons(&view.scenes) {
                eprintln!("{unresolvable_icon}");
                return;
            }
            engine::preextract_video_frames(&view.scenes, fps);
        }
        let failures = rustmotion::encode::audio_analysis::analyze_scenario_audio(&scenario);
        let audio_error = (!failures.is_empty()).then(|| {
            failures
                .iter()
                .map(|f| f.to_string())
                .collect::<Vec<_>>()
                .join(" \u{b7} ")
        });

        let mut m = shared.lock().unwrap_or_else(|e| e.into_inner());
        if Arc::ptr_eq(&m.scenario, &scenario) {
            m.audio_error = audio_error;
            m.generation = m.generation.wrapping_add(1);
        }
    });
}

fn spawn_watcher(shared: Shared) -> Sender<WatchMsg> {
    use notify::{RecursiveMode, Watcher};
    let (tx, rx) = std::sync::mpsc::channel::<WatchMsg>();
    let notify_tx = tx.clone();
    std::thread::spawn(move || {
        let mut watcher = match notify::recommended_watcher(
            move |res: std::result::Result<notify::Event, notify::Error>| {
                if let Ok(event) = res {
                    if event.kind.is_modify() || event.kind.is_create() {
                        let _ = notify_tx.send(WatchMsg::Changed);
                    }
                }
            },
        ) {
            Ok(w) => w,
            Err(_) => return,
        };
        let mut current: Option<PathBuf> = None;
        while let Ok(msg) = rx.recv() {
            match msg {
                WatchMsg::Retarget(p) => {
                    if let Some(old) = current.take() {
                        let _ = watcher.unwatch(old.as_ref());
                    }
                    let _ = watcher.watch(p.as_ref(), RecursiveMode::NonRecursive);
                    current = Some(p);
                }
                WatchMsg::Changed => {
                    if let Some(p) = current.clone() {
                        if let Ok(content) = std::fs::read_to_string(&p) {
                            if crate::scenario::is_self_write(
                                &crate::scenario::self_write_slot(),
                                &p,
                                &content,
                            ) {
                                continue;
                            }
                        }
                        let (scenario, error) = match rustmotion::loader::load_input(&p) {
                            Ok(s) => (s, None),
                            Err(e) => (crate::scenario::empty_scenario(), Some(e.to_string())),
                        };
                        let mut m = shared.lock().unwrap_or_else(|e| e.into_inner());
                        let g = m.generation.wrapping_add(1);
                        *m = StudioModel::new(scenario, error, Some(p.clone()));
                        m.generation = g;
                        drop(m);
                        spawn_scenario_warmup(shared.clone());
                    }
                }
            }
        }
    });
    tx
}
