use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use rustmotion::encode::video::FrameTask;
use rustmotion::schema::ResolvedScenario;

/// Stack for any thread that renders a frame.
///
/// The depth cost is **deserialisation**, not painting: `prepare_scene` calls
/// `deserialize_children` on every frame, and `ChildComponent` → `Component` →
/// `Container`/`Card` recurses once per level of nesting. Because those enums
/// are untagged, serde buffers each level through `ContentDeserializer`, so a
/// level is expensive in stack as well as deep — and a debug build's frames are
/// several times fatter than a release build's.
///
/// Measured: a scenario nested ~28 levels — which a UI composed from small
/// helper functions reaches without trying — needs between 2 and 4 MiB in debug.
/// Rust's default for a spawned thread is 2 MiB, so the prefetch workers
/// overflowed and aborted the process; the same file rendered fine from the CLI,
/// which happens to run on the 8 MiB main thread. The two webview asset handlers
/// get whatever stack the platform hands their callback, which is no better.
///
/// Address space is cheap and committed lazily; take a wide margin.
pub const RENDER_STACK: usize = 32 * 1024 * 1024;

/// [`render_frame`] on a thread with [`RENDER_STACK`], for callers that are not
/// on the main thread. Scoped, so the scenario and tasks are borrowed rather
/// than cloned.
pub fn render_frame_deep(
    scenario: &ResolvedScenario,
    tasks: &[rustmotion::encode::video::FrameTask],
    frame: u32,
    scale: f32,
) -> Vec<u8> {
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(RENDER_STACK)
            .spawn_scoped(scope, || render_frame(scenario, tasks, frame, scale))
            .expect("spawn render thread")
            .join()
            .unwrap_or_default()
    })
}

/// Render one frame and encode it to JPEG bytes (preview-only; the final video
/// render path does not use this). JPEG keeps the encode cost low enough for
/// the webview transport to keep up with playback.
pub fn render_frame(
    scenario: &ResolvedScenario,
    tasks: &[rustmotion::encode::video::FrameTask],
    frame: u32,
    scale: f32,
) -> Vec<u8> {
    if tasks.is_empty() {
        return Vec::new();
    }
    let idx = (frame as usize).min(tasks.len() - 1);
    let task = &tasks[idx];
    let rgba = rustmotion::encode::render_frame_task_scaled(&scenario.video, scenario, task, scale)
        .expect("render frame");
    let w = (scenario.video.width as f32 * scale) as u32;
    let h = (scenario.video.height as f32 * scale) as u32;
    let rgba_img = image::RgbaImage::from_raw(w, h, rgba).expect("rgba matches dimensions");
    // JPEG has no alpha; drop it (preview frames are opaque).
    let rgb = image::DynamicImage::ImageRgba8(rgba_img).to_rgb8();
    let mut jpeg = Vec::new();
    image::DynamicImage::ImageRgb8(rgb)
        .write_to(
            &mut std::io::Cursor::new(&mut jpeg),
            image::ImageFormat::Jpeg,
        )
        .expect("encode jpeg");
    jpeg
}

/// Cached baseline scenario for the diff mode's A-side render, keyed by
/// (path, source hash) so it is rebuilt only when the baseline itself changes
/// (Set baseline) — never per frame. Arc snapshots so callers render OUTSIDE
/// this cache's lock.
struct BaselineCache {
    path: PathBuf,
    source_hash: u64,
    scenario: Arc<ResolvedScenario>,
    tasks: Arc<Vec<FrameTask>>,
}

fn baseline_cache() -> &'static Mutex<Option<BaselineCache>> {
    static CACHE: OnceLock<Mutex<Option<BaselineCache>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// Stable hash of a baseline source string (also used as the side-A cache
/// generation in the prefetch frame cache).
pub(crate) fn source_hash(source: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    source.hash(&mut h);
    h.finish()
}

/// Arc snapshot of the BASELINE scenario built from its source string: JSON is
/// loaded directly, HTML is re-transpiled — both entirely from the string,
/// never from the (already edited) file on disk. Cached by (path, source
/// hash); returns `(source_hash, scenario, tasks)` so the caller renders
/// without holding any lock.
pub fn baseline_arcs(
    path: &Path,
    source: &str,
) -> Result<(u64, Arc<ResolvedScenario>, Arc<Vec<FrameTask>>), String> {
    let hash = source_hash(source);
    let mut guard = baseline_cache().lock().unwrap_or_else(|e| e.into_inner());

    let stale = !matches!(&*guard, Some(c) if c.path == path && c.source_hash == hash);
    if stale {
        let scenario = if rustmotion::loader::is_html_path(path) {
            let value = rustmotion::loader::html_to_scenario_json(source)
                .map_err(|e| format!("baseline transpile: {e}"))?;
            let json = serde_json::to_string(&value).map_err(|e| format!("baseline json: {e}"))?;
            rustmotion::loader::load_scenario_from_source(None, Some(&json))
                .map_err(|e| format!("baseline load: {e}"))?
        } else {
            rustmotion::loader::load_scenario_from_source(None, Some(source))
                .map_err(|e| format!("baseline load: {e}"))?
        };
        let tasks = rustmotion::encode::build_frame_tasks(&scenario);
        *guard = Some(BaselineCache {
            path: path.to_path_buf(),
            source_hash: hash,
            scenario: Arc::new(scenario),
            tasks: Arc::new(tasks),
        });
    }

    let cache = guard.as_ref().expect("baseline cache just filled");
    Ok((hash, cache.scenario.clone(), cache.tasks.clone()))
}

/// A clickable element box in percentage-of-frame coords, with its kind.
#[derive(Debug, Clone, PartialEq)]
pub struct HitPct {
    pub node_id: u32,
    pub kind: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// Full JSON Pointer into the source scenario, e.g. "/scenes/0/children/2".
    pub pointer: Option<String>,
}

/// Compute the current frame's clickable element boxes in percentage coords
/// (render only — no JPEG encode, so this is cheap per frame). `scene_prefix`
/// is the JSON-Pointer prefix of the current scene (see [`scene_prefix`]).
pub fn frame_hits(
    scenario: &ResolvedScenario,
    tasks: &[rustmotion::encode::video::FrameTask],
    frame: u32,
    scene_prefix: &str,
) -> Vec<HitPct> {
    if tasks.is_empty() {
        return Vec::new();
    }
    let idx = (frame as usize).min(tasks.len() - 1);
    let task = &tasks[idx];
    let vw = scenario.video.width as f32;
    let vh = scenario.video.height as f32;
    rustmotion::encode::render_frame_task_hits(scenario, task)
        .into_iter()
        .map(|h| HitPct {
            node_id: h.node_id,
            kind: h.kind,
            x: (h.rect.x / vw) * 100.0,
            y: (h.rect.y / vh) * 100.0,
            w: (h.rect.w / vw) * 100.0,
            h: (h.rect.h / vh) * 100.0,
            pointer: h.pointer.map(|rel| format!("{scene_prefix}{rel}")),
        })
        .collect()
}

/// JSON-Pointer prefix to the scene of the given frame, derived from the raw
/// scenario JSON (handles both top-level `scenes` and `composition`).
pub fn scene_prefix(
    raw: &serde_json::Value,
    tasks: &[rustmotion::encode::video::FrameTask],
    frame: u32,
) -> String {
    use rustmotion::encode::video::FrameTask;
    if tasks.is_empty() {
        return String::new();
    }
    let idx = (frame as usize).min(tasks.len() - 1);
    if let FrameTask::Normal {
        view_idx,
        scene_idx,
        ..
    } = &tasks[idx]
    {
        if raw.get("composition").is_some() {
            format!("/composition/{view_idx}/scenes/{scene_idx}")
        } else {
            format!("/scenes/{scene_idx}")
        }
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCENARIO: &str = r##"{ "video": { "width": 1280, "height": 720, "background": "#101418" }, "scenes": [ { "duration": 1.0 } ] }"##;

    #[test]
    fn renders_frame_to_nonempty_jpeg() {
        let scenario = rustmotion::loader::load_scenario_from_source(None, Some(SCENARIO)).unwrap();
        let tasks = rustmotion::encode::build_frame_tasks(&scenario);
        assert!(!tasks.is_empty());
        let jpeg = render_frame(&scenario, &tasks, 0, 1.0);
        assert!(jpeg.len() > 2);
        // JPEG SOI marker.
        assert_eq!(&jpeg[0..2], &[0xFF, 0xD8], "must be a JPEG");
    }

    /// TEMP diagnostic (not part of CI): replicate the prefetch worker pool on
    /// the dynamic-glass example and print RSS growth. Run with
    /// `RM_STRESS_SCALE=0.5 cargo test -p rustmotion-studio --release stress_rss -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn stress_rss_worker_pool() {
        use std::sync::Arc;
        let Ok(src) = std::fs::read_to_string("../../examples/dynamic-glass.json") else {
            eprintln!("skipped: examples/dynamic-glass.json not present");
            return;
        };
        let scenario =
            Arc::new(rustmotion::loader::load_scenario_from_source(None, Some(&src)).unwrap());
        let tasks = Arc::new(rustmotion::encode::build_frame_tasks(&scenario));
        let scale: f32 = std::env::var("RM_STRESS_SCALE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.5);
        let rss_kb = || -> u64 {
            let out = std::process::Command::new("ps")
                .args(["-o", "rss=", "-p", &std::process::id().to_string()])
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout).trim().parse().unwrap()
        };
        println!("scale={scale} start rss={} MB", rss_kb() / 1024);
        let workers: Vec<_> = (0..6u32)
            .map(|w| {
                let s = scenario.clone();
                let t = tasks.clone();
                std::thread::spawn(move || {
                    for _pass in 0..6 {
                        for f in (130..190).filter(|f| f % 6 == w) {
                            let _ = render_frame(&s, &t, f, scale);
                        }
                    }
                })
            })
            .collect();
        while workers.iter().any(|w| !w.is_finished()) {
            std::thread::sleep(std::time::Duration::from_secs(2));
            println!("rss={} MB", rss_kb() / 1024);
        }
        for w in workers {
            w.join().unwrap();
        }
        println!("end rss={} MB", rss_kb() / 1024);
    }

    #[test]
    fn renders_frame_at_reduced_scale() {
        let scenario = rustmotion::loader::load_scenario_from_source(None, Some(SCENARIO)).unwrap();
        let tasks = rustmotion::encode::build_frame_tasks(&scenario);
        let jpeg = render_frame(&scenario, &tasks, 0, 0.5);
        let img = image::load_from_memory(&jpeg).expect("decodable JPEG");
        assert_eq!((img.width(), img.height()), (640, 360), "half of 1280x720");
    }

    #[test]
    fn frame_hits_are_in_percent_and_have_kind() {
        let json = r##"{ "video": { "width": 800, "height": 600, "background": "#101418" }, "scenes": [ { "duration": 1.0, "children": [ { "type": "text", "content": "Hi", "style": { "font-size": 40 } } ] } ] }"##;
        let scenario = rustmotion::loader::load_scenario_from_source(None, Some(json)).unwrap();
        let tasks = rustmotion::encode::build_frame_tasks(&scenario);
        let hits = frame_hits(&scenario, &tasks, 0, "/scenes/0");
        let text = hits.iter().find(|h| h.kind == "text").expect("text hit");
        assert!(hits
            .iter()
            .all(|h| h.x >= 0.0 && h.x <= 100.0 && h.w <= 100.0));
        assert!(
            text.pointer
                .as_deref()
                .unwrap()
                .starts_with("/scenes/0/children/"),
            "pointer = {:?}",
            text.pointer
        );
    }

    #[test]
    fn scene_prefix_handles_top_level_scenes() {
        let json =
            r##"{ "video": { "width": 1, "height": 1 }, "scenes": [ { "duration": 1.0 } ] }"##;
        let scenario = rustmotion::loader::load_scenario_from_source(None, Some(json)).unwrap();
        let raw: serde_json::Value = serde_json::from_str(json).unwrap();
        let tasks = rustmotion::encode::build_frame_tasks(&scenario);
        assert_eq!(scene_prefix(&raw, &tasks, 0), "/scenes/0");
    }
}
