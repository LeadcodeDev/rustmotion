use dioxus::desktop::{
    use_asset_handler, wry::http::Response, AssetRequest, RequestAsyncResponder,
};
use dioxus::prelude::*;

use crate::scenario::{
    baseline_slot, diff_scenarios, get_baseline, history_slot, list_annotations, read_field,
    read_style_object, redo, undo, ChangeKind, ElementChange, Shared, View,
};

use super::annotations::AnnotationsPanel;
use super::diff_panel::{DiffPanel, DiffSide};
use super::frames::{baseline_arcs, frame_hits, render_frame, scene_prefix, HitPct};
use super::inspector::InspectorPanel;
use super::playback::{
    playback_action, use_hot_reload, use_playback_clock, PlaybackAction, PlaybackBar,
};
use super::prefetch::{frame_cache, use_prefetch_publisher, FrameKey};
use super::topbar::TopBar;

/// The hot-reload revision signal, exposed via context so the optimistic edit
/// path can nudge the canvas immediately instead of waiting for the 250 ms
/// generation poll.
#[derive(Clone, Copy)]
pub struct RevSignal(pub Signal<u64>);

/// Overlay element boxes: invisible by default. Only the single hovered element
/// (`.hov`) gets a dashed outline, and the selected element (`.sel`) a solid
/// blue box. "Hovered" is tracked explicitly (one node at a time) rather than
/// via CSS `:hover`, which would light every overlapping box under the cursor.
/// In diff mode, changed elements keep a persistent outline: `.diff-add`
/// (green) for added, `.diff-mod` (accent) for modified.
const HIT_CSS: &str = "\
.rm-hit { position:absolute; box-sizing:border-box; cursor:pointer; border:1px dashed transparent; background:transparent; transition:border-color 80ms, background 80ms; }\
.rm-hit.hov { border-color:var(--rm-overlay-border); background:var(--rm-overlay-hover); }\
.rm-hit.sel { border:1px dashed var(--rm-overlay-border); background:transparent; }\
.rm-hit.diff-add { border:1px solid #22c55e; }\
.rm-hit.diff-mod { border:1px solid var(--rm-accent); }";

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
    // Diff/review mode: toggle + which state the canvas shows (A = baseline).
    let diff_active = use_signal(|| false);
    let diff_side = use_signal(|| DiffSide::B);

    // Asset handler: GET /frame/{idx} -> JPEG of that frame. `?side=a` renders
    // the BASELINE scenario instead (diff mode flip). Cache-first: a prefetched
    // frame is served straight from memory (no model lock, no render). On miss,
    // the Arcs are cloned under a brief lock and the render runs OUTSIDE any
    // lock (catch_unwind keeps a Skia panic from poisoning anything), then the
    // frame is inserted so the next request hits. The prefetcher may race this
    // path and render the same frame once more — accepted, the cache dedupes.
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
            let side_a = uri.contains("side=a");

            let result: Result<Vec<u8>, ()> = if side_a {
                let path = {
                    let m = handler_shared.lock().unwrap_or_else(|e| e.into_inner());
                    m.path.clone()
                };
                match path
                    .and_then(|p| get_baseline(&baseline_slot(), &p).map(|b| (p, b)))
                    .ok_or(())
                    .and_then(|(p, b)| baseline_arcs(&p, &b.source).map_err(|_| ()))
                {
                    Ok((hash, scenario, tasks)) => {
                        let key = FrameKey {
                            generation: hash,
                            side: DiffSide::A,
                            frame: idx,
                        };
                        serve_or_render(key, idx, &scenario, &tasks, None)
                    }
                    Err(()) => Err(()),
                }
            } else {
                let (scenario, tasks, generation) = {
                    let m = handler_shared.lock().unwrap_or_else(|e| e.into_inner());
                    (m.scenario.clone(), m.tasks.clone(), m.generation)
                };
                let key = FrameKey {
                    generation,
                    side: DiffSide::B,
                    frame: idx,
                };
                serve_or_render(key, idx, &scenario, &tasks, Some(generation))
            };
            match result {
                Ok(jpeg) => {
                    responder.respond(
                        Response::builder()
                            .header("Content-Type", "image/jpeg")
                            .header("Cache-Control", "no-cache, no-store, must-revalidate")
                            .header("Pragma", "no-cache")
                            .header("Expires", "0")
                            .body(jpeg)
                            .unwrap(),
                    );
                }
                Err(()) => {
                    responder.respond(
                        Response::builder()
                            .status(500)
                            .header("Content-Type", "text/plain")
                            .body(b"render error".to_vec())
                            .unwrap(),
                    );
                }
            }
        },
    );

    use_playback_clock(shared.clone(), current, playing);
    use_hot_reload(shared.clone(), rev);
    use_context_provider(|| RevSignal(rev));
    // Publish the playhead/side/model snapshot for the background prefetcher.
    use_prefetch_publisher(
        shared.clone(),
        current,
        playing,
        rev,
        diff_active,
        diff_side,
    );

    let (total, err, write_err, title, annotations) = {
        let m = shared.lock().unwrap_or_else(|e| e.into_inner());
        let title = m
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled")
            .to_string();
        (
            m.total_frames,
            m.error.clone(),
            m.write_error.clone(),
            title,
            list_annotations(&m.raw),
        )
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
            let m = inspector_shared.lock().unwrap_or_else(|e| e.into_inner());
            let style = read_style_object(&m.raw, &pointer);
            let content = read_field(&m.raw, &pointer, "content");
            // Element root object (children stripped) for the schema-driven
            // Properties section.
            let element = m
                .raw
                .pointer(&pointer)
                .cloned()
                .map(|mut v| {
                    if let Some(o) = v.as_object_mut() {
                        o.remove("children");
                    }
                    v
                })
                .unwrap_or(serde_json::Value::Null);
            (pointer, style, kind, content, element)
        })
    });

    // Baseline→current diff, recomputed on hot reload (rev) while diff mode is
    // active. Reading it below also subscribes this component, so the panel and
    // highlights refresh when an agent edit lands on disk.
    let diff_shared = shared.clone();
    let diff_data = use_memo(move || {
        if !diff_active() {
            return None;
        }
        let _ = rev(); // recompute on every reload
        let m = diff_shared.lock().unwrap_or_else(|e| e.into_inner());
        let path = m.path.clone()?;
        let baseline = get_baseline(&baseline_slot(), &path)?;
        Some(diff_scenarios(&baseline.raw, &m.raw))
    });

    if let Some(e) = err {
        return rsx! {
            div { style: "padding:24px; color:var(--rm-error); background:var(--rm-bg); min-height:100vh; font:13px sans-serif;",
                "Error: {e}"
            }
        };
    }

    // Persistent outlines for changed elements (B side only: the hit overlay
    // is computed from the CURRENT model, so it can't mark baseline layouts).
    let changes: Option<Vec<ElementChange>> = diff_data();
    let diff_marks: Vec<(String, ChangeKind)> = if diff_side() == DiffSide::B {
        changes
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .filter(|c| c.kind != ChangeKind::Removed)
            .map(|c| (c.pointer.clone(), c.kind.clone()))
            .collect()
    } else {
        Vec::new()
    };

    // Panel slot: the diff panel takes over the inspector's slot while diff
    // mode is active; otherwise the inspector shows for the selected element.
    let panel = inspector();
    let diff_on = diff_active();
    let panel_w = if diff_on || panel.is_some() {
        "300px"
    } else {
        "0px"
    };

    // Undo/redo keyboard shortcuts: the root div is focusable (and focused on
    // mount) so Cmd+Z / Shift+Cmd+Z (Ctrl on non-mac) reach it — key events in
    // the subtree bubble up to it. The inspector slot stops keydown propagation
    // so text fields keep their native editing undo.
    let history = use_hook(history_slot);
    let on_shortcut = {
        let shared = shared.clone();
        let history = history.clone();
        let mut playing = playing;
        let mut current = current;
        move |evt: KeyboardEvent| {
            let mods = evt.modifiers();
            let key = evt.key();
            // Undo / redo (Cmd/Ctrl+Z, Shift for redo).
            if (mods.meta() || mods.ctrl())
                && matches!(key, Key::Character(ref c) if c.eq_ignore_ascii_case("z"))
            {
                evt.prevent_default();
                if mods.shift() {
                    redo(&shared, &history);
                } else {
                    undo(&shared, &history);
                }
                return;
            }
            // Transport: Space toggles, arrows step (Shift = ×10), Home/End
            // seek. prevent_default keeps Space from scrolling/activating and
            // arrows from scrolling the page.
            if let Some(action) = playback_action(&key, mods) {
                evt.prevent_default();
                let max = {
                    let m = shared.lock().unwrap_or_else(|e| e.into_inner());
                    m.total_frames.saturating_sub(1)
                };
                match action {
                    PlaybackAction::TogglePlay => playing.set(!playing()),
                    PlaybackAction::Step(d) => {
                        playing.set(false);
                        let next = (current() as i64 + d).clamp(0, max as i64) as u32;
                        current.set(next);
                    }
                    PlaybackAction::SeekStart => current.set(0),
                    PlaybackAction::SeekEnd => current.set(max),
                }
            }
        }
    };

    rsx! {
        div {
            style: "margin:0; background:var(--rm-bg); height:100vh; overflow:hidden; color:var(--rm-text); font:13px -apple-system,sans-serif; display:flex; flex-direction:column; outline:none;",
            tabindex: "0",
            onmounted: move |evt| {
                spawn(async move {
                    let _ = evt.set_focus(true).await;
                });
            },
            onkeydown: on_shortcut,
            TopBar {
                view,
                title,
                playing,
                current,
                show_annotations,
                show_hits,
                comment_count,
                write_error: write_err,
                diff_active,
                diff_side,
            }
            div { style: "flex:1; display:flex; flex-direction:row; flex-wrap:nowrap; align-items:stretch; min-height:0; overflow:hidden;",
                div { style: "flex:1; min-width:0; display:flex; flex-direction:column; min-height:0;",
                    Canvas { current, rev, show_hits, selected, diff_active, diff_side, diff_marks }
                    PlaybackBar { current, playing, total, diff_active, diff_side }
                }
                div {
                    style: "flex:none; display:flex; overflow:hidden; transition:width 220ms ease; width:{panel_w};",
                    // Keep Cmd+Z inside inspector inputs/textareas native
                    // (text-field undo), not intercepted by the editor.
                    onkeydown: move |evt: KeyboardEvent| evt.stop_propagation(),
                    if diff_on {
                        DiffPanel {
                            changes: changes.unwrap_or_default(),
                            selected,
                            current,
                            diff_active,
                        }
                    } else if let Some((pointer, style, kind, content, element)) = panel {
                        InspectorPanel { selected, pointer, kind, current, content, style, element }
                    }
                }
            }
            if show_annotations() {
                AnnotationsPanel { current, annotations: annotations.clone() }
            }
        }
    }
}

/// Cache-first frame fetch for the asset handler: serve the prefetched JPEG
/// when present, otherwise render outside any lock (panic-fenced) and insert
/// into the cache for the next request. `gen_b` is `Some(model generation)`
/// when serving side B (drives stale-generation eviction); side A passes the
/// baseline hash inside `key.generation` and `None` here.
fn serve_or_render(
    key: FrameKey,
    idx: u32,
    scenario: &rustmotion::schema::ResolvedScenario,
    tasks: &[rustmotion::encode::video::FrameTask],
    gen_b: Option<u64>,
) -> Result<Vec<u8>, ()> {
    if let Some(bytes) = frame_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key)
    {
        return Ok((*bytes).clone());
    }
    let rendered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        render_frame(scenario, tasks, idx, 1.0)
    }))
    .map_err(|_| ())?;
    let (gen_b, gen_a) = match key.side {
        DiffSide::B => (gen_b, None),
        DiffSide::A => (None, Some(key.generation)),
    };
    frame_cache()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key, rendered.clone(), gen_b, gen_a, idx);
    Ok(rendered)
}

/// The preview canvas: the rendered frame plus the clickable element overlay.
/// It owns the reads of `current`/`rev`/`show_hits`, so a playback tick or a
/// model reload (e.g. after an inspector edit) re-renders ONLY this subtree —
/// never the editor chrome or the inspector, which keep their own state.
/// In diff mode with side A, the image renders the baseline scenario (the
/// overlay is hidden: its boxes come from the current model's layout).
#[component]
fn Canvas(
    current: Signal<u32>,
    rev: Signal<u64>,
    show_hits: Signal<bool>,
    mut selected: Signal<Option<(u32, String, String)>>,
    diff_active: Signal<bool>,
    diff_side: Signal<DiffSide>,
    diff_marks: Vec<(String, ChangeKind)>,
) -> Element {
    let shared = use_context::<Shared>();
    // Arc snapshots under a brief lock; the hit-map layout render below runs
    // WITHOUT the model lock.
    let (scenario, tasks) = {
        let m = shared.lock().unwrap_or_else(|e| e.into_inner());
        (m.scenario.clone(), m.tasks.clone())
    };
    let (vw, vh) = (scenario.video.width, scenario.video.height);
    let max = (tasks.len() as u32).saturating_sub(1);
    let cur = current().min(max);
    let r = rev();
    let side_a = diff_active() && diff_side() == DiffSide::A;
    let side_suffix = if side_a { "&side=a" } else { "" };

    let hits = if show_hits() && !side_a {
        let prefix = {
            let m = shared.lock().unwrap_or_else(|e| e.into_inner());
            scene_prefix(&m.raw, &tasks, cur)
        };
        frame_hits(&scenario, &tasks, cur, &prefix)
    } else {
        Vec::new()
    };

    rsx! {
        div {
            style: "flex:1; min-width:0; min-height:0; display:flex; align-items:center; justify-content:center; padding:16px; overflow:hidden;",
            // Clicking the empty canvas (not an element) clears the selection.
            onclick: move |_| selected.set(None),
            // Wrapper carries the video's aspect ratio and is capped at 100% of the
            // (definite) canvas in both axes, so it scales to fit without the circular
            // `max-width:100%` collapse an inline-block shrink-wrap would cause when the
            // canvas narrows (e.g. the inspector opens). The image fills it 1:1, so the
            // overlay stays exactly aligned.
            div { style: "position:relative; aspect-ratio:{vw} / {vh}; max-width:100%; max-height:100%; min-width:0; box-shadow:0 8px 40px rgba(0,0,0,0.5); line-height:0;",
                img {
                    src: "/frame/{cur}?v={r}{side_suffix}",
                    style: "display:block; width:100%; height:100%;",
                }
                Overlay { hits, selected, diff_marks }
            }
        }
    }
}

/// The clickable element overlay over the frame. Tracks a single hovered node so
/// only the element directly under the cursor is outlined (the innermost box,
/// since it paints on top and captures the pointer). Owns its `hovered` signal
/// so moving the cursor re-renders just the overlay, not the whole editor.
/// `diff_marks` adds persistent outlines to changed elements (matched by
/// pointer) while diff mode shows the current state.
#[component]
fn Overlay(
    hits: Vec<HitPct>,
    selected: Signal<Option<(u32, String, String)>>,
    diff_marks: Vec<(String, ChangeKind)>,
) -> Element {
    let mut hovered = use_signal(|| None::<u32>);
    // Selection matches by node id (canvas clicks) or pointer (diff panel
    // clicks store u32::MAX as the id).
    let (selected_node, selected_ptr) = match selected() {
        Some((id, ptr, _)) => (Some(id), Some(ptr)),
        None => (None, None),
    };
    let hov = hovered();

    let mark_class = |hit: &HitPct| -> &'static str {
        let Some(ptr) = hit.pointer.as_deref() else {
            return "";
        };
        match diff_marks.iter().find(|(p, _)| p == ptr) {
            Some((_, ChangeKind::Added)) => " diff-add",
            Some((_, ChangeKind::Modified)) => " diff-mod",
            _ => "",
        }
    };

    rsx! {
        div { style: "position:absolute; inset:0;",
            style { "{HIT_CSS}" }
            for hit in hits.iter() {
                div {
                    key: "{hit.node_id}",
                    class: {
                        let is_sel = selected_node == Some(hit.node_id)
                            || (hit.pointer.is_some() && hit.pointer.as_deref() == selected_ptr.as_deref());
                        let base = if is_sel {
                            "rm-hit sel"
                        } else if hov == Some(hit.node_id) {
                            "rm-hit hov"
                        } else {
                            "rm-hit"
                        };
                        format!("{base}{}", mark_class(hit))
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
