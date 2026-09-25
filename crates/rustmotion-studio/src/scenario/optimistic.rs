use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use serde_json::Value;

use rustmotion::schema::ResolvedScenario;

use super::{set_field_value, set_style_value, Shared};

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

pub fn apply_optimistic(shared: &Shared, mutation: &Mutation) -> Result<(), String> {
    let mut m = shared.lock().unwrap_or_else(|e| e.into_inner());
    let Some(path) = m.path.clone() else {
        return Err("no open file".into());
    };

    if rustmotion::loader::is_html_path(&path) {
        let source = match &m.html_source {
            Some(s) => s.clone(),
            None => std::fs::read_to_string(&path).map_err(|e| format!("read: {e}"))?,
        };
        let new_source = apply_to_html(&source, mutation).ok_or("mutation didn't apply")?;
        let transpiled = rustmotion::loader::html_to_scenario_json(&new_source)
            .map_err(|e| format!("transpile: {e}"))?;
        let annotations = super::sidecar::read_sidecar(&path).unwrap_or_default();
        let new_raw = super::sidecar::merge_annotations(transpiled, annotations);
        let scenario = resolve_for_render(&new_raw, Some(&path))?;
        commit(&mut m, scenario, new_raw, Some(new_source));
    } else {
        let new_raw = apply_to_raw(m.raw.clone(), mutation).ok_or("mutation didn't apply")?;
        let scenario = resolve_for_render(&new_raw, Some(&path))?;
        commit(&mut m, scenario, new_raw, None);
    }
    Ok(())
}

pub fn adopt_source(shared: &Shared, path: &Path, source: &str) -> Result<(), String> {
    let mut m = shared.lock().unwrap_or_else(|e| e.into_inner());
    if rustmotion::loader::is_html_path(path) {
        let transpiled = rustmotion::loader::html_to_scenario_json(source)
            .map_err(|e| format!("transpile: {e}"))?;
        let annotations = super::sidecar::read_sidecar(path).unwrap_or_default();
        let new_raw = super::sidecar::merge_annotations(transpiled, annotations);
        let scenario = resolve_for_render(&new_raw, Some(path))?;
        commit(&mut m, scenario, new_raw, Some(source.to_string()));
    } else {
        let new_raw: Value = serde_json::from_str(source).map_err(|e| format!("parse: {e}"))?;
        let scenario = resolve_for_render(&new_raw, Some(path))?;
        commit(&mut m, scenario, new_raw, None);
    }
    Ok(())
}

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

pub type SelfWrites = Arc<Mutex<HashMap<PathBuf, u64>>>;

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

pub fn note_self_write(slot: &SelfWrites, path: &Path, content: &str) {
    let mut map = slot.lock().unwrap_or_else(|e| e.into_inner());
    map.insert(path.to_path_buf(), content_hash(content));
}

pub fn clear_self_write(slot: &SelfWrites, path: &Path) {
    let mut map = slot.lock().unwrap_or_else(|e| e.into_inner());
    map.remove(path);
}

pub fn is_self_write(slot: &SelfWrites, path: &Path, content: &str) -> bool {
    let map = slot.lock().unwrap_or_else(|e| e.into_inner());
    map.get(path) == Some(&content_hash(content))
}

pub type PendingWrites = Arc<Mutex<HashMap<PathBuf, Vec<Mutation>>>>;

pub fn pending_write_slot() -> PendingWrites {
    static SLOT: OnceLock<PendingWrites> = OnceLock::new();
    SLOT.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
        .clone()
}

pub fn queue_mutation(slot: &PendingWrites, path: &Path, mutation: Mutation) {
    let mut map = slot.lock().unwrap_or_else(|e| e.into_inner());
    map.entry(path.to_path_buf()).or_default().push(mutation);
}

pub fn take_pending(slot: &PendingWrites, path: &Path) -> Vec<Mutation> {
    let mut map = slot.lock().unwrap_or_else(|e| e.into_inner());
    map.remove(path).unwrap_or_default()
}

pub fn resolve_flush(
    disk_content: &str,
    is_html: bool,
    mutations: &[Mutation],
) -> Result<Option<String>, String> {
    if is_html {
        let mut current = disk_content.to_string();
        let mut changed = false;
        for mutation in mutations {
            if let Some(updated) = apply_to_html(&current, mutation) {
                current = updated;
                changed = true;
            }
        }
        Ok(changed.then_some(current))
    } else {
        let mut value: Value =
            serde_json::from_str(disk_content).map_err(|e| format!("parse: {e}"))?;
        let mut changed = false;
        for mutation in mutations {
            if let Some(updated) = apply_to_raw(value.clone(), mutation) {
                value = updated;
                changed = true;
            }
        }
        if !changed {
            return Ok(None);
        }
        let text = serde_json::to_string_pretty(&value).map_err(|e| format!("json: {e}"))?;
        Ok(Some(text))
    }
}

pub fn resolve_for_render(raw: &Value, path: Option<&Path>) -> Result<ResolvedScenario, String> {
    let mut json_value = raw.clone();
    let label = path
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<inline>".to_string());
    rustmotion::variables::apply_variables(&mut json_value, None, &label)
        .map_err(|e| e.to_string())?;
    rustmotion::expand::expand_directives(&mut json_value, &label).map_err(|e| e.to_string())?;
    if let Some(dir) = path.and_then(Path::parent) {
        rustmotion::assets::rebase_relative_paths(&mut json_value, dir);
    }
    let scenario: rustmotion::schema::Scenario =
        serde_json::from_value(json_value).map_err(|e| format!("deserialize: {e}"))?;
    let source = match path {
        Some(p) => rustmotion::include::IncludeSource::File(p.to_path_buf()),
        None => rustmotion::include::IncludeSource::Inline,
    };
    rustmotion::include::resolve_includes(scenario, &source).map_err(|e| e.to_string())
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

    #[test]
    fn chart_colors_edit_reaches_the_rendered_pixels() {
        let promo = std::path::Path::new("../../examples/rustmotion-promo.json");
        if !promo.exists() {
            panic!("examples/rustmotion-promo.json missing");
        }
        let loaded = rustmotion::loader::load_input(&promo.to_path_buf()).expect("promo loads");
        let shared: Shared = Arc::new(Mutex::new(StudioModel::new(
            loaded,
            None,
            Some(promo.to_path_buf()),
        )));

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
            rgba.as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[0] > 180 && p[1] < 90 && p[2] < 90)
                .count()
        };

        let before = render_scene(&shared);

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
        let after_prefill = render_scene(&shared);
        assert_eq!(
            count_red(&before),
            count_red(&after_prefill),
            "prefill palette renders identically by design"
        );

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
        assert!(
            count_red(&after_red) >= count_red(&before) + 10,
            "chart must actually turn red: before={} after={}",
            count_red(&before),
            count_red(&after_red)
        );
    }

    #[test]
    fn self_write_skip_decision() {
        let slot: SelfWrites = Arc::new(Mutex::new(HashMap::new()));
        let a = Path::new("/w/a.json");
        let b = Path::new("/w/b.json");
        assert!(!is_self_write(&slot, a, "content"));
        note_self_write(&slot, a, "content");
        assert!(is_self_write(&slot, a, "content"));
        assert!(!is_self_write(&slot, a, "external change"));
        assert!(!is_self_write(&slot, b, "content"));
        note_self_write(&slot, a, "content");
        clear_self_write(&slot, a);
        assert!(!is_self_write(&slot, a, "content"));
    }

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

    #[test]
    fn apply_optimistic_no_longer_gates_on_component_type_correctness() {
        let path = temp_json("bad_type", DOC);
        let shared = model_for(&path);

        let m = Mutation::Style {
            pointer: "/scenes/0/children/0".into(),
            prop: "opacity".into(),
            value: json!("0.5"),
        };
        apply_optimistic(&shared, &m).expect(
            "the optimistic layer no longer re-implements a narrower type check of its own: \
             editor/inspector/write.rs::validate_before_write is the single, authoritative gate \
             now, reusing rustmotion::cli::validation::run_checks (the same pipeline `rustmotion \
             validate`/`render` already share) right before the debounced disk write — this \
             optimistic step only needs the document to still parse and resolve",
        );

        let model = shared.lock().unwrap();
        assert_eq!(
            model.raw["scenes"][0]["children"][0]["style"]["opacity"],
            json!("0.5"),
            "committed in memory — the write-time validator is what refuses to persist it"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn optimistic_rebuild_rebases_relative_asset_paths_like_opening_the_file_does() {
        let dir = std::env::temp_dir().join(format!("rm_opt_assets_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("logo.png"), b"x").unwrap();
        let path = dir.join("scenario.json");
        std::fs::write(
            &path,
            r##"{ "video": { "width": 640, "height": 360 },
                "scenes": [ { "duration": 1.0, "children": [
                    { "type": "image", "src": "logo.png", "style": { "width": "100px" } }
                ] } ] }"##,
        )
        .unwrap();
        let shared = model_for(&path);

        let m = Mutation::Style {
            pointer: "/scenes/0/children/0".into(),
            prop: "opacity".into(),
            value: json!(0.5),
        };
        apply_optimistic(&shared, &m).expect("valid mutation applies");

        let model = shared.lock().unwrap();
        let resolved_src = model.scenario.views[0].scenes[0].children[0]["src"]
            .as_str()
            .expect("src")
            .to_string();
        assert!(
            std::path::Path::new(&resolved_src).is_absolute(),
            "the optimistic rebuild must resolve the asset against the scenario file's own \
             directory, exactly like opening the file does, not against the studio's working \
             directory: {resolved_src}"
        );
        assert_eq!(
            model.raw["scenes"][0]["children"][0]["src"],
            json!("logo.png"),
            "the persisted raw JSON must keep the author's relative path"
        );
        drop(model);
        let _ = std::fs::remove_dir_all(&dir);
    }

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

    #[test]
    fn undo_notes_self_write_and_adopts_in_memory() {
        let before = DOC;
        let after = DOC.replace("48", "72");
        let path = temp_json("undo_flow", &after);
        let shared = model_for(&path);
        let hist: crate::scenario::SharedHistory = Arc::new(Mutex::new(Default::default()));
        crate::scenario::record_edit(&hist, &path, before.to_string());

        crate::scenario::undo(&shared, &hist);

        let disk = std::fs::read_to_string(&path).unwrap();
        assert_eq!(disk, before);
        assert!(is_self_write(&self_write_slot(), &path, &disk));
        let model = shared.lock().unwrap();
        assert_eq!(
            model.raw["scenes"][0]["children"][0]["style"]["font-size"],
            json!(48)
        );
        let _ = std::fs::remove_file(&path);
    }
}
