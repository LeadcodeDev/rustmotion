use gpui_component::ActiveTheme;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    div, relative, AnyElement, App, Context, Hsla, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled,
};

use crate::app::state::Selection;
use crate::scenario::ChangeKind;

use super::frames::HitPct;
use super::view::EditorView;

pub fn mark_for<'a>(
    pointer: Option<&str>,
    diff_marks: &'a [(String, ChangeKind)],
) -> Option<&'a ChangeKind> {
    let pointer = pointer?;
    diff_marks
        .iter()
        .find(|(p, _)| p == pointer)
        .map(|(_, kind)| kind)
}

fn box_style(
    is_selected: bool,
    is_hovered: bool,
    diff_mark: Option<&ChangeKind>,
    cx: &App,
) -> (Hsla, Hsla) {
    if let Some(mark) = diff_mark {
        let color = match mark {
            ChangeKind::Added => cx.theme().success,
            ChangeKind::Modified => cx.theme().accent,
            ChangeKind::Removed => cx.theme().danger,
        };
        return (color, gpui_kit::transparent_black());
    }
    if is_selected {
        return (cx.theme().accent, gpui_kit::transparent_black());
    }
    if is_hovered {
        return (cx.theme().accent, cx.theme().accent.opacity(0.12));
    }
    (gpui_kit::transparent_black(), gpui_kit::transparent_black())
}

impl EditorView {
    pub(super) fn render_hit_overlay(
        &mut self,
        hits: &[HitPct],
        selected: Option<&Selection>,
        diff_marks: &[(String, ChangeKind)],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let hovered = self.hovered_hit;
        let selected_node = selected.map(|s| s.node_id);
        let selected_pointer = selected.map(|s| s.pointer.as_str());

        div()
            .absolute()
            .inset_0()
            .children(hits.iter().map(|hit| {
                let node_id = hit.node_id;
                let pointer = hit.pointer.clone();
                let kind = hit.kind.clone();
                let is_selected = selected_node == Some(node_id)
                    || (hit.pointer.is_some() && hit.pointer.as_deref() == selected_pointer);
                let is_hovered = hovered == Some(node_id);
                let diff_mark = mark_for(hit.pointer.as_deref(), diff_marks);
                let (border_color, bg_color) = box_style(is_selected, is_hovered, diff_mark, cx);
                let has_border = is_selected || is_hovered || diff_mark.is_some();

                div()
                    .id(SharedString::from(format!("hit-{node_id}")))
                    .absolute()
                    .left(relative(hit.x / 100.0))
                    .top(relative(hit.y / 100.0))
                    .w(relative(hit.w / 100.0))
                    .h(relative(hit.h / 100.0))
                    .cursor_pointer()
                    .when(has_border, |el| el.border_1().border_color(border_color))
                    .bg(bg_color)
                    .on_hover(cx.listener(move |this, entered: &bool, _window, cx| {
                        if *entered {
                            if this.hovered_hit != Some(node_id) {
                                this.hovered_hit = Some(node_id);
                                cx.notify();
                            }
                        } else if this.hovered_hit == Some(node_id) {
                            this.hovered_hit = None;
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |this, _event, _window, cx| {
                        cx.stop_propagation();
                        if let Some(ptr) = pointer.clone() {
                            this.editor.update(cx, |state, cx| {
                                state.selected = Some(Selection {
                                    node_id,
                                    pointer: ptr,
                                    kind: kind.clone(),
                                });
                                cx.notify();
                            });
                        }
                    }))
            }))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mark_for_finds_the_matching_pointer() {
        let marks = vec![
            ("/scenes/0/children/0".to_string(), ChangeKind::Added),
            ("/scenes/0/children/1".to_string(), ChangeKind::Modified),
        ];
        assert_eq!(
            mark_for(Some("/scenes/0/children/1"), &marks),
            Some(&ChangeKind::Modified)
        );
        assert_eq!(mark_for(Some("/scenes/0/children/9"), &marks), None);
        assert_eq!(mark_for(None, &marks), None);
    }
}
