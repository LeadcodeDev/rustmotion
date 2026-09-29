use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use crate::components::{ChildComponent, Component};
use crate::schema::{AudioTrack, ResolvedScenario, ViewType};

pub struct TimelineOffsets {
    pub scene_starts: Vec<Vec<f64>>,
    pub view_ends: Vec<f64>,
}

pub fn scene_start_offsets(scenario: &ResolvedScenario) -> Vec<Vec<f64>> {
    timeline_offsets(scenario).scene_starts
}

pub fn timeline_offsets(scenario: &ResolvedScenario) -> TimelineOffsets {
    let fps = scenario.video.fps as f64;
    let mut result: Vec<Vec<f64>> = Vec::with_capacity(scenario.views.len());
    let mut view_ends: Vec<f64> = Vec::with_capacity(scenario.views.len());
    let mut cursor = 0.0_f64;

    for (view_idx, view) in scenario.views.iter().enumerate() {
        if view_idx > 0 {
            if let Some(ref vt) = view.transition {
                cursor += vt.duration;
            }
        }

        match view.view_type {
            ViewType::Slide => {
                let spans = crate::encode::video::slide_scene_spans(view, scenario.video.fps);
                let scene_offsets: Vec<f64> = spans
                    .iter()
                    .map(|span| cursor + span.start as f64 / fps)
                    .collect();

                cursor += spans.last().map(|s| s.end as f64 / fps).unwrap_or(0.0);
                view_ends.push(cursor);
                result.push(scene_offsets);
            }

            ViewType::World => {
                eprintln!(
                    "rustmotion: embedded-video audio: view {} is a World view — \
                     audio extraction from embedded video components in world views \
                     is not supported in v1. Skipping.",
                    view_idx
                );

                let scene_offsets = vec![cursor; view.scenes.len()];

                let world_duration: f64 = view
                    .scenes
                    .iter()
                    .map(|s| (s.duration * fps).round() / fps)
                    .sum();
                cursor += world_duration;

                view_ends.push(cursor);
                result.push(scene_offsets);
            }
        }
    }

    TimelineOffsets {
        scene_starts: result,
        view_ends,
    }
}

pub fn resolved_scenario_duration(scenario: &ResolvedScenario) -> f64 {
    let ends = timeline_offsets(scenario).view_ends;
    scenario
        .views
        .iter()
        .enumerate()
        .filter(|(_, view)| !view.scenes.is_empty())
        .filter_map(|(view_idx, _)| ends.get(view_idx).copied())
        .fold(0.0f64, f64::max)
}

#[derive(Debug)]
struct VideoOccurrence {
    src: String,
    trim_start: f64,
    trim_end: Option<f64>,
    playback_rate: f64,
    volume: f32,
    start_at: f64,
    end_at: Option<f64>,
}

fn collect_videos_in_child(child: &ChildComponent, out: &mut Vec<VideoOccurrence>) {
    match &child.component {
        Component::Video(v) => {
            if v.volume > 0.0 {
                out.push(VideoOccurrence {
                    src: v.src.clone(),
                    trim_start: v.trim_start.unwrap_or(0.0),
                    trim_end: v.trim_end,
                    playback_rate: v.playback_rate.unwrap_or(1.0),
                    volume: v.volume,
                    start_at: v.timing.start_at.unwrap_or(0.0),
                    end_at: v.timing.end_at,
                });
            }
        }
        Component::Container(c) => {
            for ch in &c.children {
                collect_videos_in_child(ch, out);
            }
        }
        _ => {}
    }
}

fn collect_videos_in_scene(scene: &crate::schema::Scene, out: &mut Vec<VideoOccurrence>) {
    let children: Vec<ChildComponent> = scene
        .children
        .iter()
        .filter_map(|v| serde_json::from_value(v.clone()).ok())
        .collect();
    for child in &children {
        collect_videos_in_child(child, out);
    }
}

fn ffmpeg_available() -> bool {
    std::process::Command::new("ffmpeg")
        .args(["-version"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

pub fn build_atempo_filter(rate: f64) -> Option<String> {
    const EPSILON: f64 = 1e-9;
    if !rate.is_finite() || rate <= 0.0 {
        return None;
    }
    if (rate - 1.0).abs() < EPSILON {
        return None;
    }

    const MAX_STAGES: usize = 64;

    let mut parts: Vec<String> = Vec::new();
    let mut remaining = rate;

    if rate > 1.0 {
        while remaining > 2.0 + EPSILON {
            if parts.len() >= MAX_STAGES {
                return None;
            }
            parts.push("atempo=2.0".to_string());
            remaining /= 2.0;
        }
        parts.push(format!("atempo={:.6}", remaining));
    } else {
        while remaining < 0.5 - EPSILON {
            if parts.len() >= MAX_STAGES {
                return None;
            }
            parts.push("atempo=0.5".to_string());
            remaining /= 0.5;
        }
        parts.push(format!("atempo={:.6}", remaining));
    }

    Some(parts.join(","))
}

fn wav_cache_base_dir() -> PathBuf {
    let base = dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("rustmotion");
    let _ = std::fs::create_dir_all(&base);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o700));
    }
    base
}

fn wav_cache_path(src: &str, trim_start: f64, trim_end: Option<f64>, rate: f64) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    src.hash(&mut hasher);
    trim_start.to_bits().hash(&mut hasher);
    trim_end.map(|v| v.to_bits()).hash(&mut hasher);
    rate.to_bits().hash(&mut hasher);

    if let Ok(meta) = std::fs::metadata(src) {
        meta.len().hash(&mut hasher);
        if let Ok(modified) = meta.modified() {
            if let Ok(dur) = modified.duration_since(std::time::UNIX_EPOCH) {
                dur.as_nanos().hash(&mut hasher);
            }
        }
    }

    let hash = hasher.finish();
    wav_cache_base_dir().join(format!("rustmotion_vidaud_{:016x}.wav", hash))
}

fn cached_wav_is_trustworthy(path: &std::path::Path) -> bool {
    std::fs::symlink_metadata(path)
        .map(|m| m.file_type().is_file())
        .unwrap_or(false)
}

fn partial_wav_path(wav_path: &std::path::Path) -> PathBuf {
    let stem = wav_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("audio");
    wav_path.with_file_name(format!("{stem}.partial.wav"))
}

fn extract_audio_to_wav(
    src: &str,
    trim_start: f64,
    trim_end: Option<f64>,
    rate: f64,
) -> Option<PathBuf> {
    let wav_path = wav_cache_path(src, trim_start, trim_end, rate);

    if cached_wav_is_trustworthy(&wav_path) {
        return Some(wav_path);
    }

    let partial_path = partial_wav_path(&wav_path);

    let mut args: Vec<String> = Vec::new();

    if trim_start > 0.0 {
        args.push("-ss".to_string());
        args.push(format!("{:.6}", trim_start));
    }

    if let Some(end) = trim_end {
        args.push("-to".to_string());
        args.push(format!("{:.6}", end));
    }

    args.push("-i".to_string());
    args.push(src.to_string());

    args.push("-vn".to_string());

    if let Some(filter) = build_atempo_filter(rate) {
        args.push("-af".to_string());
        args.push(filter);
    }

    args.push("-y".to_string());
    args.push(partial_path.to_str().unwrap_or_default().to_string());

    let status = std::process::Command::new("ffmpeg")
        .args(&args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    match status {
        Ok(s) if s.success() => match std::fs::rename(&partial_path, &wav_path) {
            Ok(()) => Some(wav_path),
            Err(e) => {
                eprintln!(
                    "rustmotion: embedded-video audio: failed to finalize cached WAV for '{}': {}. Skipping.",
                    src, e
                );
                let _ = std::fs::remove_file(&partial_path);
                None
            }
        },
        Ok(_) => {
            eprintln!(
                "rustmotion: embedded-video audio: ffmpeg failed to extract audio from '{}' \
                 (trim_start={:.3}, trim_end={:?}, rate={:.3}). Skipping.",
                src, trim_start, trim_end, rate
            );
            let _ = std::fs::remove_file(&partial_path);
            None
        }
        Err(e) => {
            eprintln!(
                "rustmotion: embedded-video audio: could not spawn ffmpeg for '{}': {}. Skipping.",
                src, e
            );
            let _ = std::fs::remove_file(&partial_path);
            None
        }
    }
}

pub fn collect_video_audio_tracks(scenario: &ResolvedScenario) -> Vec<AudioTrack> {
    if !ffmpeg_available() {
        eprintln!(
            "rustmotion: ffmpeg not found — embedded video audio will be silent. \
             Install ffmpeg to include audio from video components."
        );
        return Vec::new();
    }

    let offsets = scene_start_offsets(scenario);
    let mut tracks: Vec<AudioTrack> = Vec::new();

    for (view_idx, view) in scenario.views.iter().enumerate() {
        if matches!(view.view_type, ViewType::World) {
            continue;
        }

        for (scene_idx, scene) in view.scenes.iter().enumerate() {
            let scene_start = offsets
                .get(view_idx)
                .and_then(|v| v.get(scene_idx))
                .copied()
                .unwrap_or(0.0);

            let mut occurrences: Vec<VideoOccurrence> = Vec::new();
            collect_videos_in_scene(scene, &mut occurrences);

            for occ in occurrences {
                let Some(wav_path) =
                    extract_audio_to_wav(&occ.src, occ.trim_start, occ.trim_end, occ.playback_rate)
                else {
                    continue;
                };

                let Some(wav_str) = wav_path.to_str() else {
                    eprintln!(
                        "rustmotion: embedded-video audio: temp WAV path is not UTF-8 — skipping."
                    );
                    continue;
                };

                let abs_start = scene_start + occ.start_at;

                let end = occ.end_at.map(|ea| {
                    let component_duration = ea - occ.start_at;
                    abs_start + component_duration
                });

                tracks.push(AudioTrack {
                    src: wav_str.to_string(),
                    start: abs_start,
                    end,
                    volume: occ.volume,
                    fade_in: None,
                    fade_out: None,
                    volume_keyframes: Vec::new(),
                });
            }
        }
    }

    tracks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::load_scenario_from_source;

    #[test]
    fn wav_cache_path_does_not_sit_directly_inside_the_bare_shared_temp_dir() {
        let cached = wav_cache_path("foo.mp4", 0.0, None, 1.0);
        let shared_temp = std::env::temp_dir();
        assert_ne!(
            cached.parent(),
            Some(shared_temp.as_path()),
            "the cached WAV must live under a dedicated subdirectory, not directly inside the \
             shared temp dir a same-machine, different-user attacker can also write to: got {}",
            cached.display()
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_at_the_cache_path_is_never_trusted_as_a_cache_hit() {
        let target = std::env::temp_dir().join(format!(
            "rm_vidaud_symlink_target_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&target, b"not a wav, planted by someone else").unwrap();

        let link = std::env::temp_dir().join(format!(
            "rm_vidaud_symlink_link_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(&target, &link).expect("create symlink fixture");

        assert!(
            !cached_wav_is_trustworthy(&link),
            "a symlink sitting at the cache path must never be treated as a valid cache hit, \
             regardless of what it points to"
        );

        let mut real_file = link.with_file_name(format!(
            "rm_vidaud_real_file_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        real_file.set_extension("wav");
        std::fs::write(&real_file, b"RIFF....").unwrap();
        assert!(
            cached_wav_is_trustworthy(&real_file),
            "a genuine regular file must still be trusted"
        );

        let _ = std::fs::remove_file(&target);
        let _ = std::fs::remove_file(&link);
        let _ = std::fs::remove_file(&real_file);
    }

    #[test]
    fn a_missing_path_is_not_trustworthy() {
        let path = std::env::temp_dir().join(format!(
            "rm_vidaud_never_created_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_file(&path);
        assert!(!cached_wav_is_trustworthy(&path));
    }

    fn load(json: &str) -> ResolvedScenario {
        load_scenario_from_source(None, Some(json)).expect("load")
    }

    #[test]
    fn a_world_only_scenario_lasts_the_sum_of_its_scenes() {
        let s = load(
            r#"{
            "video": {"width": 32, "height": 32, "fps": 10},
            "composition": [{
                "type": "world",
                "scenes": [
                    {"duration": 3.0, "children": []},
                    {"duration": 3.0, "children": []},
                    {"duration": 2.0, "children": []}
                ]
            }]
        }"#,
        );

        let total = resolved_scenario_duration(&s);
        assert!(
            (total - 8.0).abs() < 1e-9,
            "a world view lasts the sum of its scenes, not its last scene's 2.0s: got {total}"
        );
    }

    #[test]
    fn world_scenes_keep_their_shared_window_start() {
        let s = load(
            r#"{
            "video": {"width": 32, "height": 32, "fps": 10},
            "composition": [{
                "type": "world",
                "scenes": [
                    {"duration": 3.0, "children": []},
                    {"duration": 2.0, "children": []}
                ]
            }]
        }"#,
        );

        let offsets = scene_start_offsets(&s);
        assert_eq!(offsets[0], vec![0.0, 0.0]);
    }

    #[test]
    fn a_world_view_followed_by_a_slide_view_still_totals_correctly() {
        let s = load(
            r#"{
            "video": {"width": 32, "height": 32, "fps": 10},
            "composition": [
                {"type": "world", "scenes": [
                    {"duration": 3.0, "children": []},
                    {"duration": 2.0, "children": []}
                ]},
                {"type": "slide", "scenes": [{"duration": 1.0, "children": []}]}
            ]
        }"#,
        );

        let total = resolved_scenario_duration(&s);
        assert!(
            (total - 6.0).abs() < 1e-9,
            "world 5.0s then a 1.0s slide totals 6.0s: got {total}"
        );
    }

    #[test]
    fn a_trailing_empty_view_does_not_shorten_the_total() {
        let s = load(
            r#"{
            "video": {"width": 32, "height": 32, "fps": 10},
            "composition": [
                {"type": "world", "scenes": [
                    {"duration": 3.0, "children": []},
                    {"duration": 2.0, "children": []}
                ]},
                {"type": "slide", "scenes": []}
            ]
        }"#,
        );

        let total = resolved_scenario_duration(&s);
        assert!(
            (total - 5.0).abs() < 1e-9,
            "an empty view contributes nothing and takes nothing away: got {total}"
        );
    }

    #[test]
    fn offsets_single_view_no_transitions() {
        let s = load(
            r#"{
            "video": {"width": 32, "height": 32, "fps": 10},
            "scenes": [
                {"duration": 1.0, "children": []},
                {"duration": 2.0, "children": []},
                {"duration": 0.5, "children": []}
            ]
        }"#,
        );

        let offsets = scene_start_offsets(&s);
        assert_eq!(offsets.len(), 1);
        let v = &offsets[0];
        assert_eq!(v.len(), 3);

        assert!((v[0] - 0.0).abs() < 1e-9, "scene 0 starts at 0");
        assert!((v[1] - 1.0).abs() < 1e-9, "scene 1 starts at 1.0s");
        assert!((v[2] - 3.0).abs() < 1e-9, "scene 2 starts at 3.0s");
    }

    #[test]
    fn offsets_single_view_with_scene_transition() {
        let s = load(
            r#"{
            "video": {"width": 32, "height": 32, "fps": 10},
            "scenes": [
                {"duration": 2.0, "children": []},
                {"duration": 1.0, "transition": {"type": "fade", "duration": 0.5}, "children": []}
            ]
        }"#,
        );

        let offsets = scene_start_offsets(&s);
        let v = &offsets[0];
        assert!((v[0] - 0.0).abs() < 1e-9, "scene 0 at 0");
        assert!((v[1] - 1.5).abs() < 1e-3, "scene 1 at 1.5s got {}", v[1]);
    }

    #[test]
    fn offsets_two_views_with_view_transition() {
        let s = load(
            r#"{
            "video": {"width": 32, "height": 32, "fps": 10},
            "composition": [
                {"type": "slide", "scenes": [
                    {"duration": 1.0, "children": []},
                    {"duration": 1.0, "children": []}
                ]},
                {"type": "slide",
                 "transition": {"type": "fade", "duration": 0.5},
                 "scenes": [
                    {"duration": 1.0, "children": []}
                ]}
            ]
        }"#,
        );

        let offsets = scene_start_offsets(&s);
        assert_eq!(offsets.len(), 2);

        let v0 = &offsets[0];
        assert!((v0[0] - 0.0).abs() < 1e-9, "view0 scene0 at 0");
        assert!((v0[1] - 1.0).abs() < 1e-9, "view0 scene1 at 1.0");

        let v1 = &offsets[1];
        assert!(
            (v1[0] - 2.5).abs() < 1e-3,
            "view1 scene0 expected 2.5s, got {}",
            v1[0]
        );
    }

    #[test]
    fn atempo_rate_1_returns_none() {
        assert_eq!(build_atempo_filter(1.0), None);
    }

    #[test]
    fn atempo_rate_2_single_stage() {
        let f = build_atempo_filter(2.0).unwrap();
        assert!(f.starts_with("atempo=2.0"), "got: {f}");
        assert!(!f.contains(','), "should be single stage: {f}");
    }

    #[test]
    fn atempo_rate_4_two_stages() {
        let f = build_atempo_filter(4.0).unwrap();
        let parts: Vec<&str> = f.split(',').collect();
        assert_eq!(parts.len(), 2, "rate 4.0 → 2 stages: {f}");
        assert!(parts[0].starts_with("atempo=2.0"), "first stage: {f}");
        assert!(parts[1].starts_with("atempo=2.0"), "second stage: {f}");
    }

    #[test]
    fn atempo_rate_0_5_single_stage() {
        let f = build_atempo_filter(0.5).unwrap();
        assert!(f.starts_with("atempo=0.5"), "got: {f}");
        assert!(!f.contains(','), "single stage: {f}");
    }

    #[test]
    fn atempo_rate_0_25_two_stages() {
        let f = build_atempo_filter(0.25).unwrap();
        let parts: Vec<&str> = f.split(',').collect();
        assert_eq!(parts.len(), 2, "rate 0.25 → 2 stages: {f}");
        assert!(parts[0].starts_with("atempo=0.5"), "first: {f}");
        assert!(parts[1].starts_with("atempo=0.5"), "second: {f}");
    }

    #[test]
    fn volume_zero_is_excluded() {
        let s = load(
            r#"{
            "video": {"width": 32, "height": 32, "fps": 10},
            "scenes": [
                {"duration": 1.0, "children": [
                    {"type": "video", "src": "test.mp4", "volume": 0.0,
                     "style": {"width": "32px", "height": "32px"}}
                ]}
            ]
        }"#,
        );

        let mut occs: Vec<VideoOccurrence> = Vec::new();
        let children: Vec<ChildComponent> = s.views[0].scenes[0]
            .children
            .iter()
            .filter_map(|v| serde_json::from_value(v.clone()).ok())
            .collect();
        for ch in &children {
            collect_videos_in_child(ch, &mut occs);
        }
        assert!(occs.is_empty(), "volume=0 must not be collected");
    }

    #[test]
    fn nested_video_in_card_is_collected() {
        let s = load(
            r#"{
            "video": {"width": 32, "height": 32, "fps": 10},
            "scenes": [
                {"duration": 1.0, "children": [
                    {"type": "card", "children": [
                        {"type": "video", "src": "nested.mp4", "volume": 0.8,
                         "style": {"width": "32px", "height": "32px"}}
                    ]}
                ]}
            ]
        }"#,
        );

        let mut occs: Vec<VideoOccurrence> = Vec::new();
        let children: Vec<ChildComponent> = s.views[0].scenes[0]
            .children
            .iter()
            .filter_map(|v| serde_json::from_value(v.clone()).ok())
            .collect();
        for ch in &children {
            collect_videos_in_child(ch, &mut occs);
        }
        assert_eq!(occs.len(), 1, "nested video must be found");
        assert_eq!(occs[0].src, "nested.mp4");
        assert!((occs[0].volume - 0.8).abs() < 1e-6);
    }

    #[test]
    #[cfg_attr(not(feature = "ffmpeg_integration"), ignore)]
    fn audio_track_offset_scene2_with_start_at() {}

    #[test]
    fn wav_cache_path_is_deterministic() {
        let p1 = wav_cache_path("foo.mp4", 0.5, Some(3.0), 1.5);
        let p2 = wav_cache_path("foo.mp4", 0.5, Some(3.0), 1.5);
        assert_eq!(p1, p2);
    }

    #[test]
    fn wav_cache_path_differs_on_params() {
        let p1 = wav_cache_path("foo.mp4", 0.0, None, 1.0);
        let p2 = wav_cache_path("foo.mp4", 0.5, None, 1.0);
        assert_ne!(p1, p2);
    }

    #[test]
    fn wav_cache_path_changes_when_source_file_is_modified() {
        let src = std::env::temp_dir().join(format!(
            "rm_vidaud_src_test_{}_{}.mp4",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&src, b"version one").unwrap();
        let p1 = wav_cache_path(src.to_str().unwrap(), 0.0, None, 1.0);

        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&src, b"version two, a longer and different payload").unwrap();
        let p2 = wav_cache_path(src.to_str().unwrap(), 0.0, None, 1.0);

        assert_ne!(
            p1, p2,
            "modifying the source file's contents must invalidate the cached WAV path"
        );

        let _ = std::fs::remove_file(&src);
    }

    #[test]
    fn failed_extraction_leaves_no_residue_on_disk() {
        if !ffmpeg_available() {
            eprintln!("failed_extraction_leaves_no_residue_on_disk: ffmpeg not found — skipping");
            return;
        }
        let missing_src = std::env::temp_dir().join(format!(
            "rm_vidaud_missing_{}_{}.mp4",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_file(&missing_src);

        let wav_path = wav_cache_path(missing_src.to_str().unwrap(), 0.0, None, 1.0);
        let partial_path = partial_wav_path(&wav_path);
        let _ = std::fs::remove_file(&wav_path);
        let _ = std::fs::remove_file(&partial_path);

        let result = extract_audio_to_wav(missing_src.to_str().unwrap(), 0.0, None, 1.0);

        assert!(
            result.is_none(),
            "extraction from a nonexistent source must fail"
        );
        assert!(
            !wav_path.exists(),
            "a failed extraction must not leave a cached WAV that a later render would reuse"
        );
        assert!(
            !partial_path.exists(),
            "a failed extraction must not leave a .partial scratch file behind"
        );
    }

    #[test]
    fn successful_extraction_leaves_no_partial_file_behind() {
        if !ffmpeg_available() {
            eprintln!(
                "successful_extraction_leaves_no_partial_file_behind: ffmpeg not found — skipping"
            );
            return;
        }
        let fixture = std::env::temp_dir().join("rustmotion_test_vidaud_partial_fixture.mp4");
        let fixture_str = fixture.to_str().unwrap();
        let status = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1",
                fixture_str,
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        if !matches!(status, Ok(s) if s.success()) {
            eprintln!(
                "successful_extraction_leaves_no_partial_file_behind: fixture generation failed — skipping"
            );
            return;
        }

        let wav_path = wav_cache_path(fixture_str, 0.0, None, 1.0);
        let partial_path = partial_wav_path(&wav_path);
        let _ = std::fs::remove_file(&wav_path);
        let _ = std::fs::remove_file(&partial_path);

        let result = extract_audio_to_wav(fixture_str, 0.0, None, 1.0);

        assert!(
            result.is_some(),
            "extraction from a valid fixture must succeed"
        );
        assert!(
            wav_path.exists(),
            "successful extraction must leave the cached WAV at its final path"
        );
        assert!(
            !partial_path.exists(),
            "successful extraction must not leave the .partial scratch file behind"
        );

        let _ = std::fs::remove_file(&wav_path);
        let _ = std::fs::remove_file(&fixture);
    }

    #[test]
    fn integration_audio_track_from_embedded_video() {
        if !ffmpeg_available() {
            eprintln!("integration_audio_track_from_embedded_video: ffmpeg not found — skipping");
            return;
        }

        let fixture = std::env::temp_dir().join("rustmotion_test_vidaud_fixture.mp4");
        let fixture_str = fixture.to_str().unwrap();

        let status = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=1:size=32x32:rate=30",
                "-shortest",
                fixture_str,
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("spawn ffmpeg for fixture");

        if !status.success() {
            eprintln!(
                "integration_audio_track_from_embedded_video: fixture generation failed — skipping"
            );
            return;
        }

        let json = format!(
            r#"{{
            "video": {{"width": 32, "height": 32, "fps": 30}},
            "scenes": [
                {{"duration": 1.0, "children": []}},
                {{"duration": 1.0, "children": [
                    {{"type": "video", "src": "{}", "volume": 0.9,
                     "start_at": 0.2,
                     "style": {{"width": "32px", "height": "32px"}}}}
                ]}}
            ]
        }}"#,
            fixture_str.replace('\\', "\\\\")
        );

        let scenario = load_scenario_from_source(None, Some(&json)).expect("load");
        let tracks = collect_video_audio_tracks(&scenario);

        let _ = std::fs::remove_file(&fixture);

        assert_eq!(tracks.len(), 1, "expected one audio track");
        let t = &tracks[0];

        assert!(
            (t.start - 1.2).abs() < 1e-9,
            "expected start=1.2, got {}",
            t.start
        );
        assert!(
            (t.volume - 0.9).abs() < 1e-6,
            "expected volume=0.9, got {}",
            t.volume
        );
        assert!(!t.src.is_empty(), "src must be a WAV path");
        assert!(
            std::path::Path::new(&t.src).exists(),
            "WAV file must exist: {}",
            t.src
        );
    }
}
