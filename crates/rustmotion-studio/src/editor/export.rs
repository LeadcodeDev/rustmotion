//! Background video export: encodes the current scenario to an MP4 next to the
//! source file, reusing the exact CLI encode pipeline (ffmpeg when available,
//! embedded openh264 otherwise). The encode runs on a plain `std::thread` with
//! progress reported through a shared status slot the UI polls.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dioxus::prelude::*;

use crate::scenario::Shared;

/// Lifecycle of one export, shared between the encode thread and the UI.
#[derive(Debug, Clone, PartialEq)]
pub enum ExportStatus {
    Idle,
    Running {
        phase: &'static str,
        done: u32,
        total: u32,
    },
    Done(PathBuf),
    Failed(String),
}

impl ExportStatus {
    pub fn is_running(&self) -> bool {
        matches!(self, ExportStatus::Running { .. })
    }
}

/// The cross-thread status slot. The encode thread writes; the UI polls.
pub type SharedExport = Arc<Mutex<ExportStatus>>;

/// The app-global status slot. Global (not per-component) so a running export
/// survives a switch to the library and back: the remounted topbar re-attaches
/// to the same slot, and `start_export`'s running-check keeps exports serial
/// app-wide.
pub fn export_slot() -> SharedExport {
    static SLOT: std::sync::OnceLock<SharedExport> = std::sync::OnceLock::new();
    SLOT.get_or_init(|| Arc::new(Mutex::new(ExportStatus::Idle)))
        .clone()
}

/// Poll the shared export status into a Dioxus signal (~150 ms), following the
/// same polling pattern as `use_hot_reload`. Signal writes only fire on change
/// so idle polling doesn't re-render the topbar.
pub fn use_export_poll(status: SharedExport, mut sig: Signal<ExportStatus>) {
    use_future(move || {
        let status = status.clone();
        async move {
            loop {
                tokio::time::sleep(Duration::from_millis(150)).await;
                let s = status.lock().unwrap_or_else(|e| e.into_inner()).clone();
                if s != sig() {
                    sig.set(s);
                }
            }
        }
    });
}

/// Kick off a background export of the scenario currently loaded in `shared`.
/// No-op if an export is already running. The scenario is re-loaded from its
/// source on the worker thread (`ResolvedScenario` isn't `Clone`, and the
/// watcher keeps the on-disk file authoritative), so the model lock is held
/// only long enough to snapshot the path and raw JSON.
pub fn start_export(shared: &Shared, status: &SharedExport) {
    {
        let mut st = status.lock().unwrap_or_else(|e| e.into_inner());
        if st.is_running() {
            return;
        }
        *st = ExportStatus::Running {
            phase: "Rendering",
            done: 0,
            total: 0,
        };
    }

    let (path, raw) = {
        let m = shared.lock().unwrap_or_else(|e| e.into_inner());
        (m.path.clone(), m.raw.clone())
    };

    let status = status.clone();
    std::thread::spawn(move || {
        // A Skia/encoder panic must not leave the status stuck on Running
        // (and a poisoned status Mutex would break every later export), so
        // the whole encode is fenced with catch_unwind — same rationale as
        // the frame asset handler.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_export(path, raw, &status)
        }));
        let outcome = match result {
            Ok(Ok(out)) => ExportStatus::Done(out),
            Ok(Err(e)) => ExportStatus::Failed(e),
            Err(_) => ExportStatus::Failed("renderer panicked".to_string()),
        };
        *status.lock().unwrap_or_else(|e| e.into_inner()) = outcome;
    });
}

/// Load, render and encode; returns the output path or a user-facing reason.
fn run_export(
    path: Option<PathBuf>,
    raw: serde_json::Value,
    status: &SharedExport,
) -> Result<PathBuf, String> {
    let scenario = match &path {
        Some(p) => rustmotion::loader::load_input(p).map_err(|e| e.to_string())?,
        None => {
            if raw.is_null() {
                return Err("no scenario to export".to_string());
            }
            let json = serde_json::to_string(&raw).map_err(|e| e.to_string())?;
            rustmotion::loader::load_scenario_from_source(None, Some(&json))
                .map_err(|e| e.to_string())?
        }
    };

    if !scenario.fonts.is_empty() {
        rustmotion::engine::renderer::load_custom_fonts(&scenario.fonts);
    }

    let output = output_path(&path);
    let output_str = output
        .to_str()
        .ok_or_else(|| "non-UTF-8 output path".to_string())?;

    let mut cb = |p: rustmotion::encode::EncodeProgress| {
        *status.lock().unwrap_or_else(|e| e.into_inner()) = status_for_progress(&p);
    };

    // Same pipeline selection as the CLI: ffmpeg when available (10-bit
    // H.264), embedded openh264 otherwise. Output is fixed to .mp4, so the
    // codec is h264; the scenario's optional `crf` is honored.
    if ffmpeg_available() {
        rustmotion::encode::encode_with_ffmpeg(
            &scenario,
            output_str,
            true,
            "h264",
            scenario.video.crf,
            false,
            Some(&mut cb),
        )
        .map_err(|e| e.to_string())?;
    } else {
        rustmotion::encode::encode_video(&scenario, output_str, true, Some(&mut cb))
            .map_err(|e| e.to_string())?;
    }
    Ok(output)
}

/// Map an encoder progress event to the UI status.
fn status_for_progress(p: &rustmotion::encode::EncodeProgress) -> ExportStatus {
    use rustmotion::encode::EncodeProgress;
    match p {
        EncodeProgress::Rendering(c, t) => ExportStatus::Running {
            phase: "Rendering",
            done: *c,
            total: *t,
        },
        EncodeProgress::Encoding(c, t) => ExportStatus::Running {
            phase: "Encoding",
            done: *c,
            total: *t,
        },
        EncodeProgress::Muxing => ExportStatus::Running {
            phase: "Muxing",
            done: 0,
            total: 0,
        },
    }
}

/// v1 output location: `<scenario dir>/<stem>.mp4`, or `./export.mp4` when the
/// scenario has no path.
fn output_path(source: &Option<PathBuf>) -> PathBuf {
    match source {
        Some(p) => p.with_extension("mp4"),
        None => std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("export.mp4"),
    }
}

/// Label for the export button, with a live percentage while running.
pub fn export_label(status: &ExportStatus) -> String {
    match status {
        ExportStatus::Running { phase, done, total } if *total > 0 => {
            let pct = (*done as f64 / *total as f64 * 100.0).round() as u32;
            format!("{phase}… {pct}%")
        }
        ExportStatus::Running { phase, .. } => format!("{phase}…"),
        _ => "Export".to_string(),
    }
}

/// Same detection as the CLI: probe `ffmpeg -version`.
fn ffmpeg_available() -> bool {
    std::process::Command::new("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmotion::encode::EncodeProgress;

    #[test]
    fn output_path_sits_next_to_the_source() {
        let src = Some(PathBuf::from("/work/demo/scenario.json"));
        assert_eq!(output_path(&src), PathBuf::from("/work/demo/scenario.mp4"));
        let html = Some(PathBuf::from("/work/demo/hello.html"));
        assert_eq!(output_path(&html), PathBuf::from("/work/demo/hello.mp4"));
    }

    #[test]
    fn output_path_defaults_to_export_mp4() {
        let out = output_path(&None);
        assert_eq!(out.file_name().and_then(|s| s.to_str()), Some("export.mp4"));
    }

    #[test]
    fn progress_maps_to_running_phases() {
        assert_eq!(
            status_for_progress(&EncodeProgress::Rendering(3, 10)),
            ExportStatus::Running {
                phase: "Rendering",
                done: 3,
                total: 10
            }
        );
        assert_eq!(
            status_for_progress(&EncodeProgress::Encoding(5, 10)),
            ExportStatus::Running {
                phase: "Encoding",
                done: 5,
                total: 10
            }
        );
        assert!(status_for_progress(&EncodeProgress::Muxing).is_running());
    }

    #[test]
    fn labels_show_percentage_when_total_known() {
        assert_eq!(
            export_label(&ExportStatus::Running {
                phase: "Rendering",
                done: 42,
                total: 100
            }),
            "Rendering… 42%"
        );
        assert_eq!(
            export_label(&ExportStatus::Running {
                phase: "Muxing",
                done: 0,
                total: 0
            }),
            "Muxing…"
        );
        assert_eq!(export_label(&ExportStatus::Idle), "Export");
        assert_eq!(
            export_label(&ExportStatus::Done(PathBuf::from("/x.mp4"))),
            "Export"
        );
    }
}
