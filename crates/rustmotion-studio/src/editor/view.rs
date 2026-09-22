use gpui_component::slider::SliderState;
use gpui_component::{v_flex, ActiveTheme};
use gpui_kit::{
    div, AnyElement, AppContext as _, Context, Entity, FocusHandle, InteractiveElement,
    IntoElement, KeyBinding, Modifiers, ParentElement, Render, RenderOnce,
    StatefulInteractiveElement, Styled, Subscription, Window,
};

use crate::app::state::{EditorState, StudioState};
use crate::scenario::{
    baseline_slot, diff_scenarios, get_baseline, history_slot, list_annotations, redo, undo,
    ChangeKind, Shared,
};

use super::annotations::AnnotationsPanel;
use super::diff_panel::{DiffPanel, DiffSide};
use super::export::{export_slot, ExportStatus, ExportWatcher};
use super::frames::{baseline_arcs, frame_hits_deep, scene_prefix};
use super::inspector::InspectorPanel;
use super::playback::{self, apply_playback_action, playback_action, TransportBar};
use super::prefetch::{scale_factor, FrameKey};
use super::surface::{
    frame_surface, prefetched_frame, publish_prefetch_target, render_current_frame,
    request_next_frame_if_playing, swap_frame,
};
use super::topbar::{HistoryUi, TopBar};

gpui_kit::actions!(editor_shell, [Undo, Redo]);

#[must_use = "the returned frame still owns a GPU texture and must reach cx.drop_image"]
fn reset_for_new_document(
    state: &mut EditorState,
) -> Option<std::sync::Arc<gpui_kit::RenderImage>> {
    state.current = 0;
    state.playing = false;
    state.selected = None;
    state.diff_active = false;
    state.diff_side = DiffSide::B;
    state.frame.take()
}

type RenderKey = (u32, u64, DiffSide, u16);
type HitsKey = (u32, u64, DiffSide);

pub struct EditorView {
    shared: Shared,
    studio: Entity<StudioState>,
    pub(super) editor: Entity<EditorState>,
    inspector: Entity<InspectorPanel>,
    export_watcher: Entity<ExportWatcher>,
    scrubber: Entity<SliderState>,
    focus_handle: FocusHandle,
    history_ui: HistoryUi,
    diff_available: bool,
    export_status: ExportStatus,
    pub(super) hovered_hit: Option<u32>,
    hits_cache: Vec<super::frames::HitPct>,
    hits_key: Option<HitsKey>,
    rendering_frame: bool,
    last_frame_key: Option<RenderKey>,
    open_document: Option<std::path::PathBuf>,
    _subscriptions: Vec<Subscription>,
}

impl EditorView {
    pub fn new(state: Entity<StudioState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let shared = state.read(cx).shared.clone();
        let editor = cx.new(|_| EditorState {
            current: 0,
            playing: false,
            muted: false,
            rev: 0,
            selected: None,
            show_annotations: false,
            show_hits: true,
            diff_active: false,
            diff_side: DiffSide::B,
            preview_scale: super::prefetch::preview_scale_pct(),
            frame: None,
        });

        let inspector =
            cx.new(|cx| InspectorPanel::new(shared.clone(), editor.clone(), window, cx));
        let export_watcher = cx.new(|cx| ExportWatcher::new(window, cx));
        let scrubber = playback::new_scrubber(cx);

        cx.bind_keys([
            KeyBinding::new("space", playback::TogglePlay, Some("Editor")),
            KeyBinding::new("left", playback::StepBackward, Some("Editor")),
            KeyBinding::new("shift-left", playback::StepBackwardBig, Some("Editor")),
            KeyBinding::new("right", playback::StepForward, Some("Editor")),
            KeyBinding::new("shift-right", playback::StepForwardBig, Some("Editor")),
            KeyBinding::new("home", playback::SeekToStart, Some("Editor")),
            KeyBinding::new("end", playback::SeekToEnd, Some("Editor")),
            KeyBinding::new("cmd-z", Undo, Some("Editor")),
            KeyBinding::new("shift-cmd-z", Redo, Some("Editor")),
        ]);

        playback::spawn_playback_clock(shared.clone(), editor.clone(), cx);
        playback::spawn_hot_reload(shared.clone(), editor.clone(), cx);

        let mut subscriptions = Vec::new();
        subscriptions.push(cx.observe_in(&editor, window, |this, _editor, window, cx| {
            let snapshot = this.editor.read(cx);
            publish_prefetch_target(&this.shared, snapshot);
            this.sync_frame(window, cx);
            cx.notify();
        }));
        subscriptions.push(cx.subscribe(&scrubber, |this, _scrubber, event, cx| {
            if let gpui_component::slider::SliderEvent::Change(
                gpui_component::slider::SliderValue::Single(v),
            ) = event
            {
                let total = this
                    .shared
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .total_frames;
                let frame = playback::fraction_to_frame(*v, total);
                this.editor.update(cx, |state, cx| {
                    if state.current != frame {
                        state.current = frame;
                        cx.notify();
                    }
                });
            }
        }));

        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);

        let mut view = Self {
            shared: shared.clone(),
            studio: state,
            editor: editor.clone(),
            inspector,
            export_watcher,
            scrubber,
            focus_handle,
            history_ui: HistoryUi::default(),
            diff_available: false,
            export_status: ExportStatus::Idle,
            hovered_hit: None,
            hits_cache: Vec::new(),
            hits_key: None,
            rendering_frame: false,
            last_frame_key: None,
            open_document: None,
            _subscriptions: subscriptions,
        };
        view.spawn_topbar_polls(cx);
        {
            let snapshot = editor.read(cx);
            publish_prefetch_target(&shared, snapshot);
        }
        view.sync_frame(window, cx);
        view
    }

    fn spawn_topbar_polls(&self, cx: &mut Context<Self>) {
        let shared = self.shared.clone();
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(150))
                .await;
            let (path, raw) = {
                let m = shared.lock().unwrap_or_else(|e| e.into_inner());
                (m.path.clone(), m.raw.clone())
            };
            let history_ui = {
                let slot = history_slot();
                let st = slot.lock().unwrap_or_else(|e| e.into_inner());
                let matches = path.is_some() && st.path == path;
                HistoryUi {
                    can_undo: matches && st.history.can_undo(),
                    can_redo: matches && st.history.can_redo(),
                    saving: st.saving,
                }
            };
            let diff_available = path
                .clone()
                .and_then(|p| get_baseline(&baseline_slot(), &p))
                .map(|b| !diff_scenarios(&b.raw, &raw).is_empty())
                .unwrap_or(false);
            let export_status = export_slot()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            let alive = this
                .update(cx, |this, cx| {
                    if this.history_ui != history_ui
                        || this.diff_available != diff_available
                        || this.export_status != export_status
                    {
                        this.history_ui = history_ui;
                        this.diff_available = diff_available;
                        this.export_status = export_status;
                        cx.notify();
                    }
                })
                .is_ok();
            if !alive {
                break;
            }
        })
        .detach();
    }

    fn refresh_hits(&mut self, cx: &mut Context<Self>) {
        let should = self.editor.read(cx).should_compute_hits();
        if !should {
            if !self.hits_cache.is_empty() {
                self.hits_cache.clear();
            }
            self.hits_key = None;
            return;
        }
        let (current, rev, side) = {
            let e = self.editor.read(cx);
            let side = if e.diff_active {
                e.diff_side
            } else {
                DiffSide::B
            };
            (e.current, e.rev, side)
        };
        let key: HitsKey = (current, rev, side);
        if self.hits_key == Some(key) {
            return;
        }
        self.hits_key = Some(key);

        if side == DiffSide::A {
            self.hits_cache.clear();
            return;
        }

        let (scenario, tasks, raw) = {
            let m = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            (m.scenario.clone(), m.tasks.clone(), m.raw.clone())
        };
        let cur = current.min((tasks.len() as u32).saturating_sub(1));

        cx.spawn(async move |this, cx| {
            let hits = cx
                .background_spawn(async move {
                    let prefix = scene_prefix(&raw, &tasks, cur);
                    frame_hits_deep(&scenario, &tasks, cur, &prefix).unwrap_or_default()
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.hits_key == Some(key) {
                    this.hits_cache = hits;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn sync_document(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let path = {
            let m = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            m.path.clone()
        };
        if self.open_document == path {
            return;
        }
        self.open_document = path;

        self.hits_cache.clear();
        self.hits_key = None;
        self.hovered_hit = None;
        self.last_frame_key = None;

        self.editor.update(cx, |state, cx| {
            if let Some(stale) = reset_for_new_document(state) {
                cx.drop_image(stale, Some(window));
            }
            cx.notify();
        });
    }

    fn sync_frame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (current, rev, side, scale_pct) = {
            let e = self.editor.read(cx);
            let side = if e.diff_active {
                e.diff_side
            } else {
                DiffSide::B
            };
            (e.current, e.rev, side, e.preview_scale)
        };
        let key: RenderKey = (current, rev, side, scale_pct);
        if self.rendering_frame || self.last_frame_key == Some(key) {
            return;
        }

        let (scenario, tasks, generation) = match side {
            DiffSide::B => {
                let m = self.shared.lock().unwrap_or_else(|e| e.into_inner());
                (m.scenario.clone(), m.tasks.clone(), m.generation)
            }
            DiffSide::A => {
                let path = {
                    let m = self.shared.lock().unwrap_or_else(|e| e.into_inner());
                    m.path.clone()
                };
                let Some(path) = path else {
                    return;
                };
                let Some(baseline) = get_baseline(&baseline_slot(), &path) else {
                    return;
                };
                let Ok((hash, scenario, tasks)) = baseline_arcs(&path, &baseline.source) else {
                    return;
                };
                (scenario, tasks, hash)
            }
        };

        let cache_key = FrameKey {
            generation,
            side,
            frame: current,
            scale_pct,
        };
        if let Some(image) = prefetched_frame(&cache_key) {
            self.last_frame_key = Some(key);
            self.editor.update(cx, |state, cx| {
                swap_frame(state, image, window, cx);
                cx.notify();
            });
            return;
        }

        self.last_frame_key = Some(key);
        self.rendering_frame = true;
        let editor_entity = self.editor.clone();
        let scale = scale_factor(scale_pct);
        cx.spawn_in(window, async move |this, cx| {
            let image = cx
                .background_spawn(
                    async move { render_current_frame(&scenario, &tasks, current, scale) },
                )
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.rendering_frame = false;
                if let Ok(image) = image {
                    editor_entity.update(cx, |state, cx| {
                        swap_frame(state, image, window, cx);
                        cx.notify();
                    });
                }
                cx.notify();
                window.refresh();
            });
        })
        .detach();
    }

    fn dispatch_playback(
        &mut self,
        key: &str,
        mods: Modifiers,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(action) = playback_action(key, mods) {
            apply_playback_action(action, &self.shared, &self.editor, window, cx);
        }
    }
}

impl Render for EditorView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let error = {
            self.shared
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .error
                .clone()
        };
        if let Some(err) = error {
            return div()
                .p_6()
                .size_full()
                .bg(cx.theme().background)
                .text_color(cx.theme().danger)
                .child(format!("Error: {err}"))
                .into_any_element();
        }

        self.sync_document(window, cx);
        request_next_frame_if_playing(self.editor.read(cx).playing, window);
        self.refresh_hits(cx);

        let (title, write_error, audio_error, raw, vw, vh) = {
            let m = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            let title = m
                .path
                .as_ref()
                .and_then(|p| p.file_stem())
                .and_then(|s| s.to_str())
                .unwrap_or("Untitled")
                .to_string();
            (
                title,
                m.write_error.clone(),
                m.audio_error.clone(),
                m.raw.clone(),
                m.scenario.video.width,
                m.scenario.video.height,
            )
        };
        let annotations = list_annotations(&raw);
        let comment_count = annotations.len();

        let (diff_active, diff_side, show_annotations, selected) = {
            let e = self.editor.read(cx);
            (
                e.diff_active,
                e.diff_side,
                e.show_annotations,
                e.selected.clone(),
            )
        };

        let changes = if diff_active {
            let path = {
                self.shared
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .path
                    .clone()
            };
            path.and_then(|p| get_baseline(&baseline_slot(), &p))
                .map(|b| diff_scenarios(&b.raw, &raw))
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let diff_marks: Vec<(String, ChangeKind)> = if diff_active && diff_side == DiffSide::B {
            changes
                .iter()
                .filter(|c| c.kind != ChangeKind::Removed)
                .map(|c| (c.pointer.clone(), c.kind.clone()))
                .collect()
        } else {
            Vec::new()
        };

        let frame = self.editor.read(cx).frame.clone();
        let hits = self.hits_cache.clone();
        let overlay = self.render_hit_overlay(&hits, selected.as_ref(), &diff_marks, cx);

        let panel: Option<AnyElement> = if diff_active {
            Some(
                DiffPanel::new(self.shared.clone(), self.editor.clone(), changes)
                    .render(window, cx)
                    .into_any_element(),
            )
        } else if selected.is_some() {
            Some(self.inspector.clone().into_any_element())
        } else {
            None
        };

        let canvas = div()
            .id("canvas")
            .relative()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .items_center()
            .justify_center()
            .p_4()
            .overflow_hidden()
            .on_click(cx.listener(|this, _event, _window, cx| {
                this.editor.update(cx, |state, cx| {
                    if state.selected.is_some() {
                        state.selected = None;
                        cx.notify();
                    }
                });
            }))
            .child(
                div()
                    .relative()
                    .h_full()
                    .aspect_ratio(vw.max(1) as f32 / vh.max(1) as f32)
                    .max_w_full()
                    .max_h_full()
                    .child(frame_surface(frame))
                    .child(overlay),
            );

        let transport = TransportBar::new(
            self.shared.clone(),
            self.editor.clone(),
            self.scrubber.clone(),
        )
        .render(window, cx)
        .into_any_element();

        let topbar = TopBar::new(
            self.shared.clone(),
            self.studio.clone(),
            self.editor.clone(),
            title,
            self.history_ui,
            self.diff_available,
            comment_count,
            write_error,
            audio_error,
            self.export_status.clone(),
        )
        .render(window, cx)
        .into_any_element();

        let annotations_panel = show_annotations.then(|| {
            AnnotationsPanel::new(self.shared.clone(), self.editor.clone(), annotations)
                .render(window, cx)
                .into_any_element()
        });

        div()
            .id("editor-shell")
            .key_context("Editor")
            .track_focus(&self.focus_handle)
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_action(cx.listener(|this, _: &playback::TogglePlay, window, cx| {
                this.dispatch_playback("space", Modifiers::none(), window, cx);
            }))
            .on_action(cx.listener(|this, _: &playback::StepBackward, window, cx| {
                this.dispatch_playback("left", Modifiers::none(), window, cx);
            }))
            .on_action(
                cx.listener(|this, _: &playback::StepBackwardBig, window, cx| {
                    this.dispatch_playback(
                        "left",
                        Modifiers {
                            shift: true,
                            ..Modifiers::none()
                        },
                        window,
                        cx,
                    );
                }),
            )
            .on_action(cx.listener(|this, _: &playback::StepForward, window, cx| {
                this.dispatch_playback("right", Modifiers::none(), window, cx);
            }))
            .on_action(
                cx.listener(|this, _: &playback::StepForwardBig, window, cx| {
                    this.dispatch_playback(
                        "right",
                        Modifiers {
                            shift: true,
                            ..Modifiers::none()
                        },
                        window,
                        cx,
                    );
                }),
            )
            .on_action(cx.listener(|this, _: &playback::SeekToStart, window, cx| {
                this.dispatch_playback("home", Modifiers::none(), window, cx);
            }))
            .on_action(cx.listener(|this, _: &playback::SeekToEnd, window, cx| {
                this.dispatch_playback("end", Modifiers::none(), window, cx);
            }))
            .on_action(cx.listener(|this, _: &Undo, _window, cx| {
                undo(&this.shared, &history_slot());
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Redo, _window, cx| {
                redo(&this.shared, &history_slot());
                cx.notify();
            }))
            .child(topbar)
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_row()
                    .min_h_0()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .child(canvas)
                            .child(transport),
                    )
                    .children(panel),
            )
            .children(annotations_panel)
            .child(self.export_watcher.clone())
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor_state_mid_session() -> EditorState {
        EditorState {
            current: 412,
            playing: true,
            muted: false,
            rev: 9,
            selected: Some(crate::app::state::Selection {
                node_id: 7,
                pointer: "/scenes/2/children/3".into(),
                kind: "text".into(),
            }),
            show_annotations: true,
            show_hits: true,
            diff_active: true,
            diff_side: DiffSide::A,
            preview_scale: 50,
            frame: None,
        }
    }

    #[test]
    fn opening_another_document_drops_the_selection_pointing_into_the_old_one() {
        let mut state = editor_state_mid_session();
        let _ = reset_for_new_document(&mut state);
        assert!(
            state.selected.is_none(),
            "a pointer like /scenes/2/children/3 resolves in the new document too, \
             and would silently edit a different element"
        );
    }

    #[test]
    fn opening_another_document_rewinds_the_playhead_and_stops_playback() {
        let mut state = editor_state_mid_session();
        let _ = reset_for_new_document(&mut state);
        assert_eq!(
            state.current, 0,
            "frame 412 may not exist in the new document"
        );
        assert!(!state.playing);
    }

    #[test]
    fn opening_another_document_leaves_diff_mode() {
        let mut state = editor_state_mid_session();
        let _ = reset_for_new_document(&mut state);
        assert!(
            !state.diff_active,
            "the baseline belonged to the old document"
        );
        assert_eq!(state.diff_side, DiffSide::B);
    }

    #[test]
    fn opening_another_document_keeps_the_session_preferences() {
        let mut state = editor_state_mid_session();
        let _ = reset_for_new_document(&mut state);
        assert_eq!(
            state.preview_scale, 50,
            "preview quality is a session choice, not a document one"
        );
        assert!(state.show_hits);
        assert!(state.show_annotations);
    }
}
