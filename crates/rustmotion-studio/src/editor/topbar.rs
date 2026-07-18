use dioxus::prelude::*;
use dioxus_icons::lucide::{ChevronLeft, Eye, MessageSquare, Monitor, Moon, Play, Sun};

use crate::components::button::{Button, ButtonSize, ButtonVariant};
use crate::scenario::{Theme, View};

/// The editor's top bar (open-slide style): a back-to-library control on the
/// left, the centered document title, and the action cluster on the right
/// (theme swap, Inspect overlay toggle, Comments panel toggle, and Present).
#[component]
pub fn TopBar(
    view: Signal<View>,
    title: String,
    playing: Signal<bool>,
    current: Signal<u32>,
    show_annotations: Signal<bool>,
    show_hits: Signal<bool>,
    comment_count: usize,
) -> Element {
    let mut theme = use_context::<Signal<Theme>>();
    let inspecting = show_hits();
    let commenting = show_annotations();
    let current_theme = theme();

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

            // ── Right: actions ───────────────────────────────────────
            div { style: "display:flex; align-items:center; gap:6px; z-index:1;",
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
