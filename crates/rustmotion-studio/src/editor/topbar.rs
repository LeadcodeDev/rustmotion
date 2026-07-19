use std::time::Duration;

use dioxus::prelude::*;
use dioxus_icons::lucide::{
    Camera, ChevronLeft, Download, Eye, GitCompareArrows, MessageSquare, Monitor, Moon, Play,
    Redo2, Sun, Undo2,
};

use crate::components::button::{Button, ButtonSize, ButtonVariant};
use crate::scenario::{
    baseline_slot, diff_scenarios, get_baseline, history_slot, redo, set_baseline, undo, Shared,
    SharedHistory, Theme, View,
};

use super::diff_panel::DiffSide;
use super::export::{export_label, export_slot, start_export, use_export_poll, ExportStatus};

/// Snapshot of the history slot for the topbar UI (polled, set-on-change).
#[derive(Clone, PartialEq, Default)]
struct HistoryUi {
    can_undo: bool,
    can_redo: bool,
    saving: bool,
}

/// Poll the history slot into a signal (~150 ms), same pattern as the export
/// poll. Undo/redo availability only counts when the slot belongs to the
/// currently open file.
fn use_history_poll(shared: Shared, slot: SharedHistory, mut sig: Signal<HistoryUi>) {
    use_future(move || {
        let shared = shared.clone();
        let slot = slot.clone();
        async move {
            loop {
                tokio::time::sleep(Duration::from_millis(150)).await;
                let path = {
                    let m = shared.lock().unwrap_or_else(|e| e.into_inner());
                    m.path.clone()
                };
                let ui = {
                    let st = slot.lock().unwrap_or_else(|e| e.into_inner());
                    let matches = path.is_some() && st.path == path;
                    HistoryUi {
                        can_undo: matches && st.history.can_undo(),
                        can_redo: matches && st.history.can_redo(),
                        saving: st.saving,
                    }
                };
                if ui != sig() {
                    sig.set(ui);
                }
            }
        }
    });
}

/// Poll whether the current scenario differs from its baseline (~300 ms) —
/// drives the Diff toggle's enabled state.
fn use_diff_poll(shared: Shared, mut sig: Signal<bool>) {
    use_future(move || {
        let shared = shared.clone();
        async move {
            loop {
                tokio::time::sleep(Duration::from_millis(300)).await;
                let (path, raw) = {
                    let m = shared.lock().unwrap_or_else(|e| e.into_inner());
                    (m.path.clone(), m.raw.clone())
                };
                let changed = path
                    .and_then(|p| get_baseline(&baseline_slot(), &p))
                    .map(|b| !diff_scenarios(&b.raw, &raw).is_empty())
                    .unwrap_or(false);
                if changed != sig() {
                    sig.set(changed);
                }
            }
        }
    });
}

/// The editor's top bar (open-slide style): a back-to-library control on the
/// left, the centered document title, and the action cluster on the right
/// (theme swap, Set baseline, Diff toggle, Inspect overlay toggle, Comments
/// panel toggle, Export, and Present). `write_error` is `Some` when the last
/// inspector write failed; shown as a discrete warning indicator using the
/// `--rm-error` token. The export state (slot + polling) lives here too — the
/// topbar is its only consumer.
#[component]
pub fn TopBar(
    view: Signal<View>,
    title: String,
    playing: Signal<bool>,
    current: Signal<u32>,
    show_annotations: Signal<bool>,
    show_hits: Signal<bool>,
    comment_count: usize,
    write_error: Option<String>,
    diff_active: Signal<bool>,
    diff_side: Signal<DiffSide>,
) -> Element {
    let shared = use_context::<Shared>();
    let mut theme = use_context::<Signal<Theme>>();
    let inspecting = show_hits();
    let commenting = show_annotations();
    let current_theme = theme();

    // Export state: the cross-thread slot the encode thread writes, and the
    // polled signal that drives the button label / status text.
    let export = use_hook(export_slot);
    let mut export_status = use_signal(|| ExportStatus::Idle);
    use_export_poll(export.clone(), export_status);
    let status = export_status();
    let exporting = status.is_running();

    // Undo/redo + pending-write ("Saving…") state, polled from the history slot.
    let history = use_hook(history_slot);
    let history_ui = use_signal(HistoryUi::default);
    use_history_poll(shared.clone(), history.clone(), history_ui);
    let hist = history_ui();

    // Diff availability (scenario differs from baseline).
    let diff_available = use_signal(|| false);
    use_diff_poll(shared.clone(), diff_available);
    let diffing = diff_active();
    let can_diff = diff_available();

    rsx! {
        div {
            style: "position:relative; display:flex; align-items:center; justify-content:space-between; height:40px; padding:0 12px; border-bottom:1px solid var(--rm-border); background:var(--rm-surface-2); flex:none;",
            // A focused topbar button owns its keys (Space activates IT, not
            // play/pause) — same isolation pattern as the inspector panel.
            onkeydown: move |evt: KeyboardEvent| evt.stop_propagation(),
            div { style: "display:flex; align-items:center; gap:8px; z-index:1;",
                Button {
                    variant: ButtonVariant::Outline,
                    size: ButtonSize::Sm,
                    onclick: move |_| view.set(View::Library),
                    ChevronLeft { size: 16 }
                    "Library"
                }
            }

            // ── Center: document title (absolutely centered) ─────────
            div { style: "position:absolute; left:50%; top:50%; transform:translate(-50%,-50%); color:var(--rm-text-strong); font-weight:500; max-width:40%; overflow:hidden; text-overflow:ellipsis; white-space:nowrap;",
                "{title}"
            }

            // ── Right: indicators + actions ──────────────────────────
            div { style: "display:flex; align-items:center; gap:6px; z-index:1;",
                if hist.saving {
                    span { style: "color:var(--rm-text-muted); font-size:11px; white-space:nowrap;",
                        "Saving…"
                    }
                }
                if let Some(ref msg) = write_error {
                    span {
                        title: "{msg}",
                        style: "color:var(--rm-error); font-size:11px; white-space:nowrap; max-width:200px; overflow:hidden; text-overflow:ellipsis;",
                        "Changes not saved: {msg}"
                    }
                }
                Button {
                    variant: ButtonVariant::Ghost,
                    size: ButtonSize::IconSm,
                    title: "Undo (Cmd+Z)",
                    disabled: !hist.can_undo,
                    onclick: {
                        let shared = shared.clone();
                        let history = history.clone();
                        move |_| undo(&shared, &history)
                    },
                    Undo2 { size: 15, stroke: if hist.can_undo { "var(--rm-text)" } else { "var(--rm-text-muted)" } }
                }
                Button {
                    variant: ButtonVariant::Ghost,
                    size: ButtonSize::IconSm,
                    title: "Redo (Shift+Cmd+Z)",
                    disabled: !hist.can_redo,
                    onclick: {
                        let shared = shared.clone();
                        let history = history.clone();
                        move |_| redo(&shared, &history)
                    },
                    Redo2 { size: 15, stroke: if hist.can_redo { "var(--rm-text)" } else { "var(--rm-text-muted)" } }
                }
                Button {
                    variant: ButtonVariant::Outline,
                    size: ButtonSize::IconSm,
                    title: "Theme: {current_theme.label()}",
                    onclick: move |_| theme.set(theme().next()),
                    match current_theme {
                        Theme::Dark => rsx! { Moon { size: 15 } },
                        Theme::Light => rsx! { Sun { size: 15 } },
                        Theme::System => rsx! { Monitor { size: 15 } },
                    }
                }
                Button {
                    variant: ButtonVariant::Outline,
                    size: ButtonSize::IconSm,
                    title: "Set baseline (snapshot the current state for diff review)",
                    onclick: {
                        let shared = shared.clone();
                        move |_| set_baseline_now(&shared)
                    },
                    Camera { size: 15 }
                }
                Button {
                    variant: if diffing { ButtonVariant::Secondary } else { ButtonVariant::Outline },
                    size: ButtonSize::Sm,
                    // Stays enabled while active so it can always be switched off.
                    disabled: !can_diff && !diffing,
                    title: if can_diff || diffing { "Compare against the baseline" } else { "No changes since baseline" },
                    onclick: move |_| {
                        let next = !diff_active();
                        diff_active.set(next);
                        if next {
                            diff_side.set(DiffSide::B);
                        }
                    },
                    GitCompareArrows { size: 15 }
                    "Diff"
                }
                Button {
                    variant: if inspecting { ButtonVariant::Secondary } else { ButtonVariant::Outline },
                    size: ButtonSize::Sm,
                    onclick: move |_| show_hits.set(!show_hits()),
                    Eye { size: 15 }
                    "Inspect"
                }
                Button {
                    variant: if commenting { ButtonVariant::Secondary } else { ButtonVariant::Outline },
                    size: ButtonSize::Sm,
                    onclick: move |_| show_annotations.set(!show_annotations()),
                    MessageSquare { size: 15 }
                    "Comments"
                    if comment_count > 0 {
                        span { style: "background:var(--rm-border-2); color:var(--rm-text); border-radius:999px; padding:0 6px; font-size:11px; line-height:18px;",
                            "{comment_count}"
                        }
                    }
                }
                Button {
                    variant: ButtonVariant::Secondary,
                    size: ButtonSize::Sm,
                    disabled: exporting,
                    onclick: {
                        let shared = shared.clone();
                        let export = export.clone();
                        move |_| {
                            if !exporting {
                                start_export(&shared, &export);
                                // Instant feedback; the poll refines it within 150 ms.
                                export_status.set(ExportStatus::Running {
                                    phase: "Rendering",
                                    done: 0,
                                    total: 0,
                                });
                            }
                        }
                    },
                    Download { size: 14 }
                    "{export_label(&status)}"
                }
                Button {
                    variant: ButtonVariant::Primary,
                    size: ButtonSize::Sm,
                    onclick: move |_| {
                        current.set(0);
                        playing.set(true);
                    },
                    Play { size: 14 }
                    "Present"
                }
            }
        }
    }
}

/// Re-snapshot the baseline from the current state: source text re-read from
/// disk, raw taken from the live model (so it matches what future models will
/// hold). Read failures surface through `write_error`.
fn set_baseline_now(shared: &Shared) {
    let (path, raw) = {
        let m = shared.lock().unwrap_or_else(|e| e.into_inner());
        (m.path.clone(), m.raw.clone())
    };
    let Some(path) = path else {
        return;
    };
    match std::fs::read_to_string(&path) {
        Ok(source) => set_baseline(&baseline_slot(), &path, source, raw),
        Err(e) => {
            let mut m = shared.lock().unwrap_or_else(|e2| e2.into_inner());
            m.write_error = Some(format!("baseline: {e}"));
            m.generation = m.generation.wrapping_add(1);
        }
    }
}
