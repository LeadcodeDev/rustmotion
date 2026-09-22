use std::time::Duration;

use gpui_kit::{Context, Window};

use crate::editor::properties::PropKind;
use crate::scenario::{
    apply_optimistic, history_slot, note_self_write, pending_write_slot, queue_mutation,
    resolve_flush, self_write_slot, set_saving, take_pending, Mutation,
};

use super::InspectorPanel;

#[derive(Clone)]
pub enum Target {
    Style(String),
    Root(String, PropKind),
    Nested(String, String, PropKind),
}

pub fn parse_root_value(kind: &PropKind, text: &str) -> Result<serde_json::Value, ()> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(serde_json::Value::Null);
    }
    Ok(match kind {
        PropKind::Integer => match t.parse::<i64>() {
            Ok(i) => serde_json::Value::from(i),
            Err(_) => {
                let f: f64 = t.parse().map_err(|_| ())?;
                if f.fract() == 0.0 && f.abs() < 9_007_199_254_740_992.0 {
                    serde_json::Value::from(f as i64)
                } else {
                    return Err(());
                }
            }
        },
        PropKind::Float => {
            let f: f64 = t.parse().map_err(|_| ())?;
            if f.fract() == 0.0 && f.abs() < 9_007_199_254_740_992.0 {
                serde_json::Value::from(f as i64)
            } else {
                serde_json::Value::from(f)
            }
        }
        PropKind::Bool => serde_json::Value::Bool(t == "true"),
        PropKind::Complex => serde_json::from_str(t).map_err(|_| ())?,
        _ => serde_json::Value::String(text.to_string()),
    })
}

pub fn mutate_nested(
    root_value: &serde_json::Value,
    key: &str,
    leaf: serde_json::Value,
) -> serde_json::Value {
    crate::editor::properties::mutate_object_field(root_value, key, leaf)
}

impl InspectorPanel {
    pub(super) fn commit_text(
        &mut self,
        target: &Target,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match target {
            Target::Style(name) => {
                if text.trim().is_empty() {
                    self.write_style_removal(name, window, cx);
                } else {
                    self.write_prop(name, text, window, cx);
                }
            }
            Target::Root(name, kind) => {
                if let Ok(v) = parse_root_value(kind, text) {
                    self.write_root_field(name, v, window, cx);
                }
            }
            Target::Nested(root, key, kind) => {
                if let Ok(v) = parse_root_value(kind, text) {
                    self.write_nested_field(root, key, v, window, cx);
                }
            }
        }
    }

    pub(super) fn write_prop(
        &mut self,
        prop: &str,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if value.trim().is_empty() {
            return;
        }
        let Some(pointer) = self.selection.as_ref().map(|s| s.pointer.clone()) else {
            return;
        };
        let mutation = Mutation::Style {
            pointer,
            prop: prop.to_string(),
            value: serde_json::Value::String(value.to_string()),
        };
        self.optimistic(&mutation, cx);
        self.schedule_write(mutation, window, cx);
    }

    pub(super) fn write_style_removal(
        &mut self,
        prop: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pointer) = self.selection.as_ref().map(|s| s.pointer.clone()) else {
            return;
        };
        let mutation = Mutation::Style {
            pointer,
            prop: prop.to_string(),
            value: serde_json::Value::Null,
        };
        self.optimistic(&mutation, cx);
        self.schedule_write(mutation, window, cx);
    }

    pub(super) fn write_root_field(
        &mut self,
        field: &str,
        value: serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pointer) = self.selection.as_ref().map(|s| s.pointer.clone()) else {
            return;
        };
        let mutation = Mutation::Field {
            pointer,
            field: field.to_string(),
            value,
        };
        self.optimistic(&mutation, cx);
        self.schedule_write(mutation, window, cx);
    }

    pub(super) fn write_content(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.write_root_field(
            "content",
            serde_json::Value::String(text.to_string()),
            window,
            cx,
        );
    }

    pub(super) fn write_nested_field(
        &mut self,
        root: &str,
        key: &str,
        leaf: serde_json::Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pointer) = self.selection.as_ref().map(|s| s.pointer.clone()) else {
            return;
        };
        let current_root = {
            let m = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            m.raw
                .pointer(&pointer)
                .and_then(|el| el.get(root))
                .cloned()
                .unwrap_or(serde_json::Value::Null)
        };
        let next = mutate_nested(&current_root, key, leaf);
        self.write_root_field(root, next, window, cx);
    }

    fn optimistic(&mut self, mutation: &Mutation, cx: &mut Context<Self>) {
        if apply_optimistic(&self.shared, mutation).is_ok() {
            self.editor.update(cx, |state, cx| {
                state.rev = state.rev.wrapping_add(1);
                cx.notify();
            });
        }
    }

    fn schedule_write(&mut self, mutation: Mutation, _window: &mut Window, cx: &mut Context<Self>) {
        let path = {
            let m = self.shared.lock().unwrap_or_else(|e| e.into_inner());
            m.path.clone()
        };
        let Some(path) = path else {
            return;
        };
        queue_mutation(&pending_write_slot(), &path, mutation);
        set_saving(&history_slot(), true);

        let shared = self.shared.clone();
        self.pending_write = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(250))
                .await;

            let mutations = take_pending(&pending_write_slot(), &path);
            set_saving(&history_slot(), false);
            if mutations.is_empty() {
                return;
            }

            let is_html = rustmotion::loader::is_html_path(&path);
            let read = std::fs::read_to_string(&path).map_err(|e| format!("read: {e}"));
            let result: Result<bool, String> = match &read {
                Ok(content) => match resolve_flush(content, is_html, &mutations) {
                    Ok(Some(new_content)) => write_and_note(&path, &new_content).map(|()| true),
                    Ok(None) => Ok(false),
                    Err(e) => Err(e),
                },
                Err(e) => Err(e.clone()),
            };
            if let (Ok(true), Ok(snapshot)) = (&result, &read) {
                crate::scenario::record_edit(&history_slot(), &path, snapshot.clone());
            }

            let mut m = shared.lock().unwrap_or_else(|e| e.into_inner());
            match result {
                Ok(_) => m.write_error = None,
                Err(e) => {
                    m.write_error = Some(e);
                    m.generation = m.generation.wrapping_add(1);
                }
            }
            drop(m);
            let _ = this.update(cx, |_, cx| cx.notify());
        }));
    }
}

fn write_and_note(path: &std::path::Path, content: &str) -> Result<(), String> {
    std::fs::write(path, content).map_err(|e| format!("write: {e}"))?;
    note_self_write(&self_write_slot(), path, content);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_root_value_never_writes_a_float_into_an_integer_field() {
        assert_eq!(
            parse_root_value(&PropKind::Integer, "12"),
            Ok(serde_json::Value::from(12))
        );
        assert_eq!(
            parse_root_value(&PropKind::Integer, "12.0"),
            Ok(serde_json::Value::from(12))
        );
        assert!(parse_root_value(&PropKind::Integer, "12.5").is_err());
    }

    #[test]
    fn parse_root_value_empty_text_removes_the_field() {
        assert_eq!(
            parse_root_value(&PropKind::String, ""),
            Ok(serde_json::Value::Null)
        );
        assert_eq!(
            parse_root_value(&PropKind::Integer, "   "),
            Ok(serde_json::Value::Null)
        );
    }

    #[test]
    fn parse_root_value_complex_requires_valid_json() {
        assert!(parse_root_value(&PropKind::Complex, "{not json").is_err());
        assert_eq!(
            parse_root_value(&PropKind::Complex, "{\"a\":1}"),
            Ok(serde_json::json!({"a": 1}))
        );
    }

    #[test]
    fn mutate_nested_removes_on_null() {
        let root = serde_json::json!({"direction": "up", "value": "1"});
        let next = mutate_nested(&root, "direction", serde_json::Value::Null);
        assert_eq!(next, serde_json::json!({"value": "1"}));
    }
}
