# Studio Dioxus Rework Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the existing winit/softbuffer/Skia-drawn preview window **in place** in the `rustmotion-studio` crate with a Dioxus desktop application: Dioxus owns the window and UI, the existing Skia engine renders frames in the background, and frames stream into the webview via `use_asset_handler`. No new crate.

**Architecture:** Keep `rustmotion-studio`'s public surface (`run`, `command`, `run_preview`, `run_preview_with_error`) so the CLI (`crates/rustmotion-cli/src/lib.rs:224`) keeps working unchanged. `run_preview_inner` keeps the reusable parts (asset prefetch, `build_frame_tasks`, the `notify` file watcher) and swaps the winit event loop + `PreviewApp` for `dioxus::LaunchBuilder::desktop()`. A shared `Arc<Mutex<StudioModel>>` (scenario + frame tasks) is provided to the Dioxus root via context; a custom asset handler renders the requested frame to PNG on demand and the UI shows it in an `<img>`. The hand-drawn UI (`app.rs`, `ui.rs`) and the winit dependency are removed.

**Tech Stack:** Rust, Dioxus 0.7 (`desktop` feature, wry/webview), the `rustmotion` engine crate, the `image` crate (PNG encode), `notify` (file watch).

**This rework crosses a fast-moving 0.x API.** Two consequences:
1. The exact `use_asset_handler` signature, the `LaunchBuilder`/context API, and `use_signal` forms are **pinned empirically in Task 1** against the installed Dioxus 0.7 crate (recorded in `crates/rustmotion-studio/DIOXUS_NOTES.md`). Later tasks state the *expected* shape with verify-and-adjust steps — do not invent; pin against the crate.
2. **Performance and visual verification must be done by the human on macOS** — a headless agent cannot open a webview with a display. Steps needing a display are labeled **HUMAN STEP**.

**Context for the engineer:**
- We are on branch `feat/rework-studio`. Workspace crates live under `crates/`.
- **Preserve these public functions unchanged** (the CLI calls the last two): `rustmotion_studio::run() -> Result<()>`, `rustmotion_studio::command() -> clap::Command`, `run_preview(scenario: ResolvedScenario, input_path: Option<PathBuf>, watch: bool) -> Result<()>`, `run_preview_with_error(initial_error: String, input_path: Option<PathBuf>, watch: bool) -> Result<()>`.
- Canonical frame render (study it, then reproduce): `crates/rustmotion-studio/src/preview/render_worker.rs::render_one` calls `rustmotion::encode::render_frame_task_scaled(&sc.video, sc, task, sf) -> Result<Vec<u8> /*rgba*/>` after `rustmotion::encode::build_frame_tasks(&scenario) -> Vec<FrameTask>`. `FrameTask` is `rustmotion::encode::video::FrameTask`.
- The current `run_preview_inner` (`crates/rustmotion-studio/src/preview/mod.rs:61`) shows the prefetch + watcher code to keep.
- Run a single crate's tests: `cargo test -p rustmotion-studio`.
- Commit style: `<type>: <verb> <message>`. NEVER add `Co-Authored-By` or any Claude/Anthropic mention.

---

### Task 1: Replace the winit shell with a Dioxus window (deps + entry), preserving the public API

Swap dependencies, delete the hand-drawn UI, and rewrite `run_preview_inner` to launch a Dioxus desktop window that displays scenario metadata from a shared model. Frame display arrives in Task 2.

**Files:**
- Modify: `crates/rustmotion-studio/Cargo.toml`
- Modify: `crates/rustmotion-studio/src/preview/mod.rs` (rewrite `run_preview_inner`; keep `run_preview`/`run_preview_with_error` signatures)
- Delete: `crates/rustmotion-studio/src/preview/app.rs`, `crates/rustmotion-studio/src/preview/ui.rs`, `crates/rustmotion-studio/src/preview/render_worker.rs`
- Create: `crates/rustmotion-studio/src/preview/model.rs` (the shared `StudioModel`)
- Create: `crates/rustmotion-studio/src/preview/app_ui.rs` (the Dioxus root component)
- Create: `crates/rustmotion-studio/DIOXUS_NOTES.md` (pinned API record)

- [ ] **Step 1: Swap dependencies.** Replace the `[dependencies]` block in `crates/rustmotion-studio/Cargo.toml` with:

```toml
[dependencies]
rustmotion = { path = "../rustmotion" }
dioxus = { version = "0.7", features = ["desktop"] }
clap = { version = "4", features = ["derive"] }
notify = "7"
image = { version = "0.25", default-features = false, features = ["png", "jpeg", "webp"] }
serde_json = "1"
```
(Removed: `skia-safe`, `winit`, `softbuffer`, `rfd`, `notify-rust` — they were used only by the deleted winit UI. Export-from-studio and desktop notifications are dropped for now; rendering/export remains available via the `rustmotion` CLI.)

- [ ] **Step 2: Delete the winit UI files.**
```bash
git rm crates/rustmotion-studio/src/preview/app.rs crates/rustmotion-studio/src/preview/ui.rs crates/rustmotion-studio/src/preview/render_worker.rs
```

- [ ] **Step 3: Create the shared model.** Write `crates/rustmotion-studio/src/preview/model.rs`:

```rust
use std::sync::{Arc, Mutex};

use rustmotion::encode::video::FrameTask;
use rustmotion::schema::ResolvedScenario;

/// The live studio state shared between the file watcher and the UI/asset
/// handler. Wrapped in `Arc<Mutex<_>>` (`Shared`) so the watcher thread can
/// swap in a reloaded scenario.
pub struct StudioModel {
    pub scenario: ResolvedScenario,
    pub tasks: Vec<FrameTask>,
    pub total_frames: u32,
    pub error: Option<String>,
}

pub type Shared = Arc<Mutex<StudioModel>>;

impl StudioModel {
    pub fn new(scenario: ResolvedScenario, error: Option<String>) -> Self {
        let tasks = rustmotion::encode::build_frame_tasks(&scenario);
        let total_frames = tasks.len() as u32;
        Self { scenario, tasks, total_frames, error }
    }
}
```

- [ ] **Step 4: Create the Dioxus root.** Write `crates/rustmotion-studio/src/preview/app_ui.rs` (placeholder UI that reads the shared model from context — proves Dioxus + context wiring):

```rust
use dioxus::prelude::*;

use super::model::Shared;

#[component]
pub fn StudioApp() -> Element {
    let shared = use_context::<Shared>();
    let (w, h, frames, err) = {
        let m = shared.lock().unwrap();
        (m.scenario.video.width, m.scenario.video.height, m.total_frames, m.error.clone())
    };

    rsx! {
        div { style: "font: 16px sans-serif; padding: 24px; color:#ddd; background:#15171c; min-height:100vh;",
            h1 { "Rustmotion Studio" }
            if let Some(e) = err {
                div { style: "color:#ff6b6b;", "Error: {e}" }
            }
            div { "Video: {w}×{h}, {frames} frames" }
        }
    }
}
```

- [ ] **Step 5: Rewrite `run_preview_inner` and the module wiring.** In `crates/rustmotion-studio/src/preview/mod.rs`, replace the whole file with the version below. It keeps `run_preview`/`run_preview_with_error` signatures and the prefetch + watcher logic, builds the shared model, and launches Dioxus instead of winit:

```rust
mod app_ui;
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
            std::thread::spawn(move || {
                use notify::{RecursiveMode, Watcher};
                let (tx, rx) = std::sync::mpsc::channel();
                let mut watcher = notify::recommended_watcher(
                    move |res: std::result::Result<notify::Event, notify::Error>| {
                        if let Ok(event) = res {
                            if event.kind.is_modify() || event.kind.is_create() {
                                let _ = tx.send(());
                            }
                        }
                    },
                )
                .expect("create file watcher");
                watcher
                    .watch(path.as_ref(), RecursiveMode::NonRecursive)
                    .expect("watch file");
                while rx.recv().is_ok() {
                    if let Ok(scenario) = rustmotion::loader::load_scenario(&path) {
                        if let Ok(mut m) = shared_w.lock() {
                            *m = StudioModel::new(scenario, None);
                        }
                    }
                    // Drain rapid duplicate events.
                    std::thread::sleep(Duration::from_millis(50));
                    while rx.try_recv().is_ok() {}
                }
            });
        }
    }

    // Launch the Dioxus desktop app, providing the shared model as root context.
    // VERIFY the context API against DIOXUS_NOTES.md (Task 1, Step 7).
    dioxus::LaunchBuilder::desktop()
        .with_context(shared)
        .launch(StudioApp);

    Ok(())
}
```

- [ ] **Step 6: Build.** Run: `cargo build -p rustmotion-studio`.
Expected: compiles. If `LaunchBuilder::desktop()`, `.with_context(...)`, or `use_context` differ in 0.7, the compiler will say so — find the correct 0.7 forms in the `dioxus` prelude / `dioxus-desktop` source and adjust. Common alternatives: `dioxus::launch(StudioApp)` (no context — then use a different state-passing mechanism), or `consume_context::<Shared>()` instead of `use_context`.

- [ ] **Step 7: Pin the Dioxus API.** Locate and read the installed crate; record verbatim in `crates/rustmotion-studio/DIOXUS_NOTES.md`:
```bash
find ~/.cargo/registry/src -maxdepth 1 -type d -name 'dioxus-desktop-*'
grep -rn "pub fn use_asset_handler" ~/.cargo/registry/src/*/dioxus-desktop-*/src/
```
Record: the `use_asset_handler` full signature; its request type and how to read the URL path; its responder type and the method to send bytes; the import path for the HTTP `Response` type (likely `dioxus::desktop::wry::http::Response`); how the handler name maps to the served URL prefix; the confirmed launch/context API; and the `use_signal` form. These feed Tasks 2–5.

- [ ] **Step 8: Run the window (HUMAN STEP).** Hand off: human runs `cargo run -p rustmotion-cli -- studio -f <scenario.json>` (or `cargo run -p rustmotion-studio -- -f <scenario.json>`) on macOS and confirms a window opens showing the scenario's dimensions and frame count.

- [ ] **Step 9: Commit.**
```bash
git add crates/rustmotion-studio/Cargo.toml crates/rustmotion-studio/src/preview/mod.rs crates/rustmotion-studio/src/preview/model.rs crates/rustmotion-studio/src/preview/app_ui.rs crates/rustmotion-studio/DIOXUS_NOTES.md
git commit -m "feat: replace winit studio shell with dioxus window"
```

---

### Task 2: Render frames and stream them into the webview

Add a frame producer (RGBA→PNG, reusing the engine) and an asset handler that serves `/frame/{idx}`; display the current frame in an `<img>`.

**Files:**
- Create: `crates/rustmotion-studio/src/preview/frames.rs`
- Modify: `crates/rustmotion-studio/src/preview/mod.rs` (add `mod frames;`)
- Modify: `crates/rustmotion-studio/src/preview/app_ui.rs` (register handler + `<img>`)

- [ ] **Step 1: Write the failing test.** Create `crates/rustmotion-studio/src/preview/frames.rs`:

```rust
use std::time::{Duration, Instant};

use rustmotion::schema::ResolvedScenario;

/// Render one frame to PNG bytes. Returns (png, width, height, render_time, encode_time).
pub fn render_png(
    scenario: &ResolvedScenario,
    tasks: &[rustmotion::encode::video::FrameTask],
    frame: u32,
    scale: f32,
) -> (Vec<u8>, u32, u32, Duration, Duration) {
    let task = &tasks[(frame as usize).min(tasks.len().saturating_sub(1))];

    let t0 = Instant::now();
    let rgba = rustmotion::encode::render_frame_task_scaled(&scenario.video, scenario, task, scale)
        .expect("render frame");
    let render_time = t0.elapsed();

    let w = (scenario.video.width as f32 * scale) as u32;
    let h = (scenario.video.height as f32 * scale) as u32;

    let t1 = Instant::now();
    let img = image::RgbaImage::from_raw(w, h, rgba).expect("rgba matches dimensions");
    let mut png = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .expect("encode png");
    let encode_time = t1.elapsed();

    (png, w, h, render_time, encode_time)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCENARIO: &str =
        r#"{ "video": { "width": 1280, "height": 720, "background": "#101418" }, "scenes": [ { "duration": 1.0 } ] }"#;

    #[test]
    fn renders_frame_to_nonempty_png() {
        let scenario = rustmotion::loader::load_scenario_from_source(None, Some(SCENARIO)).unwrap();
        let tasks = rustmotion::encode::build_frame_tasks(&scenario);
        assert!(!tasks.is_empty());
        let (png, w, h, _r, _e) = render_png(&scenario, &tasks, 0, 1.0);
        assert_eq!((w, h), (1280, 720));
        assert!(png.len() > 8);
        assert_eq!(&png[0..4], &[0x89, b'P', b'N', b'G'], "must be a PNG");
    }
}
```

- [ ] **Step 2: Wire the module + run the failing test.** Add `mod frames;` to `mod.rs`. Run: `cargo test -p rustmotion-studio renders_frame_to_nonempty_png`. It should PASS immediately (the impl is complete) — this is a regression guard, not red→green. If it fails to compile, fix the signature against the engine API.

- [ ] **Step 3: Add the asset handler + `<img>`.** In `app_ui.rs`, register a `frame` handler that serves PNG bytes for the requested index from the shared model, and show the current frame. Use the API pinned in `DIOXUS_NOTES.md`; the expected shape (VERIFY and adjust):

```rust
use dioxus::prelude::*;
use dioxus::desktop::{use_asset_handler, wry::http::Response};

use super::frames::render_png;
use super::model::Shared;

#[component]
pub fn StudioApp() -> Element {
    let shared = use_context::<Shared>();

    let handler_shared = shared.clone();
    use_asset_handler("frame", move |request, responder| {
        let uri = request.uri().to_string();
        let idx: u32 = uri.rsplit('/').next().and_then(|s| s.split('?').next())
            .and_then(|s| s.parse().ok()).unwrap_or(0);
        let png = {
            let m = handler_shared.lock().unwrap();
            render_png(&m.scenario, &m.tasks, idx, 1.0).0
        };
        responder.respond(
            Response::builder().header("Content-Type", "image/png").body(png).unwrap(),
        );
    });

    let frames = { shared.lock().unwrap().total_frames };

    rsx! {
        div { style: "margin:0; background:#0c0d10; min-height:100vh;",
            img { src: "/frame/0", style: "display:block; max-width:100%; height:auto; margin:0 auto;" }
            div { style: "color:#888; font:12px sans-serif; padding:8px;", "{frames} frames" }
        }
    }
}
```

- [ ] **Step 4: Build.** Run: `cargo build -p rustmotion-studio`. Expected: compiles.

- [ ] **Step 5: Run + verify (HUMAN STEP).** Hand off: human runs the studio against a real scenario and confirms frame 0 displays correctly.

- [ ] **Step 6: Commit.**
```bash
git add crates/rustmotion-studio/src/preview/frames.rs crates/rustmotion-studio/src/preview/mod.rs crates/rustmotion-studio/src/preview/app_ui.rs
git commit -m "feat: stream engine-rendered frames into the studio webview"
```

---

### Task 3: Perf bench + measurement

Add a `--bench` path to the studio binary that renders+encodes a batch and prints median latencies (no window), so we can judge feasibility against the spec thresholds.

**Files:**
- Create: `crates/rustmotion-studio/src/preview/perf.rs`
- Modify: `crates/rustmotion-studio/src/lib.rs` (recognize `--bench`)
- Modify: `crates/rustmotion-studio/src/preview/mod.rs` (`mod perf;` + a `pub fn run_bench`)

- [ ] **Step 1: Write the perf helper with tests.** Create `crates/rustmotion-studio/src/preview/perf.rs`:

```rust
use std::time::Duration;

/// Median of a set of durations (zero for an empty slice).
pub fn median(mut samples: Vec<Duration>) -> Duration {
    if samples.is_empty() {
        return Duration::ZERO;
    }
    samples.sort_unstable();
    samples[samples.len() / 2]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn median_of_odd_set() {
        let s = vec![Duration::from_millis(10), Duration::from_millis(30), Duration::from_millis(20)];
        assert_eq!(median(s), Duration::from_millis(20));
    }
    #[test]
    fn median_of_empty_is_zero() {
        assert_eq!(median(vec![]), Duration::ZERO);
    }
}
```

- [ ] **Step 2: Run the perf tests.** Add `mod perf;` to `mod.rs`. Run: `cargo test -p rustmotion-studio perf`. Expected: PASS (2 tests).

- [ ] **Step 3: Add `run_bench` to `mod.rs`:**

```rust
pub fn run_bench(scenario: ResolvedScenario) {
    use std::time::Duration;
    let tasks = rustmotion::encode::build_frame_tasks(&scenario);
    let n = tasks.len().min(120) as u32;
    println!("scenario: {}x{}, {} frames (measuring {})",
        scenario.video.width, scenario.video.height, tasks.len(), n);
    let mut render = Vec::new();
    let mut encode = Vec::new();
    let mut bytes = 0usize;
    for f in 0..n {
        let (png, _w, _h, r, e) = frames::render_png(&scenario, &tasks, f, 1.0);
        render.push(r);
        encode.push(e);
        bytes += png.len();
    }
    let mr = perf::median(render);
    let me = perf::median(encode);
    println!("median render: {:?}", mr);
    println!("median encode: {:?}", me);
    println!("median total:  {:?}", mr + me);
    println!("avg png size:  {} KB", if n > 0 { bytes / n as usize / 1024 } else { 0 });
    println!("=> max fps (render+encode only): {:.1}",
        if (mr + me).as_secs_f64() > 0.0 { 1.0 / (mr + me).as_secs_f64() } else { 0.0 });
    let _ = Duration::ZERO;
}
```
Make `run_bench` reachable: add `pub use preview::run_bench;` next to the other re-exports in `lib.rs`.

- [ ] **Step 4: Recognize `--bench` in `run()`.** In `crates/rustmotion-studio/src/lib.rs`, update `run()` so that when the args contain `--bench`, it loads the scenario and calls `run_bench` instead of `run_preview`:

```rust
pub fn run() -> Result<()> {
    let bench = std::env::args().any(|a| a == "--bench");
    let cli = Cli::parse();
    match load_scenario(&cli.file) {
        Ok(scenario) => {
            if bench {
                preview::run_bench(scenario);
                Ok(())
            } else {
                run_preview(scenario, Some(cli.file), true)
            }
        }
        Err(e) => run_preview_with_error(format!("{}", e), Some(cli.file), true),
    }
}
```
(Note: `--bench` must not be rejected by clap. If clap errors on the unknown flag, add `#[arg(long)] bench: bool` to `Cli` and branch on `cli.bench` instead.)

- [ ] **Step 5: Build + self-check.** Run: `cargo build -p rustmotion-studio` then `cargo test -p rustmotion-studio`. Expected: compiles, all tests pass.

- [ ] **Step 6: MEASURE (HUMAN STEP).** Hand off: human runs `cargo run -p rustmotion-studio -- -f <real-scenario.json> --bench` at 1920×1080 and 1180×2256, and records median render/encode/total, implied fps, and avg PNG size in `DIOXUS_NOTES.md` under "Measurements".

- [ ] **Step 7: Commit.**
```bash
git add crates/rustmotion-studio/src/preview/perf.rs crates/rustmotion-studio/src/preview/mod.rs crates/rustmotion-studio/src/lib.rs
git commit -m "feat: add studio frame perf bench"
```

---

### Task 4: Playback, scrubbing, and hot-reload in Dioxus

Add the current-frame state, a play/pause + scrub timeline, and watcher→UI refresh. (Detailed forms confirmed by `DIOXUS_NOTES.md` from Task 1.)

**Files:**
- Modify: `crates/rustmotion-studio/src/preview/app_ui.rs`
- Modify: `crates/rustmotion-studio/src/preview/model.rs` (add a reload generation counter)

- [ ] **Step 1: Add a reload generation to the model.** In `model.rs`, add `pub gen: u64` to `StudioModel` (default 0), and have the watcher in `mod.rs` increment it on each reload (`m.gen = m.gen.wrapping_add(1);` after `*m = StudioModel::new(...)` — restructure so `gen` is preserved/incremented rather than reset). This lets the UI detect reloads.

- [ ] **Step 2: Add frame + rev signals and a timeline.** In `app_ui.rs`, add:

```rust
    let mut current = use_signal(|| 0u32);
    let mut rev = use_signal(|| 0u64); // bump to force <img> refetch

    // Poll the shared model's reload generation ~4x/sec; bump rev on change.
    let poll_shared = shared.clone();
    use_future(move || {
        let poll_shared = poll_shared.clone();
        async move {
            let mut last = 0u64;
            loop {
                let g = poll_shared.lock().unwrap().gen;
                if g != last { last = g; rev += 1; }
                gloo_timers_sleep(250).await; // see note
            }
        }
    });
```
Note: Dioxus's async sleep helper differs by version — use the form recorded in `DIOXUS_NOTES.md` (e.g. `dioxus::desktop::tokio::time::sleep` if the tokio runtime is available, or a `dioxus`-provided timer). If no async timer is readily available, fall back to a thread that bumps a channel and a `use_coroutine` receiver. Pin this in Task 1's notes before implementing.

- [ ] **Step 3: Render the frame with a cache-busting query + a range scrubber.** Update the `rsx!` to use `src: "/frame/{current}?v={rev}"`, and add an `<input type="range">` bound to `current` (min 0, max `frames-1`) plus prev/next buttons. Play/pause uses a `use_future` loop that increments `current` at the scenario fps when playing.

- [ ] **Step 4: Build + (HUMAN STEP) verify** scrubbing changes the frame, play advances frames, and editing the scenario file reloads the preview.

- [ ] **Step 5: Commit.**
```bash
git add crates/rustmotion-studio/src/preview/app_ui.rs crates/rustmotion-studio/src/preview/model.rs
git commit -m "feat: add studio playback scrubbing and hot reload"
```

---

### Task 5: Clickable hotspot overlay (percentage coordinates)

Prove the overlay model the inspector/annotation features will use: an absolutely-positioned layer over the `<img>` with hotspot `<div>`s in `%`. Start with a hardcoded hotspot (real hit-map wiring — feeding rects from `paint_tree_with_hits` through the frame service — is a later plan).

**Files:**
- Modify: `crates/rustmotion-studio/src/preview/app_ui.rs`

- [ ] **Step 1: Add an overlay with a hardcoded `%` hotspot and selection toggle** over the `<img>` (wrap both in a `position:relative` container; overlay is `position:absolute; inset:0`; hotspot uses `left/top/width/height` in `%`; `onclick` toggles a `use_signal(|| false)` that switches the border to a solid selection box). Use the `use_signal` form from `DIOXUS_NOTES.md`.

- [ ] **Step 2: Build + (HUMAN STEP) verify** the hotspot stays aligned to the frame on window resize and clicking toggles the selection box.

- [ ] **Step 3: Commit.**
```bash
git add crates/rustmotion-studio/src/preview/app_ui.rs
git commit -m "feat: add percentage hotspot overlay to studio"
```

---

## Decision Gate (after Task 3 measurement)

Compare measured median render+encode against the spec thresholds
(`docs/superpowers/specs/2026-06-09-studio-v2-dioxus-design.md`): edit refresh < ~50 ms, scrub < ~100 ms, playback ≥ 24–30 fps.
- **Under budget at 1180×2256** → continue (inspector + real hit-map wiring next).
- **Encode dominates / PNG too big** → switch to JPEG/WebP, add a frame cache (the old `render_worker` background pre-render pattern), and/or downscale the preview (full-res only at export); re-measure.
- **Webview transport can't keep up even with mitigations** → evaluate the Blitz/wgpu-texture path.
Record the dated decision at the bottom of `DIOXUS_NOTES.md`.

## Self-Review

**Spec coverage** (against `docs/superpowers/specs/2026-06-09-studio-v2-dioxus-design.md`):
- "Dioxus est l'app du studio … remplace le studio winit/softbuffer" → Task 1. ✓
- "rustmotion-core reste Dioxus-free; Dioxus isolé dans rustmotion-studio" → only `rustmotion-studio/Cargo.toml` gains Dioxus; the engine crates are untouched. ✓
- "Transport des frames via use_asset_handler; `<img src=/frame/...>`" → Task 2. ✓
- "Spike de perf … 1180×2256 et 1920×1080 … mitigations" → Task 3 + Decision Gate. ✓
- "hotspots en % des dimensions vidéo" → Task 5. ✓
- Watch→reload → Task 4. ✓
- Public CLI entry preserved (`run_preview`/`run_preview_with_error`) → Task 1 keeps signatures; `crates/rustmotion-cli/src/lib.rs:224` unchanged. ✓

**Intentional deferral:** real hit-map wiring (feeding `paint_tree_with_hits` rects, NodeId→pointer/kind resolution, rect→%) is NOT in this plan — Task 5 uses a hardcoded hotspot to validate the overlay mechanism. The engine hit-map already exists (Plan 1); wiring it through a frame-service is the next plan, after the perf Decision Gate confirms the webview path.

**Placeholder scan:** The only unpinned items are Dioxus-0.x API shapes (launch/context, `use_asset_handler`, async timer, `use_signal`), front-loaded into Task 1's `DIOXUS_NOTES.md` pinning with verify-and-adjust steps and named fallbacks at each use site. All engine-facing Rust (frame producer, perf median, model) is complete and unit-tested.

**Type consistency:** `StudioModel { scenario, tasks, total_frames, error, gen }`, `Shared = Arc<Mutex<StudioModel>>`, `frames::render_png(scenario, tasks, frame, scale) -> (Vec<u8>, u32, u32, Duration, Duration)`, `perf::median(Vec<Duration>) -> Duration`, `run_bench(ResolvedScenario)` — used identically across tasks. Public fns `run`, `command`, `run_preview`, `run_preview_with_error` keep their existing signatures.

**Human-in-the-loop:** every display-dependent step (window boot, frame display, perf measurement, scrubbing, overlay alignment, click) is labeled HUMAN STEP — a headless agent cannot open a webview.

## Hand-off note

After Task 1's build is green and the API is pinned, Tasks 2–3 give a measurable foundation. The Decision Gate (perf) determines whether we proceed on webview to the inspector + real hit-map wiring (next plan) or reconsider Blitz.
