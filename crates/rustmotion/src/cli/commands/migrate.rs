use rustmotion::error::{Result, RustmotionError};
use serde_json::Value;
use std::path::{Path, PathBuf};

use super::validate::{fixable_source, refuse_fix};

fn incoming_transition_seconds(scene: &Value) -> f64 {
    scene
        .get("transition")
        .and_then(|t| t.get("duration"))
        .and_then(Value::as_f64)
        .unwrap_or(0.0)
}

fn scenario_fps(root: &Value) -> u32 {
    root.get("video")
        .and_then(|v| v.get("fps"))
        .and_then(Value::as_u64)
        .and_then(|f| u32::try_from(f).ok())
        .filter(|&f| f > 0)
        .unwrap_or(30)
}

fn migrate_scenes_array(scenes: &mut [Value], fps: u32, label: &str) -> Result<()> {
    let durations: Vec<f64> = scenes
        .iter()
        .map(|s| {
            s.get("duration").and_then(Value::as_f64).ok_or_else(|| {
                RustmotionError::Generic(format!(
                    "migrate: a scene in '{label}' has no numeric `duration` — refusing to \
                     guess a compensated value for it"
                ))
            })
        })
        .collect::<Result<_>>()?;
    let incoming: Vec<f64> = scenes.iter().map(incoming_transition_seconds).collect();

    let spans = rustmotion::encode::video::quantised_spans(&durations, &incoming, fps);

    let n = scenes.len();
    let at_frames: Vec<u32> = spans.iter().map(|s| s.start).collect();
    let mut new_duration_frames: Vec<u32> = spans.iter().map(|s| s.frames()).collect();
    let mut has_outgoing_transition = vec![false; n];
    for i in 0..n.saturating_sub(1) {
        let cut = spans[i + 1].start;
        if cut < spans[i].end {
            new_duration_frames[i] = cut - spans[i].start;
            has_outgoing_transition[i] = true;
        }
    }

    for (i, scene) in scenes.iter_mut().enumerate() {
        let obj = scene.as_object_mut().ok_or_else(|| {
            RustmotionError::Generic(format!(
                "migrate: a scene in '{label}' is not a JSON object"
            ))
        })?;
        let new_duration = new_duration_frames[i] as f64 / fps as f64;
        obj.insert("duration".into(), serde_json::json!(new_duration));
        let at_seconds = at_frames[i] as f64 / fps as f64;
        obj.insert("at".into(), serde_json::json!(at_seconds));
        if has_outgoing_transition[i] {
            obj.insert("tail".into(), Value::String("continue".into()));
        }
    }

    Ok(())
}

pub fn cmd_migrate(input: &PathBuf, output: Option<&Path>) -> Result<()> {
    let raw_source = std::fs::read_to_string(input).map_err(|e| RustmotionError::FileRead {
        path: input.display().to_string(),
        source: e,
    })?;

    if let Some(refusal) = refuse_fix(input, &raw_source) {
        return Err(RustmotionError::Generic(refusal.explain_for_migrate(input)));
    }

    let mut root = fixable_source(&raw_source)?;

    if matches!(root.get("timing").and_then(Value::as_str), Some("v2")) {
        return Err(RustmotionError::Generic(format!(
            "migrate: {} already declares `\"timing\": \"v2\"` — nothing to migrate",
            input.display()
        )));
    }

    let fps = scenario_fps(&root);

    if let Some(composition) = root.get_mut("composition").and_then(Value::as_array_mut) {
        for (vi, view) in composition.iter_mut().enumerate() {
            if let Some(scenes) = view.get_mut("scenes").and_then(Value::as_array_mut) {
                migrate_scenes_array(scenes, fps, &format!("composition[{vi}].scenes"))?;
            }
        }
    } else if let Some(scenes) = root.get_mut("scenes").and_then(Value::as_array_mut) {
        migrate_scenes_array(scenes, fps, "scenes")?;
    }

    let obj = root.as_object_mut().ok_or_else(|| {
        RustmotionError::Generic("migrate: scenario root is not a JSON object".into())
    })?;
    obj.insert("timing".into(), Value::String("v2".into()));

    let pretty = serde_json::to_string_pretty(&root).map_err(|e| {
        RustmotionError::Generic(format!("migrate: serialize migrated scenario: {e}"))
    })?;

    let dest = output.unwrap_or(input.as_path());
    std::fs::write(dest, &pretty).map_err(|e| RustmotionError::FileRead {
        path: dest.display().to_string(),
        source: e,
    })?;

    let before = rustmotion::loader::load_scenario_from_source(None, Some(&raw_source))
        .map(|s| rustmotion::encode::build_frame_tasks(&s).len());
    let after = rustmotion::loader::load_scenario_from_source(None, Some(&pretty))
        .map(|s| rustmotion::encode::build_frame_tasks(&s).len());

    eprintln!("Migrated {} to `\"timing\": \"v2\"`", input.display());
    if dest != input.as_path() {
        eprintln!("  Wrote {}", dest.display());
    }
    match (before, after) {
        (Ok(b), Ok(a)) => {
            eprintln!(
                "  Duration: {:.3}s -> {:.3}s ({} -> {} frames @ {fps}fps){}",
                b as f64 / fps as f64,
                a as f64 / fps as f64,
                b,
                a,
                if b == a {
                    " — frame-identical"
                } else {
                    " — MISMATCH"
                }
            );
            if b != a {
                return Err(RustmotionError::Generic(format!(
                    "migrate: {} rendered {b} frames before migration but {a} frames after — \
                     refusing to leave a migrated file that changed the render. This is a bug in \
                     `migrate`'s own compensation, not in the source file.",
                    input.display()
                )));
            }
        }
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("  Warning: could not confirm frame parity ({e})");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_fixture(name: &str, content: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "rm_migrate_test_{}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            name
        ));
        std::fs::write(&path, content).expect("write fixture");
        path
    }

    fn six_scene_reel_json() -> String {
        serde_json::json!({
            "video": { "width": 640, "height": 360, "fps": 30 },
            "scenes": [
                { "duration": 2.2, "children": [] },
                { "duration": 2.6, "transition": { "type": "fade", "duration": 0.3 }, "children": [] },
                { "duration": 2.6, "transition": { "type": "fade", "duration": 0.3 }, "children": [] },
                { "duration": 3.12, "transition": { "type": "fade", "duration": 0.3 }, "children": [] },
                { "duration": 2.08, "transition": { "type": "fade", "duration": 0.25 }, "children": [] },
                { "duration": 2.4, "transition": { "type": "fade", "duration": 0.35 }, "children": [] }
            ]
        })
        .to_string()
    }

    #[test]
    fn migrates_the_six_scene_reel_frame_identically() {
        let path = write_fixture("six_scene.json", &six_scene_reel_json());
        let result = cmd_migrate(&path, None);
        let migrated = std::fs::read_to_string(&path).expect("read migrated file");
        std::fs::remove_file(&path).ok();
        result.expect("migration must succeed");

        let value: Value = serde_json::from_str(&migrated).unwrap();
        assert_eq!(value["timing"], "v2");

        let scenario = rustmotion::loader::load_scenario_from_source(None, Some(&migrated))
            .expect("migrated scenario loads");
        let frames = rustmotion::encode::build_frame_tasks(&scenario).len();
        let duration = frames as f64 / 30.0;

        let original =
            rustmotion::loader::load_scenario_from_source(None, Some(&six_scene_reel_json()))
                .expect("original scenario loads");
        assert_eq!(
            frames,
            rustmotion::encode::build_frame_tasks(&original).len(),
            "migrated file must render the exact same frame count as the pre-migration source"
        );
        assert!(
            (duration - 13.5).abs() < 0.1,
            "expected roughly 13.5s, got {duration}s ({frames} frames)"
        );
    }

    #[test]
    fn migrated_scenes_have_explicit_at_and_continue_tail() {
        let path = write_fixture("six_scene_fields.json", &six_scene_reel_json());
        cmd_migrate(&path, None).expect("migration succeeds");
        let migrated = std::fs::read_to_string(&path).expect("read migrated file");
        std::fs::remove_file(&path).ok();

        let value: Value = serde_json::from_str(&migrated).unwrap();
        let scenes = value["scenes"].as_array().unwrap();
        assert_eq!(scenes.len(), 6);
        assert_eq!(scenes[0]["at"], serde_json::json!(0.0));
        for s in &scenes[..5] {
            assert_eq!(s["tail"], "continue");
        }
        assert!(
            scenes[5].get("tail").is_none(),
            "last scene has no outgoing transition to continue through"
        );
    }

    #[test]
    fn snap_beat_on_top_of_the_migrated_file_moves_cuts_and_changes_duration() {
        let path = write_fixture("six_scene_snap.json", &six_scene_reel_json());
        cmd_migrate(&path, None).expect("migration succeeds");
        let migrated = std::fs::read_to_string(&path).expect("read migrated file");
        std::fs::remove_file(&path).ok();

        let mut value: Value = serde_json::from_str(&migrated).unwrap();
        value["bpm"] = serde_json::json!(11.0);
        value["snap"] = serde_json::json!("beat");

        let snapped = serde_json::to_string(&value).unwrap();
        let scenario = rustmotion::loader::load_scenario_from_source(None, Some(&snapped))
            .expect("snapped scenario loads");
        let frames = rustmotion::encode::build_frame_tasks(&scenario).len();

        let migrated_frames = 405;
        let beat_grid_pushes_the_last_cut_to = 379;
        let last_scene_frames = 72;
        assert_eq!(
            frames,
            beat_grid_pushes_the_last_cut_to + last_scene_frames,
            "with bpm 11 the six cuts snap onto three beats (0s, 5.4545s, 10.9091s); three \
             of them land inside the previous scene and are pushed forward, leaving the last \
             scene starting at frame {beat_grid_pushes_the_last_cut_to}"
        );
        assert_ne!(
            frames, migrated_frames,
            "the point of the test is that snapping moves cuts, so it must not leave the \
             migrated duration untouched"
        );
    }

    #[test]
    fn refuses_a_templated_scenario_and_leaves_the_file_untouched() {
        let original = r#"{
            "config": { "title": { "type": "string", "default": "hi" } },
            "video": { "width": 320, "height": 240, "fps": 30 },
            "scenes": [{ "duration": 1.0, "children": [] }]
        }"#;
        let path = write_fixture("templated.json", original);
        let result = cmd_migrate(&path, None);
        let after = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).ok();

        assert!(result.is_err(), "a templated scenario must be refused");
        assert_eq!(
            after, original,
            "file must be byte-identical after a refused migrate"
        );
    }

    #[test]
    fn refuses_a_scenario_using_include() {
        let original = r#"{
            "video": { "width": 320, "height": 240, "fps": 30 },
            "scenes": [{ "include": "part.json" }]
        }"#;
        let path = write_fixture("uses_include.json", original);
        let result = cmd_migrate(&path, None);
        std::fs::remove_file(&path).ok();
        assert!(result.is_err(), "a scenario using include must be refused");
    }

    #[test]
    fn refuses_a_scenario_already_on_v2() {
        let original = serde_json::json!({
            "video": { "width": 320, "height": 240, "fps": 30 },
            "timing": "v2",
            "scenes": [{ "duration": 1.0, "children": [] }]
        })
        .to_string();
        let path = write_fixture("already_v2.json", &original);
        let result = cmd_migrate(&path, None);
        let after = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).ok();

        assert!(result.is_err(), "an already-v2 scenario must be refused");
        assert_eq!(after, original, "file must be untouched");
    }

    #[test]
    fn a_scene_with_no_following_transition_keeps_its_duration() {
        let original = serde_json::json!({
            "video": { "width": 320, "height": 240, "fps": 30 },
            "scenes": [
                { "duration": 1.0, "children": [] },
                { "duration": 2.0, "children": [] }
            ]
        })
        .to_string();
        let path = write_fixture("no_transition.json", &original);
        cmd_migrate(&path, None).expect("migration succeeds");
        let migrated = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).ok();

        let value: Value = serde_json::from_str(&migrated).unwrap();
        assert_eq!(value["scenes"][0]["duration"], serde_json::json!(1.0));
        assert_eq!(value["scenes"][1]["duration"], serde_json::json!(2.0));
        assert!(value["scenes"][0].get("tail").is_none());
    }
}
