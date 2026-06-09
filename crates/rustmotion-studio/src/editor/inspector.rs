use dioxus::prelude::*;

use crate::scenario::{set_style, Shared};

use super::annotations::AnnotationBox;

/// The fixed right-hand inspector panel for the selected element: a header, the
/// editable style fields, and the "comment for the agent" box. Driven entirely
/// by props (the stored pointer + current values), so it stays memoized and
/// doesn't re-render on playback frame changes.
#[component]
pub fn InspectorPanel(
    selected: Signal<Option<(u32, String, String)>>,
    pointer: String,
    color: String,
    font_size: String,
    background: String,
    kind: String,
    current: Signal<u32>,
) -> Element {
    rsx! {
        div { style: "position:fixed; top:40px; right:0; width:248px; height:calc(100vh - 40px); background:var(--rm-surface); border-left:1px solid var(--rm-border); padding:16px; box-sizing:border-box; display:flex; flex-direction:column; gap:14px; overflow:auto;",
            div { style: "display:flex; justify-content:space-between; align-items:center;",
                div { style: "color:var(--rm-accent); font-weight:600;", "Inspector" }
                button {
                    style: "cursor:pointer; background:none; border:none; color:var(--rm-text-muted); font-size:16px; line-height:1;",
                    onclick: move |_| selected.set(None),
                    "✕"
                }
            }
            if !kind.is_empty() {
                div { style: "color:var(--rm-text-muted);", "{kind}" }
            }
            InspectorFields { pointer: pointer.clone(), color, font_size, background }
            AnnotationBox { pointer, kind, current }
        }
    }
}

/// The inspector's editable fields, isolated in a child component so they are
/// memoized by their props and don't re-render on playback frame changes
/// (which would steal focus / reset the input mid-typing).
#[component]
fn InspectorFields(pointer: String, color: String, font_size: String, background: String) -> Element {
    let shared = use_context::<Shared>();

    rsx! {
        label { style: "display:flex; flex-direction:column; gap:4px;",
            "color"
            input {
                style: "padding:6px; background:var(--rm-bg); color:var(--rm-text); border:1px solid var(--rm-border-2);",
                value: "{color}",
                oninput: {
                    let shared = shared.clone();
                    let p = pointer.clone();
                    move |e: FormEvent| write_prop(&shared, &p, "color", &e.value())
                }
            }
        }
        label { style: "display:flex; flex-direction:column; gap:4px;",
            "font-size"
            input {
                style: "padding:6px; background:var(--rm-bg); color:var(--rm-text); border:1px solid var(--rm-border-2);",
                value: "{font_size}",
                oninput: {
                    let shared = shared.clone();
                    let p = pointer.clone();
                    move |e: FormEvent| write_prop(&shared, &p, "font-size", &e.value())
                }
            }
        }
        label { style: "display:flex; flex-direction:column; gap:4px;",
            "background"
            input {
                style: "padding:6px; background:var(--rm-bg); color:var(--rm-text); border:1px solid var(--rm-border-2);",
                value: "{background}",
                oninput: {
                    let shared = shared.clone();
                    let p = pointer.clone();
                    move |e: FormEvent| write_prop(&shared, &p, "background", &e.value())
                }
            }
        }
    }
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
    if let (Some(path), Some(updated)) = (path, set_style(raw, pointer, prop, value)) {
        if let Ok(text) = serde_json::to_string_pretty(&updated) {
            let _ = std::fs::write(&path, text);
        }
    }
}
