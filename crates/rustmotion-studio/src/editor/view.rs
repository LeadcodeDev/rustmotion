use dioxus::desktop::{use_asset_handler, wry::http::Response, AssetRequest, RequestAsyncResponder};
use dioxus::prelude::*;

use crate::scenario::{list_annotations, read_style_object, Shared, View};

use super::annotations::AnnotationsPanel;
use super::frames::{frame_hits, render_frame, scene_prefix, HitPct};
use super::inspector::InspectorPanel;
use super::playback::{use_hot_reload, use_playback_clock, PlaybackBar};
use super::topbar::TopBar;

/// Overlay element boxes: invisible by default. Only the single hovered element
/// (`.hov`) gets a dashed outline, and the selected element (`.sel`) a solid
/// blue box. "Hovered" is tracked explicitly (one node at a time) rather than
/// via CSS `:hover`, which would light every overlapping box under the cursor.
const HIT_CSS: &str = "\
.rm-hit { position:absolute; box-sizing:border-box; cursor:pointer; border:1px dashed transparent; background:transparent; transition:border-color 80ms, background 80ms; }\
.rm-hit.hov { border-color:var(--rm-overlay-border); background:var(--rm-overlay-hover); }\
.rm-hit.sel { border:1px dashed var(--rm-overlay-border); background:transparent; }";

/// The editor view. Reads the shared studio model from context, registers an
/// asset handler that renders frames to JPEG on demand, and assembles the
/// topbar, canvas (with the clickable element overlay), transport bar, the
/// inspector, and the comments panel.
#[component]
pub fn StudioApp(view: Signal<View>) -> Element {
    let shared = use_context::<Shared>();

    let current = use_signal(|| 0u32);
    let playing = use_signal(|| false);
    let rev = use_signal(|| 0u64);
    // Selection stores (node_id, pointer, kind) so the inspector stays open via
    // the stored pointer even if the element collapses out of the hit-map.
    let selected = use_signal(|| None::<(u32, String, String)>);
    let show_annotations = use_signal(|| false);
    // Whether the clickable element overlay is shown (the "Inspect" toggle).
    let show_hits = use_signal(|| true);

    // Asset handler: GET /frame/{idx} -> JPEG of that frame.
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
                render_frame(&m.scenario, &m.tasks, idx, 1.0)
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

    use_playback_clock(shared.clone(), current, playing);
    use_hot_reload(shared.clone(), rev);

    let (total, err, title, annotations) = {
        let m = shared.lock().unwrap();
        let title = m
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled")
            .to_string();
        (m.total_frames, m.error.clone(), title, list_annotations(&m.raw))
    };
    let comment_count = annotations.len();

    // Inspector data for the selected element, computed ONCE per selection (not
    // on every model reload). Editing a property reloads the model and refreshes
    // the preview, but this memo doesn't recompute (selection is unchanged), so
    // the inspector's controls aren't re-rendered and keep their own state — no
    // focus/popover disruption. Driven by the stored pointer, so it stays open
    // even if the element collapses out of the current frame's hit-map.
    let inspector_shared = shared.clone();
    let inspector = use_memo(move || {
        selected().map(|(_, pointer, kind)| {
            let m = inspector_shared.lock().unwrap();
            let style = read_style_object(&m.raw, &pointer);
            (pointer, style, kind)
        })
    });

    if let Some(e) = err {
        return rsx! {
            div { style: "padding:24px; color:var(--rm-error); background:var(--rm-bg); min-height:100vh; font:13px sans-serif;",
                "Error: {e}"
            }
        };
    }

    rsx! {
        div { style: "margin:0; background:var(--rm-bg); min-height:100vh; color:var(--rm-text); font:13px -apple-system,sans-serif; display:flex; flex-direction:column;",
            TopBar {
                view,
                title,
                playing,
                current,
                show_annotations,
                show_hits,
                comment_count,
            }
            Canvas { current, rev, show_hits, selected }
            PlaybackBar { current, playing, total }
            if let Some((pointer, style, kind)) = inspector() {
                InspectorPanel { selected, pointer, kind, current, style }
            }
            if show_annotations() {
                AnnotationsPanel { current, annotations: annotations.clone() }
            }
        }
    }
}

/// The preview canvas: the rendered frame plus the clickable element overlay.
/// It owns the reads of `current`/`rev`/`show_hits`, so a playback tick or a
/// model reload (e.g. after an inspector edit) re-renders ONLY this subtree —
/// never the editor chrome or the inspector, which keep their own state.
#[component]
fn Canvas(
    current: Signal<u32>,
    rev: Signal<u64>,
    show_hits: Signal<bool>,
    mut selected: Signal<Option<(u32, String, String)>>,
) -> Element {
    let shared = use_context::<Shared>();
    let max = shared.lock().unwrap().total_frames.saturating_sub(1);
    let cur = current().min(max);
    let r = rev();

    let hits = if show_hits() {
        let m = shared.lock().unwrap();
        let prefix = scene_prefix(&m.raw, &m.tasks, cur);
        frame_hits(&m.scenario, &m.tasks, cur, &prefix)
    } else {
        Vec::new()
    };

    rsx! {
        div {
            style: "flex:1; display:flex; align-items:center; justify-content:center; padding:16px; overflow:hidden;",
            // Clicking the empty canvas (not an element) clears the selection.
            onclick: move |_| selected.set(None),
            div { style: "position:relative; display:inline-block; box-shadow:0 8px 40px rgba(0,0,0,0.5); line-height:0;",
                img {
                    src: "/frame/{cur}?v={r}",
                    style: "display:block; max-width:100%; max-height:78vh; height:auto;",
                }
                Overlay { hits, selected }
            }
        }
    }
}

/// The clickable element overlay over the frame. Tracks a single hovered node so
/// only the element directly under the cursor is outlined (the innermost box,
/// since it paints on top and captures the pointer). Owns its `hovered` signal
/// so moving the cursor re-renders just the overlay, not the whole editor.
#[component]
fn Overlay(hits: Vec<HitPct>, selected: Signal<Option<(u32, String, String)>>) -> Element {
    let mut hovered = use_signal(|| None::<u32>);
    let selected_node = selected().map(|(id, _, _)| id);
    let hov = hovered();

    rsx! {
        div { style: "position:absolute; inset:0;",
            style { "{HIT_CSS}" }
            for hit in hits.iter() {
                div {
                    key: "{hit.node_id}",
                    class: if selected_node == Some(hit.node_id) {
                        "rm-hit sel"
                    } else if hov == Some(hit.node_id) {
                        "rm-hit hov"
                    } else {
                        "rm-hit"
                    },
                    style: format!(
                        "left:{}%; top:{}%; width:{}%; height:{}%;",
                        hit.x, hit.y, hit.w, hit.h,
                    ),
                    onmouseenter: {
                        let id = hit.node_id;
                        move |_| hovered.set(Some(id))
                    },
                    onmouseleave: {
                        let id = hit.node_id;
                        move |_| {
                            if hovered() == Some(id) {
                                hovered.set(None);
                            }
                        }
                    },
                    onclick: {
                        let id = hit.node_id;
                        let ptr = hit.pointer.clone();
                        let kind = hit.kind.clone();
                        move |evt: MouseEvent| {
                            // Don't let the click bubble to the canvas (which deselects).
                            evt.stop_propagation();
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
