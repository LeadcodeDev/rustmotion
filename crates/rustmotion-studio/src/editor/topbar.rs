use dioxus::prelude::*;
use dioxus_icons::lucide::{ChevronLeft, Download, Eye, MessageSquare, Monitor, Moon, Play, Sun};

use crate::components::button::{Button, ButtonSize, ButtonVariant};
use crate::scenario::{Shared, Theme, View};

use super::export::{export_label, export_slot, start_export, use_export_poll, ExportStatus};

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
