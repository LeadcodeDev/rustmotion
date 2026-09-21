//! Regression tests for the studio's file-write pipeline and a few other
//! previously-untestable decision points: a debounced write that must rebase
//! onto the current disk instead of replaying a stale in-memory snapshot,
//! coalesced edits inside one debounce window that must all survive (not
//! just the last), undo/redo cancelling a still-pending write before it can
//! clobber the just-restored state, a render-thread panic that must surface
//! as an error instead of a cached empty JPEG, and the preview audio mixer
//! only re-running when the audio itself changed.
//!
//! `rustmotion-studio` has no `[dev-dependencies]` and cannot gain one in
//! this change, so every test below drives the crate's existing public
//! surface (`scenario::*`, `editor::audio`, `editor::frames`) against real
//! temp files with plain synchronous `#[test]`s — no Dioxus runtime, no
//! async executor. That public surface is itself the fix for the crate
//! having no integration tests: the defects lived in a debounce timer and
//! Dioxus event handlers that cannot be driven from a test, so each one was
//! reduced to a pure decision over plain data (`resolve_flush`, the
//! pending-write queue, `audio_fingerprint`) and the handler calls that
//! instead of deciding inline.

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rustmotion_studio::editor::audio::audio_fingerprint;
use rustmotion_studio::editor::frames::render_frame_deep;
use rustmotion_studio::scenario::{
    apply_optimistic, empty_scenario, pending_write_slot, queue_mutation, record_edit,
    resolve_flush, take_pending, undo, Mutation, Shared, SharedHistory, StudioModel,
};

fn temp_path(tag: &str, ext: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "rustmotion_studio_test_{tag}_{}_{nanos}.{ext}",
        std::process::id()
    ))
}

fn write(path: &std::path::Path, content: &str) {
    fs::write(path, content).unwrap();
}

fn read(path: &std::path::Path) -> String {
    fs::read_to_string(path).unwrap()
}

// ── A debounced flush rebases onto the current disk, never a stale snapshot ──

#[test]
fn debounced_flush_rebases_onto_disk_and_survives_a_concurrent_external_edit() {
    let path = temp_path("stale_snapshot", "json");
    let base =
        r#"{"scenes":[{"duration":1.0,"children":[{"type":"text","content":"Hi","style":{}}]}]}"#;
    write(&path, base);

    let slot = pending_write_slot();
    queue_mutation(
        &slot,
        &path,
        Mutation::Style {
            pointer: "/scenes/0/children/0".into(),
            prop: "color".into(),
            value: serde_json::json!("#ff0000"),
        },
    );

    // An agent (or another editor) writes the file while the debounce window
    // is still open.
    let external_edit = r#"{"scenes":[{"duration":1.0,"children":[{"type":"text","content":"AGENT EDIT","style":{}}]}]}"#;
    write(&path, external_edit);

    let mutations = take_pending(&slot, &path);
    let disk = read(&path);
    let flushed = resolve_flush(&disk, false, &mutations)
        .expect("disk parses as JSON")
        .expect("the queued mutation is not a no-op");
    write(&path, &flushed);

    let on_disk: serde_json::Value = serde_json::from_str(&read(&path)).unwrap();
    assert_eq!(
        on_disk["scenes"][0]["children"][0]["content"],
        serde_json::json!("AGENT EDIT"),
        "the external edit must survive the flush"
    );
    assert_eq!(
        on_disk["scenes"][0]["children"][0]["style"]["color"],
        serde_json::json!("#ff0000"),
        "the queued edit must still land"
    );
    let _ = fs::remove_file(&path);
}

// ── Every coalesced HTML mutation in a burst survives, not just the last ────

#[test]
fn coalesced_html_edits_in_one_debounce_window_all_survive_the_flush() {
    let path = temp_path("coalesced_html", "html");
    let html_v0 = r##"<rustmotion width="640" height="360"><scene duration="1"><rm-text>Hi</rm-text></scene></rustmotion>"##;
    write(&path, html_v0);

    let slot = pending_write_slot();
    queue_mutation(
        &slot,
        &path,
        Mutation::Style {
            pointer: "/scenes/0/children/0".into(),
            prop: "color".into(),
            value: serde_json::json!("#ff0000"),
        },
    );
    queue_mutation(
        &slot,
        &path,
        Mutation::Style {
            pointer: "/scenes/0/children/0".into(),
            prop: "font-size".into(),
            value: serde_json::json!("48px"),
        },
    );

    let mutations = take_pending(&slot, &path);
    assert_eq!(mutations.len(), 2, "both coalesced edits are queued");
    let disk = read(&path);
    let flushed = resolve_flush(&disk, true, &mutations)
        .expect("html rebase never fails to parse")
        .expect("two real mutations are not a no-op");
    write(&path, &flushed);

    let on_disk = read(&path);
    assert!(on_disk.contains("color"), "first coalesced edit survives");
    assert!(
        on_disk.contains("font-size"),
        "second coalesced edit survives"
    );
    let _ = fs::remove_file(&path);
}

// ── Undo/redo cancel the pending write queue before touching the file ──────

#[test]
fn undo_cancels_the_pending_write_queue_before_touching_the_file() {
    let path = temp_path("undo_cancels_pending", "json");
    let state_a = r#"{"scenes":[{"duration":1.0,"children":[{"type":"text","content":"Hi","style":{"font-size":48}}]}]}"#;
    let state_b = state_a.replace("48", "72");
    write(&path, &state_b);

    let hist: SharedHistory = Arc::new(Mutex::new(Default::default()));
    record_edit(&hist, &path, state_a.to_string());

    // An edit is still waiting out its debounce window when undo fires.
    let slot = pending_write_slot();
    queue_mutation(
        &slot,
        &path,
        Mutation::Style {
            pointer: "/scenes/0/children/0".into(),
            prop: "font-size".into(),
            value: serde_json::json!(72),
        },
    );

    let shared: Shared = Arc::new(Mutex::new(StudioModel::new(
        empty_scenario(),
        None,
        Some(path.clone()),
    )));
    undo(&shared, &hist);

    assert_eq!(read(&path), state_a, "undo applied");
    assert!(
        hist.lock().unwrap().history.can_redo(),
        "redo entry available right after undo"
    );
    assert!(
        take_pending(&slot, &path).is_empty(),
        "undo must drop the pending edit so an orphaned flush later has nothing to replay"
    );

    let _ = fs::remove_file(&path);
}

// ── The write pipeline is reachable end to end without a Dioxus runtime ────

#[test]
fn the_write_pipeline_round_trips_an_edit_through_a_real_file() {
    let path = temp_path("full_pipeline", "json");
    let base = r#"{"video":{"width":64,"height":64},"scenes":[{"duration":1.0,"children":[{"type":"text","content":"Hi","style":{}}]}]}"#;
    write(&path, base);

    let shared: Shared = Arc::new(Mutex::new(StudioModel::new(
        empty_scenario(),
        None,
        Some(path.clone()),
    )));
    let mutation = Mutation::Style {
        pointer: "/scenes/0/children/0".into(),
        prop: "color".into(),
        value: serde_json::json!("#00ff00"),
    };
    apply_optimistic(&shared, &mutation).expect("optimistic apply succeeds");
    assert_eq!(
        shared.lock().unwrap().raw["scenes"][0]["children"][0]["style"]["color"],
        serde_json::json!("#00ff00"),
        "the canvas sees the edit immediately, before any disk write"
    );

    let slot = pending_write_slot();
    queue_mutation(&slot, &path, mutation);
    let mutations = take_pending(&slot, &path);
    let disk = read(&path);
    let flushed = resolve_flush(&disk, false, &mutations)
        .expect("disk parses")
        .expect("not a no-op");
    write(&path, &flushed);

    let on_disk: serde_json::Value = serde_json::from_str(&read(&path)).unwrap();
    assert_eq!(
        on_disk["scenes"][0]["children"][0]["style"]["color"],
        serde_json::json!("#00ff00"),
        "disk ends up with the same edit the in-memory model already has"
    );
    let _ = fs::remove_file(&path);
}

// ── A render-thread panic surfaces as an error, never a cached empty JPEG ──

#[test]
fn a_render_thread_panic_is_reported_as_an_error_not_an_empty_jpeg() {
    let big = rustmotion::loader::load_scenario_from_source(
        None,
        Some(r##"{ "video": { "width": 64, "height": 64 }, "scenes": [ { "duration": 0.1 }, { "duration": 0.1 } ] }"##),
    )
    .unwrap();
    let tasks = rustmotion::encode::build_frame_tasks(&big);
    assert!(
        tasks.len() >= 2,
        "need frames spanning both scenes to reach scene_idx 1"
    );

    // Deliberately mismatched: `tasks` reference a second scene this smaller
    // scenario does not have, which panics inside the render thread.
    let small = rustmotion::loader::load_scenario_from_source(
        None,
        Some(r##"{ "video": { "width": 64, "height": 64 }, "scenes": [ { "duration": 0.1 } ] }"##),
    )
    .unwrap();

    let last_frame = (tasks.len() - 1) as u32;
    let result = render_frame_deep(&small, &tasks, last_frame, 1.0);
    assert!(
        result.is_err(),
        "a scenario/task mismatch panics inside the render thread and must surface as Err"
    );
}

#[test]
fn a_normal_render_returns_nonempty_jpeg_bytes() {
    let scenario = rustmotion::loader::load_scenario_from_source(
        None,
        Some(r##"{ "video": { "width": 64, "height": 64 }, "scenes": [ { "duration": 0.1 } ] }"##),
    )
    .unwrap();
    let tasks = rustmotion::encode::build_frame_tasks(&scenario);
    let jpeg = render_frame_deep(&scenario, &tasks, 0, 1.0).expect("a normal render succeeds");
    assert!(!jpeg.is_empty());
}

// ── The preview audio mixer only re-runs when the audio actually changed ───

#[test]
fn audio_fingerprint_ignores_unrelated_edits_and_reacts_to_real_audio_changes() {
    let scenario_a = rustmotion::loader::load_scenario_from_source(
        None,
        Some(r##"{ "video": { "width": 64, "height": 64 }, "audio": [ { "src": "a.mp3" } ], "scenes": [ { "duration": 1.0 } ] }"##),
    )
    .unwrap();
    let scenario_b_same_audio = rustmotion::loader::load_scenario_from_source(
        None,
        Some(r##"{ "video": { "width": 64, "height": 64 }, "audio": [ { "src": "a.mp3" } ], "scenes": [ { "duration": 1.0, "children": [ { "type": "text", "content": "Hi" } ] } ] }"##),
    )
    .unwrap();
    let scenario_c_different_audio = rustmotion::loader::load_scenario_from_source(
        None,
        Some(r##"{ "video": { "width": 64, "height": 64 }, "audio": [ { "src": "a.mp3", "volume": 0.5 } ], "scenes": [ { "duration": 1.0 } ] }"##),
    )
    .unwrap();

    let fp_a = audio_fingerprint(&scenario_a.audio, 1.0);
    let fp_b = audio_fingerprint(&scenario_b_same_audio.audio, 1.0);
    let fp_c = audio_fingerprint(&scenario_c_different_audio.audio, 1.0);

    assert_eq!(
        fp_a, fp_b,
        "an edit unrelated to audio (layout, text) must not change the fingerprint"
    );
    assert_ne!(
        fp_a, fp_c,
        "a real audio change (volume) must change the fingerprint"
    );
    assert_ne!(
        audio_fingerprint(&scenario_a.audio, 1.0),
        audio_fingerprint(&scenario_a.audio, 2.0),
        "a total-duration change (silence padding) must also change the fingerprint"
    );
}
