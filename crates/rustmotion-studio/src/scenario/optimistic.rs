//! Optimistic in-memory edits: apply every edit event to the live model
//! immediately (rebuild scenario + tasks from memory, bump generation) so the
//! canvas refreshes in ~one render, while the DISK keeps the existing
//! debounced write path untouched (250 ms, history/undo, write_error).
//!
//! Also owns the self-write ledger: the debounced writer and undo/redo record
//! a hash of what they wrote; the watcher skips reloads whose disk content
//! matches the last self-write (the in-memory model is already up to date —
//! and possibly NEWER under continuous typing, so the skip is a correctness
//! fix, not just an optimization). External edits (agent, editor) hash
//! differently and reload normally.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::Value;

use rustmotion::schema::ResolvedScenario;

use super::{set_field_value, set_style_value, Shared};

/// One in-memory edit, mirroring the debounced write payloads.
/// `Value::Null` removes the property / field.
#[derive(Debug, Clone)]
pub enum Mutation {
    Style {
        pointer: String,
        prop: String,
        value: Value,
    },
    Field {
        pointer: String,
        field: String,
        value: Value,
    },
}

/// Apply a mutation to the in-memory model: mutate the raw (JSON) or the
/// in-memory HTML source (retranspiled), rebuild scenario/tasks (fresh Arcs —
/// the prefetcher follows), bump generation. On rebuild failure (transiently
/// invalid edit, e.g. mid-typing in a JSON area) the model is left UNTOUCHED
/// and no write_error is raised — the disk is only ever written by the
/// debounced path, which has its own guards.
pub fn apply_optimistic(shared: &Shared, mutation: &Mutation) -> Result<(), String> {
    let mut m = shared.lock().unwrap_or_else(|e| e.into_inner());
    let Some(path) = m.path.clone() else {
        return Err("no open file".into());
    };

    if rustmotion::loader::is_html_path(&path) {
        // HTML: mutate the in-memory source, retranspile, rebuild.
        let source = match &m.html_source {
            Some(s) => s.clone(),
            None => std::fs::read_to_string(&path).map_err(|e| format!("read: {e}"))?,
        };
        let new_source = apply_to_html(&source, mutation).ok_or("mutation didn't apply")?;
        let transpiled = rustmotion::loader::html_to_scenario_json(&new_source)
            .map_err(|e| format!("transpile: {e}"))?;
        let annotations = super::sidecar::read_sidecar(&path).unwrap_or_default();
        let new_raw = super::sidecar::merge_annotations(transpiled, annotations);
        let scenario = rebuild_from_value(&new_raw)?;
        commit(&mut m, scenario, new_raw, Some(new_source));
    } else {
        // JSON: mutate the raw value, rebuild.
        let new_raw = apply_to_raw(m.raw.clone(), mutation).ok_or("mutation didn't apply")?;
        let scenario = rebuild_from_value(&new_raw)?;
        commit(&mut m, scenario, new_raw, None);
    }
    Ok(())
}

/// Adopt a full source text as the new in-memory state (undo/redo: they
/// rewrite the disk themselves and the watcher skips the self-write, so the
/// memory must be updated here). Rebuilds like `apply_optimistic`.
pub fn adopt_source(shared: &Shared, path: &Path, source: &str) -> Result<(), String> {
    let mut m = shared.lock().unwrap_or_else(|e| e.into_inner());
    if rustmotion::loader::is_html_path(path) {
        let transpiled = rustmotion::loader::html_to_scenario_json(source)
            .map_err(|e| format!("transpile: {e}"))?;
        let annotations = super::sidecar::read_sidecar(path).unwrap_or_default();
        let new_raw = super::sidecar::merge_annotations(transpiled, annotations);
        let scenario = rebuild_from_value(&new_raw)?;
        commit(&mut m, scenario, new_raw, Some(source.to_string()));
    } else {
        let new_raw: Value = serde_json::from_str(source).map_err(|e| format!("parse: {e}"))?;
        let scenario = rebuild_from_value(&new_raw)?;
        commit(&mut m, scenario, new_raw, None);
    }
    Ok(())
}

/// Swap the rebuilt state into the model: fresh Arcs (the prefetcher follows),
/// new totals, generation bump.
fn commit(
    m: &mut super::StudioModel,
    scenario: ResolvedScenario,
    raw: Value,
    html_source: Option<String>,
) {
    let tasks = rustmotion::encode::build_frame_tasks(&scenario);
    m.total_frames = tasks.len() as u32;
    m.scenario = Arc::new(scenario);
    m.tasks = Arc::new(tasks);
    m.raw = raw;
    if html_source.is_some() {
        m.html_source = html_source;
    }
    m.generation = m.generation.wrapping_add(1);
}

/// Apply a mutation to a raw JSON document (pure).
fn apply_to_raw(raw: Value, mutation: &Mutation) -> Option<Value> {
    match mutation {
        Mutation::Style {
            pointer,
            prop,
            value,
        } => set_style_value(raw, pointer, prop, value.clone()),
        Mutation::Field {
            pointer,
            field,
            value,
        } => set_field_value(raw, pointer, field, value.clone()),
    }
}

/// Apply a mutation to an HTML source string (pure). `content` maps to the
/// element's text node; other fields are attributes.
fn apply_to_html(source: &str, mutation: &Mutation) -> Option<String> {
    match mutation {
        Mutation::Style {
            pointer,
            prop,
            value,
        } => match value {
            Value::Null => rustmotion::loader::remove_html_inline_style(source, pointer, prop),
            Value::String(s) => rustmotion::loader::set_html_inline_style(source, pointer, prop, s),
            other => {
                rustmotion::loader::set_html_inline_style(source, pointer, prop, &other.to_string())
            }
        },
        Mutation::Field {
            pointer,
            field,
            value,
        } => {
            if field == "content" {
                let text = match value {
                    Value::String(s) => s.clone(),
                    Value::Null => String::new(),
                    other => other.to_string(),
                };
                rustmotion::loader::set_html_text_content(source, pointer, &text)
            } else {
                let attr = match value {
                    Value::Null => String::new(),
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                rustmotion::loader::set_html_attribute(source, pointer, field, &attr)
            }
        }
    }
}

// ── Self-write ledger ────────────────────────────────────────────────────────

pub type SelfWrites = Arc<Mutex<HashMap<PathBuf, u64>>>;

/// App-global ledger: path → hash of the last content this process wrote.
pub fn self_write_slot() -> SelfWrites {
    static SLOT: OnceLock<SelfWrites> = OnceLock::new();
    SLOT.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
        .clone()
}

fn content_hash(content: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut h);
    h.finish()
}

/// Record that this process wrote `content` to `path`.
pub fn note_self_write(slot: &SelfWrites, path: &Path, content: &str) {
    let mut map = slot.lock().unwrap_or_else(|e| e.into_inner());
    map.insert(path.to_path_buf(), content_hash(content));
}

/// Forget the note for `path` (adoption failed → let the watcher reload).
pub fn clear_self_write(slot: &SelfWrites, path: &Path) {
    let mut map = slot.lock().unwrap_or_else(|e| e.into_inner());
    map.remove(path);
}

/// Whether `content` on disk is exactly the last self-write for `path`
/// (watcher: true → skip the reload).
pub fn is_self_write(slot: &SelfWrites, path: &Path, content: &str) -> bool {
    let map = slot.lock().unwrap_or_else(|e| e.into_inner());
    map.get(path) == Some(&content_hash(content))
}

// ── Rebuild ──────────────────────────────────────────────────────────────────

/// Build a `ResolvedScenario` from a raw scenario JSON value — the same
/// pipeline as the loader (variable defaults + include resolution; includes
/// resolve as Inline, so file-relative includes are a known limitation shared
/// with the diff baseline render).
fn rebuild_from_value(raw: &Value) -> Result<ResolvedScenario, String> {
    let json = serde_json::to_string(raw).map_err(|e| format!("serialize: {e}"))?;
    rustmotion::loader::load_scenario_from_source(None, Some(&json)).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::{empty_scenario, StudioModel};
    use serde_json::json;

    fn temp_json(tag: &str, content: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("rm_opt_{tag}_{}.json", std::process::id()));
        std::fs::write(&p, content).unwrap();
        p
    }

    fn model_for(path: &Path) -> Shared {
        Arc::new(Mutex::new(StudioModel::new(
            empty_scenario(),
            None,
            Some(path.to_path_buf()),
        )))
    }

    const DOC: &str = r##"{ "video": { "width": 640, "height": 360, "background": "#101418" },
        "scenes": [ { "duration": 1.0, "children": [
            { "type": "text", "content": "Hi", "style": { "font-size": 48 } }
        ] } ] }"##;

    // ── Integration: full chart-colors write path (user bug repro) ──────

    /// End-to-end repro of "editing chart colors does nothing": promo file →
    /// optimistic Field mutation with the 8-color palette (the prefill
    /// write), second mutation turning colors[0] red, rebuild, pixel check on
    /// the chart's scene. Proves the DATA path (pointer → set_field_value →
    /// rebuild → painter `get_color`) end to end.
    #[test]
    fn chart_colors_edit_reaches_the_rendered_pixels() {
        let promo = std::path::Path::new("../../examples/rustmotion-promo.json");
        if !promo.exists() {
            panic!("examples/rustmotion-promo.json missing");
        }
        // Real open path: typed scenario loaded from the file (model_for uses
        // an empty scenario and would have no frames before the first edit).
        let loaded = rustmotion::loader::load_input(&promo.to_path_buf()).expect("promo loads");
        let shared: Shared = Arc::new(Mutex::new(StudioModel::new(
            loaded,
            None,
            Some(promo.to_path_buf()),
        )));

        // Find the first chart component in the raw (nested scene→div→card→chart).
        fn find_chart(node: &Value, ptr: String, out: &mut Option<String>) {
            if out.is_some() {
                return;
            }
            if node.get("type").and_then(|t| t.as_str()) == Some("chart") {
                *out = Some(ptr);
                return;
            }
            if let Some(children) = node.get("children").and_then(|c| c.as_array()) {
                for (i, c) in children.iter().enumerate() {
                    find_chart(c, format!("{ptr}/children/{i}"), out);
                }
            }
        }
        let (pointer, scene_idx) = {
            let m = shared.lock().unwrap();
            let scenes = m.raw["scenes"].as_array().expect("promo scenes").clone();
            let mut found = None;
            let mut scene_idx = 0usize;
            for (si, scene) in scenes.iter().enumerate() {
                let mut ptr = None;
                find_chart(scene, format!("/scenes/{si}"), &mut ptr);
                if let Some(p) = ptr {
                    found = Some(p);
                    scene_idx = si;
                    break;
                }
            }
            (found.expect("promo contains a chart"), scene_idx)
        };

        let render_scene = |shared: &Shared| -> Vec<u8> {
            let m = shared.lock().unwrap();
            let base = m
                .tasks
                .iter()
                .position(|t| {
                    matches!(t, rustmotion::encode::video::FrameTask::Normal { scene_idx: s, .. }
                        if *s == scene_idx)
                })
                .expect("scene has frames");
            // Mid-scene so staggered entrances have landed.
            let idx = (base + 45).min(m.tasks.len() - 1);
            rustmotion::encode::render_frame_task_scaled(
                &m.scenario.video,
                &m.scenario,
                &m.tasks[idx],
                0.25,
            )
            .expect("render")
        };
        let count_red = |rgba: &[u8]| {
            rgba.chunks_exact(4)
                .filter(|p| p[0] > 180 && p[1] < 90 && p[2] < 90)
                .count()
        };

        let before = render_scene(&shared);

        // Edit 1: the prefill write (exactly what "+ Add color" commits).
        let palette: Vec<Value> = rustmotion::components::chart::DEFAULT_PALETTE
            .iter()
            .map(|c| Value::String(c.to_string()))
            .collect();
        apply_optimistic(
            &shared,
            &Mutation::Field {
                pointer: pointer.clone(),
                field: "colors".into(),
                value: Value::Array(palette.clone()),
            },
        )
        .expect("palette write applies");
        // Identical palette → the canvas must NOT change (the UX trap).
        let after_prefill = render_scene(&shared);
        assert_eq!(
            count_red(&before),
            count_red(&after_prefill),
            "prefill palette renders identically by design"
        );

        // Edit 2: the user picks red for the first series.
        let mut reddened = palette;
        reddened[0] = Value::String("#FF0000".into());
        apply_optimistic(
            &shared,
            &Mutation::Field {
                pointer: pointer.clone(),
                field: "colors".into(),
                value: Value::Array(reddened),
            },
        )
        .expect("red write applies");
        {
            let m = shared.lock().unwrap();
            assert_eq!(
                m.raw.pointer(&pointer).unwrap()["colors"][0],
                serde_json::json!("#FF0000")
            );
        }
        let after_red = render_scene(&shared);
        // At 0.25 scale the red series line is thin — a clear nonzero jump
        // is the signal (measured ~14 px; before: 0).
        assert!(
            count_red(&after_red) >= count_red(&before) + 10,
            "chart must actually turn red: before={} after={}",
            count_red(&before),
            count_red(&after_red)
        );
    }

    // ── Self-write skip decision ────────────────────────────────────────

    #[test]
    fn self_write_skip_decision() {
        let slot: SelfWrites = Arc::new(Mutex::new(HashMap::new()));
        let a = Path::new("/w/a.json");
        let b = Path::new("/w/b.json");
        // Nothing recorded → reload (not a self-write).
        assert!(!is_self_write(&slot, a, "content"));
        note_self_write(&slot, a, "content");
        // Identical content → skip.
        assert!(is_self_write(&slot, a, "content"));
        // Different content (external edit) → reload.
        assert!(!is_self_write(&slot, a, "external change"));
        // Same content on a DIFFERENT path → reload.
        assert!(!is_self_write(&slot, b, "content"));
        // Cleared → reload again.
        note_self_write(&slot, a, "content");
        clear_self_write(&slot, a);
        assert!(!is_self_write(&slot, a, "content"));
    }

    // ── Optimistic rebuild (JSON) ───────────────────────────────────────

    #[test]
    fn optimistic_style_mutation_rebuilds_the_scenario() {
        let path = temp_json("style", DOC);
        let shared = model_for(&path);
        let gen_before = shared.lock().unwrap().generation;

        let m = Mutation::Style {
            pointer: "/scenes/0/children/0".into(),
            prop: "font-size".into(),
            value: json!(64),
        };
        apply_optimistic(&shared, &m).expect("valid mutation applies");

        let model = shared.lock().unwrap();
        assert_eq!(
            model.raw["scenes"][0]["children"][0]["style"]["font-size"],
            json!(64)
        );
        // The rebuilt scenario carries the new value too (children are raw values).
        assert_eq!(
            model.scenario.views[0].scenes[0].children[0]["style"]["font-size"],
            json!(64)
        );
        assert!(model.generation > gen_before, "generation bumped");
        assert!(model.total_frames > 0);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn invalid_optimistic_mutation_leaves_the_model_untouched() {
        let path = temp_json("invalid", DOC);
        let shared = model_for(&path);
        let (gen_before, raw_before) = {
            let m = shared.lock().unwrap();
            (m.generation, m.raw.clone())
        };

        // width: "abc" breaks the typed Scenario parse → rebuild fails.
        let m = Mutation::Field {
            pointer: "/video".into(),
            field: "width".into(),
            value: json!("abc"),
        };
        assert!(apply_optimistic(&shared, &m).is_err());

        let model = shared.lock().unwrap();
        assert_eq!(model.generation, gen_before, "no bump on failure");
        assert_eq!(model.raw, raw_before, "raw untouched on failure");
        let _ = std::fs::remove_file(&path);
    }

    // ── Optimistic rebuild (HTML) ───────────────────────────────────────

    #[test]
    fn optimistic_html_mutation_retranspiles_in_memory() {
        let p = std::env::temp_dir().join(format!("rm_opt_html_{}.html", std::process::id()));
        std::fs::write(
            &p,
            r##"<rustmotion width="640" height="360"><scene duration="1"><rm-counter from="0" to="10"></rm-counter></scene></rustmotion>"##,
        )
        .unwrap();
        let shared = model_for(&p);

        let m = Mutation::Field {
            pointer: "/scenes/0/children/0".into(),
            field: "from".into(),
            value: json!(250),
        };
        apply_optimistic(&shared, &m).expect("html mutation applies");

        let model = shared.lock().unwrap();
        // Retranspiled + re-typed by the transpiler's coercion.
        assert_eq!(model.raw["scenes"][0]["children"][0]["from"], json!(250));
        assert!(
            model
                .html_source
                .as_deref()
                .is_some_and(|s| s.contains("from=\"250\"")),
            "in-memory HTML source updated"
        );
        let _ = std::fs::remove_file(&p);
    }

    // ── Undo + self-write flow ──────────────────────────────────────────

    #[test]
    fn undo_notes_self_write_and_adopts_in_memory() {
        let before = DOC;
        let after = DOC.replace("48", "72");
        let path = temp_json("undo_flow", &after);
        let shared = model_for(&path);
        let hist: crate::scenario::SharedHistory = Arc::new(Mutex::new(Default::default()));
        crate::scenario::record_edit(&hist, &path, before.to_string());

        crate::scenario::undo(&shared, &hist);

        // Disk restored…
        let disk = std::fs::read_to_string(&path).unwrap();
        assert_eq!(disk, before);
        // …the write is recorded as a self-write (watcher will skip it)…
        assert!(is_self_write(&self_write_slot(), &path, &disk));
        // …and the memory adopted the restored state itself.
        let model = shared.lock().unwrap();
        assert_eq!(
            model.raw["scenes"][0]["children"][0]["style"]["font-size"],
            json!(48)
        );
        let _ = std::fs::remove_file(&path);
    }
}
