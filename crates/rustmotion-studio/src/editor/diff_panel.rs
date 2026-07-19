//! The diff/review panel: lists baseline→current changes grouped by scene,
//! and the A|B flip side selector state. Replaces the inspector panel slot
//! while diff mode is active.

use dioxus::prelude::*;

use crate::components::button::{Button, ButtonSize, ButtonVariant};
use crate::scenario::{ChangeKind, ElementChange, Shared};

/// Which state the canvas renders in diff mode: A = baseline, B = current.
/// `Eq + Hash` so it can key the frame-prefetch cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiffSide {
    A,
    B,
}

const GROUP_HEADER: &str = "color:var(--rm-text-muted); font-size:10px; font-weight:600; letter-spacing:0.06em; text-transform:uppercase; margin-top:6px;";

/// Badge glyph + color per change kind.
fn kind_badge(kind: &ChangeKind) -> (&'static str, &'static str) {
    match kind {
        ChangeKind::Added => ("+", "#22c55e"),
        ChangeKind::Removed => ("−", "var(--rm-error)"),
        ChangeKind::Modified => ("~", "var(--rm-accent)"),
    }
}

/// Display form for a field-diff endpoint ("" = the field was absent).
fn endpoint(s: &str) -> String {
    if s.is_empty() {
        "—".to_string()
    } else {
        s.to_string()
    }
}

/// Map a change's pointer to the first frame of its scene (plus `start_at`
/// offset when known) so clicking an entry scrubs to where the element lives.
fn frame_for_change(shared: &Shared, change: &ElementChange) -> Option<u32> {
    // "/scenes/N/…" → (0, N); "/composition/V/scenes/N/…" → (V, N).
    let segs: Vec<&str> = change.pointer.split('/').collect();
    let (view, scene) = match segs.as_slice() {
        ["", "scenes", n, ..] => (0usize, n.parse::<usize>().ok()?),
        ["", "composition", v, "scenes", n, ..] => {
            (v.parse::<usize>().ok()?, n.parse::<usize>().ok()?)
        }
        _ => return None,
    };
    // Arc snapshot; the task scan runs without the model lock.
    let (fps, tasks) = {
        let m = shared.lock().unwrap_or_else(|e| e.into_inner());
        (m.scenario.video.fps.max(1), m.tasks.clone())
    };
    let base = tasks.iter().position(|t| {
        matches!(t, rustmotion::encode::video::FrameTask::Normal { view_idx, scene_idx, .. }
            if *view_idx == view && *scene_idx == scene)
    })?;
    let offset = (change.start_at.unwrap_or(0.0).max(0.0) * fps as f64) as usize;
    let frame = (base + offset).min(tasks.len().saturating_sub(1));
    Some(frame as u32)
}

/// The right-hand change list, grouped by scene. Clicking an entry selects the
/// element (Added/Modified — removals no longer exist in the current tree) and
/// scrubs to its first visible frame.
#[component]
pub fn DiffPanel(
    changes: Vec<ElementChange>,
    selected: Signal<Option<(u32, String, String)>>,
    current: Signal<u32>,
    diff_active: Signal<bool>,
) -> Element {
    // Group labels in first-appearance order (entries arrive grouped by
    // construction, but be robust to interleaving).
    let mut groups: Vec<String> = Vec::new();
    for c in &changes {
        if !groups.contains(&c.group) {
            groups.push(c.group.clone());
        }
    }

    rsx! {
        div { style: "width:300px; flex:none; min-height:0; background:var(--rm-surface); border-left:1px solid var(--rm-border); box-sizing:border-box; display:flex; flex-direction:column; overflow:auto;",
            div { style: "padding:14px; display:flex; justify-content:space-between; align-items:center;",
                div {
                    div { style: "color:var(--rm-accent); font-weight:600;", "Changes" }
                    div { style: "color:var(--rm-text-muted); font-size:11px;",
                        "{changes.len()} since baseline"
                    }
                }
                Button {
                    variant: ButtonVariant::Ghost,
                    size: ButtonSize::IconXs,
                    onclick: move |_| diff_active.set(false),
                    "✕"
                }
            }
            if changes.is_empty() {
                div { style: "padding:0 14px; color:var(--rm-text-muted);", "No changes since baseline." }
            }
            for group in groups {
                div { style: "display:flex; flex-direction:column; gap:6px; padding:8px 14px; border-top:1px solid var(--rm-border);",
                    div { style: "{GROUP_HEADER}", "{group}" }
                    for change in changes.iter().filter(|c| c.group == group).cloned() {
                        ChangeEntry { change, selected, current }
                    }
                }
            }
        }
    }
}

/// One change row: kind badge, label, and the field before→after list.
#[component]
fn ChangeEntry(
    change: ElementChange,
    mut selected: Signal<Option<(u32, String, String)>>,
    current: Signal<u32>,
) -> Element {
    let shared = use_context::<Shared>();
    let (glyph, color) = kind_badge(&change.kind);
    let removable = change.kind != ChangeKind::Removed;

    let onclick = {
        let shared = shared.clone();
        let change = change.clone();
        move |_| {
            if let Some(frame) = frame_for_change(&shared, &change) {
                current.set(frame);
            }
            // Removed elements have no counterpart in the current tree; the
            // pointer would resolve to nothing (or the wrong element).
            if removable {
                selected.set(Some((
                    u32::MAX,
                    change.pointer.clone(),
                    change.element_type.clone(),
                )));
            }
        }
    };

    rsx! {
        div {
            style: "border:1px solid var(--rm-border-2); border-radius:6px; padding:8px; display:flex; flex-direction:column; gap:5px; cursor:pointer;",
            onclick,
            div { style: "display:flex; align-items:center; gap:7px;",
                span { style: "flex:none; width:16px; height:16px; border-radius:4px; display:flex; align-items:center; justify-content:center; font-weight:700; font-size:12px; color:{color}; border:1px solid {color};",
                    "{glyph}"
                }
                span { style: "color:var(--rm-text-strong); overflow:hidden; text-overflow:ellipsis; white-space:nowrap;",
                    "{change.label}"
                }
                span { style: "color:var(--rm-text-muted); font-size:11px; margin-left:auto;",
                    "{change.element_type}"
                }
            }
            for f in change.fields.iter() {
                div { style: "display:flex; gap:6px; font-size:11px; align-items:baseline;",
                    span { style: "color:var(--rm-text-muted); flex:none;", "{f.field}" }
                    span { style: "overflow:hidden; text-overflow:ellipsis; white-space:nowrap;",
                        span { style: "color:var(--rm-error);", "{endpoint(&f.before)}" }
                        span { style: "color:var(--rm-text-muted);", " → " }
                        span { style: "color:#22c55e;", "{endpoint(&f.after)}" }
                    }
                }
            }
        }
    }
}
