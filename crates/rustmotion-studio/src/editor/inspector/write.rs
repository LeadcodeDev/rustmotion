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
    Style(String, PropKind),
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

pub fn style_mutation(pointer: &str, prop: &str, kind: &PropKind, text: &str) -> Option<Mutation> {
    let value = parse_root_value(kind, text).ok()?;
    Some(Mutation::Style {
        pointer: pointer.to_string(),
        prop: prop.to_string(),
        value,
    })
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
            Target::Style(name, kind) => {
                let Some(pointer) = self.selection.as_ref().map(|s| s.pointer.clone()) else {
                    return;
                };
                let Some(mutation) = style_mutation(&pointer, name, kind, text) else {
                    return;
                };
                self.apply_and_write(mutation, window, cx);
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
        self.apply_and_write(mutation, window, cx);
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

    fn apply_and_write(&mut self, mutation: Mutation, window: &mut Window, cx: &mut Context<Self>) {
        if self.optimistic(&mutation, cx) {
            self.schedule_write(mutation, window, cx);
        }
    }

    fn optimistic(&mut self, mutation: &Mutation, cx: &mut Context<Self>) -> bool {
        let applied = apply_optimistic(&self.shared, mutation).is_ok();
        if applied {
            self.editor.update(cx, |state, cx| {
                state.rev = state.rev.wrapping_add(1);
                cx.notify();
            });
        }
        applied
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
                    Ok(Some(new_content)) => validate_before_write(&path, &new_content)
                        .and_then(|()| write_and_note(&path, &new_content).map(|()| true)),
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

pub fn validate_before_write(path: &std::path::Path, content: &str) -> Result<(), String> {
    let is_html = rustmotion::loader::is_html_path(path);
    let raw: serde_json::Value = if is_html {
        let transpiled = rustmotion::loader::html_to_scenario_json(content)
            .map_err(|e| format!("transpile: {e}"))?;
        let annotations = crate::scenario::read_sidecar(path).unwrap_or_default();
        crate::scenario::merge_annotations(transpiled, annotations)
    } else {
        serde_json::from_str(content).map_err(|e| format!("parse: {e}"))?
    };
    let scenario = crate::scenario::resolve_for_render(&raw, Some(path))?;
    let loaded = rustmotion::cli::validation::LoadedScenario {
        raw,
        scenario,
        source_path: Some(path.to_path_buf()),
    };
    let report = rustmotion::cli::validation::run_checks(&loaded, false);
    if report.is_blocking(false) {
        return Err(format_validation_report(&report));
    }
    Ok(())
}

fn format_validation_report(report: &rustmotion::cli::validation::ValidationReport) -> String {
    let mut lines: Vec<String> = report
        .schema_errors
        .iter()
        .map(|e| format!("schema: {e}"))
        .collect();
    for v in &report.geom_violations {
        lines.push(format!(
            "geometry: {:?} on {} at {} (view {}, scene {}) — {}",
            v.kind, v.component, v.path, v.view_index, v.scene_index, v.hint
        ));
    }
    lines.join("\n")
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

    #[test]
    fn style_mutation_types_a_float_prop_as_a_number_not_a_string() {
        let m = style_mutation("/scenes/0/children/0", "opacity", &PropKind::Float, "0.5")
            .expect("a valid opacity value must produce a mutation");
        let Mutation::Style {
            pointer,
            prop,
            value,
        } = m
        else {
            panic!("expected a Style mutation");
        };
        assert_eq!(pointer, "/scenes/0/children/0");
        assert_eq!(prop, "opacity");
        assert_eq!(
            value,
            serde_json::json!(0.5),
            "opacity is CssStyle::opacity: Option<f32> — a JSON string here fails schema validation \
             and the whole component is dropped from the render"
        );
    }

    #[test]
    fn style_mutation_types_an_integer_prop_as_a_number_not_a_string() {
        let m = style_mutation("/p", "z-index", &PropKind::Integer, "3")
            .expect("a valid z-index value must produce a mutation");
        let Mutation::Style { value, .. } = m else {
            panic!("expected a Style mutation");
        };
        assert_eq!(value, serde_json::json!(3));
    }

    #[test]
    fn style_mutation_empty_text_removes_the_prop() {
        let m = style_mutation("/p", "opacity", &PropKind::Float, "")
            .expect("empty text must still produce a removal mutation");
        let Mutation::Style { value, .. } = m else {
            panic!("expected a Style mutation");
        };
        assert_eq!(value, serde_json::Value::Null);
    }

    #[test]
    fn style_mutation_rejects_unparseable_text_instead_of_writing_a_bad_string() {
        assert!(style_mutation("/p", "z-index", &PropKind::Integer, "not-a-number").is_none());
    }

    fn scratch_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rm_validate_before_write_{tag}_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_viewport_overflowing_edit_is_rejected_before_it_reaches_disk() {
        let dir = scratch_dir("overflow");
        let path = dir.join("scenario.json");
        let overflowing = r##"{ "video": { "width": 200, "height": 200 },
            "scenes": [ { "duration": 1.0, "children": [
                { "type": "card", "style": { "position": "absolute", "top": "5000px", "left": "0px", "width": "50px", "height": "50px", "background": "#ffffff" } }
            ] } ] }"##;
        std::fs::write(&path, overflowing).unwrap();

        let err = validate_before_write(&path, overflowing).expect_err(
            "a card placed 5000px below a 200px-tall viewport must fail geometry validation, \
             the same way `rustmotion validate` would refuse this file",
        );
        assert!(
            err.to_lowercase().contains("viewport") || err.to_lowercase().contains("geometry"),
            "the message must name the violation and its position, not just say \"invalid\": {err}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn validate_before_write_accepts_an_ordinary_edit() {
        let dir = scratch_dir("ok");
        let path = dir.join("scenario.json");
        let ok = r##"{ "video": { "width": 640, "height": 360 },
            "scenes": [ { "duration": 1.0, "children": [
                { "type": "text", "content": "Hi", "style": { "color": "#ffffff", "font-size": 32 } }
            ] } ] }"##;
        std::fs::write(&path, ok).unwrap();

        assert_eq!(
            validate_before_write(&path, ok),
            Ok(()),
            "an ordinary, valid edit must never be blocked"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn validate_before_write_on_the_promo_example_stays_fast_enough_for_the_debounce() {
        let promo = std::path::Path::new("../../examples/rustmotion-promo.json");
        if !promo.exists() {
            eprintln!("skipped: examples/rustmotion-promo.json missing");
            return;
        }
        let content = std::fs::read_to_string(promo).unwrap();
        let start = std::time::Instant::now();
        let result = validate_before_write(promo, &content);
        let elapsed = start.elapsed();
        eprintln!(
            "validate_before_write(rustmotion-promo.json) took {elapsed:?} (result: {result:?})"
        );
        assert!(
            elapsed.as_millis() < 2000,
            "run_checks's geometry pass runs once per 250ms debounced write, not per keystroke; \
             it must stay well under that window on a realistic scenario or the debounce stops \
             hiding it from typing — took {elapsed:?}. Whether the example is currently valid is \
             not this test's concern (other work in this checkout can change that): only the \
             cost of running the check is measured here."
        );
    }

    #[test]
    fn validate_before_write_resolves_a_relative_include_against_the_scenario_directory() {
        let dir = scratch_dir("include");
        std::fs::write(
            dir.join("included.json"),
            r##"{ "video": { "width": 640, "height": 360 },
                "scenes": [ { "duration": 1.0, "children": [
                    { "type": "text", "content": "Included", "style": { "color": "#ffffff" } }
                ] } ] }"##,
        )
        .unwrap();
        let path = dir.join("scenario.json");
        let source = r##"{ "video": { "width": 640, "height": 360 },
            "scenes": [ { "include": "included.json" } ] }"##;
        std::fs::write(&path, source).unwrap();

        assert_eq!(
            validate_before_write(&path, source),
            Ok(()),
            "a relative `include:` must resolve against the scenario file's own directory, \
             the same way `resolve_for_render` already does for rendering — not be rejected \
             the way `ValidationSource::Inline` would reject it"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn validate_before_write_rejects_a_wrongly_typed_style_value_without_a_separate_component_check(
    ) {
        let dir = scratch_dir("wrong_type");
        let path = dir.join("scenario.json");
        let bad = r##"{ "video": { "width": 640, "height": 360 },
            "scenes": [ { "duration": 1.0, "children": [
                { "type": "text", "content": "hi", "style": { "opacity": "0.5", "color": "#fff" } }
            ] } ] }"##;
        std::fs::write(&path, bad).unwrap();

        let err = validate_before_write(&path, bad).expect_err(
            "opacity is CssStyle::opacity: Option<f32> — validate_attrs::check_component_attrs \
             (folded unconditionally into run_checks's schema_errors) already re-deserializes \
             every child as a ChildComponent and blocks on exactly this, so validate_before_write \
             needs no separate component-level check of its own to catch it",
        );
        assert!(
            err.contains("invalid component") || err.contains("invalid type"),
            "error should name the offending child and the type mismatch: {err}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
