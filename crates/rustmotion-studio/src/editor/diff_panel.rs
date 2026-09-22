use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{h_flex, v_flex, ActiveTheme, IconName, Sizable as _, StyledExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, px, App, Entity, Hsla, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    SharedString, StatefulInteractiveElement, Styled, Window,
};

use crate::app::state::{EditorState, Selection};
use crate::scenario::{ChangeKind, ElementChange, Shared};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DiffSide {
    A,
    B,
}

pub fn frame_for_change(shared: &Shared, change: &ElementChange) -> Option<u32> {
    let segments: Vec<&str> = change.pointer.split('/').collect();
    let (view, scene) = match segments.as_slice() {
        ["", "scenes", n, ..] => (0usize, n.parse::<usize>().ok()?),
        ["", "composition", v, "scenes", n, ..] => {
            (v.parse::<usize>().ok()?, n.parse::<usize>().ok()?)
        }
        _ => return None,
    };
    let (fps, tasks) = {
        let m = shared.lock().unwrap_or_else(|e| e.into_inner());
        (m.scenario.video.fps.max(1), m.tasks.clone())
    };
    let scene_start = tasks.iter().position(|t| {
        matches!(
            t,
            rustmotion::encode::video::FrameTask::Normal { view_idx, scene_idx, .. }
                if *view_idx == view && *scene_idx == scene
        )
    })?;
    let offset = (change.start_at.unwrap_or(0.0).max(0.0) * fps as f64) as usize;
    let frame = (scene_start + offset).min(tasks.len().saturating_sub(1));
    Some(frame as u32)
}

fn kind_badge(kind: &ChangeKind, cx: &App) -> (&'static str, Hsla) {
    match kind {
        ChangeKind::Added => ("+", cx.theme().success),
        ChangeKind::Removed => ("\u{2212}", cx.theme().danger),
        ChangeKind::Modified => ("~", cx.theme().accent),
    }
}

fn endpoint(s: &str) -> SharedString {
    if s.is_empty() {
        "\u{2014}".into()
    } else {
        s.to_string().into()
    }
}

pub struct DiffPanel {
    shared: Shared,
    editor: Entity<EditorState>,
    changes: Vec<ElementChange>,
}

impl DiffPanel {
    pub fn new(shared: Shared, editor: Entity<EditorState>, changes: Vec<ElementChange>) -> Self {
        Self {
            shared,
            editor,
            changes,
        }
    }
}

impl RenderOnce for DiffPanel {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let DiffPanel {
            shared,
            editor,
            changes,
        } = self;

        let mut groups: Vec<String> = Vec::new();
        for change in &changes {
            if !groups.contains(&change.group) {
                groups.push(change.group.clone());
            }
        }

        let close_editor = editor.clone();
        let change_count = changes.len();

        let header = h_flex()
            .items_center()
            .justify_between()
            .p_3p5()
            .child(
                v_flex()
                    .child(
                        div()
                            .font_semibold()
                            .text_color(cx.theme().accent)
                            .child("Changes"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("{change_count} since baseline")),
                    ),
            )
            .child(
                Button::new("close-diff-panel")
                    .icon(IconName::Close)
                    .ghost()
                    .xsmall()
                    .on_click(move |_, _, cx| {
                        close_editor.update(cx, |state, cx| {
                            state.diff_active = false;
                            cx.notify();
                        });
                    }),
            );

        let group_sections = groups.into_iter().map(|group| {
            let entries = changes
                .iter()
                .filter(|change| change.group == group)
                .cloned()
                .map(|change| change_entry(change, shared.clone(), editor.clone(), cx));
            v_flex()
                .gap_1p5()
                .px_3p5()
                .py_2()
                .border_t_1()
                .border_color(cx.theme().border)
                .child(
                    div()
                        .text_xs()
                        .font_semibold()
                        .text_color(cx.theme().muted_foreground)
                        .child(group.to_uppercase()),
                )
                .children(entries)
        });

        v_flex()
            .id("diff-panel")
            .flex_none()
            .w(px(300.))
            .h_full()
            .bg(cx.theme().sidebar)
            .border_l_1()
            .border_color(cx.theme().border)
            .overflow_y_scroll()
            .child(header)
            .when(changes.is_empty(), |this| {
                this.child(
                    div()
                        .px_3p5()
                        .text_color(cx.theme().muted_foreground)
                        .child("No changes since baseline."),
                )
            })
            .children(group_sections)
    }
}

fn change_entry(
    change: ElementChange,
    shared: Shared,
    editor: Entity<EditorState>,
    cx: &App,
) -> impl IntoElement {
    let (glyph, tone) = kind_badge(&change.kind, cx);
    let has_current_counterpart = change.kind != ChangeKind::Removed;
    let element_type = change.element_type.clone();
    let label = change.label.clone();
    let fields = change.fields.clone();
    let row_id: SharedString = format!("diff-entry-{}", change.pointer).into();
    let click_target = change.clone();

    v_flex()
        .id(row_id)
        .cursor_pointer()
        .gap_1p5()
        .p_2()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .on_click(move |_, _, cx| {
            if let Some(frame) = frame_for_change(&shared, &click_target) {
                editor.update(cx, |state, cx| {
                    state.current = frame;
                    cx.notify();
                });
            }
            if has_current_counterpart {
                editor.update(cx, |state, cx| {
                    state.selected = Some(Selection {
                        node_id: u32::MAX,
                        pointer: click_target.pointer.clone(),
                        kind: click_target.element_type.clone(),
                    });
                    cx.notify();
                });
            }
        })
        .child(
            h_flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_none()
                        .w(px(16.))
                        .h(px(16.))
                        .rounded(px(4.))
                        .border_1()
                        .border_color(tone)
                        .text_color(tone)
                        .text_sm()
                        .font_semibold()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(glyph),
                )
                .child(
                    div()
                        .flex_1()
                        .truncate()
                        .text_color(cx.theme().foreground)
                        .child(label),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(element_type),
                ),
        )
        .children(fields.into_iter().map(|field| {
            h_flex()
                .gap_1p5()
                .text_xs()
                .items_baseline()
                .child(
                    div()
                        .flex_none()
                        .text_color(cx.theme().muted_foreground)
                        .child(field.field),
                )
                .child(
                    h_flex()
                        .gap_1()
                        .overflow_hidden()
                        .child(
                            div()
                                .text_color(cx.theme().danger)
                                .child(endpoint(&field.before)),
                        )
                        .child(
                            div()
                                .text_color(cx.theme().muted_foreground)
                                .child("\u{2192}"),
                        )
                        .child(
                            div()
                                .text_color(cx.theme().success)
                                .child(endpoint(&field.after)),
                        ),
                )
        }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    use crate::scenario::StudioModel;

    const TWO_SCENES: &str = r##"{
        "video": { "width": 10, "height": 10, "fps": 10 },
        "scenes": [ { "duration": 1.0 }, { "duration": 2.0 } ]
    }"##;

    const TWO_VIEWS: &str = r##"{
        "video": { "width": 10, "height": 10, "fps": 10 },
        "composition": [
            { "type": "slide", "scenes": [ { "duration": 1.0 } ] },
            { "type": "slide", "scenes": [ { "duration": 1.0 } ] }
        ]
    }"##;

    fn shared_for(json: &str) -> Shared {
        let scenario = rustmotion::loader::load_scenario_from_source(None, Some(json)).unwrap();
        Arc::new(Mutex::new(StudioModel::new(scenario, None, None)))
    }

    fn change_at(pointer: &str, start_at: Option<f64>) -> ElementChange {
        ElementChange {
            pointer: pointer.to_string(),
            kind: ChangeKind::Modified,
            element_type: "text".to_string(),
            label: "label".to_string(),
            group: "Scene".to_string(),
            fields: Vec::new(),
            start_at,
        }
    }

    #[test]
    fn frame_for_change_finds_the_scene_start() {
        let shared = shared_for(TWO_SCENES);
        let change = change_at("/scenes/1/children/0", None);
        assert_eq!(frame_for_change(&shared, &change), Some(10));
    }

    #[test]
    fn frame_for_change_applies_the_start_at_offset() {
        let shared = shared_for(TWO_SCENES);
        let change = change_at("/scenes/1/children/0", Some(0.5));
        assert_eq!(frame_for_change(&shared, &change), Some(15));
    }

    #[test]
    fn frame_for_change_clamps_to_the_last_frame() {
        let shared = shared_for(TWO_SCENES);
        let change = change_at("/scenes/1/children/0", Some(100.0));
        assert_eq!(frame_for_change(&shared, &change), Some(29));
    }

    #[test]
    fn frame_for_change_resolves_composition_pointers() {
        let shared = shared_for(TWO_VIEWS);
        let change = change_at("/composition/1/scenes/0/children/0", None);
        assert_eq!(frame_for_change(&shared, &change), Some(10));
    }

    #[test]
    fn frame_for_change_rejects_non_scene_pointers() {
        let shared = shared_for(TWO_SCENES);
        let change = change_at("/video", None);
        assert_eq!(frame_for_change(&shared, &change), None);
    }

    #[test]
    fn endpoint_shows_an_em_dash_for_an_absent_field() {
        assert_eq!(endpoint(""), SharedString::from("\u{2014}"));
        assert_eq!(endpoint("48"), SharedString::from("48"));
    }
}
