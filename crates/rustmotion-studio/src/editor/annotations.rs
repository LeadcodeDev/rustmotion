use dioxus::prelude::*;

use crate::{
    components::{
        button::{Button, ButtonSize, ButtonVariant},
        textarea::{Textarea, TextareaVariant},
    },
    scenario::{append_annotation, remove_annotation, Shared},
};

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
        let raw = append_annotation(raw, ann);
        // Annotations live in the scenario JSON; HTML sources have no place to
        // store them, so skip the write rather than overwrite the HTML.
        if let (Some(path), Ok(t)) = (path, serde_json::to_string_pretty(&raw)) {
            if !rustmotion::loader::is_html_path(&path) {
                let _ = std::fs::write(&path, t);
                note.set(String::new());
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

/// Remove an annotation by id from the scenario file (the watcher reloads).
fn delete_annotation(shared: &Shared, id: &str) {
    let (path, raw) = {
        let m = shared.lock().unwrap();
        (m.path.clone(), m.raw.clone())
    };
    let raw = remove_annotation(raw, id);
    if let (Some(path), Ok(t)) = (path, serde_json::to_string_pretty(&raw)) {
        if !rustmotion::loader::is_html_path(&path) {
            let _ = std::fs::write(&path, t);
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
