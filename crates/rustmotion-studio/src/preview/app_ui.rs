use std::time::Duration;

use dioxus::desktop::{use_asset_handler, wry::http::Response, AssetRequest, RequestAsyncResponder};
use dioxus::prelude::*;

use super::frames::render_frame;
use super::model::Shared;

/// Root component. Reads the shared studio model from context, registers an
/// asset handler that renders frames to PNG on demand, and provides playback,
/// scrubbing, and hot-reload of the current frame.
#[component]
pub fn StudioApp() -> Element {
    let shared = use_context::<Shared>();

    let mut current = use_signal(|| 0u32);
    let mut playing = use_signal(|| false);
    let rev = use_signal(|| 0u64);
    // Selection stores (node_id, pointer, kind) so the inspector stays open via
    // the stored pointer even if the element collapses out of the hit-map.
    let mut selected = use_signal(|| None::<(u32, String, String)>);
    let mut show_annotations = use_signal(|| false);

    // Asset handler: GET /frame/{idx} -> PNG of that frame.
    let handler_shared = shared.clone();
    use_asset_handler(
        "frame",
        move |request: AssetRequest, responder: RequestAsyncResponder| {
            let uri = request.uri().to_string();
            let idx: u32 = uri
                .rsplit('/')
                .next()
                .and_then(|s| s.split('?').next())
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let jpeg = {
                let m = handler_shared.lock().unwrap();
                render_frame(&m.scenario, &m.tasks, idx, 1.0).0
            };
            responder.respond(
                Response::builder()
                    .header("Content-Type", "image/jpeg")
                    .header("Cache-Control", "no-cache, no-store, must-revalidate")
                    .header("Pragma", "no-cache")
                    .header("Expires", "0")
                    .body(jpeg)
                    .unwrap(),
            );
        },
    );

    // Playback clock: while playing, advance the frame at the scenario fps.
    let clock_shared = shared.clone();
    use_future(move || {
        let clock_shared = clock_shared.clone();
        let mut current = current;
        let playing = playing;
        async move {
            loop {
                let fps = clock_shared.lock().unwrap().scenario.video.fps.max(1);
                tokio::time::sleep(Duration::from_secs_f64(1.0 / fps as f64)).await;
                if playing() {
                    let total = clock_shared.lock().unwrap().total_frames.max(1);
                    let next = (current() + 1) % total;
                    current.set(next);
                }
            }
        }
    });

    // Hot-reload poller: when the watcher bumps the model generation, bump rev
    // so the <img> refetches the (now changed) current frame.
    let poll_shared = shared.clone();
    use_future(move || {
        let poll_shared = poll_shared.clone();
        let mut rev = rev;
        async move {
            let mut last_gen = 0u64;
            loop {
                tokio::time::sleep(Duration::from_millis(250)).await;
                let g = poll_shared.lock().unwrap().generation;
                if g != last_gen {
                    last_gen = g;
                    rev.set(rev() + 1);
                }
            }
        }
    });

    let (total, err) = {
        let m = shared.lock().unwrap();
        (m.total_frames, m.error.clone())
    };
    let max = total.saturating_sub(1);
    let cur = current().min(max);
    let r = rev();
    let is_playing = playing();

    // Real element hotspots for the current frame (render only, no encode).
    let hits = {
        let m = shared.lock().unwrap();
        let prefix = super::frames::scene_prefix(&m.raw, &m.tasks, cur);
        super::frames::frame_hits(&m.scenario, &m.tasks, cur, &prefix)
    };
    let selected_node = selected().map(|(id, _, _)| id);
    let selected_kind = selected().map(|(_, _, kind)| kind);

    // Inspector data for the selected element: its current style prop values.
    // Driven by the STORED pointer, so it stays open even if the element
    // collapses out of the current frame's hit-map (e.g. font-size 0).
    let inspector = selected().map(|(_, pointer, _)| {
        let m = shared.lock().unwrap();
        let color = super::edit::read_style(&m.raw, &pointer, "color").unwrap_or_default();
        let font_size = super::edit::read_style(&m.raw, &pointer, "font-size").unwrap_or_default();
        let background = super::edit::read_style(&m.raw, &pointer, "background").unwrap_or_default();
        (pointer, color, font_size, background)
    });

    let annotations = {
        let m = shared.lock().unwrap();
        super::edit::list_annotations(&m.raw)
    };

    if let Some(e) = err {
        return rsx! {
            div { style: "padding:24px; color:#ff6b6b; background:#0c0d10; min-height:100vh; font:13px sans-serif;",
                "Error: {e}"
            }
        };
    }

    rsx! {
        div { style: "margin:0; background:#0c0d10; min-height:100vh; color:#cfd3dc; font:13px -apple-system,sans-serif; display:flex; flex-direction:column;",
            div { style: "flex:1; display:flex; align-items:center; justify-content:center; padding:16px; overflow:hidden;",
                div { style: "position:relative; display:inline-block; box-shadow:0 8px 40px rgba(0,0,0,0.5); line-height:0;",
                    img {
                        src: "/frame/{cur}?v={r}",
                        style: "display:block; max-width:100%; max-height:78vh; height:auto;",
                    }
                    div { style: "position:absolute; inset:0;",
                        for hit in hits.iter() {
                            div {
                                key: "{hit.node_id}",
                                style: format!(
                                    "position:absolute; left:{}%; top:{}%; width:{}%; height:{}%; box-sizing:border-box; cursor:pointer; border:{};",
                                    hit.x, hit.y, hit.w, hit.h,
                                    if selected_node == Some(hit.node_id) { "2px solid #4c8dff; background:rgba(76,141,255,0.15)" }
                                    else { "1px dashed rgba(255,255,255,0.22)" }
                                ),
                                onclick: {
                                    let id = hit.node_id;
                                    let ptr = hit.pointer.clone();
                                    let kind = hit.kind.clone();
                                    move |_| {
                                        if let Some(ptr) = ptr.clone() {
                                            selected.set(Some((id, ptr, kind.clone())));
                                        }
                                    }
                                },
                            }
                        }
                    }
                }
            }
            div { style: "display:flex; align-items:center; gap:12px; padding:12px 20px; border-top:1px solid #1c1f27; background:#10131a;",
                button {
                    style: "min-width:64px; padding:6px 10px; cursor:pointer;",
                    onclick: move |_| playing.set(!playing()),
                    if is_playing { "Pause" } else { "Play" }
                }
                button {
                    style: "padding:6px 10px; cursor:pointer;",
                    onclick: move |_| current.set(cur.saturating_sub(1)),
                    "‹"
                }
                button {
                    style: "padding:6px 10px; cursor:pointer;",
                    onclick: move |_| current.set((cur + 1).min(max)),
                    "›"
                }
                input {
                    r#type: "range",
                    min: "0",
                    max: "{max}",
                    value: "{cur}",
                    style: "flex:1;",
                    oninput: move |e| {
                        if let Ok(v) = e.value().parse::<u32>() {
                            current.set(v);
                        }
                    },
                }
                div { style: "min-width:120px; text-align:right; color:#7a7f8a;",
                    "{cur} / {max}"
                }
                button {
                    style: "padding:6px 10px; cursor:pointer;",
                    onclick: move |_| show_annotations.set(!show_annotations()),
                    "Comments ({annotations.len()})"
                }
                if let Some(kind) = selected_kind.as_deref() {
                    div { style: "min-width:120px; color:#4c8dff;", "selected: {kind}" }
                }
            }
            if let Some((pointer, color, font_size, background)) = inspector {
                div { style: "position:fixed; top:0; right:0; width:248px; height:100vh; background:#13161d; border-left:1px solid #1c1f27; padding:16px; box-sizing:border-box; display:flex; flex-direction:column; gap:14px; overflow:auto;",
                    div { style: "display:flex; justify-content:space-between; align-items:center;",
                        div { style: "color:#4c8dff; font-weight:600;", "Inspector" }
                        button {
                            style: "cursor:pointer; background:none; border:none; color:#7a7f8a; font-size:16px; line-height:1;",
                            onclick: move |_| selected.set(None),
                            "✕"
                        }
                    }
                    if let Some(kind) = selected_kind.as_deref() {
                        div { style: "color:#7a7f8a;", "{kind}" }
                    }
                    // Fields live in a child component so they're memoized by
                    // their props and do NOT re-render on playback frame changes
                    // (which would steal focus / reset the input while typing).
                    InspectorFields { pointer: pointer.clone(), color, font_size, background }
                    AnnotationBox { pointer, kind: selected_kind.clone().unwrap_or_default(), current }
                }
            }
            if show_annotations() {
                div { style: "position:fixed; top:0; left:0; width:280px; height:100vh; background:#13161d; border-right:1px solid #1c1f27; padding:16px; box-sizing:border-box; overflow:auto; display:flex; flex-direction:column; gap:10px;",
                    div { style: "color:#4c8dff; font-weight:600;", "Comments" }
                    if annotations.is_empty() {
                        div { style: "color:#7a7f8a;", "No comments yet." }
                    }
                    for (id, note, frame, kind) in annotations.iter().cloned() {
                        div {
                            key: "{id}",
                            style: "border:1px solid #2a2f3a; border-radius:6px; padding:8px; display:flex; flex-direction:column; gap:6px;",
                            div { style: "color:#7a7f8a; font-size:11px;", "{kind} · frame {frame}" }
                            div { "{note}" }
                            div { style: "display:flex; gap:8px;",
                                button {
                                    style: "cursor:pointer; padding:2px 8px;",
                                    onclick: move |_| current.set(frame as u32),
                                    "Go to frame"
                                }
                                button {
                                    style: "cursor:pointer; padding:2px 8px; color:#ff6b6b;",
                                    onclick: {
                                        let shared = shared.clone();
                                        let id = id.clone();
                                        move |_| delete_annotation(&shared, &id)
                                    },
                                    "Delete"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Remove an annotation by id from the scenario file (the watcher reloads).
fn delete_annotation(shared: &Shared, id: &str) {
    let (path, raw) = {
        let m = shared.lock().unwrap();
        (m.path.clone(), m.raw.clone())
    };
    let raw = super::edit::remove_annotation(raw, id);
    if let (Some(path), Ok(t)) = (path, serde_json::to_string_pretty(&raw)) {
        let _ = std::fs::write(&path, t);
    }
}

/// The inspector's editable fields, isolated in a child component so they are
/// memoized by their props and don't re-render on playback frame changes
/// (which would steal focus / reset the input mid-typing).
#[component]
fn InspectorFields(pointer: String, color: String, font_size: String, background: String) -> Element {
    let shared = use_context::<Shared>();
    let edit_gen = use_signal(|| 0u64);
    let pending = use_signal(|| None::<(String, String, String)>);

    rsx! {
        label { style: "display:flex; flex-direction:column; gap:4px;",
            "color"
            input {
                style: "padding:6px; background:#0c0d10; color:#cfd3dc; border:1px solid #2a2f3a;",
                value: "{color}",
                oninput: {
                    let shared = shared.clone();
                    let p = pointer.clone();
                    move |e| schedule_write(&shared, edit_gen, pending, p.clone(), "color", e.value())
                }
            }
        }
        label { style: "display:flex; flex-direction:column; gap:4px;",
            "font-size"
            input {
                style: "padding:6px; background:#0c0d10; color:#cfd3dc; border:1px solid #2a2f3a;",
                value: "{font_size}",
                oninput: {
                    let shared = shared.clone();
                    let p = pointer.clone();
                    move |e| schedule_write(&shared, edit_gen, pending, p.clone(), "font-size", e.value())
                }
            }
        }
        label { style: "display:flex; flex-direction:column; gap:4px;",
            "background"
            input {
                style: "padding:6px; background:#0c0d10; color:#cfd3dc; border:1px solid #2a2f3a;",
                value: "{background}",
                oninput: {
                    let shared = shared.clone();
                    let p = pointer.clone();
                    move |e| schedule_write(&shared, edit_gen, pending, p.clone(), "background", e.value())
                }
            }
        }
    }
}

/// The "leave a comment for the agent" capture box, isolated in a child so its
/// textarea keeps focus during playback. Reads the playhead (`current`) only in
/// the submit handler, so it isn't subscribed to it.
#[component]
fn AnnotationBox(pointer: String, kind: String, current: Signal<u32>) -> Element {
    let shared = use_context::<Shared>();
    let mut note = use_signal(String::new);

    let submit = move |_| {
        let text = note();
        if text.trim().is_empty() {
            return;
        }
        let frame = current();
        let (path, raw, view, scene) = {
            let m = shared.lock().unwrap();
            let (view, scene) = match m.tasks.get(frame as usize) {
                Some(rustmotion::encode::video::FrameTask::Normal {
                    view_idx,
                    scene_idx,
                    ..
                }) => (*view_idx, *scene_idx),
                _ => (0, 0),
            };
            (m.path.clone(), m.raw.clone(), view, scene)
        };
        let ann = serde_json::json!({
            "id": annotation_id(), "note": text, "status": "open", "frame": frame,
            "view": view, "scene": scene,
            "target": { "pointer": pointer, "kind": kind }
        });
        let raw = super::edit::append_annotation(raw, ann);
        if let (Some(path), Ok(t)) = (path, serde_json::to_string_pretty(&raw)) {
            let _ = std::fs::write(&path, t);
            note.set(String::new());
        }
    };

    rsx! {
        div { style: "border-top:1px solid #1c1f27; padding-top:12px; display:flex; flex-direction:column; gap:8px;",
            div { style: "color:#7a7f8a;", "Leave a comment for the agent" }
            textarea {
                style: "min-height:64px; padding:6px; background:#0c0d10; color:#cfd3dc; border:1px solid #2a2f3a; resize:vertical; font:inherit;",
                value: "{note}",
                oninput: move |e| note.set(e.value()),
            }
            button {
                style: "padding:6px 10px; cursor:pointer; align-self:flex-end;",
                onclick: submit,
                "Add comment"
            }
        }
    }
}

/// Unique-ish id for a new annotation.
fn annotation_id() -> String {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("an_{n:x}")
}

/// Debounced inspector write: records the latest edit and, after a quiet
/// period, writes it. Each keystroke bumps `edit_gen`; only the spawned task
/// whose generation is still current performs the write, so rapid typing
/// collapses to a single write 350 ms after the last keystroke.
fn schedule_write(
    shared: &Shared,
    mut edit_gen: Signal<u64>,
    mut pending: Signal<Option<(String, String, String)>>,
    pointer: String,
    prop: &'static str,
    value: String,
) {
    pending.set(Some((pointer, prop.to_string(), value)));
    let g = edit_gen() + 1;
    edit_gen.set(g);
    let shared = shared.clone();
    spawn(async move {
        tokio::time::sleep(Duration::from_millis(350)).await;
        if edit_gen() == g {
            if let Some((ptr, p, v)) = pending() {
                write_prop(&shared, &ptr, &p, &v);
            }
        }
    });
}

/// Write a single style property back to the scenario file. The file watcher
/// then reloads the model and refreshes the preview. Empty values are ignored
/// so clearing a field mid-edit doesn't collapse the element.
fn write_prop(shared: &Shared, pointer: &str, prop: &str, value: &str) {
    if value.trim().is_empty() {
        return;
    }
    let (path, raw) = {
        let m = shared.lock().unwrap();
        (m.path.clone(), m.raw.clone())
    };
    if let (Some(path), Some(updated)) = (path, super::edit::set_style(raw, pointer, prop, value)) {
        if let Ok(text) = serde_json::to_string_pretty(&updated) {
            let _ = std::fs::write(&path, text);
        }
    }
}
