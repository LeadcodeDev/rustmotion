//! The application shell: process entry points, the launch wiring, and the
//! re-targetable file watcher.

mod root;

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use rustmotion::engine;
use rustmotion::error::Result;
use rustmotion::schema::ResolvedScenario;

use crate::library::{LibraryState, SharedLibrary, WatchMsg};
use crate::scenario::{empty_scenario, Shared, StudioModel};
use root::StudioRoot;

fn default_workspace() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// CLI-facing entry: open a scenario directly into the editor. The library
/// home uses the file's parent directory as the workspace.
pub fn run_preview(
    scenario: ResolvedScenario,
    input_path: Option<PathBuf>,
    watch: bool,
) -> Result<()> {
    let workspace = workspace_for(&input_path);
    run_preview_root(scenario, None, input_path, workspace, true, watch)
}

/// CLI-facing entry for the error case (file failed to load).
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

/// Launch the studio root: library home + editor, sharing one model, a library
/// state, and a re-targetable file watcher.
pub fn run_preview_root(
    scenario: ResolvedScenario,
    initial_error: Option<String>,
    input_path: Option<PathBuf>,
    workspace: PathBuf,
    start_in_editor: bool,
    watch: bool,
) -> Result<()> {
    for view in &scenario.views {
        engine::prefetch_icons(&view.scenes);
        engine::preextract_video_frames(&view.scenes, scenario.video.fps);
    }
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

    if watch {
        let tx = spawn_watcher(shared.clone());
        if let Some(p) = input_path.clone() {
            let _ = tx.send(WatchMsg::Retarget(p));
        }
        library.lock().unwrap_or_else(|e| e.into_inner()).watch_tx = Some(tx);
    }

    dioxus::LaunchBuilder::desktop()
        .with_context(shared)
        .with_context(library)
        .launch(StudioRoot);

    Ok(())
}

/// One long-lived watcher thread owning a single `notify::Watcher`, driven by a
/// channel of [`WatchMsg`]. `Retarget` switches the watched file; `Changed`
/// (from the notify callback) reloads the current file into the shared model.
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
                        let (scenario, error) = match rustmotion::loader::load_input(&p) {
                            Ok(s) => (s, None),
                            Err(e) => (crate::scenario::empty_scenario(), Some(e.to_string())),
                        };
                        let mut m = shared.lock().unwrap_or_else(|e| e.into_inner());
                        let g = m.generation.wrapping_add(1);
                        *m = StudioModel::new(scenario, error, Some(p.clone()));
                        m.generation = g;
                    }
                }
            }
        }
    });
    tx
}
