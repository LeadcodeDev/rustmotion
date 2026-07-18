use std::time::Duration;

use dioxus::prelude::*;
use dioxus_icons::lucide::{
    ChevronLeft, Download, Eye, MessageSquare, Monitor, Moon, Play, Redo2, Sun, Undo2,
};

use crate::components::button::{Button, ButtonSize, ButtonVariant};
use crate::scenario::{history_slot, redo, undo, Shared, SharedHistory, Theme, View};

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

/// The editor's top bar (open-slide style): a back-to-library control on the
/// left, the centered document title, and the action cluster on the right
/// (theme swap, Inspect overlay toggle, Comments panel toggle, Export, and
/// Present). `write_error` is `Some` when the last inspector write failed;
/// shown as a discrete warning indicator using the `--rm-error` token. The
/// export state (slot + polling) lives here too — the topbar is its only
/// consumer.
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

    rsx! {
        div { style: "position:relative; display:flex; align-items:center; justify-content:space-between; height:40px; padding:0 12px; border-bottom:1px solid var(--rm-border); background:var(--rm-surface-2); flex:none;",
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
                match &status {
                    ExportStatus::Done(path) => rsx! {
                        span {
                            title: "{path.display()}",
                            style: "color:var(--rm-text-muted); font-size:11px; white-space:nowrap; max-width:240px; overflow:hidden; text-overflow:ellipsis;",
                            "Exported: {path.display()}"
                        }
                    },
                    ExportStatus::Failed(reason) => rsx! {
                        span {
                            title: "{reason}",
                            style: "color:var(--rm-error); font-size:11px; white-space:nowrap; max-width:240px; overflow:hidden; text-overflow:ellipsis;",
                            "Export failed: {reason}"
                        }
                    },
                    _ => rsx! {},
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
