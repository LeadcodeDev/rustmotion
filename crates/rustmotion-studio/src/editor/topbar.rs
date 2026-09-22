use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{
    h_flex, ActiveTheme, Disableable as _, IconName, Sizable as _, StyledExt as _,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, px, App, Entity, InteractiveElement, IntoElement, ParentElement, RenderOnce, Styled,
    Window,
};

use crate::app::state::{EditorState, StudioState};
use crate::scenario::{baseline_slot, history_slot, redo, set_baseline, undo, Shared, View};

use super::export::{export_label, ExportStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HistoryUi {
    pub can_undo: bool,
    pub can_redo: bool,
    pub saving: bool,
}

pub struct TopBar {
    shared: Shared,
    studio: Entity<StudioState>,
    editor: Entity<EditorState>,
    title: String,
    history_ui: HistoryUi,
    diff_available: bool,
    comment_count: usize,
    write_error: Option<String>,
    audio_error: Option<String>,
    export_status: ExportStatus,
}

impl TopBar {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        shared: Shared,
        studio: Entity<StudioState>,
        editor: Entity<EditorState>,
        title: String,
        history_ui: HistoryUi,
        diff_available: bool,
        comment_count: usize,
        write_error: Option<String>,
        audio_error: Option<String>,
        export_status: ExportStatus,
    ) -> Self {
        Self {
            shared,
            studio,
            editor,
            title,
            history_ui,
            diff_available,
            comment_count,
            write_error,
            audio_error,
            export_status,
        }
    }
}

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

impl RenderOnce for TopBar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let TopBar {
            shared,
            studio,
            editor,
            title,
            history_ui,
            diff_available,
            comment_count,
            write_error,
            audio_error,
            export_status,
        } = self;

        let theme_pref = studio.read(cx).theme_pref;
        let diff_active = editor.read(cx).diff_active;
        let show_hits = editor.read(cx).show_hits;
        let show_annotations = editor.read(cx).show_annotations;
        let exporting = export_status.is_running();

        let library_studio = studio.clone();
        let undo_shared = shared.clone();
        let redo_shared = shared.clone();
        let baseline_shared = shared.clone();
        let diff_editor = editor.clone();
        let inspect_editor = editor.clone();
        let comments_editor = editor.clone();
        let export_shared = shared.clone();
        let present_editor = editor.clone();

        h_flex()
            .id("topbar")
            .relative()
            .flex_none()
            .h(px(40.))
            .w_full()
            .items_center()
            .justify_between()
            .px_3()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().secondary)
            .child(
                h_flex().items_center().gap_2().child(
                    Button::new("topbar-library")
                        .icon(IconName::ChevronLeft)
                        .label("Library")
                        .ghost()
                        .small()
                        .on_click(move |_, _, cx| {
                            library_studio.update(cx, |state, cx| {
                                state.view = View::Library;
                                cx.notify();
                            });
                        }),
                ),
            )
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .font_semibold()
                            .text_color(cx.theme().foreground)
                            .child(title),
                    ),
            )
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .when(history_ui.saving, |el| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child("Saving…"),
                        )
                    })
                    .when_some(write_error, |el, msg| {
                        el.child(
                            div()
                                .text_xs()
                                .max_w(px(200.))
                                .truncate()
                                .text_color(cx.theme().danger)
                                .child(format!("Changes not saved: {msg}")),
                        )
                    })
                    .when_some(audio_error, |el, msg| {
                        el.child(
                            div()
                                .text_xs()
                                .max_w(px(220.))
                                .truncate()
                                .text_color(cx.theme().danger)
                                .child(format!("Audio not analysed: {msg}")),
                        )
                    })
                    .child(
                        Button::new("topbar-undo")
                            .icon(IconName::Undo2)
                            .tooltip("Undo (Cmd+Z)")
                            .ghost()
                            .xsmall()
                            .disabled(!history_ui.can_undo)
                            .on_click(move |_, _, _cx| {
                                undo(&undo_shared, &history_slot());
                            }),
                    )
                    .child(
                        Button::new("topbar-redo")
                            .icon(IconName::Redo2)
                            .tooltip("Redo (Shift+Cmd+Z)")
                            .ghost()
                            .xsmall()
                            .disabled(!history_ui.can_redo)
                            .on_click(move |_, _, _cx| {
                                redo(&redo_shared, &history_slot());
                            }),
                    )
                    .child(
                        Button::new("topbar-theme")
                            .when_some(theme_pref_icon(theme_pref), |b, icon| b.icon(icon))
                            .label(theme_pref.label())
                            .tooltip(format!("Theme: {}", theme_pref.label()))
                            .ghost()
                            .xsmall()
                            .on_click(move |_, window, cx| {
                                let next = studio.read(cx).theme_pref.next();
                                crate::theme::set(&studio, next, window, cx);
                            }),
                    )
                    .child(
                        Button::new("topbar-baseline")
                            .icon(IconName::Frame)
                            .label("Baseline")
                            .tooltip("Set baseline (snapshot the current state for diff review)")
                            .ghost()
                            .xsmall()
                            .on_click(move |_, _, _cx| {
                                set_baseline_now(&baseline_shared);
                            }),
                    )
                    .child(
                        Button::new("topbar-diff")
                            .label("Diff")
                            .tooltip(if diff_available || diff_active {
                                "Compare against the baseline"
                            } else {
                                "No changes since baseline"
                            })
                            .when(diff_active, |b| b.primary())
                            .when(!diff_active, |b| b.secondary())
                            .small()
                            .disabled(!diff_available && !diff_active)
                            .on_click(move |_, _, cx| {
                                diff_editor.update(cx, |state, cx| {
                                    let next = !state.diff_active;
                                    state.diff_active = next;
                                    if next {
                                        state.diff_side = super::diff_panel::DiffSide::B;
                                    }
                                    cx.notify();
                                });
                            }),
                    )
                    .child(
                        Button::new("topbar-inspect")
                            .icon(if show_hits {
                                IconName::Eye
                            } else {
                                IconName::EyeOff
                            })
                            .label("Inspect")
                            .when(show_hits, |b| b.primary())
                            .when(!show_hits, |b| b.secondary())
                            .small()
                            .on_click(move |_, _, cx| {
                                inspect_editor.update(cx, |state, cx| {
                                    state.show_hits = !state.show_hits;
                                    cx.notify();
                                });
                            }),
                    )
                    .child(
                        Button::new("topbar-comments")
                            .icon(IconName::Inbox)
                            .label(if comment_count > 0 {
                                format!("Comments ({comment_count})")
                            } else {
                                "Comments".to_string()
                            })
                            .when(show_annotations, |b| b.primary())
                            .when(!show_annotations, |b| b.secondary())
                            .small()
                            .on_click(move |_, _, cx| {
                                comments_editor.update(cx, |state, cx| {
                                    state.show_annotations = !state.show_annotations;
                                    cx.notify();
                                });
                            }),
                    )
                    .child(
                        Button::new("topbar-export")
                            .label(export_label(&export_status))
                            .secondary()
                            .small()
                            .disabled(exporting)
                            .on_click(move |_, _, _cx| {
                                if !exporting {
                                    super::export::start_export(
                                        &export_shared,
                                        &super::export::export_slot(),
                                    );
                                }
                            }),
                    )
                    .child(
                        Button::new("topbar-present")
                            .icon(IconName::Play)
                            .label("Present")
                            .primary()
                            .small()
                            .on_click(move |_, _, cx| {
                                present_editor.update(cx, |state, cx| {
                                    state.current = 0;
                                    state.playing = true;
                                    cx.notify();
                                });
                            }),
                    ),
            )
    }
}

fn theme_pref_icon(pref: crate::app::state::ThemePref) -> Option<IconName> {
    use crate::app::state::ThemePref;
    match pref {
        ThemePref::Dark => Some(IconName::Moon),
        ThemePref::Light => Some(IconName::Sun),
        ThemePref::System => None,
    }
}
