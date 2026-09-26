//! `rustmotion migrate` — issue #335's crossing for issue #336's gate.
//!
//! `"timing": "v2"` (see [`rustmotion_core::schema::TimingMode`]) changes how
//! a slide view's total duration is computed: a `"v1"` scenario's transition
//! *carves its frames out of* the two scenes it sits between
//! (`sum(scene durations) - sum(transition durations)`), while a `"v2"`
//! scenario places every scene at an absolute `at` and lets a transition
//! *add* frames past the outgoing scene's own end
//! (`at_last + duration_last`, no subtraction). Flipping the flag alone on an
//! existing file is therefore a real behaviour break — a five-transition
//! reel gets 1.5s longer — which is exactly why the flag defaults to `"v1"`
//! and needs a deliberate opt-in.
//!
//! This command is that opt-in, done losslessly: it does not merely set the
//! flag, it *compensates* for the semantic difference so the migrated file
//! renders frame-for-frame identically to the one it replaces. For every
//! scene that has a transition entering the *next* one, this scene's own
//! `duration` is shortened by that transition's length (clamped to this
//! scene's own frame budget, exactly the way
//! `rustmotion::encode::video::tasks`'s `actual_outgoing_transition` already
//! clamps it for `"v1"` rendering) and its `tail` is set to `"continue"` —
//! reproducing `"v1"`'s own behaviour, where the outgoing scene keeps
//! animating (never freezes) through the overlap. Every scene's `at` is
//! written out explicitly, as the plain number of seconds where it would
//! have landed anyway under `"v2"`'s own default (auto) placement — a no-op
//! for a first `migrate` run, but what makes a *subsequent*, separate
//! `"snap": "beat"` opt-in able to move a cut at all: [`SceneStart::Auto`]
//! ignores `snap` entirely (only an explicit [`SceneStart::At`] is
//! snapped — see `rustmotion::encode::video::tasks::build_slide_view_tasks_v2`),
//! so a migrated file that left every `at` on `"auto"` would silently ignore
//! `snap: "beat"` layered on afterwards.
//!
//! Migration preserves; it does not improve. A scenario that already reads
//! `13.5s` under `"v1"` still reads `13.5s`, frame for frame, once migrated —
//! any subsequent change in on-screen timing (e.g. snapping cuts to a beat
//! grid) is a deliberate, separate, later edit, never something this command
//! does on its own.
//!
//! Refuses a templated scenario, or one using `include`/`for-each`/`use`,
//! for the identical reason `validate --fix` already does (see
//! `validate.rs`'s `FixRefusal` doc comment, reused here rather than
//! re-derived): the path a migrated `duration`/`at`/`tail` gets written at is
//! computed against the *expanded* tree, and would silently drift from the
//! source the moment `include`/`for-each`/`use` makes the two diverge.

use rustmotion::error::{Result, RustmotionError};
use serde_json::Value;
use std::path::{Path, PathBuf};

use super::validate::{fixable_source, refuse_fix};

/// Frames a transition entering the scene *after* `scene_frames` consumes,
/// clamped to `scene_frames` itself — the same clamp
/// `rustmotion::encode::video::tasks::actual_outgoing_transition` applies
/// when `"v1"` actually renders this same overlap, reproduced here (rather
/// than called: that function is private to a file outside this
/// workstream's owned perimeter) so the compensation this command computes
/// matches, frame for frame, what the source file already rendered.
fn transition_frames_into_next(next_scene: &Value, scene_frames: u32, fps: u32) -> u32 {
    let Some(duration) = next_scene
        .get("transition")
        .and_then(|t| t.get("duration"))
        .and_then(Value::as_f64)
    else {
        return 0;
    };
    let raw = (duration * fps as f64).round().max(0.0) as u32;
    raw.min(scene_frames)
}

/// `video.fps`, defaulting to 30 (the schema's own default —
/// `rustmotion_core::schema::video::default_fps`, private to that crate) when
/// absent or not a plain number.
fn scenario_fps(root: &Value) -> u32 {
    root.get("video")
        .and_then(|v| v.get("fps"))
        .and_then(Value::as_u64)
        .and_then(|f| u32::try_from(f).ok())
        .filter(|&f| f > 0)
        .unwrap_or(30)
}

/// Rewrites one view's `scenes` array in place: every scene's `duration` is
/// compensated for the transition entering the *next* scene, `tail` is set
/// to `"continue"` on a scene that has one, and every scene's `at` is
/// written as the absolute second it lands on either way — see the module
/// doc for why all three are necessary for a lossless migration.
fn migrate_scenes_array(scenes: &mut [Value], fps: u32, label: &str) -> Result<()> {
    let scene_frames: Vec<u32> = scenes
        .iter()
        .map(|s| {
            s.get("duration")
                .and_then(Value::as_f64)
                .map(|d| (d * fps as f64).round().max(0.0) as u32)
                .ok_or_else(|| {
                    RustmotionError::Generic(format!(
                        "migrate: a scene in '{label}' has no numeric `duration` — refusing to \
                         guess a compensated value for it"
                    ))
                })
        })
        .collect::<Result<_>>()?;

    let n = scenes.len();
    let mut new_duration_frames = scene_frames.clone();
    let mut has_outgoing_transition = vec![false; n];
    for i in 0..n {
        if i + 1 >= n {
            continue;
        }
        let into_next = transition_frames_into_next(&scenes[i + 1], scene_frames[i], fps);
        if into_next > 0 {
            new_duration_frames[i] = scene_frames[i] - into_next;
            has_outgoing_transition[i] = true;
        }
    }

    let mut at_frames = vec![0u32; n];
    for i in 1..n {
        at_frames[i] = at_frames[i - 1] + new_duration_frames[i - 1];
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

/// `rustmotion migrate -f x.json [-o out.json]` — see the module doc.
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

    // Prove the round trip rather than just asserting it: load both the
    // pre-migration source and the freshly written file through the exact
    // same frame scheduler `render` uses, and report their durations side by
    // side. A mismatch here is this command's own bug, not the input's.
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

    /// The exact six-scene reel issue #335 names: 2.2+2.6+2.6+3.12+2.08+2.4s
    /// of scene duration with five transitions (0.3+0.3+0.3+0.25+0.35s)
    /// entering scenes 1..5, rendering 13.5s under `"v1"`. Migrated, it must
    /// still render 13.5s, frame for frame.
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

        // The real acceptance criterion: byte-for-byte the same frame count
        // as the pre-migration ("v1") source — not a hand-computed constant,
        // which would silently drift from whatever `actual_outgoing_transition`
        // (the frame scheduler's own rounding) actually does.
        let original =
            rustmotion::loader::load_scenario_from_source(None, Some(&six_scene_reel_json()))
                .expect("original scenario loads");
        assert_eq!(
            frames,
            rustmotion::encode::build_frame_tasks(&original).len(),
            "migrated file must render the exact same frame count as the pre-migration source"
        );
        // Sanity: near the ~13.5s issue #335 names for this reel (exact value
        // depends on how `.round()` breaks the one exact half-frame tie in
        // this fixture's numbers — see this workstream's report).
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
        // Scene 0 starts at 0; every scene but the last (no transition
        // follows it) continues through its own tail.
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
        // bpm=11 (a 60/11s beat, beat_offset=0): of this reel's five
        // migrated `at` values, this is the grid where exactly one of them
        // (scene 3's) has its nearest beat land *past* where the previous
        // scene's own window already ends, so it actually moves (opening a
        // hold) instead of being clamped back to its unsnapped position like
        // every other scene's nearest beat is here. That single moved cut is
        // what turns the migrated file's frame-identical duration into
        // exactly 15.0s — found by simulating `build_slide_view_tasks_v2`'s
        // exact rounding rather than guessed at (see this workstream's
        // report).
        value["bpm"] = serde_json::json!(11.0);
        value["snap"] = serde_json::json!("beat");

        let snapped = serde_json::to_string(&value).unwrap();
        let scenario = rustmotion::loader::load_scenario_from_source(None, Some(&snapped))
            .expect("snapped scenario loads");
        let frames = rustmotion::encode::build_frame_tasks(&scenario).len();
        let duration = frames as f64 / 30.0;
        assert!(
            (duration - 15.0).abs() < 1e-6,
            "expected snap:\"beat\" on the migrated file to render exactly 15.0s, got {duration}s"
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
