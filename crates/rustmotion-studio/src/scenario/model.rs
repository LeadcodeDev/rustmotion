use std::sync::{Arc, Mutex};

use rustmotion::encode::video::FrameTask;
use rustmotion::schema::ResolvedScenario;

/// Live studio state shared between the file watcher and the UI/asset handler.
/// Wrapped in `Arc<Mutex<_>>` (`Shared`) so the watcher thread can swap in a
/// reloaded scenario while the webview reads frames.
///
/// `scenario`/`tasks` are `Arc` snapshots: consumers that render clone the
/// Arcs under a brief lock and work WITHOUT the model lock (frame handler,
/// hit-map, prefetcher). Watcher reloads build fresh Arcs; in-flight renders
/// keep the old snapshot alive until they finish.
pub struct StudioModel {
    pub scenario: Arc<ResolvedScenario>,
    pub tasks: Arc<Vec<FrameTask>>,
    pub total_frames: u32,
    /// Load/parse error surfaced as a full-screen banner in the editor.
    pub error: Option<String>,
    /// Disk-write error surfaced as a topbar indicator. Cleared on the next
    /// successful write or on model reload.
    pub write_error: Option<String>,
    /// Audio tracks the analyser could not read, surfaced as a topbar
    /// indicator. Without it an undecodable track just makes `waveform` and
    /// `audio_spectrum` draw their flat fallback, with nothing to explain it.
    pub audio_error: Option<String>,
    /// Bumped on every hot-reload so the UI can detect a change.
    pub generation: u64,
    /// Path to the scenario file (for inspector write-back).
    pub path: Option<std::path::PathBuf>,
    /// Raw parsed scenario JSON (for reading/editing element props by pointer).
    pub raw: serde_json::Value,
    /// In-memory HTML source for `.html` scenarios (kept in sync by the
    /// optimistic edit path; `None` for JSON sources).
    pub html_source: Option<String>,
}

pub type Shared = Arc<Mutex<StudioModel>>;

impl StudioModel {
    pub fn new(
        scenario: ResolvedScenario,
        error: Option<String>,
        path: Option<std::path::PathBuf>,
    ) -> Self {
        // For HTML sources the raw JSON is the transpiled scenario, plus the
        // annotations sidecar merged in so `list_annotations` and the comments
        // panel work unchanged; the inspector reads element props by pointer.
        let mut html_source = None;
        let raw = path
            .as_ref()
            .and_then(|p| {
                let s = std::fs::read_to_string(p).ok()?;
                let raw = if rustmotion::loader::is_html_path(p) {
                    let raw = rustmotion::loader::html_to_scenario_json(&s).ok()?;
                    // A corrupt sidecar already failed the scenario load in the
                    // loader (error banner); a read failure here can only be a
                    // race, so fall back to the bare transpile.
                    let annotations = super::sidecar::read_sidecar(p).unwrap_or_default();
                    html_source = Some(s.clone());
                    super::sidecar::merge_annotations(raw, annotations)
                } else {
                    serde_json::from_str(&s).ok()?
                };
                // First-open snapshot for the diff/review mode. Insert-if-absent,
                // so watcher reloads of an edited file keep the original baseline.
                super::baseline::ensure_baseline(&super::baseline::baseline_slot(), p, &s, &raw);
                Some(raw)
            })
            .unwrap_or(serde_json::Value::Null);
        let tasks = rustmotion::encode::build_frame_tasks(&scenario);
        let total_frames = tasks.len() as u32;
        // Analyse here rather than once at launch: every reload path — the
        // watcher, opening a file from the library, undo — funnels through this
        // constructor, and a scenario that gained a track or had its path fixed
        // must not keep the previous scenario's (or no) analysis.
        let failures = rustmotion::encode::audio_analysis::analyze_scenario_audio(&scenario);
        let audio_error = (!failures.is_empty()).then(|| {
            failures
                .iter()
                .map(|f| f.to_string())
                .collect::<Vec<_>>()
                .join(" · ")
        });
        Self {
            scenario: Arc::new(scenario),
            tasks: Arc::new(tasks),
            total_frames,
            error,
            write_error: None,
            audio_error,
            generation: 0,
            path,
            raw,
            html_source,
        }
    }
}

/// A minimal placeholder scenario (no scenes). Used as the initial model when
/// the studio launches into the library with no file open, and for the
/// error-display model. The editor isn't shown for it; thumbnail/frame
/// rendering guards against the empty task list.
pub fn empty_scenario() -> ResolvedScenario {
    use rustmotion::schema::{EasingType, ResolvedView, VideoConfig, ViewType};
    ResolvedScenario {
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
            camera_easing: EasingType::Linear,
            camera_pan_duration: 0.0,
        }],
        included_paths: vec![],
    }
}
