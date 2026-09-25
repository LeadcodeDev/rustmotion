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
use crate::scenario::{Shared, StudioModel, View};

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
    let library: SharedLibrary = Arc::new(Mutex::new(LibraryState::new(workspace)));

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

fn handle_watched_path_changed(shared: &Shared, p: &PathBuf) {
    if let Ok(content) = std::fs::read_to_string(p) {
        if crate::scenario::is_self_write(&crate::scenario::self_write_slot(), p, &content) {
            return;
        }
    }
    crate::scenario::clear_self_write(&crate::scenario::self_write_slot(), p);
    let (scenario, error) = match rustmotion::loader::load_input(p) {
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

fn event_touches_target(event_paths: &[PathBuf], target: &std::path::Path) -> bool {
    let target_name = target.file_name();
    target_name.is_some() && event_paths.iter().any(|p| p.file_name() == target_name)
}

fn watch_dir_for(path: &std::path::Path) -> Option<PathBuf> {
    let dir = path.parent()?;
    if dir.as_os_str().is_empty() {
        Some(PathBuf::from("."))
    } else {
        Some(dir.to_path_buf())
    }
}

fn spawn_watcher(shared: Shared) -> Sender<WatchMsg> {
    use notify::{RecursiveMode, Watcher};
    let (tx, rx) = std::sync::mpsc::channel::<WatchMsg>();
    let notify_tx = tx.clone();
    let target: Arc<Mutex<Option<PathBuf>>> = Arc::new(Mutex::new(None));
    let watcher_target = target.clone();
    std::thread::spawn(move || {
        let mut watcher = match notify::recommended_watcher(
            move |res: std::result::Result<notify::Event, notify::Error>| {
                let Ok(event) = res else { return };
                if !(event.kind.is_modify() || event.kind.is_create() || event.kind.is_remove()) {
                    return;
                }
                let t = watcher_target.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(target_path) = t.as_deref() {
                    if event_touches_target(&event.paths, target_path) {
                        let _ = notify_tx.send(WatchMsg::Changed);
                    }
                }
            },
        ) {
            Ok(w) => w,
            Err(_) => return,
        };
        let mut current_dir: Option<PathBuf> = None;
        while let Ok(msg) = rx.recv() {
            match msg {
                WatchMsg::Retarget(p) => {
                    let new_dir = watch_dir_for(&p);
                    if new_dir != current_dir {
                        if let Some(old_dir) = current_dir.take() {
                            let _ = watcher.unwatch(old_dir.as_ref());
                        }
                        if let Some(dir) = &new_dir {
                            let _ = watcher.watch(dir.as_ref(), RecursiveMode::NonRecursive);
                        }
                        current_dir = new_dir;
                    }
                    *target.lock().unwrap_or_else(|e| e.into_inner()) = Some(p);
                }
                WatchMsg::Changed => {
                    let p = target.lock().unwrap_or_else(|e| e.into_inner()).clone();
                    if let Some(p) = p {
                        handle_watched_path_changed(&shared, &p);
                    }
                }
            }
        }
    });
    tx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_touches_target_matches_by_filename_regardless_of_directory() {
        let target = std::path::Path::new("/a/b/scene.json");
        assert!(event_touches_target(
            &[PathBuf::from("/a/b/scene.json")],
            target
        ));
        assert!(!event_touches_target(
            &[PathBuf::from("/a/b/scene.json.tmp")],
            target
        ));
        assert!(!event_touches_target(&[], target));
    }

    #[test]
    fn watch_dir_for_returns_the_parent_directory() {
        assert_eq!(
            watch_dir_for(std::path::Path::new("/a/b/scene.json")),
            Some(PathBuf::from("/a/b"))
        );
    }

    #[test]
    fn watch_dir_for_a_bare_filename_watches_the_current_directory() {
        assert_eq!(
            watch_dir_for(std::path::Path::new("scene.json")),
            Some(PathBuf::from("."))
        );
    }

    #[test]
    fn watcher_survives_an_atomic_save_that_replaces_the_inode() {
        use std::time::{Duration, Instant};

        let dir = std::env::temp_dir().join(format!("rm_app_watch_atomic_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("scene.json");
        std::fs::write(&path, scenario_json(111)).unwrap();
        let shared = shared_for(&path, 111);

        let tx = spawn_watcher(shared.clone());
        tx.send(WatchMsg::Retarget(path.clone())).unwrap();
        std::thread::sleep(Duration::from_millis(300));

        let tmp = dir.join("scene.json.tmp");
        std::fs::write(&tmp, scenario_json(222)).unwrap();
        std::fs::rename(&tmp, &path).unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if shared.lock().unwrap().scenario.video.width == 222 {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "watcher did not pick up an atomic-save that replaced the inode within 5s"
            );
            std::thread::sleep(Duration::from_millis(50));
        }

        std::fs::remove_dir_all(&dir).ok();
    }

    fn scenario_json(width: u32) -> String {
        format!(
            r##"{{"video":{{"width":{width},"height":10,"fps":10}},"scenes":[{{"duration":1.0}}]}}"##
        )
    }

    fn temp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("rm_app_watch_{tag}_{}.json", std::process::id()))
    }

    fn shared_for(path: &std::path::Path, width: u32) -> Shared {
        let scenario =
            rustmotion::loader::load_scenario_from_source(None, Some(&scenario_json(width)))
                .unwrap();
        Arc::new(Mutex::new(StudioModel::new(
            scenario,
            None,
            Some(path.to_path_buf()),
        )))
    }

    #[test]
    fn a_genuine_external_change_reaches_the_model_even_after_it_reverts_to_a_content_the_studio_once_wrote_itself(
    ) {
        let path = temp_path("revert_to_stale_self_write");
        let content_a = scenario_json(111);
        let content_b = scenario_json(222);

        std::fs::write(&path, &content_a).unwrap();
        crate::scenario::note_self_write(&crate::scenario::self_write_slot(), &path, &content_a);
        let shared = shared_for(&path, 111);

        handle_watched_path_changed(&shared, &path);
        assert_eq!(
            shared.lock().unwrap().scenario.video.width,
            111,
            "the studio's own echo of what it just wrote must not trigger a reload"
        );

        std::fs::write(&path, &content_b).unwrap();
        handle_watched_path_changed(&shared, &path);
        assert_eq!(
            shared.lock().unwrap().scenario.video.width,
            222,
            "a genuine external edit must reach the model"
        );

        std::fs::write(&path, &content_a).unwrap();
        handle_watched_path_changed(&shared, &path);
        assert_eq!(
            shared.lock().unwrap().scenario.video.width,
            111,
            "an external revert back to a content the studio once wrote itself must still \
             reach the model — the stale self-write record from the very first write must not \
             keep matching forever"
        );

        let _ = std::fs::remove_file(&path);
    }
}
