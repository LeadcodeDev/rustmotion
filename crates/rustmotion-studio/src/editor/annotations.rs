use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::input::{Textarea, TextareaState};
use gpui_component::{h_flex, v_flex, ActiveTheme, Disableable as _, Sizable as _, StyledExt as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    deferred, div, px, App, AppContext as _, Context, Entity, InteractiveElement, IntoElement,
    ParentElement, Render, RenderOnce, SharedString, StatefulInteractiveElement, Styled, Window,
};

use crate::app::state::EditorState;
use crate::scenario::{
    append_annotation, append_sidecar_annotation, history_slot, record_edit, remove_annotation,
    remove_sidecar_annotation, Shared,
};

const TOPBAR_HEIGHT: gpui_kit::Pixels = px(40.);
const PANEL_WIDTH: gpui_kit::Pixels = px(280.);

fn write_file(path: &std::path::Path, content: &str) -> Result<(), String> {
    std::fs::write(path, content).map_err(|e| format!("write: {e}"))
}

fn annotation_id() -> String {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("an_{n:x}")
}

pub fn submit_annotation(
    shared: &Shared,
    frame: u32,
    pointer: String,
    kind: String,
    text: String,
) -> Result<(), String> {
    let (path, raw, view, scene) = {
        let m = shared.lock().unwrap_or_else(|e| e.into_inner());
        let (view, scene) = match m.tasks.get(frame as usize) {
            Some(rustmotion::encode::video::FrameTask::Normal {
                view_idx,
                scene_idx,
                ..
            }) => (*view_idx, *scene_idx),
            _ => (0, 0),
        };
        (m.path.clone(), m.raw.clone(), view, scene)
    };
    let ann = serde_json::json!({
        "id": annotation_id(), "note": text, "status": "open", "frame": frame,
        "view": view, "scene": scene,
        "target": { "pointer": pointer, "kind": kind }
    });
    let Some(path) = path else {
        return Err("no scenario file open".to_string());
    };

    if rustmotion::loader::is_html_path(&path) {
        let result = append_sidecar_annotation(&path, ann.clone());
        let mut m = shared.lock().unwrap_or_else(|e| e.into_inner());
        match result {
            Ok(()) => {
                m.write_error = None;
                m.raw = append_annotation(std::mem::take(&mut m.raw), ann);
                m.generation = m.generation.wrapping_add(1);
                Ok(())
            }
            Err(e) => {
                m.write_error = Some(e.clone());
                m.generation = m.generation.wrapping_add(1);
                Err(e)
            }
        }
    } else {
        let updated = append_annotation(raw, ann);
        let text = serde_json::to_string_pretty(&updated).map_err(|e| format!("json: {e}"))?;
        let snapshot = std::fs::read_to_string(&path).ok();
        let write_result = write_file(&path, &text);
        let mut m = shared.lock().unwrap_or_else(|e| e.into_inner());
        match write_result {
            Ok(()) => {
                if let Some(s) = snapshot {
                    record_edit(&history_slot(), &path, s);
                }
                m.write_error = None;
                Ok(())
            }
            Err(e) => {
                m.write_error = Some(e.clone());
                m.generation = m.generation.wrapping_add(1);
                Err(e)
            }
        }
    }
}

pub fn delete_annotation(shared: &Shared, id: &str) {
    let (path, raw) = {
        let m = shared.lock().unwrap_or_else(|e| e.into_inner());
        (m.path.clone(), m.raw.clone())
    };
    let Some(path) = path else {
        return;
    };

    if rustmotion::loader::is_html_path(&path) {
        let result = remove_sidecar_annotation(&path, id);
        let mut m = shared.lock().unwrap_or_else(|e| e.into_inner());
        match result {
            Ok(()) => {
                m.write_error = None;
                m.raw = remove_annotation(std::mem::take(&mut m.raw), id);
                m.generation = m.generation.wrapping_add(1);
            }
            Err(e) => {
                m.write_error = Some(e);
                m.generation = m.generation.wrapping_add(1);
            }
        }
    } else {
        let updated = remove_annotation(raw, id);
        let snapshot = std::fs::read_to_string(&path).ok();
        let write_result = serde_json::to_string_pretty(&updated)
            .map_err(|e| format!("json: {e}"))
            .and_then(|t| write_file(&path, &t));
        match write_result {
            Ok(()) => {
                if let Some(s) = snapshot {
                    record_edit(&history_slot(), &path, s);
                }
            }
            Err(e) => {
                let mut m = shared.lock().unwrap_or_else(|e2| e2.into_inner());
                m.write_error = Some(e);
                m.generation = m.generation.wrapping_add(1);
            }
        }
    }
}

pub struct AnnotationsPanel {
    shared: Shared,
    editor: Entity<EditorState>,
    annotations: Vec<(String, String, u64, String)>,
}

impl AnnotationsPanel {
    pub fn new(
        shared: Shared,
        editor: Entity<EditorState>,
        annotations: Vec<(String, String, u64, String)>,
    ) -> Self {
        Self {
            shared,
            editor,
            annotations,
        }
    }
}

impl RenderOnce for AnnotationsPanel {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let AnnotationsPanel {
            shared,
            editor,
            annotations,
        } = self;
        let is_empty = annotations.is_empty();

        let cards = annotations.into_iter().map(|(id, note, frame, kind)| {
            annotation_card(id, note, frame, kind, &shared, &editor, cx)
        });

        let panel = v_flex()
            .id("annotations-panel")
            .absolute()
            .left_0()
            .top(TOPBAR_HEIGHT)
            .bottom_0()
            .w(PANEL_WIDTH)
            .bg(cx.theme().sidebar)
            .border_r_1()
            .border_color(cx.theme().border)
            .p_4()
            .gap_2p5()
            .overflow_y_scroll()
            .child(
                div()
                    .font_semibold()
                    .text_color(cx.theme().accent)
                    .child("Comments"),
            )
            .when(is_empty, |this| {
                this.child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child("No comments yet."),
                )
            })
            .children(cards);

        deferred(panel).with_priority(1)
    }
}

fn annotation_card(
    id: String,
    note: String,
    frame: u64,
    kind: String,
    shared: &Shared,
    editor: &Entity<EditorState>,
    cx: &App,
) -> impl IntoElement {
    let goto_editor = editor.clone();
    let delete_shared = shared.clone();
    let delete_editor = editor.clone();
    let delete_id = id.clone();
    let card_id: SharedString = format!("annotation-{id}").into();
    let goto_id: SharedString = format!("annotation-goto-{id}").into();
    let delete_button_id: SharedString = format!("annotation-delete-{id}").into();

    v_flex()
        .id(card_id)
        .gap_1p5()
        .p_2()
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(format!("{kind} \u{b7} frame {frame}")),
        )
        .child(div().text_color(cx.theme().foreground).child(note))
        .child(
            h_flex()
                .gap_2()
                .child(
                    Button::new(goto_id)
                        .label("Go to frame")
                        .ghost()
                        .xsmall()
                        .on_click(move |_, _, cx| {
                            goto_editor.update(cx, |state, cx| {
                                state.current = frame as u32;
                                cx.notify();
                            });
                        }),
                )
                .child(
                    Button::new(delete_button_id)
                        .label("Delete")
                        .danger()
                        .xsmall()
                        .on_click(move |_, _, cx| {
                            delete_annotation(&delete_shared, &delete_id);
                            delete_editor.update(cx, |_, cx| cx.notify());
                        }),
                ),
        )
}

pub struct AnnotationCaptureBox {
    shared: Shared,
    editor: Entity<EditorState>,
    textarea: Entity<TextareaState>,
}

impl AnnotationCaptureBox {
    pub fn new(
        shared: Shared,
        editor: Entity<EditorState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let textarea = cx.new(|cx| {
            TextareaState::new(window, cx)
                .placeholder("Leave a comment for the agent")
                .auto_grow(3, 8)
        });
        Self {
            shared,
            editor,
            textarea,
        }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = self.textarea.read(cx).value().to_string();
        if text.trim().is_empty() {
            return;
        }
        let Some(target) = self.editor.read(cx).selected.clone() else {
            return;
        };
        let frame = self.editor.read(cx).current;
        let submitted = submit_annotation(&self.shared, frame, target.pointer, target.kind, text);
        if submitted.is_ok() {
            self.textarea
                .update(cx, |state, cx| state.set_value("", window, cx));
        }
        self.editor.update(cx, |_, cx| cx.notify());
        cx.notify();
    }
}

impl Render for AnnotationCaptureBox {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let has_target = self.editor.read(cx).selected.is_some();
        let textarea = self.textarea.clone();

        v_flex()
            .id("annotation-capture-box")
            .gap_2()
            .p_4()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child("Leave a comment for the agent"),
            )
            .child(Textarea::new(&textarea).h(px(96.)))
            .child(
                Button::new("submit-annotation")
                    .label("Add comment")
                    .primary()
                    .disabled(!has_target)
                    .on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    use crate::scenario::StudioModel;

    const SCENARIO_JSON: &str = r##"{
        "video": { "width": 10, "height": 10, "fps": 10 },
        "scenes": [ { "duration": 1.0 } ]
    }"##;

    fn temp_path(tag: &str, ext: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("rm_annotations_{tag}_{}.{ext}", std::process::id()))
    }

    fn shared_for_json_source(path: &std::path::Path) -> Shared {
        let scenario =
            rustmotion::loader::load_scenario_from_source(None, Some(SCENARIO_JSON)).unwrap();
        Arc::new(Mutex::new(StudioModel::new(
            scenario,
            None,
            Some(path.to_path_buf()),
        )))
    }

    fn shared_with_detached_path(path: std::path::PathBuf, raw: serde_json::Value) -> Shared {
        let scenario =
            rustmotion::loader::load_scenario_from_source(None, Some(SCENARIO_JSON)).unwrap();
        let shared = Arc::new(Mutex::new(StudioModel::new(scenario, None, None)));
        {
            let mut m = shared.lock().unwrap();
            m.path = Some(path);
            m.raw = raw;
        }
        shared
    }

    #[test]
    fn submit_annotation_rewrites_the_json_file_and_records_history() {
        let path = temp_path("submit_json", "json");
        std::fs::write(&path, SCENARIO_JSON).unwrap();
        let shared = shared_for_json_source(&path);

        let result = submit_annotation(
            &shared,
            0,
            "/scenes/0".to_string(),
            "scene".to_string(),
            "smaller".to_string(),
        );
        assert!(result.is_ok());

        let text = std::fs::read_to_string(&path).unwrap();
        let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
        let annotations = doc["annotations"].as_array().unwrap();
        assert_eq!(annotations.len(), 1);
        assert_eq!(annotations[0]["note"], "smaller");
        assert_eq!(annotations[0]["view"], 0);
        assert_eq!(annotations[0]["scene"], 0);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn submit_annotation_without_a_path_errors() {
        let scenario =
            rustmotion::loader::load_scenario_from_source(None, Some(SCENARIO_JSON)).unwrap();
        let shared = Arc::new(Mutex::new(StudioModel::new(scenario, None, None)));

        let result = submit_annotation(
            &shared,
            0,
            "/scenes/0".to_string(),
            "scene".to_string(),
            "note".to_string(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn submit_annotation_writes_the_sidecar_and_updates_raw_for_html_sources() {
        let path = temp_path("submit_html", "html");
        let sidecar_path = path.with_extension("annotations.json");
        let _ = std::fs::remove_file(&sidecar_path);
        let raw = serde_json::json!({ "video": {}, "scenes": [] });
        let shared = shared_with_detached_path(path.clone(), raw);

        let result = submit_annotation(
            &shared,
            0,
            "/scenes/0".to_string(),
            "scene".to_string(),
            "note".to_string(),
        );
        assert!(result.is_ok());

        let sidecar_text = std::fs::read_to_string(&sidecar_path).unwrap();
        let doc: serde_json::Value = serde_json::from_str(&sidecar_text).unwrap();
        assert_eq!(doc["annotations"].as_array().unwrap().len(), 1);

        let m = shared.lock().unwrap();
        assert_eq!(m.raw["annotations"].as_array().unwrap().len(), 1);
        drop(m);

        let _ = std::fs::remove_file(&sidecar_path);
    }

    #[test]
    fn delete_annotation_removes_it_from_the_json_file() {
        let path = temp_path("delete_json", "json");
        let with_annotation = serde_json::json!({
            "video": { "width": 10, "height": 10, "fps": 10 },
            "scenes": [ { "duration": 1.0 } ],
            "annotations": [ { "id": "an_1", "note": "n", "status": "open", "frame": 0,
                "target": { "pointer": "/scenes/0", "kind": "scene" } } ]
        });
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&with_annotation).unwrap(),
        )
        .unwrap();
        let shared = shared_for_json_source(&path);

        delete_annotation(&shared, "an_1");

        let text = std::fs::read_to_string(&path).unwrap();
        let doc: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert!(doc["annotations"].as_array().unwrap().is_empty());

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn delete_annotation_removes_the_sidecar_once_it_is_empty_and_updates_raw() {
        let path = temp_path("delete_html", "html");
        let sidecar_path = path.with_extension("annotations.json");
        let ann = serde_json::json!({ "id": "an_1", "note": "n", "status": "open", "frame": 0,
            "target": { "pointer": "/scenes/0", "kind": "scene" } });
        std::fs::write(
            &sidecar_path,
            serde_json::to_string_pretty(&serde_json::json!({ "annotations": [ann.clone()] }))
                .unwrap(),
        )
        .unwrap();
        let raw = serde_json::json!({ "video": {}, "scenes": [], "annotations": [ann] });
        let shared = shared_with_detached_path(path.clone(), raw);

        delete_annotation(&shared, "an_1");

        assert!(!sidecar_path.exists());
        let m = shared.lock().unwrap();
        assert!(m.raw["annotations"].as_array().unwrap().is_empty());
    }

    #[test]
    fn annotation_id_produces_distinct_values() {
        let a = annotation_id();
        let b = annotation_id();
        assert_ne!(a, b);
        assert!(a.starts_with("an_"));
    }
}
