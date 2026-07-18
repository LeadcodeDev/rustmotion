use dioxus::prelude::*;

use crate::{
    components::{
        button::{Button, ButtonSize, ButtonVariant},
        textarea::{Textarea, TextareaVariant},
    },
    scenario::{
        append_annotation, append_sidecar_annotation, remove_annotation, remove_sidecar_annotation,
        Shared,
    },
};

/// Write `content` to `path`, returning a user-facing error string on failure.
fn write_file(path: &std::path::Path, content: &str) -> Result<(), String> {
    std::fs::write(path, content).map_err(|e| format!("write: {e}"))
}

/// The left-hand "Comments" panel: lists the scenario's annotations, with
/// per-comment "go to frame" and "delete" actions.
#[component]
pub fn AnnotationsPanel(
    current: Signal<u32>,
    annotations: Vec<(String, String, u64, String)>,
) -> Element {
    let shared = use_context::<Shared>();
    rsx! {
        div { style: "position:fixed; top:40px; left:0; width:280px; height:calc(100vh - 40px); background:var(--rm-surface); border-right:1px solid var(--rm-border); padding:16px; box-sizing:border-box; overflow:auto; display:flex; flex-direction:column; gap:10px;",
            div { style: "color:var(--rm-accent); font-weight:600;", "Comments" }
            if annotations.is_empty() {
                div { style: "color:var(--rm-text-muted);", "No comments yet." }
            }
            for (id, note, frame, kind) in annotations.iter().cloned() {
                div {
                    key: "{id}",
                    style: "border:1px solid var(--rm-border-2); border-radius:6px; padding:8px; display:flex; flex-direction:column; gap:6px;",
                    div { style: "color:var(--rm-text-muted); font-size:11px;", "{kind} · frame {frame}" }
                    div { "{note}" }
                    div { style: "display:flex; gap:8px;",
                        Button {
                            variant: ButtonVariant::Ghost,
                            size: ButtonSize::Xs,
                            onclick: move |_| current.set(frame as u32),
                            "Go to frame"
                        }
                        Button {
                            variant: ButtonVariant::Destructive,
                            size: ButtonSize::Xs,
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

/// The "leave a comment for the agent" capture box, isolated in a child so its
/// textarea keeps focus during playback. Reads the playhead (`current`) only in
/// the submit handler, so it isn't subscribed to it.
#[component]
pub fn AnnotationBox(pointer: String, kind: String, current: Signal<u32>) -> Element {
    let shared = use_context::<Shared>();
    let mut note = use_signal(String::new);

    let submit = {
        let shared = shared.clone();
        move |_| {
            let text = note();
            if text.trim().is_empty() {
                return;
            }
            let frame = current();
            let (path, raw, view, scene) = {
                let m = shared.lock().unwrap_or_else(|e| e.into_inner());
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
            let Some(path) = path else {
                return;
            };
            if rustmotion::loader::is_html_path(&path) {
                // HTML sources: annotations live in the `<stem>.annotations.json`
                // sidecar; the HTML file itself is never modified.
                let result = append_sidecar_annotation(&path, ann.clone());
                let mut m = shared.lock().unwrap_or_else(|e| e.into_inner());
                match result {
                    Ok(()) => {
                        m.write_error = None;
                        // The watcher doesn't observe the sidecar, so reflect
                        // the change in the in-memory raw directly.
                        m.raw = append_annotation(std::mem::take(&mut m.raw), ann);
                        m.generation = m.generation.wrapping_add(1);
                        note.set(String::new());
                    }
                    Err(e) => {
                        m.write_error = Some(e);
                        m.generation = m.generation.wrapping_add(1);
                    }
                }
            } else {
                let updated = append_annotation(raw, ann);
                match serde_json::to_string_pretty(&updated) {
                    Ok(t) => {
                        let write_result = write_file(&path, &t);
                        let mut m = shared.lock().unwrap_or_else(|e| e.into_inner());
                        match write_result {
                            Ok(()) => {
                                m.write_error = None;
                                note.set(String::new());
                            }
                            Err(e) => {
                                m.write_error = Some(e);
                                m.generation = m.generation.wrapping_add(1);
                            }
                        }
                    }
                    Err(e) => {
                        let mut m = shared.lock().unwrap_or_else(|e| e.into_inner());
                        m.write_error = Some(format!("json: {e}"));
                        m.generation = m.generation.wrapping_add(1);
                    }
                }
            }
        }
    };

    rsx! {
        div { style: "border-top:1px solid var(--rm-border); padding-top:12px; padding:16px; display:flex; flex-direction:column; gap:8px;",
            div { style: "color:var(--rm-text-muted);", "Leave a comment for the agent" },
            Textarea {
                variant: TextareaVariant::Outline,
                placeholder: "Enter your description",
                value: note,
                rows: 5,
                oninput: move |e: FormEvent| {
                  note.set(e.value())
                },
            }
            Button {
              variant: ButtonVariant::Primary,
              onclick: submit,
              "Add comment"
            }
        }
    }
}

/// Remove an annotation by id. JSON sources rewrite the scenario file (the
/// watcher reloads); HTML sources rewrite the annotations sidecar (deleted
/// when it becomes empty) and update the in-memory raw directly.
fn delete_annotation(shared: &Shared, id: &str) {
    let (path, raw) = {
        let m = shared.lock().unwrap_or_else(|e| e.into_inner());
        (m.path.clone(), m.raw.clone())
    };
    let Some(path) = path else {
        return;
    };
    if rustmotion::loader::is_html_path(&path) {
        let result = remove_sidecar_annotation(&path, id);
        let mut m = shared.lock().unwrap_or_else(|e| e.into_inner());
        match result {
            Ok(()) => {
                m.write_error = None;
                m.raw = remove_annotation(std::mem::take(&mut m.raw), id);
                m.generation = m.generation.wrapping_add(1);
            }
            Err(e) => {
                m.write_error = Some(e);
                m.generation = m.generation.wrapping_add(1);
            }
        }
    } else {
        let updated = remove_annotation(raw, id);
        let write_result = serde_json::to_string_pretty(&updated)
            .map_err(|e| format!("json: {e}"))
            .and_then(|t| write_file(&path, &t));
        if let Err(e) = write_result {
            let mut m = shared.lock().unwrap_or_else(|e2| e2.into_inner());
            m.write_error = Some(e);
            m.generation = m.generation.wrapping_add(1);
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
