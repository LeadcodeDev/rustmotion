use dioxus::prelude::*;
use dioxus_icons::lucide::{ChevronLeft, Eye, MessageSquare, Monitor, Moon, Play, Sun};

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
                button {
                    style: "display:flex; align-items:center; gap:4px; padding:4px 8px; cursor:pointer; background:none; color:var(--rm-text); border:1px solid var(--rm-border-2); border-radius:8px; font:inherit;",
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
                button {
                    style: "display:flex; align-items:center; justify-content:center; width:30px; height:28px; cursor:pointer; background:none; color:var(--rm-text); border:1px solid var(--rm-border-2); border-radius:8px;",
                    title: "Theme: {current_theme.label()}",
                    onclick: move |_| theme.set(theme().next()),
                    match current_theme {
                        Theme::Dark => rsx! { Moon { size: 15 } },
                        Theme::Light => rsx! { Sun { size: 15 } },
                        Theme::System => rsx! { Monitor { size: 15 } },
                    }
                }
                button {
                    style: pill_style(inspecting),
                    onclick: move |_| show_hits.set(!show_hits()),
                    Eye { size: 15 }
                    "Inspect"
                }
                button {
                    style: pill_style(commenting),
                    onclick: move |_| show_annotations.set(!show_annotations()),
                    MessageSquare { size: 15 }
                    "Comments"
                    if comment_count > 0 {
                        span { style: "background:var(--rm-border-2); color:var(--rm-text); border-radius:999px; padding:0 6px; font-size:11px; line-height:18px;",
                            "{comment_count}"
                        }
                    }
                }
                button {
                    style: "display:flex; align-items:center; gap:6px; padding:6px 12px; cursor:pointer; background:var(--rm-accent); color:var(--rm-on-accent); border:none; border-radius:8px; font:inherit; font-weight:600;",
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

/// Pill button style for a toggle action; highlighted when `active`.
fn pill_style(active: bool) -> String {
    let (bg, fg, border) = if active {
        ("var(--rm-surface-3)", "var(--rm-text-strong)", "var(--rm-accent)")
    } else {
        ("none", "var(--rm-text)", "var(--rm-border-2)")
    };
    format!(
        "display:flex; align-items:center; gap:6px; padding:4px 8px; cursor:pointer; background:{bg}; color:{fg}; border:1px solid {border}; border-radius:8px; font:inherit;"
    )
}
