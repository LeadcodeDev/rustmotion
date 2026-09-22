use std::sync::{Arc, Mutex};

use rustmotion::encode::video::FrameTask;
use rustmotion::schema::ResolvedScenario;

pub struct StudioModel {
    pub scenario: Arc<ResolvedScenario>,
    pub tasks: Arc<Vec<FrameTask>>,
    pub total_frames: u32,
    pub error: Option<String>,
    pub write_error: Option<String>,
    pub audio_error: Option<String>,
    pub generation: u64,
    pub path: Option<std::path::PathBuf>,
    pub raw: serde_json::Value,
    pub html_source: Option<String>,
}

pub type Shared = Arc<Mutex<StudioModel>>;

impl StudioModel {
    pub fn new(
        scenario: ResolvedScenario,
        error: Option<String>,
        path: Option<std::path::PathBuf>,
    ) -> Self {
        let mut html_source = None;
        let raw = path
            .as_ref()
            .and_then(|p| {
                let s = std::fs::read_to_string(p).ok()?;
                let raw = if rustmotion::loader::is_html_path(p) {
                    let raw = rustmotion::loader::html_to_scenario_json(&s).ok()?;
                    let annotations = super::sidecar::read_sidecar(p).unwrap_or_default();
                    html_source = Some(s.clone());
                    super::sidecar::merge_annotations(raw, annotations)
                } else {
                    serde_json::from_str(&s).ok()?
                };
                super::baseline::ensure_baseline(&super::baseline::baseline_slot(), p, &s, &raw);
                Some(raw)
            })
            .unwrap_or(serde_json::Value::Null);
        let tasks = rustmotion::encode::build_frame_tasks(&scenario);
        let total_frames = tasks.len() as u32;
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
